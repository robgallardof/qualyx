//! Application services shared by every front end (CLI, MCP, HTTP API).
//!
//! Front ends parse input and format output; everything else (config, running, persisting,
//! reading evidence back) lives here so the behaviour is identical whichever door is used.

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
}

/// A run that has an id and a directory but hasn't started yet, so callers (MCP) can hand the
/// id out before the work begins.
pub struct PreparedRun {
    pub run_id: String,
    pub run_dir: PathBuf,
    profile: ProjectProfile,
    layout: Layout,
    opts: RunOptions,
}

pub fn prepare(path: &Path, req: &TestRequest, cancel: CancelToken) -> Result<PreparedRun> {
    let profile = discover(path)?;
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
            cancel,
        },
    })
}

impl PreparedRun {
    pub fn profile(&self) -> &ProjectProfile {
        &self.profile
    }

    /// Execute, then persist `manifest.json`, `report.md` and the SQLite index.
    pub fn execute(
        self,
        on_runner: impl FnMut(&qualyx_core::DetectedEcosystem),
    ) -> Result<RunManifest> {
        let manifest = qualyx_runners::run(
            &self.profile,
            &self.run_id,
            &self.run_dir,
            &self.opts,
            on_runner,
        )?;
        persist(&self.layout, &self.run_dir, &manifest)?;
        Ok(manifest)
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
