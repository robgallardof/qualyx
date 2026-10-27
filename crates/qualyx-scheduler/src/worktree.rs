//! One git worktree per writing agent, with an exclusive lock so two writers never share one
//! (master prompt: "Never let two writers share a worktree").
//!
//! Layout: `<repo>/.qualyx/worktrees/<name>/` on branch `qualyx/<name>`, lock file
//! `<repo>/.qualyx/worktrees/<name>.lock` created with `create_new` (atomic on every OS).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn base_dir(repo: &Path) -> PathBuf {
    repo.join(".qualyx").join("worktrees")
}

/// Create `name` from `base` (a revision). Fails if it already exists.
pub fn create(repo: &Path, name: &str, base: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err(format!("invalid worktree name `{name}`"));
    }
    if base.starts_with('-') {
        return Err(format!("invalid base `{base}`"));
    }
    let dir = base_dir(repo);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Keep worktrees out of the parent repo's status.
    let ignore = repo.join(".qualyx").join(".gitignore");
    if !ignore.exists() {
        let _ = fs::write(ignore, "*\n");
    }
    let path = dir.join(name);
    let branch = format!("qualyx/{name}");
    git(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &path.to_string_lossy(),
            base,
        ],
    )?;
    Ok(path)
}

/// Exclusive writer lock; released on drop.
#[derive(Debug)]
pub struct WriterLock {
    path: PathBuf,
}

impl WriterLock {
    pub fn acquire(repo: &Path, name: &str, owner: &str) -> Result<WriterLock, String> {
        if !valid_name(name) {
            return Err(format!("invalid worktree name `{name}`"));
        }
        if !base_dir(repo).join(name).is_dir() {
            return Err(format!("worktree `{name}` does not exist"));
        }
        let path = base_dir(repo).join(format!("{name}.lock"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                let _ = writeln!(f, "{owner} pid={}", std::process::id());
                Ok(WriterLock { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = fs::read_to_string(&path).unwrap_or_default();
                Err(format!(
                    "worktree `{name}` already has a writer: {}",
                    holder.trim()
                ))
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Remove the worktree (refuses while a writer holds it) and its branch.
pub fn remove(repo: &Path, name: &str) -> Result<(), String> {
    if !valid_name(name) {
        return Err(format!("invalid worktree name `{name}`"));
    }
    if base_dir(repo).join(format!("{name}.lock")).exists() {
        return Err(format!("worktree `{name}` is locked by a writer"));
    }
    let path = base_dir(repo).join(name);
    git(
        repo,
        &["worktree", "remove", "--force", &path.to_string_lossy()],
    )?;
    git(repo, &["branch", "-D", &format!("qualyx/{name}")])?;
    Ok(())
}

pub fn list(repo: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(base_dir(repo))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let g = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args([
                        "-c",
                        "user.name=t",
                        "-c",
                        "user.email=t@t",
                        "-c",
                        "commit.gpgsign=false"
                    ])
                    .args(args)
                    .current_dir(d.path())
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        };
        g(&["init", "-q"]);
        fs::write(d.path().join("f.txt"), "x").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-q", "-m", "init"]);
        d
    }

    #[test]
    fn one_writer_per_worktree() {
        let r = repo();
        let p = create(r.path(), "unit-agent", "HEAD").unwrap();
        assert!(p.join("f.txt").is_file());
        assert!(
            create(r.path(), "unit-agent", "HEAD").is_err(),
            "duplicate create"
        );

        let lock = WriterLock::acquire(r.path(), "unit-agent", "agent-1").unwrap();
        let err = WriterLock::acquire(r.path(), "unit-agent", "agent-2").unwrap_err();
        assert!(err.contains("agent-1"), "{err}");
        assert!(
            remove(r.path(), "unit-agent").is_err(),
            "removed while locked"
        );
        drop(lock);
        WriterLock::acquire(r.path(), "unit-agent", "agent-2")
            .map(drop)
            .unwrap();

        // Separate worktrees are independent writers.
        create(r.path(), "browser-agent", "HEAD").unwrap();
        let _a = WriterLock::acquire(r.path(), "unit-agent", "a").unwrap();
        let _b = WriterLock::acquire(r.path(), "browser-agent", "b").unwrap();
        assert_eq!(list(r.path()), ["browser-agent", "unit-agent"]);
    }

    #[test]
    fn remove_cleans_up_and_names_are_validated() {
        let r = repo();
        create(r.path(), "w1", "HEAD").unwrap();
        remove(r.path(), "w1").unwrap();
        assert!(list(r.path()).is_empty());
        for bad in ["../x", "a/b", "", "a b"] {
            assert!(create(r.path(), bad, "HEAD").is_err(), "{bad}");
        }
        assert!(create(r.path(), "ok", "--orphan").is_err());
    }
}
