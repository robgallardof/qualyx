//! Child process execution: no shell, stdin closed, stdout+stderr into one log, an allowlisted
//! environment, a hard timeout and cooperative cancellation that kills the whole process tree.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use wait_timeout::ChildExt;

/// Shared flag to stop a run from another thread (MCP `qa_cancel`, Ctrl-C).
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}

/// Resolve the first available program on `PATH` (honours `PATHEXT` on Windows, so `npm`
/// finds `npm.cmd`).
pub fn resolve(candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().find_map(|c| which::which(c).ok())
}

/// Variables a child process may inherit (threat model T4). Names are compared upper-cased.
const ENV_EXACT: &[&str] = &[
    "PATH",
    "PATHEXT",
    "HOME",
    "USER",
    "USERNAME",
    "LOGNAME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "LANG",
    "TZ",
    "TMP",
    "TEMP",
    "TMPDIR",
    "SHELL",
    "TERM",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "COMMONPROGRAMFILES",
    "COMMONPROGRAMFILES(X86)",
    "COMMONPROGRAMW6432",
    "APPDATA",
    "LOCALAPPDATA",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "NUMBER_OF_PROCESSORS",
    "OS",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "VIRTUAL_ENV",
    "JAVA_HOME",
    "GOPATH",
    "VOLTA_HOME",
    "PNPM_HOME",
    "CI",
];
const ENV_PREFIXES: &[&str] = &[
    "LC_",
    "XDG_",
    "PROCESSOR_",
    "DOTNET_",
    "NUGET_",
    "MSBUILD",
    "NODE_",
    "NVM_",
    "PYTHON",
    "CONDA_",
    "PIP_",
];
/// Even an allowlisted prefix never passes a variable whose name looks like a credential.
const ENV_SECRET_MARKERS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "_KEY",
    "AUTH",
    "CREDENTIAL",
];

pub fn is_allowed_env(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    if ENV_SECRET_MARKERS.iter().any(|m| n.contains(m)) {
        return false;
    }
    ENV_EXACT.contains(&n.as_str()) || ENV_PREFIXES.iter().any(|p| n.starts_with(p))
}

/// The filtered environment plus `extra` names explicitly allowed by the project config.
pub fn child_env(extra: &[String]) -> Vec<(OsString, OsString)> {
    std::env::vars_os()
        .filter(|(k, _)| {
            let k = k.to_string_lossy();
            is_allowed_env(&k) || extra.iter().any(|e| e.eq_ignore_ascii_case(&k))
        })
        .collect()
}

pub fn command(
    program: &Path,
    args: &[String],
    cwd: &Path,
    env: &[(OsString, OsString)],
) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0");
    #[cfg(unix)]
    {
        // Own process group, so the whole tree can be signalled at once.
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd
}

/// Kill the child and everything it spawned (wrappers like `npm.cmd` or `dotnet` build nodes).
pub fn kill_tree(child: &mut Child) {
    let pid = child.id().to_string();
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

const POLL: Duration = Duration::from_millis(200);

/// Run to completion, timeout or cancellation, writing combined output to `log`.
pub fn run(
    mut cmd: Command,
    log: &Path,
    timeout: Duration,
    cancel: &CancelToken,
) -> io::Result<ProcessOutcome> {
    let out = File::create(log)?;
    let err = out.try_clone()?;
    let started_at = Utc::now();
    let deadline = Instant::now() + timeout;
    let mut child = cmd
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()?;
    let (exit_code, timed_out, cancelled) = loop {
        if let Some(status) = child.wait_timeout(POLL)? {
            break (status.code(), false, false);
        }
        if cancel.is_cancelled() {
            kill_tree(&mut child);
            break (None, false, true);
        }
        if Instant::now() >= deadline {
            kill_tree(&mut child);
            break (None, true, false);
        }
    };
    Ok(ProcessOutcome {
        exit_code,
        timed_out,
        cancelled,
        started_at,
        finished_at: Utc::now(),
    })
}

/// First non-empty line of `<program> <args>`, used to record tool versions.
pub fn version(program: &Path, args: &[&str], cwd: &Path) -> Option<String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let output = command(program, &args, cwd, &child_env(&[]))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_allowlist_keeps_toolchains_and_drops_secrets() {
        for ok in [
            "PATH",
            "Path",
            "DOTNET_ROOT",
            "NODE_OPTIONS",
            "LC_ALL",
            "CARGO_HOME",
        ] {
            assert!(is_allowed_env(ok), "{ok}");
        }
        for bad in [
            "GITHUB_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "NODE_AUTH_TOKEN",
            "NUGET_API_KEY",
            "OPENAI_API_KEY",
            "DATABASE_URL",
        ] {
            assert!(!is_allowed_env(bad), "{bad}");
        }
    }

    fn sleeper() -> Command {
        // A grandchild that outlives a naive kill: the shell spawns a long sleep.
        #[cfg(windows)]
        let (p, a): (&str, &[&str]) = (
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Start-Process -NoNewWindow -Wait powershell -ArgumentList '-NoProfile','-Command','Start-Sleep 60'",
            ],
        );
        #[cfg(unix)]
        let (p, a): (&str, &[&str]) = ("sh", &["-c", "sleep 60 & wait"]);
        let args: Vec<String> = a.iter().map(|s| s.to_string()).collect();
        command(
            &resolve(&[p]).unwrap(),
            &args,
            Path::new("."),
            &child_env(&[]),
        )
    }

    #[test]
    fn timeout_kills_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let t = Instant::now();
        let o = run(
            sleeper(),
            &dir.path().join("log"),
            Duration::from_secs(2),
            &CancelToken::default(),
        )
        .unwrap();
        assert!(o.timed_out);
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "took {:?}",
            t.elapsed()
        );
    }

    #[test]
    fn cancellation_stops_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancelToken::default();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            c2.cancel();
        });
        let t = Instant::now();
        let o = run(
            sleeper(),
            &dir.path().join("log"),
            Duration::from_secs(120),
            &cancel,
        )
        .unwrap();
        assert!(o.cancelled && !o.timed_out);
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "took {:?}",
            t.elapsed()
        );
    }

    #[test]
    fn child_does_not_see_secrets() {
        // cargo sets CARGO_* variables for test binaries; none may reach the child.
        let dir = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let (p, a) = ("cmd", vec!["/C".to_string(), "set".to_string()]);
        #[cfg(unix)]
        let (p, a) = ("env", vec![]);
        let mut env = child_env(&[]);
        env.push(("PATH".into(), std::env::var_os("PATH").unwrap()));
        let parent_secret = std::env::vars().find(|(k, _)| !is_allowed_env(k));
        let cmd = command(&resolve(&[p]).unwrap(), &a, dir.path(), &env);
        let log = dir.path().join("log");
        run(cmd, &log, Duration::from_secs(30), &CancelToken::default()).unwrap();
        let out = std::fs::read_to_string(log).unwrap();
        if let Some((k, _)) = parent_secret {
            assert!(
                !out.lines().any(|l| l.starts_with(&format!("{k}="))),
                "{k} leaked"
            );
        }
    }
}
