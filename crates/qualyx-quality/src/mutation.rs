//! Mutation testing for Rust through cargo-mutants (external process). A surviving ("missed")
//! mutant means no test notices that behaviour changing: a test-quality gap, not a bug.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct MutationReport {
    pub tool_version: Option<String>,
    pub total: u64,
    pub caught: u64,
    pub missed: u64,
    pub timeout: u64,
    pub unviable: u64,
    /// caught / (caught + missed); `None` when nothing was testable.
    pub score: Option<f64>,
    /// `file:line:col: description` of every surviving mutant.
    pub missed_mutants: Vec<String>,
    pub exit_code: Option<i32>,
}

pub fn parse(out_dir: &Path, exit_code: Option<i32>) -> Result<MutationReport, String> {
    let dir = out_dir.join("mutants.out");
    let outcomes: Value = fs::read(dir.join("outcomes.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or("cargo-mutants wrote no outcomes.json (did the baseline build/test fail? see mutants.log)")?;
    let n = |k: &str| outcomes[k].as_u64().unwrap_or(0);
    let (caught, missed) = (n("caught"), n("missed"));
    let missed_mutants = fs::read_to_string(dir.join("missed.txt"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().replace('\\', "/"))
        .filter(|l| !l.is_empty())
        .collect();
    Ok(MutationReport {
        tool_version: outcomes["cargo_mutants_version"].as_str().map(String::from),
        total: n("total_mutants"),
        caught,
        missed,
        timeout: n("timeout"),
        unviable: n("unviable"),
        score: (caught + missed > 0).then(|| caught as f64 / (caught + missed) as f64),
        missed_mutants,
        exit_code,
    })
}

/// Run `cargo mutants` on the Rust project at `root`, writing `mutants.out` under `out_dir`.
pub fn run(root: &Path, out_dir: &Path, timeout: Duration) -> Result<MutationReport, String> {
    let cargo = qualyx_runners::process::resolve(&["cargo"]).ok_or("cargo not found")?;
    let probe = std::process::Command::new(&cargo)
        .args(["mutants", "--version"])
        .output()
        .map_err(|e| e.to_string())?;
    if !probe.status.success() {
        return Err("cargo-mutants is not installed (cargo install cargo-mutants --locked)".into());
    }
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let args: Vec<String> = vec![
        "mutants".into(),
        "--no-shuffle".into(),
        "--colors=never".into(),
        "-o".into(),
        out_dir.display().to_string(),
    ];
    let env = qualyx_runners::process::child_env(&[]);
    let cmd = qualyx_runners::process::command(&cargo, &args, root, &env);
    let o = qualyx_runners::process::run(
        cmd,
        &out_dir.join("mutants.log"),
        timeout,
        &Default::default(),
    )
    .map_err(|e| e.to_string())?;
    if o.timed_out {
        return Err("cargo-mutants timed out".into());
    }
    parse(out_dir, o.exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_cargo_mutants_27_output() {
        let d = tempfile::tempdir().unwrap();
        let m = d.path().join("mutants.out");
        fs::create_dir_all(&m).unwrap();
        // Shape of cargo-mutants 27.1.0 output on fixtures/mutation/weak.
        fs::write(m.join("outcomes.json"), r#"{"outcomes":[],"total_mutants":3,"missed":1,"caught":2,"timeout":0,"unviable":0,"success":0,"cargo_mutants_version":"27.1.0"}"#).unwrap();
        fs::write(
            m.join("missed.txt"),
            "src\\lib.rs:3:5: replace is_adult -> bool with true\n",
        )
        .unwrap();
        let r = parse(d.path(), Some(2)).unwrap();
        assert_eq!((r.total, r.caught, r.missed), (3, 2, 1));
        assert!((r.score.unwrap() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(
            r.missed_mutants,
            ["src/lib.rs:3:5: replace is_adult -> bool with true"]
        );
    }

    #[test]
    fn missing_outcomes_is_an_error_not_a_score() {
        let d = tempfile::tempdir().unwrap();
        assert!(parse(d.path(), Some(1)).is_err());
    }
}
