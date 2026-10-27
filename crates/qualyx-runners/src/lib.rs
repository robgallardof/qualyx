//! Runs the native test tools of a discovered project and builds the run manifest.
//!
//! The pipeline is a library (not CLI code) so the MCP server can reuse it (ADR 0003).

pub mod cargo_text;
pub mod process;
pub mod report;
pub mod tools;

use std::fs;
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use qualyx_core::{
    Counts, DetectedEcosystem, GitInfo, MANIFEST_SCHEMA_VERSION, ProjectProfile, RunManifest,
    RunnerResult, Status,
};

pub use tools::runner_for;

#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Per-runner wall-clock limit.
    pub timeout: Duration,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            timeout: Duration::from_secs(30 * 60),
        }
    }
}

/// Run one ecosystem's tests. Never fails: problems become an `Error` result with notes.
pub fn execute(
    detected: &DetectedEcosystem,
    root: &Path,
    run_dir: &Path,
    opts: &RunOptions,
) -> RunnerResult {
    let runner = runner_for(detected.tool);
    let inv = runner.plan(detected, run_dir);
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

    let Some(program) = process::resolve(inv.program) else {
        let wanted = inv.program.join(" or ");
        let command = std::iter::once(inv.program[0].to_string())
            .chain(inv.args.clone())
            .collect();
        return failed(command, notes, format!("`{wanted}` was not found on PATH"));
    };
    let command: Vec<String> = std::iter::once(program.to_string_lossy().into_owned())
        .chain(inv.args.iter().cloned())
        .collect();
    let tool_version = process::version(&program, inv.version_args, root);

    let outcome = match process::run(
        &program,
        &inv.args,
        root,
        &run_dir.join(&log_name),
        opts.timeout,
    ) {
        Ok(o) => o,
        Err(e) => return failed(command, notes, format!("could not start the process: {e}")),
    };

    let log = fs::read(run_dir.join(&log_name))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let collected = runner.collect(&log, run_dir);
    notes.extend(collected.notes);
    let status = tools::grade(
        inv.detail,
        &collected.tests,
        outcome.exit_code,
        outcome.timed_out,
        &mut notes,
    );

    let mut artifacts = vec![log_name];
    artifacts.extend(collected.artifacts);
    let duration_ms = (outcome.finished_at - outcome.started_at)
        .num_milliseconds()
        .max(0) as u64;
    RunnerResult {
        ecosystem: detected.ecosystem,
        tool: detected.tool,
        tool_version,
        command,
        exit_code: outcome.exit_code,
        timed_out: outcome.timed_out,
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
    let runners: Vec<RunnerResult> = profile
        .ecosystems
        .iter()
        .map(|d| {
            on_runner(d);
            execute(d, &profile.root, run_dir, opts)
        })
        .collect();

    let mut counts = Counts::default();
    for r in &runners {
        counts.add(r.counts);
    }
    Ok(RunManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        run_id: run_id.to_string(),
        qualyx_version: env!("CARGO_PKG_VERSION").to_string(),
        project_root: profile.root.clone(),
        git,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        started_at,
        finished_at: Utc::now(),
        status: Status::worst(runners.iter().map(|r| r.status)),
        counts,
        runners,
    })
}
