//! Child process execution: no shell, stdin closed, stdout+stderr into one log, hard timeout.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use chrono::{DateTime, Utc};
use wait_timeout::ChildExt;

#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}

/// Resolve the first available program on `PATH` (honours `PATHEXT` on Windows, so `npm`
/// finds `npm.cmd`).
pub fn resolve(candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().find_map(|c| which::which(c).ok())
}

fn command(program: &Path, args: &[String], cwd: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0");
    cmd
}

/// Run to completion (or timeout), writing combined output to `log`.
///
/// Known M1 limitation: on timeout only the direct child is killed; grandchildren spawned
/// through wrappers such as `npm.cmd` may survive. Process-tree kill is an M2 item.
pub fn run(
    program: &Path,
    args: &[String],
    cwd: &Path,
    log: &Path,
    timeout: Duration,
) -> io::Result<ProcessOutcome> {
    let out = File::create(log)?;
    let err = out.try_clone()?;
    let started_at = Utc::now();
    let mut child = command(program, args, cwd)
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()?;
    let (exit_code, timed_out) = match child.wait_timeout(timeout)? {
        Some(status) => (status.code(), false),
        None => {
            child.kill()?;
            child.wait()?;
            (None, true)
        }
    };
    Ok(ProcessOutcome {
        exit_code,
        timed_out,
        started_at,
        finished_at: Utc::now(),
    })
}

/// First non-empty line of `<program> <args>`, used to record tool versions.
pub fn version(program: &Path, args: &[&str], cwd: &Path) -> Option<String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let output = command(program, &args, cwd).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}
