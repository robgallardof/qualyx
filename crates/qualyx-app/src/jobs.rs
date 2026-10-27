//! Asynchronous test runs shared by the long-lived front ends (MCP server, HTTP API):
//! a root allowlist, a bounded registry of in-flight runs, cancellation and crash recovery.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use qualyx_core::{RunManifest, Status};
use qualyx_runners::CancelToken;
use serde::Serialize;

use crate::{Layout, TestRequest, prepare};

/// Directories a remote caller may act on. Paths are canonicalized before the check, so
/// `..` and symlink tricks resolve first (threat model T3/T6).
#[derive(Debug, Clone)]
pub struct Roots(Arc<Vec<PathBuf>>);

impl Roots {
    pub fn new(roots: &[PathBuf]) -> std::io::Result<Roots> {
        Ok(Roots(Arc::new(
            roots
                .iter()
                .map(|r| r.canonicalize())
                .collect::<Result<_, _>>()?,
        )))
    }

    pub fn check(&self, path: &str) -> Result<PathBuf, String> {
        let p = Path::new(path)
            .canonicalize()
            .map_err(|e| format!("cannot access `{path}`: {e}"))?;
        if self.0.iter().any(|r| p.starts_with(r)) {
            Ok(p)
        } else {
            Err(format!("`{path}` is outside the allowed roots"))
        }
    }

    pub fn list(&self) -> &[PathBuf] {
        &self.0
    }
}

enum State {
    Running,
    Finished(Box<RunManifest>),
    Failed(String),
}

struct Entry {
    project: PathBuf,
    cancel: CancelToken,
    state: State,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    pub run_id: String,
    pub project: PathBuf,
    /// "running", "finished" or "failed".
    pub state: String,
    pub cancel_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<RunManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Jobs {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
    max_running: usize,
}

impl Jobs {
    pub fn new(max_running: usize) -> Jobs {
        Jobs {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_running,
        }
    }

    pub fn running(&self) -> usize {
        self.inner
            .lock()
            .map(|m| {
                m.values()
                    .filter(|e| matches!(e.state, State::Running))
                    .count()
            })
            .unwrap_or(0)
    }

    /// Prepare and start a run in the background; returns its id and ecosystems at once.
    pub fn start(&self, project: &Path, req: &TestRequest) -> Result<(String, Vec<String>)> {
        // Reserve capacity under the same lock used to register the run. Checking
        // and inserting separately lets simultaneous HTTP requests exceed the cap.
        let mut registry = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("job registry poisoned"))?;
        if registry
            .values()
            .filter(|e| matches!(e.state, State::Running))
            .count()
            >= self.max_running
        {
            bail!(
                "{} runs already in progress; wait or cancel one",
                self.max_running
            );
        }
        let cancel = CancelToken::default();
        let prepared = prepare(project, req, cancel.clone())?;
        let run_id = prepared.run_id.clone();
        let ecosystems = prepared
            .profile()
            .ecosystems
            .iter()
            .map(|d| d.ecosystem.as_str().to_string())
            .collect();
        // Finished manifests remain on disk; retain only a bounded polling history.
        if registry.len() >= 128 {
            let oldest = registry
                .iter()
                .filter(|(_, e)| !matches!(e.state, State::Running))
                .map(|(id, _)| id.clone())
                .min();
            if let Some(id) = oldest {
                registry.remove(&id);
            }
        }
        registry.insert(
            run_id.clone(),
            Entry {
                project: project.to_path_buf(),
                cancel,
                state: State::Running,
            },
        );
        drop(registry);
        let (inner, id) = (self.inner.clone(), run_id.clone());
        std::thread::spawn(move || {
            let state = match prepared.execute(|_| {}) {
                Ok(m) => State::Finished(Box::new(m)),
                Err(e) => State::Failed(format!("{e:#}")),
            };
            if let Ok(mut m) = inner.lock()
                && let Some(e) = m.get_mut(&id)
            {
                e.state = state;
            }
        });
        Ok((run_id, ecosystems))
    }

    fn view(id: &str, e: &Entry) -> JobView {
        let (state, manifest, error) = match &e.state {
            State::Running => ("running", None, None),
            State::Finished(m) => ("finished", Some((**m).clone()), None),
            State::Failed(err) => ("failed", None, Some(err.clone())),
        };
        JobView {
            run_id: id.into(),
            project: e.project.clone(),
            state: state.into(),
            cancel_requested: e.cancel.is_cancelled(),
            manifest,
            error,
        }
    }

    pub fn get(&self, run_id: &str) -> Option<JobView> {
        let m = self.inner.lock().ok()?;
        m.get(run_id).map(|e| Self::view(run_id, e))
    }

    pub fn list(&self) -> Vec<JobView> {
        let Ok(m) = self.inner.lock() else {
            return Vec::new();
        };
        let mut v: Vec<JobView> = m.iter().map(|(k, e)| Self::view(k, e)).collect();
        v.sort_by(|a, b| b.run_id.cmp(&a.run_id));
        v
    }

    /// Request cancellation. `None` for unknown ids, `Some(false)` if it already finished.
    pub fn cancel(&self, run_id: &str) -> Option<bool> {
        let m = self.inner.lock().ok()?;
        let e = m.get(run_id)?;
        e.cancel.cancel();
        Some(matches!(e.state, State::Running))
    }
}

/// After a crash, run dirs without a manifest are evidence of interrupted runs. Record each as
/// an `error` manifest (never a pass) so history and reports stay truthful. Returns their ids.
pub fn recover_interrupted(project: &Path) -> Result<Vec<String>> {
    let layout = Layout::new(project);
    let runs_dir = layout.dir.join("runs");
    let Ok(entries) = fs::read_dir(&runs_dir) else {
        return Ok(Vec::new());
    };
    let mut recovered = Vec::new();
    for e in entries.flatten() {
        let dir = e.path();
        let id = e.file_name().to_string_lossy().into_owned();
        if !dir.is_dir()
            || dir.join("manifest.json").exists()
            || !qualyx_store::is_valid_run_id(&id)
        {
            continue;
        }
        // Only the test pipeline writes runner logs at the top level; other commands
        // (security, perf, a11y…) keep evidence in subfolders and have their own reports.
        let has_logs = fs::read_dir(&dir)
            .map(|d| {
                d.flatten()
                    .any(|f| f.path().extension().is_some_and(|x| x == "log"))
            })
            .unwrap_or(false);
        if !has_logs {
            continue;
        }
        let now = chrono::Utc::now();
        let m = RunManifest {
            schema_version: qualyx_core::MANIFEST_SCHEMA_VERSION,
            run_id: id.clone(),
            qualyx_version: env!("CARGO_PKG_VERSION").into(),
            project_root: project.to_path_buf(),
            git: None,
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            isolation: Default::default(),
            selection: Some("recovered: the process stopped before this run finished".into()),
            services: Vec::new(),
            started_at: now,
            finished_at: now,
            status: Status::Error,
            counts: Default::default(),
            runners: Vec::new(),
            artifact_sha256: Default::default(),
        };
        crate::persist(&layout, &dir, &m)?;
        recovered.push(id);
    }
    recovered.sort();
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_reject_escapes() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let r = Roots::new(&[root.path().to_path_buf()]).unwrap();
        assert!(r.check(&root.path().display().to_string()).is_ok());
        assert!(r.check(&other.path().display().to_string()).is_err());
        let sneaky = root
            .path()
            .join("..")
            .join(other.path().file_name().unwrap());
        assert!(r.check(&sneaky.display().to_string()).is_err());
    }

    #[test]
    fn interrupted_runs_are_recorded_as_errors_once() {
        let d = tempfile::tempdir().unwrap();
        let layout = Layout::new(d.path());
        layout.ensure().unwrap();
        let dir = layout.run_dir("20260101T000000000Z-1");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("python.log"), "collected 3 items\n").unwrap();
        // Evidence folder of another command: must not be touched.
        fs::create_dir_all(layout.run_dir("20260101T000000000Z-2").join("security")).unwrap();
        assert_eq!(
            recover_interrupted(d.path()).unwrap(),
            ["20260101T000000000Z-1"]
        );
        let (m, _) = crate::load_run(d.path(), Some("20260101T000000000Z-1")).unwrap();
        assert_eq!(m.status, Status::Error);
        assert!(
            recover_interrupted(d.path()).unwrap().is_empty(),
            "idempotent"
        );
    }
}
