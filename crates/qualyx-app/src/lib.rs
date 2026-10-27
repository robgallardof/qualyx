//! Application services shared by every front end (CLI, MCP, HTTP API).
//!
//! Front ends parse input and format output; everything else (config, running, persisting,
//! reading evidence back) lives here so the behaviour is identical whichever door is used.

pub mod assurance;
pub mod browser;
pub mod quality;
pub mod render;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use qualyx_core::{Ecosystem, Isolation, ProjectProfile, RunManifest};
use qualyx_runners::{CancelToken, RunOptions, process};
use qualyx_store::{Layout, RunSummary, Store};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub run: RunConfig,
    /// Optional model provider for LLM-assisted roles (triage hypotheses). Off when absent.
    pub llm: Option<qualyx_llm::ProviderConfig>,
}

#[derive(Deserialize)]
#[serde(default)]
pub struct RunConfig {
    pub timeout_secs: u64,
    pub isolation: Isolation,
    /// Extra environment variable names the tests need (they're filtered out by default).
    pub env_passthrough: Vec<String>,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig {
            timeout_secs: RunOptions::default().timeout.as_secs(),
            isolation: Isolation::Process,
            env_passthrough: Vec::new(),
        }
    }
}

pub const DEFAULT_CONFIG: &str = "# Qualyx project settings.

[run]
# Per-runner wall-clock limit in seconds.
timeout_secs = 1800
# \"process\" (local subprocess) or \"container\" (docker/podman over a copy of the project).
isolation = \"process\"
# Environment variables are filtered for test processes. List extra names to pass through.
env_passthrough = []

# Optional LLM for model-assisted roles. Qualyx works fully without it.
# [llm]
# kind = \"openai-compatible\"          # or \"anthropic\"
# base_url = \"https://api.deepseek.com\" # Ollama: http://localhost:11434/v1
# model = \"<model name>\"
# api_key_env = \"DEEPSEEK_API_KEY\"       # omit for local servers
";

pub fn load_config(layout: &Layout) -> Result<Config> {
    match fs::read_to_string(layout.config()) {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("invalid {}", layout.config().display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn discover(path: &Path) -> Result<ProjectProfile> {
    qualyx_discovery::discover(path)
        .with_context(|| format!("cannot read project at {}", path.display()))
}

pub fn init(path: &Path) -> Result<PathBuf> {
    let layout = Layout::new(path);
    layout.ensure()?;
    if !layout.config().exists() {
        fs::write(layout.config(), DEFAULT_CONFIG)?;
    }
    Ok(layout.dir)
}

/// Overrides for one `test` request; `None` falls back to the project config.
#[derive(Debug, Clone, Default)]
pub struct TestRequest {
    pub timeout_secs: Option<u64>,
    pub isolation: Option<Isolation>,
    /// Run only what changed since this git revision, or since the last `qualyx index` when
    /// the value is `"index"`.
    pub changed_since: Option<String>,
    /// Ecosystems run in parallel up to this many at once (0 or 1: sequential).
    pub jobs: usize,
    /// Compose file whose services are started for the run and removed afterwards.
    pub compose: Option<PathBuf>,
}

/// A run that has an id and a directory but hasn't started yet, so callers (MCP) can hand the
/// id out before the work begins.
pub struct PreparedRun {
    pub run_id: String,
    pub run_dir: PathBuf,
    profile: ProjectProfile,
    layout: Layout,
    opts: RunOptions,
    jobs: usize,
    selection: Option<String>,
    compose: Option<PathBuf>,
}

pub fn prepare(path: &Path, req: &TestRequest, cancel: CancelToken) -> Result<PreparedRun> {
    let mut profile = discover(path)?;
    let mut selection = None;
    if let Some(base) = &req.changed_since {
        let plan = impact(&profile, base)?;
        selection = Some(selection_summary(&plan));
        profile = plan.apply(&profile);
    }
    let layout = Layout::new(&profile.root);
    layout.ensure()?;
    let config = load_config(&layout)?;
    let run_id = qualyx_store::new_run_id(chrono::Utc::now());
    Ok(PreparedRun {
        run_dir: layout.run_dir(&run_id),
        run_id,
        profile,
        layout,
        opts: RunOptions {
            timeout: Duration::from_secs(req.timeout_secs.unwrap_or(config.run.timeout_secs)),
            isolation: req.isolation.unwrap_or(config.run.isolation),
            env_passthrough: config.run.env_passthrough,
            extra_env: Vec::new(),
            cancel,
        },
        jobs: req.jobs,
        selection,
        compose: req.compose.clone(),
    })
}

/// Changed files and the resulting selection for `profile`.
pub fn impact(profile: &ProjectProfile, base: &str) -> Result<qualyx_impact::ImpactPlan> {
    let (changed, source) = if base == "index" {
        (
            qualyx_impact::changed_since_index(&profile.root),
            "content index".to_string(),
        )
    } else {
        (
            qualyx_impact::changed_since_git(&profile.root, base),
            format!("git {base}"),
        )
    };
    let changed = changed.map_err(|e| anyhow::anyhow!(e))?;
    Ok(qualyx_impact::analyze(profile, changed, &source))
}

fn selection_summary(plan: &qualyx_impact::ImpactPlan) -> String {
    let parts: Vec<String> = plan
        .ecosystems
        .iter()
        .map(|e| {
            let scope = match &e.scope {
                qualyx_impact::Scope::Skip => "skipped".to_string(),
                qualyx_impact::Scope::All => "all".to_string(),
                qualyx_impact::Scope::Packages(p) => format!("packages {}", p.join(", ")),
            };
            format!("{} {scope} ({})", e.ecosystem.as_str(), e.confidence)
        })
        .collect();
    format!(
        "changed since {}: {} file(s); {}",
        plan.source,
        plan.changed_files.len(),
        parts.join("; ")
    )
}

impl PreparedRun {
    pub fn profile(&self) -> &ProjectProfile {
        &self.profile
    }

    /// Execute, then persist `manifest.json`, `report.md` and the SQLite index.
    pub fn execute(
        mut self,
        on_runner: impl FnMut(&qualyx_core::DetectedEcosystem),
    ) -> Result<RunManifest> {
        // The environment lives for the whole run; dropping it tears it down, even on error.
        let mut _services = None;
        let mut service_list = Vec::new();
        if let Some(file) = self.compose.clone() {
            if self.opts.isolation == Isolation::Container {
                bail!(
                    "--compose with container isolation is not supported: service ports are on the host loopback"
                );
            }
            let env = qualyx_runners::compose::ComposeEnv::up(
                &file,
                &self.run_id,
                Duration::from_secs(300),
            )
            .map_err(anyhow::Error::msg)?;
            service_list = env.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
            self.opts.extra_env.extend(env.env.clone());
            _services = Some(env);
        }
        let mut manifest = if self.jobs > 1 && self.profile.ecosystems.len() > 1 {
            self.execute_parallel(on_runner)?
        } else {
            qualyx_runners::run(
                &self.profile,
                &self.run_id,
                &self.run_dir,
                &self.opts,
                on_runner,
            )?
        };
        manifest.selection = self.selection.clone();
        manifest.services = service_list;
        persist(&self.layout, &self.run_dir, &manifest)?;
        Ok(manifest)
    }

    /// One scheduler task per ecosystem; results keep discovery order.
    fn execute_parallel(
        &self,
        mut on_runner: impl FnMut(&qualyx_core::DetectedEcosystem),
    ) -> Result<RunManifest> {
        use std::sync::{Arc, Mutex};
        fs::create_dir_all(&self.run_dir)?;
        let started_at = chrono::Utc::now();
        let git = qualyx_runners::git_info(&self.profile.root);
        let slots: Arc<Mutex<Vec<Option<qualyx_core::RunnerResult>>>> =
            Arc::new(Mutex::new(vec![None; self.profile.ecosystems.len()]));
        let mut dag = qualyx_scheduler::Dag::new();
        for (i, d) in self.profile.ecosystems.iter().enumerate() {
            on_runner(d);
            let (d, root, run_id, run_dir, opts, slots) = (
                d.clone(),
                self.profile.root.clone(),
                self.run_id.clone(),
                self.run_dir.clone(),
                self.opts.clone(),
                slots.clone(),
            );
            dag.add(format!("{i:03}-{}", d.ecosystem.as_str()), &[], move |_| {
                let r = qualyx_runners::execute(&d, &root, &run_id, &run_dir, &opts);
                slots.lock().map_err(|e| e.to_string())?[i] = Some(r);
                Ok(())
            });
        }
        let limits = qualyx_scheduler::Limits {
            max_concurrency: self.jobs,
            budget: None,
        };
        dag.run(&limits, &self.opts.cancel)
            .map_err(|e| anyhow::anyhow!(e))?;
        let runners = slots
            .lock()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .drain(..)
            .flatten()
            .collect();
        Ok(qualyx_runners::assemble(
            &self.profile,
            &self.run_id,
            &self.run_dir,
            self.opts.isolation,
            started_at,
            git,
            runners,
        ))
    }
}

pub fn persist(layout: &Layout, run_dir: &Path, manifest: &RunManifest) -> Result<()> {
    let manifest_path = qualyx_store::write_manifest(run_dir, manifest)?;
    fs::write(run_dir.join("report.md"), render::markdown(manifest))?;
    Store::open(&layout.db())?.insert_run(manifest, &manifest_path)?;
    Ok(())
}

pub fn test(
    path: &Path,
    req: &TestRequest,
    on_runner: impl FnMut(&qualyx_core::DetectedEcosystem),
) -> Result<(RunManifest, PathBuf)> {
    let prepared = prepare(path, req, CancelToken::default())?;
    let dir = prepared.run_dir.clone();
    Ok((prepared.execute(on_runner)?, dir))
}

fn open_store(root: &Path) -> Result<(Layout, Store)> {
    let layout = Layout::new(root);
    if !layout.db().exists() {
        bail!(
            "no runs recorded in {} (run `qualyx test` first)",
            root.display()
        );
    }
    let store = Store::open(&layout.db())?;
    Ok((layout, store))
}

/// Load a stored run (the latest when `run_id` is `None`).
pub fn load_run(path: &Path, run_id: Option<&str>) -> Result<(RunManifest, PathBuf)> {
    let root = discover(path)?.root;
    let (_, store) = open_store(&root)?;
    let id = match run_id {
        Some(id) if qualyx_store::is_valid_run_id(id) => id.to_string(),
        Some(id) => bail!("invalid run id `{id}`"),
        None => store.latest_run_id()?.context("no runs recorded")?,
    };
    let manifest_path = store
        .manifest_path(&id)?
        .with_context(|| format!("unknown run `{id}`"))?;
    let manifest = qualyx_store::read_manifest(&manifest_path)?;
    let run_dir = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    Ok((manifest, run_dir))
}

pub fn list_runs(path: &Path, limit: u32) -> Result<Vec<RunSummary>> {
    let root = discover(path)?.root;
    let (_, store) = open_store(&root)?;
    Ok(store.list_runs(limit)?)
}

/// Read one artifact of a run, refusing names that escape the run directory.
pub fn read_artifact(path: &Path, run_id: &str, name: &str, max_bytes: usize) -> Result<String> {
    let (manifest, run_dir) = load_run(path, Some(run_id))?;
    let known = name == "manifest.json"
        || name == "report.md"
        || manifest
            .runners
            .iter()
            .any(|r| r.artifacts.iter().any(|a| a == name));
    if !known {
        bail!("`{name}` is not an artifact of run {run_id}");
    }
    let bytes = fs::read(run_dir.join(name))?;
    let start = bytes.len().saturating_sub(max_bytes);
    let mut text = String::from_utf8_lossy(&bytes[start..]).into_owned();
    if start > 0 {
        text.insert_str(0, &format!("[… {start} earlier bytes omitted …]\n"));
    }
    Ok(text)
}

/// Snapshot the project's files as the baseline for `changed_since = "index"`.
pub fn index(path: &Path) -> Result<usize> {
    let root = discover(path)?.root;
    Ok(qualyx_impact::write_index(&root)?)
}

#[derive(Debug, Clone, Serialize)]
pub struct TriageReport {
    #[serde(flatten)]
    pub triage: qualyx_triage::Triage,
    /// Model-generated explanation. A hypothesis to check, never evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_hypothesis: Option<String>,
}

const TRIAGE_SYSTEM_PROMPT: &str = "You are a senior test engineer. Using only the failure data \
given, state the most likely root cause of each group and the single next check that would \
confirm it. Be brief. Say 'unknown' when the data is insufficient. Never invent file contents.";

/// Deterministic triage of a run, plus an optional model hypothesis when `[llm]` is configured.
pub fn triage(path: &Path, run_id: Option<&str>, use_llm: bool) -> Result<TriageReport> {
    let (manifest, _) = load_run(path, run_id)?;
    let triage = qualyx_triage::triage(&manifest);
    let llm_hypothesis = if use_llm && !triage.groups.is_empty() {
        let config = load_config(&Layout::new(&manifest.project_root))?;
        let cfg = config
            .llm
            .context("`--llm` needs an [llm] section in .qualyx/config.toml")?;
        let facts = serde_json::to_string_pretty(&triage.groups)?;
        let reply = qualyx_llm::complete(
            &cfg,
            TRIAGE_SYSTEM_PROMPT,
            &qualyx_runners::redact::redact(&facts),
            800,
            false,
        )
        .map_err(|e| anyhow::anyhow!(e))?;
        Some(reply.text)
    } else {
        None
    };
    Ok(TriageReport {
        triage,
        llm_hypothesis,
    })
}

/// Probe the configured `[llm]` endpoint.
pub fn llm_probe(path: &Path) -> Result<(qualyx_llm::ProviderConfig, qualyx_llm::Capabilities)> {
    let root = discover(path)?.root;
    let cfg = load_config(&Layout::new(&root))?
        .llm
        .context("no [llm] section in .qualyx/config.toml")?;
    let caps = qualyx_llm::probe(&cfg);
    Ok((cfg, caps))
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolStatus {
    pub name: &'static str,
    pub required: bool,
    pub version: Option<String>,
}

struct ToolCheck {
    name: &'static str,
    program: &'static [&'static str],
    version_args: &'static [&'static str],
    needed_by: Option<Ecosystem>,
}

const TOOLS: &[ToolCheck] = &[
    ToolCheck {
        name: "git",
        program: &["git"],
        version_args: &["--version"],
        needed_by: None,
    },
    ToolCheck {
        name: "cargo",
        program: &["cargo"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Rust),
    },
    ToolCheck {
        name: "dotnet",
        program: &["dotnet"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Dotnet),
    },
    ToolCheck {
        name: "node",
        program: &["node"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Node),
    },
    ToolCheck {
        name: "npm",
        program: &["npm"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Node),
    },
    ToolCheck {
        name: "python",
        program: &["python", "python3"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Python),
    },
    ToolCheck {
        name: "pytest",
        program: &["python", "python3"],
        version_args: &["-m", "pytest", "--version"],
        needed_by: Some(Ecosystem::Python),
    },
    ToolCheck {
        name: "docker",
        program: &["docker", "podman"],
        version_args: &["--version"],
        needed_by: None,
    },
];

/// Installed tool versions, flagging the ones the project at `path` needs.
pub fn doctor(path: &Path) -> Result<(ProjectProfile, Vec<ToolStatus>)> {
    let profile = discover(path)?;
    let needed: Vec<Ecosystem> = profile.ecosystems.iter().map(|d| d.ecosystem).collect();
    let tools = TOOLS
        .iter()
        .map(|t| ToolStatus {
            name: t.name,
            required: t.needed_by.is_some_and(|e| needed.contains(&e)),
            version: process::resolve(t.program)
                .and_then(|p| process::version(&p, t.version_args, &profile.root)),
        })
        .collect();
    Ok((profile, tools))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let layout = Layout::new(dir.path());
        assert_eq!(
            load_config(&layout).unwrap().run.isolation,
            Isolation::Process
        );
        init(dir.path()).unwrap();
        // The generated file must parse to the defaults.
        let c = load_config(&layout).unwrap();
        assert_eq!(c.run.timeout_secs, 1800);
        fs::write(
            layout.config(),
            "[run]\nisolation = \"container\"\nenv_passthrough = [\"MY_VAR\"]\n",
        )
        .unwrap();
        let c = load_config(&layout).unwrap();
        assert_eq!(c.run.isolation, Isolation::Container);
        assert_eq!(c.run.env_passthrough, ["MY_VAR"]);
        fs::write(layout.config(), "[run]\nisolation = \"vm\"\n").unwrap();
        assert!(load_config(&layout).is_err());
    }
}
