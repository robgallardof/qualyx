//! Performance and security services (M5): config-driven authorization, evidence on disk.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use qualyx_perf::{Comparison, Limits, PerfRun, Workload};
use qualyx_security::{Scanner, SecurityReport, dast::DastReport};
use serde::Deserialize;

use crate::{Layout, discover};

#[derive(Deserialize, Default)]
#[serde(default)]
struct AssuranceConfig {
    performance: Limits,
    security: SecurityConfig,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SecurityConfig {
    dast_allowed_targets: Vec<String>,
}

fn config(root: &Path) -> Result<AssuranceConfig> {
    match fs::read_to_string(Layout::new(root).config()) {
        Ok(t) => {
            toml::from_str(&t).context("invalid [performance]/[security] in .qualyx/config.toml")
        }
        Err(_) => Ok(AssuranceConfig::default()),
    }
}

fn perf_root(root: &Path, workload: &str) -> PathBuf {
    root.join(".qualyx")
        .join("perf")
        .join(qualyx_perf::sanitize(workload))
}

pub fn load_workload(file: &Path) -> Result<Workload> {
    let text =
        fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?;
    toml::from_str(&text).with_context(|| format!("invalid workload {}", file.display()))
}

/// Authorize, run with k6 and store under `.qualyx/perf/<workload>/<run-id>/`.
/// `max_total_secs` caps duration x repeat (the MCP server uses it to bound a call).
pub fn perf_run(
    project: &Path,
    workload_file: &Path,
    repeat: u32,
    mark_baseline: bool,
    max_total_secs: Option<u32>,
) -> Result<(PerfRun, PathBuf)> {
    let root = discover(project)?.root;
    let w = load_workload(workload_file)?;
    let limits = config(&root)?.performance;
    qualyx_perf::authorize(&w, &limits).map_err(anyhow::Error::msg)?;
    if let Some(max) = max_total_secs
        && (w.duration_secs + w.warmup_secs) * repeat.max(1) > max
    {
        bail!("duration x repeat exceeds {max}s for this caller");
    }
    let k6 = qualyx_runners::process::resolve(&["k6"])
        .context("k6 is not on PATH (https://grafana.com/docs/k6/latest/set-up/install-k6/)")?;
    let run_id = qualyx_store::new_run_id(chrono::Utc::now());
    let dir = perf_root(&root, &w.name).join(&run_id);
    let r = qualyx_perf::run(&w, &k6, &dir, &run_id, repeat).map_err(anyhow::Error::msg)?;
    if mark_baseline {
        fs::write(perf_root(&root, &w.name).join("BASELINE"), &run_id)?;
    }
    Ok((r, dir))
}

fn load_perf(dir: &Path) -> Result<PerfRun> {
    Ok(serde_json::from_slice(
        &fs::read(dir.join("result.json"))
            .with_context(|| format!("no result in {}", dir.display()))?,
    )?)
}

/// Compare a candidate (default: latest run) against the baseline (default: the marked one).
pub fn perf_compare(
    project: &Path,
    workload: &str,
    baseline: Option<&str>,
    candidate: Option<&str>,
) -> Result<Comparison> {
    let root = discover(project)?.root;
    let base_dir = perf_root(&root, workload);
    let valid = |id: &str| -> Result<String> {
        if qualyx_store::is_valid_run_id(id) {
            Ok(id.to_string())
        } else {
            bail!("invalid run id `{id}`")
        }
    };
    let baseline = match baseline {
        Some(b) => valid(b)?,
        None => fs::read_to_string(base_dir.join("BASELINE"))
            .map(|s| s.trim().to_string())
            .context("no baseline marked (run `qualyx perf run --baseline`)")?,
    };
    let candidate = match candidate {
        Some(c) => valid(c)?,
        None => {
            let mut runs: Vec<String> = fs::read_dir(&base_dir)?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().join("result.json").is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|id| *id != baseline)
                .collect();
            runs.sort();
            runs.pop()
                .context("no candidate run besides the baseline")?
        }
    };
    let (bd, cd) = (base_dir.join(&baseline), base_dir.join(&candidate));
    let (b, c) = (load_perf(&bd)?, load_perf(&cd)?);
    let (bl, cl) = (
        qualyx_perf::load_latencies(&bd, &b),
        qualyx_perf::load_latencies(&cd, &c),
    );
    Ok(qualyx_perf::compare(&b, &bl, &c, &cl, 0.01, 0.10))
}

/// Run the requested scanners (all when empty) into `.qualyx/runs/<id>/security/`.
pub fn security_scan(project: &Path, scanners: &[Scanner]) -> Result<(SecurityReport, PathBuf)> {
    let root = discover(project)?.root;
    let layout = Layout::new(&root);
    layout.ensure()?;
    let run_id = qualyx_store::new_run_id(chrono::Utc::now());
    let out = layout.run_dir(&run_id).join("security");
    let list: Vec<Scanner> = if scanners.is_empty() {
        qualyx_security::ALL.to_vec()
    } else {
        scanners.to_vec()
    };
    let r = qualyx_security::scan(&root, &list, &out, Duration::from_secs(1800))?;
    Ok((r, out))
}

/// ZAP baseline against an authorized target, evidence under `.qualyx/runs/<id>/dast/`.
pub fn dast(project: &Path, target: &str) -> Result<(DastReport, PathBuf)> {
    let root = discover(project)?.root;
    let allowed = config(&root)?.security.dast_allowed_targets;
    qualyx_security::dast::authorize(target, &allowed).map_err(anyhow::Error::msg)?;
    let layout = Layout::new(&root);
    layout.ensure()?;
    let out = layout
        .run_dir(&qualyx_store::new_run_id(chrono::Utc::now()))
        .join("dast");
    let r = qualyx_security::dast::baseline(target, &allowed, &out, Duration::from_secs(1800))
        .map_err(anyhow::Error::msg)?;
    Ok((r, out))
}
