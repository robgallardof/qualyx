//! Qualyx MCP server (ADR 0003): compact, task-oriented tools over stdio.
//!
//! Safety model (threat model T2, T3, T6):
//! - Every path is canonicalized and must sit under one of the `--root` directories.
//! - Tools take structured arguments only; nothing reaches a shell.
//! - Long work returns a run id immediately; status, cancel and artifacts are separate tools.
//! - Every call is appended to an audit log (`<first root>/.qualyx/mcp-audit.jsonl`).

pub mod clients;

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use qualyx_app::TestRequest;
use qualyx_core::{Isolation, RunManifest, Status};
use qualyx_runners::CancelToken;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Concurrent `qa_run`s per server; more are refused instead of queued.
pub const MAX_CONCURRENT_RUNS: usize = 2;
const DEFAULT_ARTIFACT_BYTES: usize = 20_000;
const MAX_ARTIFACT_BYTES: usize = 200_000;
const MAX_LISTED_FAILURES: usize = 20;

const INSTRUCTIONS: &str = "Qualyx runs a project's existing tests with their native tools \
(cargo, dotnet, node --test, pytest) and records evidence. Typical flow: qa_discover -> qa_run \
(returns a run_id at once) -> poll qa_run_status until state is 'finished' -> qa_report or \
qa_get_artifact for details. Status 'inconclusive' or 'error' is never a pass. Only paths under \
the configured roots are allowed.";

enum RunState {
    Running,
    Finished(Box<RunManifest>),
    Failed(String),
}

struct RunEntry {
    project: PathBuf,
    cancel: CancelToken,
    state: RunState,
}

#[derive(Clone)]
pub struct QualyxServer {
    roots: Arc<Vec<PathBuf>>,
    runs: Arc<Mutex<HashMap<String, RunEntry>>>,
    audit: Arc<Mutex<Option<std::fs::File>>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct PathArgs {
    /// Project directory (absolute, or relative to the server's working directory).
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct RunArgs {
    /// Project directory.
    pub path: String,
    /// Per-runner timeout in seconds (defaults to the project config, 1800).
    pub timeout_secs: Option<u64>,
    /// "process" (default) or "container".
    pub isolation: Option<String>,
    /// Only run what changed since this git revision (e.g. "origin/main"), or "index".
    pub changed_since: Option<String>,
    /// Ecosystems in parallel (default 1).
    pub jobs: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct BrowseArgs {
    /// Project whose .qualyx/ receives the evidence.
    pub path: String,
    /// Flow JSON file (must be under an allowed root).
    pub flow_path: String,
    /// Browser ids to use (default: every installed browser).
    pub browsers: Option<Vec<String>>,
    /// Directory to serve on loopback as the base URL (must be under an allowed root).
    pub serve_dir: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct DesktopArgs {
    /// "probe" or "snapshot". Read-only: no input is ever sent.
    pub action: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct SecurityArgs {
    /// Project directory to scan.
    pub path: String,
    /// Subset of: gitleaks, semgrep, trivy, osv-scanner (default: all installed).
    pub scanners: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct PerfArgs {
    /// Project whose .qualyx/ stores the results.
    pub path: String,
    /// Workload TOML file (must be under an allowed root).
    pub workload_path: String,
    /// Repeats (default 2, max 3).
    pub repeat: Option<u32>,
    /// Mark the run as baseline.
    pub baseline: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ImpactArgs {
    /// Project directory.
    pub path: String,
    /// Git revision to diff against (e.g. "origin/main", "HEAD~1"), or "index".
    pub base: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TriageArgs {
    /// Project directory.
    pub path: String,
    /// Run id; the latest run when omitted.
    pub run_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct RunIdArgs {
    /// Id returned by qa_run.
    pub run_id: String,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ReportArgs {
    /// Project directory.
    pub path: String,
    /// Run id; the latest run when omitted.
    pub run_id: Option<String>,
    /// "summary" (default) or "markdown".
    pub format: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ListArgs {
    /// Project directory.
    pub path: String,
    /// Maximum runs to return (default 10, max 100).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct ArtifactArgs {
    /// Project directory.
    pub path: String,
    pub run_id: String,
    /// Artifact name as listed in the run (e.g. "python.log", "pytest.junit.xml", "report.md").
    pub name: String,
    /// Return at most this many bytes from the end of the file (default 20000, max 200000).
    pub max_bytes: Option<usize>,
}

fn ok_json(v: Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string_pretty(&v).unwrap_or_default(),
    )]))
}

fn tool_error(msg: impl Into<String>) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::error(vec![ContentBlock::text(msg.into())]))
}

/// Small, model-friendly view of a manifest (full detail stays in artifacts).
pub fn summarize(m: &RunManifest) -> Value {
    let failures: Vec<Value> = m
        .runners
        .iter()
        .flat_map(|r| r.tests.iter().map(move |t| (r, t)))
        .filter(|(_, t)| matches!(t.status, Status::Failed | Status::Error))
        .take(MAX_LISTED_FAILURES)
        .map(|(r, t)| {
            json!({
                "ecosystem": r.ecosystem.as_str(),
                "suite": t.suite,
                "name": t.name,
                "status": t.status.as_str(),
                "message": t.message.as_deref().map(|m| m.lines().take(6).collect::<Vec<_>>().join("\n")),
            })
        })
        .collect();
    json!({
        "run_id": m.run_id,
        "status": m.status.as_str(),
        "counts": m.counts,
        "isolation": m.isolation.as_str(),
        "runners": m.runners.iter().map(|r| json!({
            "ecosystem": r.ecosystem.as_str(),
            "tool": r.tool.as_str(),
            "status": r.status.as_str(),
            "evidence": if r.detail == qualyx_core::Detail::PerTest { "per-test" } else { "exit-code-only" },
            "exit_code": r.exit_code,
            "counts": r.counts,
            "artifacts": r.artifacts,
            "notes": r.notes,
        })).collect::<Vec<_>>(),
        "failures": failures,
    })
}

fn canonical(p: &Path) -> std::io::Result<PathBuf> {
    p.canonicalize()
}

impl QualyxServer {
    pub fn new(roots: Vec<PathBuf>) -> std::io::Result<Self> {
        let roots: Vec<PathBuf> = roots
            .iter()
            .map(|r| canonical(r))
            .collect::<Result<_, _>>()?;
        let audit = roots.first().and_then(|r| {
            let dir = r.join(qualyx_store::DIR);
            qualyx_store::Layout { dir: dir.clone() }.ensure().ok()?;
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("mcp-audit.jsonl"))
                .ok()
        });
        Ok(QualyxServer {
            roots: Arc::new(roots),
            runs: Arc::new(Mutex::new(HashMap::new())),
            audit: Arc::new(Mutex::new(audit)),
            tool_router: Self::tool_router()
                + Self::intel_router()
                + Self::ui_router()
                + Self::assurance_router(),
        })
    }

    fn audit(&self, tool: &str, args: &impl Serialize, decision: &str, detail: &str) {
        if let Ok(mut guard) = self.audit.lock()
            && let Some(f) = guard.as_mut()
        {
            let line = json!({
                "ts": chrono::Utc::now().to_rfc3339(),
                "tool": tool,
                "args": args,
                "decision": decision,
                "detail": detail,
            });
            let _ = writeln!(f, "{line}");
        }
    }

    /// Resolve a client path and enforce the root allowlist.
    fn allowed(&self, tool: &str, args: &impl Serialize, path: &str) -> Result<PathBuf, String> {
        let resolved =
            canonical(Path::new(path)).map_err(|e| format!("cannot access `{path}`: {e}"));
        let decision = resolved.and_then(|p| {
            if self.roots.iter().any(|r| p.starts_with(r)) {
                Ok(p)
            } else {
                Err(format!("`{path}` is outside the allowed roots"))
            }
        });
        match &decision {
            Ok(_) => self.audit(tool, args, "allowed", ""),
            Err(e) => self.audit(tool, args, "denied", e),
        }
        decision
    }

    fn running_count(&self) -> usize {
        self.runs
            .lock()
            .map(|r| {
                r.values()
                    .filter(|e| matches!(e.state, RunState::Running))
                    .count()
            })
            .unwrap_or(0)
    }
}

#[tool_router]
impl QualyxServer {
    #[tool(
        description = "What this Qualyx server can do: supported ecosystems, isolation modes, allowed roots and installed container engine.",
        annotations(read_only_hint = true)
    )]
    async fn qa_capabilities(&self) -> Result<CallToolResult, McpError> {
        ok_json(json!({
            "qualyx_version": env!("CARGO_PKG_VERSION"),
            "ecosystems": [
                {"ecosystem": "rust", "tool": "cargo-test", "evidence": "per-test"},
                {"ecosystem": "dotnet", "tool": "dotnet-test", "evidence": "per-test (TRX)"},
                {"ecosystem": "node", "tool": "node-test", "evidence": "per-test (JUnit) when the test script is `node --test`, otherwise exit-code-only"},
                {"ecosystem": "python", "tool": "pytest", "evidence": "per-test (JUnit)"},
            ],
            "isolation": ["process", "container"],
            "container_engine": qualyx_runners::container::engine().map(|p| p.display().to_string()),
            "max_concurrent_runs": MAX_CONCURRENT_RUNS,
            "roots": self.roots.iter().map(|r| r.display().to_string()).collect::<Vec<_>>(),
        }))
    }

    #[tool(
        description = "Detect a project's ecosystems and the native test tool Qualyx will use for each.",
        annotations(read_only_hint = true)
    )]
    async fn qa_discover(
        &self,
        Parameters(args): Parameters<PathArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_discover", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        match qualyx_app::discover(&path) {
            Ok(p) => ok_json(serde_json::to_value(p).unwrap_or_default()),
            Err(e) => tool_error(format!("{e:#}")),
        }
    }

    #[tool(
        description = "Installed toolchain versions and which ones the project needs.",
        annotations(read_only_hint = true)
    )]
    async fn qa_doctor(
        &self,
        Parameters(args): Parameters<PathArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_doctor", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        match tokio::task::spawn_blocking(move || qualyx_app::doctor(&path)).await {
            Ok(Ok((profile, tools))) => ok_json(json!({"root": profile.root, "tools": tools})),
            Ok(Err(e)) => tool_error(format!("{e:#}")),
            Err(e) => tool_error(e.to_string()),
        }
    }

    #[tool(
        description = "Start running the project's tests. Returns immediately with a run_id; poll qa_run_status. Executes the project's own test code.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn qa_run(
        &self,
        Parameters(args): Parameters<RunArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_run", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        let isolation = match args.isolation.as_deref() {
            None | Some("process") => Isolation::Process,
            Some("container") => Isolation::Container,
            Some(other) => {
                return tool_error(format!(
                    "unknown isolation `{other}` (use process or container)"
                ));
            }
        };
        if self.running_count() >= MAX_CONCURRENT_RUNS {
            return tool_error(format!(
                "{MAX_CONCURRENT_RUNS} runs already in progress; wait or qa_cancel one"
            ));
        }
        let cancel = CancelToken::default();
        let req = TestRequest {
            timeout_secs: args.timeout_secs,
            isolation: Some(isolation),
            changed_since: args.changed_since.clone(),
            jobs: args.jobs.unwrap_or(1).clamp(1, 8),
        };
        let prepared = match qualyx_app::prepare(&path, &req, cancel.clone()) {
            Ok(p) => p,
            Err(e) => return tool_error(format!("{e:#}")),
        };
        let run_id = prepared.run_id.clone();
        let ecosystems: Vec<&str> = prepared
            .profile()
            .ecosystems
            .iter()
            .map(|d| d.ecosystem.as_str())
            .collect();
        let response = json!({"run_id": run_id, "state": "running", "ecosystems": ecosystems});
        self.runs.lock().expect("runs lock").insert(
            run_id.clone(),
            RunEntry {
                project: path,
                cancel,
                state: RunState::Running,
            },
        );
        let runs = self.runs.clone();
        std::thread::spawn(move || {
            let state = match prepared.execute(|_| {}) {
                Ok(m) => RunState::Finished(Box::new(m)),
                Err(e) => RunState::Failed(format!("{e:#}")),
            };
            if let Ok(mut r) = runs.lock()
                && let Some(entry) = r.get_mut(&run_id)
            {
                entry.state = state;
            }
        });
        ok_json(response)
    }

    #[tool(
        description = "State of a run started by qa_run: 'running', or 'finished' with status, counts and the first failures.",
        annotations(read_only_hint = true)
    )]
    async fn qa_run_status(
        &self,
        Parameters(args): Parameters<RunIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        let runs = self.runs.lock().expect("runs lock");
        let Some(entry) = runs.get(&args.run_id) else {
            return tool_error(format!(
                "unknown run `{}` (only runs started by this server are tracked; use qa_report for older ones)",
                args.run_id
            ));
        };
        match &entry.state {
            RunState::Running => ok_json(json!({
                "run_id": args.run_id,
                "state": "running",
                "cancel_requested": entry.cancel.is_cancelled(),
            })),
            RunState::Finished(m) => {
                let mut v = summarize(m);
                v["state"] = json!("finished");
                v["project"] = json!(entry.project);
                ok_json(v)
            }
            RunState::Failed(e) => {
                ok_json(json!({"run_id": args.run_id, "state": "failed", "error": e}))
            }
        }
    }

    #[tool(
        description = "Cancel a running qa_run. The process tree (or container) is killed and the run is recorded as error.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true
        )
    )]
    async fn qa_cancel(
        &self,
        Parameters(args): Parameters<RunIdArgs>,
    ) -> Result<CallToolResult, McpError> {
        let runs = self.runs.lock().expect("runs lock");
        let Some(entry) = runs.get(&args.run_id) else {
            return tool_error(format!("unknown run `{}`", args.run_id));
        };
        self.audit("qa_cancel", &args, "allowed", "");
        entry.cancel.cancel();
        let state = if matches!(entry.state, RunState::Running) {
            "cancelling"
        } else {
            "already-finished"
        };
        ok_json(json!({"run_id": args.run_id, "state": state}))
    }

    #[tool(
        description = "Report of a stored run (latest by default): 'summary' JSON or the full 'markdown' report.",
        annotations(read_only_hint = true)
    )]
    async fn qa_report(
        &self,
        Parameters(args): Parameters<ReportArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_report", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        match qualyx_app::load_run(&path, args.run_id.as_deref()) {
            Ok((m, _)) => match args.format.as_deref() {
                None | Some("summary") => ok_json(summarize(&m)),
                Some("markdown") => Ok(CallToolResult::success(vec![ContentBlock::text(
                    qualyx_app::render::markdown(&m),
                )])),
                Some(f) => tool_error(format!("unknown format `{f}` (use summary or markdown)")),
            },
            Err(e) => tool_error(format!("{e:#}")),
        }
    }

    #[tool(
        description = "Recent runs of a project, newest first.",
        annotations(read_only_hint = true)
    )]
    async fn qa_list_runs(
        &self,
        Parameters(args): Parameters<ListArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_list_runs", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        match qualyx_app::list_runs(&path, args.limit.unwrap_or(10).min(100)) {
            Ok(runs) => ok_json(json!(
                runs.iter()
                    .map(|r| json!({
                        "run_id": r.id, "status": r.status.as_str(), "total": r.total,
                        "passed": r.passed, "failed": r.failed, "started_at": r.started_at,
                    }))
                    .collect::<Vec<_>>()
            )),
            Err(e) => tool_error(format!("{e:#}")),
        }
    }

    #[tool(
        description = "Tail of one artifact of a run (logs, JUnit/TRX reports, report.md, manifest.json). Only files listed in the run are readable.",
        annotations(read_only_hint = true)
    )]
    async fn qa_get_artifact(
        &self,
        Parameters(args): Parameters<ArtifactArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_get_artifact", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        let max = args
            .max_bytes
            .unwrap_or(DEFAULT_ARTIFACT_BYTES)
            .min(MAX_ARTIFACT_BYTES);
        match qualyx_app::read_artifact(&path, &args.run_id, &args.name, max) {
            Ok(text) => Ok(CallToolResult::success(vec![ContentBlock::text(text)])),
            Err(e) => tool_error(format!("{e:#}")),
        }
    }

    #[tool(
        description = "Check that a run's evidence files still match the SHA-256 recorded in its manifest.",
        annotations(read_only_hint = true)
    )]
    async fn qa_verify(
        &self,
        Parameters(args): Parameters<ReportArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_verify", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        match qualyx_app::load_run(&path, args.run_id.as_deref()) {
            Ok((m, dir)) => {
                let modified = qualyx_runners::verify_artifacts(&m, &dir);
                ok_json(
                    json!({"run_id": m.run_id, "checked": m.artifact_sha256.len(), "intact": modified.is_empty(), "modified_or_missing": modified}),
                )
            }
            Err(e) => tool_error(format!("{e:#}")),
        }
    }
}

#[tool_router(router = intel_router)]
impl QualyxServer {
    #[tool(
        description = "Which tests a change affects: changed files since a git revision (or the content index) mapped to ecosystems and, for Rust workspaces, packages plus their dependents. Falls back to the whole ecosystem when unsure.",
        annotations(read_only_hint = true)
    )]
    async fn qa_test_impact(
        &self,
        Parameters(args): Parameters<ImpactArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_test_impact", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        let base = args.base.clone();
        let r =
            tokio::task::spawn_blocking(move || -> anyhow::Result<qualyx_impact::ImpactPlan> {
                let profile = qualyx_app::discover(&path)?;
                qualyx_app::impact(&profile, &base)
            })
            .await;
        match r {
            Ok(Ok(plan)) => ok_json(serde_json::to_value(plan).unwrap_or_default()),
            Ok(Err(e)) => tool_error(format!("{e:#}")),
            Err(e) => tool_error(e.to_string()),
        }
    }

    #[tool(
        description = "Deterministic triage of a run's failures: groups by signature, category, source locations and the exact command to re-run each failing test alone.",
        annotations(read_only_hint = true)
    )]
    async fn qa_analyze_failure(
        &self,
        Parameters(args): Parameters<TriageArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_analyze_failure", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        match qualyx_app::triage(&path, args.run_id.as_deref(), false) {
            Ok(t) => ok_json(serde_json::to_value(t).unwrap_or_default()),
            Err(e) => tool_error(format!("{e:#}")),
        }
    }
}

#[tool_router(router = ui_router)]
impl QualyxServer {
    #[tool(
        description = "Run a JSON browser flow (goto/fill/click/expect_text/wait_for/screenshot) in installed browsers via CDP, WebDriver BiDi or WebDriver. Only loopback URLs and origins listed in [browser] allowed_origins are visited. Returns per-step results, console, failed requests and screenshot paths.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn qa_browser_session(
        &self,
        Parameters(args): Parameters<BrowseArgs>,
    ) -> Result<CallToolResult, McpError> {
        let project = match self.allowed("qa_browser_session", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        let flow = match self.allowed("qa_browser_session", &args, &args.flow_path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        let serve = match &args.serve_dir {
            Some(d) => match self.allowed("qa_browser_session", &args, d) {
                Ok(p) => Some(p),
                Err(e) => return tool_error(e),
            },
            None => None,
        };
        let req = qualyx_app::browser::BrowseRequest {
            flow_file: flow,
            browsers: args.browsers.clone().unwrap_or_default(),
            serve_dir: serve,
            base_url: None,
            headed: false,
            restrict_origins: true,
        };
        match tokio::task::spawn_blocking(move || qualyx_app::browser::browse(&project, &req)).await
        {
            Ok(Ok(r)) => ok_json(serde_json::to_value(r).unwrap_or_default()),
            Ok(Err(e)) => tool_error(format!("{e:#}")),
            Err(e) => tool_error(e.to_string()),
        }
    }

    #[tool(
        description = "Host desktop, read-only: 'probe' reports session type and accessibility/screenshot availability; 'snapshot' lists top-level windows from the OS accessibility API. Never sends input.",
        annotations(read_only_hint = true)
    )]
    async fn qa_desktop_session(
        &self,
        Parameters(args): Parameters<DesktopArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.audit("qa_desktop_session", &args, "allowed", "");
        let action = args.action.clone();
        let r = tokio::task::spawn_blocking(move || match action.as_str() {
            "probe" => Ok(serde_json::to_value(qualyx_desktop::probe()).unwrap_or_default()),
            "snapshot" => {
                qualyx_desktop::snapshot().map(|w| serde_json::to_value(w).unwrap_or_default())
            }
            other => Err(format!("unknown action `{other}` (use probe or snapshot)")),
        })
        .await;
        match r {
            Ok(Ok(v)) => ok_json(v),
            Ok(Err(e)) => tool_error(e),
            Err(e) => tool_error(e.to_string()),
        }
    }
}

#[tool_router(router = assurance_router)]
impl QualyxServer {
    #[tool(
        description = "Scan a project for secrets (Gitleaks, redacted), code issues (Semgrep) and vulnerable dependencies (Trivy, OSV-Scanner). Returns normalized findings; full SARIF stays on disk.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn qa_security_scan(
        &self,
        Parameters(args): Parameters<SecurityArgs>,
    ) -> Result<CallToolResult, McpError> {
        let path = match self.allowed("qa_security_scan", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        let mut list = Vec::new();
        for s in args.scanners.clone().unwrap_or_default() {
            match qualyx_security::Scanner::parse(&s) {
                Some(x) => list.push(x),
                None => return tool_error(format!("unknown scanner `{s}`")),
            }
        }
        match tokio::task::spawn_blocking(move || {
            qualyx_app::assurance::security_scan(&path, &list)
        })
        .await
        {
            Ok(Ok((mut r, dir))) => {
                let total = r.findings.len();
                r.findings.truncate(50);
                ok_json(json!({"report": r, "total_findings": total, "evidence": dir}))
            }
            Ok(Err(e)) => tool_error(format!("{e:#}")),
            Err(e) => tool_error(e.to_string()),
        }
    }

    #[tool(
        description = "Run an authorized k6 workload (loopback or [performance] allowed_targets, within the configured VU/RPS/duration ceilings, at most 120 s per call) and compare with the baseline when one exists.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn qa_performance_run(
        &self,
        Parameters(args): Parameters<PerfArgs>,
    ) -> Result<CallToolResult, McpError> {
        let project = match self.allowed("qa_performance_run", &args, &args.path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        let workload = match self.allowed("qa_performance_run", &args, &args.workload_path) {
            Ok(p) => p,
            Err(e) => return tool_error(e),
        };
        let repeat = args.repeat.unwrap_or(2).clamp(1, 3);
        let mark = args.baseline.unwrap_or(false);
        let r = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
            let (run, dir) =
                qualyx_app::assurance::perf_run(&project, &workload, repeat, mark, Some(120))?;
            let cmp = if mark {
                None
            } else {
                qualyx_app::assurance::perf_compare(
                    &project,
                    &run.workload.name,
                    None,
                    Some(&run.run_id),
                )
                .ok()
            };
            Ok(json!({"run": run, "comparison": cmp, "evidence": dir}))
        })
        .await;
        match r {
            Ok(Ok(v)) => ok_json(v),
            Ok(Err(e)) => tool_error(format!("{e:#}")),
            Err(e) => tool_error(e.to_string()),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for QualyxServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(INSTRUCTIONS)
    }
}

/// Serve on stdin/stdout until the client disconnects.
pub fn serve_stdio(roots: Vec<PathBuf>) -> anyhow::Result<()> {
    let server = QualyxServer::new(roots)?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let service = server.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        Ok(())
    })
}
