//! Runs the native test tools of a discovered project and builds the run manifest.
//!
//! The pipeline is a library (not CLI code) so the MCP server can reuse it (ADR 0003).

pub mod cargo_text;
pub mod compose;
pub mod container;
pub mod process;
pub mod redact;
pub mod report;
pub mod tools;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use qualyx_core::{
    Counts, DetectedEcosystem, GitInfo, Isolation, MANIFEST_SCHEMA_VERSION, ProjectProfile,
    RunManifest, RunnerResult, Status,
};
use sha2::{Digest, Sha256};

/// Lines of the tool log copied into the notes when a runner errors without test results.
const LOG_TAIL_LINES: usize = 15;

pub use process::CancelToken;
pub use tools::runner_for;

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Per-runner wall-clock limit.
    pub timeout: Duration,
    pub isolation: Isolation,
    /// Extra environment variable names a project explicitly lets through (config).
    pub env_passthrough: Vec<String>,
    /// Variables set by Qualyx itself for the tests (e.g. Compose service addresses).
    pub extra_env: Vec<(String, String)>,
    pub cancel: CancelToken,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            timeout: Duration::from_secs(30 * 60),
            isolation: Isolation::Process,
            env_passthrough: Vec::new(),
            extra_env: Vec::new(),
            cancel: CancelToken::default(),
        }
    }
}

/// Run one ecosystem's tests. Never fails: problems become an `Error` result with notes.
pub fn execute(
    detected: &DetectedEcosystem,
    root: &Path,
    run_id: &str,
    run_dir: &Path,
    opts: &RunOptions,
) -> RunnerResult {
    let runner = runner_for(detected.tool);
    let in_container = opts.isolation == Isolation::Container;
    let inv = if in_container {
        runner.plan(detected, &|f| format!("{}/{f}", container::OUT))
    } else {
        runner.plan(detected, &|f| {
            run_dir.join(f).to_string_lossy().into_owned()
        })
    };
    let log_name = format!("{}.log", detected.ecosystem.as_str());
    let mut notes = detected.notes.clone();
    let now = Utc::now();

    let failed = |command: Vec<String>, mut notes: Vec<String>, why: String| {
        notes.push(why);
        RunnerResult {
            ecosystem: detected.ecosystem,
            tool: detected.tool,
            tool_version: None,
            command,
            exit_code: None,
            timed_out: false,
            cancelled: false,
            started_at: now,
            finished_at: Utc::now(),
            duration_ms: 0,
            status: Status::Error,
            detail: inv.detail,
            counts: Counts::default(),
            tests: Vec::new(),
            artifacts: Vec::new(),
            notes,
        }
    };

    // Build the concrete command for the chosen isolation.
    let mut env = process::child_env(&opts.env_passthrough);
    env.extend(opts.extra_env.iter().map(|(k, v)| (k.into(), v.into())));
    let mut _workspace: Option<tempfile::TempDir> = None;
    let mut container_cleanup: Option<(std::path::PathBuf, String)> = None;
    let (command_line, cmd, tool_version) = if in_container {
        let Some(engine) = container::engine() else {
            return failed(
                vec!["docker".into()],
                notes,
                "container isolation requested but neither docker nor podman is on PATH".into(),
            );
        };
        let ws = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => {
                return failed(
                    vec![],
                    notes,
                    format!("cannot create container workspace: {e}"),
                );
            }
        };
        if let Err(e) = container::copy_project(root, ws.path()) {
            return failed(
                vec![],
                notes,
                format!("cannot copy project for the container: {e}"),
            );
        }
        let name = container::container_name(run_id, detected.ecosystem);
        let mut args = container::run_args(
            &name,
            detected.ecosystem,
            ws.path(),
            run_dir,
            inv.program[0],
            &inv.args,
        );
        for (k, v) in &inv.env {
            // Container paths are already mapped by `plan`.
            let pos = args.iter().position(|a| a == "-w").unwrap_or(0);
            args.insert(pos, format!("{k}={v}"));
            args.insert(pos, "-e".into());
        }
        let line = std::iter::once(engine.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .collect();
        let cmd = process::command(&engine, &args, root, &env);
        _workspace = Some(ws);
        container_cleanup = Some((engine, name));
        notes.push(format!(
            "ran in container image {}",
            container::image(detected.ecosystem)
        ));
        (
            line,
            cmd,
            Some(format!(
                "container:{}",
                container::image(detected.ecosystem)
            )),
        )
    } else {
        let Some(program) = process::resolve(inv.program) else {
            let wanted = inv.program.join(" or ");
            let command = std::iter::once(inv.program[0].to_string())
                .chain(inv.args.clone())
                .collect();
            return failed(command, notes, format!("`{wanted}` was not found on PATH"));
        };
        let line = std::iter::once(program.to_string_lossy().into_owned())
            .chain(inv.args.iter().cloned())
            .collect();
        let version = process::version(&program, inv.version_args, root);
        let mut cmd = process::command(&program, &inv.args, root, &env);
        cmd.envs(inv.env.iter().map(|(k, v)| (k, v)));
        (line, cmd, version)
    };

    let log_path = run_dir.join(&log_name);
    let outcome = match process::run(cmd, &log_path, opts.timeout, &opts.cancel) {
        Ok(o) => o,
        Err(e) => {
            return failed(
                command_line,
                notes,
                format!("could not start the process: {e}"),
            );
        }
    };
    if let (Some((engine, name)), true) =
        (&container_cleanup, outcome.cancelled || outcome.timed_out)
    {
        container::force_remove(engine, name);
    }
    if let Ok(true) = redact::redact_file(&log_path) {
        notes.push("secrets were masked in the log".into());
    }

    let log = fs::read(&log_path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let mut collected = runner.collect(&log, run_dir);
    for t in &mut collected.tests {
        if let Some(m) = t.message.as_mut() {
            *m = redact::redact(m);
        }
    }
    notes.extend(collected.notes);
    let status = if outcome.cancelled {
        notes.push("cancelled on request".into());
        Status::Error
    } else {
        tools::grade(
            inv.detail,
            &collected.tests,
            outcome.exit_code,
            outcome.timed_out,
            &mut notes,
        )
    };
    // When the tool died before reporting tests, the log tail is the only explanation.
    if status == Status::Error && collected.tests.is_empty() {
        let tail: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = tail[tail.len().saturating_sub(LOG_TAIL_LINES)..].join("\n");
        if !tail.is_empty() {
            notes.push(format!("log tail:\n{}", redact::redact(&tail)));
        }
    }

    let mut artifacts = vec![log_name];
    artifacts.extend(collected.artifacts);
    let duration_ms = (outcome.finished_at - outcome.started_at)
        .num_milliseconds()
        .max(0) as u64;
    RunnerResult {
        ecosystem: detected.ecosystem,
        tool: detected.tool,
        tool_version,
        command: command_line,
        exit_code: outcome.exit_code,
        timed_out: outcome.timed_out,
        cancelled: outcome.cancelled,
        started_at: outcome.started_at,
        finished_at: outcome.finished_at,
        duration_ms,
        status,
        detail: inv.detail,
        counts: Counts::from_tests(&collected.tests),
        tests: collected.tests,
        artifacts,
        notes,
    }
}

/// Commit and dirtiness of the repository containing `root`, if it is one.
pub fn git_info(root: &Path) -> Option<GitInfo> {
    let git = process::resolve(&["git"])?;
    let commit = process::version(&git, &["rev-parse", "HEAD"], root)?;
    let status = std::process::Command::new(&git)
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .ok()?;
    Some(GitInfo {
        commit,
        dirty: !status.stdout.is_empty(),
    })
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Run every detected ecosystem in order and assemble the manifest.
///
/// `on_runner` is called before each runner starts (progress reporting).
pub fn run(
    profile: &ProjectProfile,
    run_id: &str,
    run_dir: &Path,
    opts: &RunOptions,
    mut on_runner: impl FnMut(&DetectedEcosystem),
) -> std::io::Result<RunManifest> {
    fs::create_dir_all(run_dir)?;
    let started_at = Utc::now();
    let git = git_info(&profile.root);
    let mut runners = Vec::new();
    for d in &profile.ecosystems {
        if opts.cancel.is_cancelled() {
            break;
        }
        on_runner(d);
        runners.push(execute(d, &profile.root, run_id, run_dir, opts));
    }
    Ok(assemble(
        profile,
        run_id,
        run_dir,
        opts.isolation,
        started_at,
        git,
        runners,
    ))
}

/// Build the manifest from finished runner results (also used by the parallel scheduler).
pub fn assemble(
    profile: &ProjectProfile,
    run_id: &str,
    run_dir: &Path,
    isolation: Isolation,
    started_at: chrono::DateTime<Utc>,
    git: Option<GitInfo>,
    runners: Vec<RunnerResult>,
) -> RunManifest {
    let mut counts = Counts::default();
    let mut artifact_sha256 = BTreeMap::new();
    for r in &runners {
        counts.add(r.counts);
        for a in &r.artifacts {
            if let Ok(h) = sha256_file(&run_dir.join(a)) {
                artifact_sha256.insert(a.clone(), h);
            }
        }
    }
    // A run that skipped ecosystems because of cancellation is not complete evidence.
    let mut status = Status::worst(runners.iter().map(|r| r.status));
    if runners.len() < profile.ecosystems.len() {
        status = Status::worst([status, Status::Error]);
    }
    RunManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        run_id: run_id.to_string(),
        qualyx_version: env!("CARGO_PKG_VERSION").to_string(),
        project_root: profile.root.clone(),
        git,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        isolation,
        selection: None,
        services: Vec::new(),
        started_at,
        finished_at: Utc::now(),
        status,
        counts,
        runners,
        artifact_sha256,
    }
}

/// Recompute artifact hashes and list every artifact whose content no longer matches.
pub fn verify_artifacts(manifest: &RunManifest, run_dir: &Path) -> Vec<String> {
    manifest
        .artifact_sha256
        .iter()
        .filter(|(name, hash)| sha256_file(&run_dir.join(name)).ok().as_ref() != Some(*hash))
        .map(|(name, _)| name.clone())
        .collect()
}
