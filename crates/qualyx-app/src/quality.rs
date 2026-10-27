//! Test-quality services (M6): repeats and flaky history, the independent reviewer,
//! mutation testing, API contracts and accessibility audits.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use qualyx_core::{Ecosystem, RunManifest};
use qualyx_quality::{
    contract::ContractReport, flaky::FlakyTest, mutation::MutationReport, review::Review,
};
use qualyx_runners::CancelToken;
use serde::Deserialize;

use crate::{Layout, TestRequest, discover, list_runs, load_run, prepare};

/// Run the tests `n` times back to back on the same code and report tests that disagree.
pub fn test_repeat(
    path: &Path,
    req: &TestRequest,
    n: u32,
    mut on_run: impl FnMut(u32),
) -> Result<(Vec<RunManifest>, Vec<FlakyTest>)> {
    let mut manifests = Vec::new();
    for i in 1..=n.max(1) {
        on_run(i);
        manifests.push(prepare(path, req, CancelToken::default())?.execute(|_| {})?);
    }
    let flaky = qualyx_quality::flaky::detect(&manifests, true);
    Ok((manifests, flaky))
}

/// Flaky tests across the last `last` stored runs (same commit, clean tree).
pub fn flaky_history(path: &Path, last: u32) -> Result<Vec<FlakyTest>> {
    let manifests: Vec<RunManifest> = list_runs(path, last)?
        .iter()
        .filter_map(|r| load_run(path, Some(&r.id)).ok().map(|(m, _)| m))
        .collect();
    Ok(qualyx_quality::flaky::detect(&manifests, false))
}

pub fn review(path: &Path, base: &str) -> Result<Review> {
    let root = discover(path)?.root;
    qualyx_quality::review::review_git(&root, base).map_err(anyhow::Error::msg)
}

pub fn mutation(path: &Path) -> Result<(MutationReport, PathBuf)> {
    let profile = discover(path)?;
    if !profile
        .ecosystems
        .iter()
        .any(|e| e.ecosystem == Ecosystem::Rust)
    {
        bail!("mutation testing is implemented for Rust (cargo-mutants) only");
    }
    let layout = Layout::new(&profile.root);
    layout.ensure()?;
    let out = layout
        .run_dir(&qualyx_store::new_run_id(chrono::Utc::now()))
        .join("mutation");
    let r = qualyx_quality::mutation::run(&profile.root, &out, Duration::from_secs(3600))
        .map_err(anyhow::Error::msg)?;
    fs::write(
        out.join("mutation-report.json"),
        serde_json::to_vec_pretty(&r)?,
    )?;
    Ok((r, out))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ContractConfig {
    contract: AllowedTargets,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AllowedTargets {
    allowed_targets: Vec<String>,
}

pub fn contract(
    project: &Path,
    schema_url: &str,
    base_url: Option<&str>,
    max_examples: u32,
) -> Result<(ContractReport, PathBuf)> {
    let root = discover(project)?.root;
    let layout = Layout::new(&root);
    layout.ensure()?;
    let allowed = fs::read_to_string(layout.config())
        .ok()
        .map(|t| toml::from_str::<ContractConfig>(&t))
        .transpose()
        .context("invalid [contract] in .qualyx/config.toml")?
        .unwrap_or_default()
        .contract
        .allowed_targets;
    let out = layout
        .run_dir(&qualyx_store::new_run_id(chrono::Utc::now()))
        .join("contract");
    let r = qualyx_quality::contract::run(
        schema_url,
        base_url,
        &allowed,
        max_examples,
        &out,
        Duration::from_secs(1800),
    )
    .map_err(anyhow::Error::msg)?;
    fs::write(
        out.join("contract-report.json"),
        serde_json::to_vec_pretty(&r)?,
    )?;
    Ok((r, out))
}

/// One security finding the team has reviewed and accepted, with the reason.
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Suppression {
    pub tool: String,
    pub rule_id: String,
    /// Restrict to one file (suffix match); all files when absent.
    pub file: Option<String>,
    pub reason: String,
}

#[derive(Deserialize, Default)]
struct SuppressionFile {
    #[serde(default)]
    suppress: Vec<Suppression>,
}

/// `.qualyx/suppressions.toml`. Every entry needs a non-empty reason.
pub fn load_suppressions(root: &Path) -> Result<Vec<Suppression>> {
    let path = Layout::new(root).dir.join("suppressions.toml");
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let f: SuppressionFile =
        toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))?;
    if let Some(s) = f.suppress.iter().find(|s| s.reason.trim().is_empty()) {
        bail!("suppression for {}/{} has no reason", s.tool, s.rule_id);
    }
    Ok(f.suppress)
}

pub fn suppressed_by<'a>(
    f: &qualyx_security::Finding,
    rules: &'a [Suppression],
) -> Option<&'a Suppression> {
    rules.iter().find(|s| {
        f.tool
            .to_ascii_lowercase()
            .contains(&s.tool.to_ascii_lowercase())
            && f.rule_id == s.rule_id
            && s.file
                .as_ref()
                .is_none_or(|x| f.file.as_deref().is_some_and(|ff| ff.ends_with(x.as_str())))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suppressions_need_reasons_and_match_narrowly() {
        let d = tempfile::tempdir().unwrap();
        Layout::new(d.path()).ensure().unwrap();
        let p = Layout::new(d.path()).dir.join("suppressions.toml");
        fs::write(&p, "[[suppress]]\ntool = \"gitleaks\"\nrule_id = \"generic-api-key\"\nfile = \"tests/fixtures/fake.env\"\nreason = \"\"\n").unwrap();
        assert!(load_suppressions(d.path()).is_err());
        fs::write(&p, "[[suppress]]\ntool = \"gitleaks\"\nrule_id = \"generic-api-key\"\nfile = \"tests/fixtures/fake.env\"\nreason = \"test fixture, not a real key\"\n").unwrap();
        let rules = load_suppressions(d.path()).unwrap();
        let f = |file: &str| qualyx_security::Finding {
            tool: "gitleaks".into(),
            rule_id: "generic-api-key".into(),
            level: "error".into(),
            message: String::new(),
            file: Some(file.into()),
            line: None,
        };
        assert!(suppressed_by(&f("repo/tests/fixtures/fake.env"), &rules).is_some());
        assert!(suppressed_by(&f("src/config.env"), &rules).is_none());
    }
}
