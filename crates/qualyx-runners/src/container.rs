//! Container isolation (ADR 0004): the project is copied to a scratch dir, mounted with the run
//! dir, and the native tool runs in a throwaway container with dropped capabilities.
//!
//! The container gets network access (package restores need it) but never the host's
//! environment, credentials or the original working tree.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use qualyx_core::Ecosystem;

pub const WORK: &str = "/work";
pub const OUT: &str = "/qualyx-out";

/// Directories never copied into the container workspace.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".qualyx",
    "target",
    "node_modules",
    "bin",
    "obj",
    "__pycache__",
    ".pytest_cache",
    ".venv",
    "venv",
];

/// Official image per ecosystem. Pinned to a major line; the resolved digest is recorded in
/// the log by the engine on pull.
pub fn image(e: Ecosystem) -> &'static str {
    match e {
        Ecosystem::Rust => "docker.io/library/rust:1",
        Ecosystem::Dotnet => "mcr.microsoft.com/dotnet/sdk:10.0",
        Ecosystem::Node => "docker.io/library/node:24-slim",
        Ecosystem::Python => "docker.io/library/python:3.13-slim",
    }
}

/// Shell prelude run inside the container before the tool (the tool args are passed as `$@`,
/// so nothing Qualyx forwards is ever re-parsed by the shell).
fn prelude(e: Ecosystem) -> &'static str {
    match e {
        Ecosystem::Python => {
            "python -m pip install -q --user --disable-pip-version-check pytest >/dev/null && "
        }
        _ => "",
    }
}

pub fn engine() -> Option<PathBuf> {
    crate::process::resolve(&["docker", "podman"])
}

pub fn copy_project(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            continue; // never follow links out of the project
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if ty.is_dir() {
            if !SKIP_DIRS.contains(&name.to_string_lossy().as_ref()) {
                copy_project(&src, &dst)?;
            }
        } else {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

pub fn container_name(run_id: &str, e: Ecosystem) -> String {
    format!("qualyx-{}-{}", run_id.to_ascii_lowercase(), e.as_str())
}

/// Arguments for `<engine> run ...` executing `program args` in the ecosystem image.
pub fn run_args(
    name: &str,
    e: Ecosystem,
    workspace: &Path,
    run_dir: &Path,
    program: &str,
    args: &[String],
) -> Vec<String> {
    let mut v: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        name.into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
        "--pids-limit=1024".into(),
        "--memory=4g".into(),
        "-v".into(),
        format!("{}:{WORK}", workspace.display()),
        "-v".into(),
        format!("{}:{OUT}", run_dir.display()),
        "-w".into(),
        WORK.into(),
        "-e".into(),
        "HOME=/tmp".into(),
        "-e".into(),
        "CARGO_HOME=/tmp/cargo".into(),
        "-e".into(),
        "DOTNET_CLI_HOME=/tmp".into(),
        "-e".into(),
        "NO_COLOR=1".into(),
    ];
    #[cfg(unix)]
    {
        // Files written into the mounts stay owned by the invoking user.
        let id = |flag: &str| {
            Command::new("id")
                .arg(flag)
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        if let (Some(u), Some(g)) = (id("-u"), id("-g")) {
            v.push(format!("--user={u}:{g}"));
        }
    }
    v.extend([
        image(e).to_string(),
        "sh".into(),
        "-c".into(),
        format!("{}exec \"$@\"", prelude(e)),
        "sh".into(),
        program.into(),
    ]);
    v.extend(args.iter().cloned());
    v
}

/// Best-effort removal of a container left running after cancel/timeout.
pub fn force_remove(engine: &Path, name: &str) {
    let _ = Command::new(engine)
        .args(["rm", "-f", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_args_are_passed_positionally_not_through_the_shell() {
        let a = run_args(
            "n",
            Ecosystem::Python,
            Path::new("/w"),
            Path::new("/o"),
            "python",
            &[
                "-m".into(),
                "pytest".into(),
                "--junitxml=/qualyx-out/x; rm -rf /".into(),
            ],
        );
        let i = a.iter().position(|x| x == "-c").unwrap();
        assert!(a[i + 1].ends_with("exec \"$@\""));
        assert_eq!(a.last().unwrap(), "--junitxml=/qualyx-out/x; rm -rf /");
        assert!(a.contains(&"--cap-drop=ALL".to_string()));
    }

    #[test]
    fn copy_skips_build_outputs_and_vcs() {
        let src = tempfile::tempdir().unwrap();
        for d in ["src", "target", ".git", "node_modules"] {
            fs::create_dir_all(src.path().join(d)).unwrap();
            fs::write(src.path().join(d).join("f"), "x").unwrap();
        }
        let dst = tempfile::tempdir().unwrap();
        copy_project(src.path(), dst.path()).unwrap();
        assert!(dst.path().join("src/f").exists());
        for d in ["target", ".git", "node_modules"] {
            assert!(!dst.path().join(d).exists(), "{d}");
        }
    }
}
