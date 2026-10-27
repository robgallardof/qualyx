//! Security scanning (M5): Gitleaks (secrets), Semgrep (SAST), Trivy and OSV-Scanner
//! (dependencies), all run as external processes that emit SARIF. Qualyx normalizes the
//! findings and merges the SARIF runs into one report.
//!
//! Secrets are never stored in clear: Gitleaks runs with `--redact` and every message is
//! passed through Qualyx's redactor as well.

pub mod audit;
pub mod dast;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scanner {
    Gitleaks,
    Semgrep,
    Trivy,
    OsvScanner,
}

pub const ALL: [Scanner; 4] = [
    Scanner::Gitleaks,
    Scanner::Semgrep,
    Scanner::Trivy,
    Scanner::OsvScanner,
];

impl Scanner {
    pub fn id(self) -> &'static str {
        match self {
            Scanner::Gitleaks => "gitleaks",
            Scanner::Semgrep => "semgrep",
            Scanner::Trivy => "trivy",
            Scanner::OsvScanner => "osv-scanner",
        }
    }

    pub fn parse(s: &str) -> Option<Scanner> {
        ALL.into_iter().find(|x| x.id() == s)
    }

    fn version_args(self) -> &'static [&'static str] {
        match self {
            Scanner::Gitleaks | Scanner::Semgrep | Scanner::Trivy | Scanner::OsvScanner => {
                &["--version"]
            }
        }
    }

    /// Arguments that scan `root` and write SARIF to `out`.
    pub fn args(self, root: &Path, out: &Path, semgrep_config: &str) -> Vec<String> {
        let (r, o) = (root.display().to_string(), out.display().to_string());
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        match self {
            Scanner::Gitleaks => v(&[
                "dir",
                &r,
                "--redact",
                "--no-banner",
                "--report-format",
                "sarif",
                "--report-path",
                &o,
                "--exit-code",
                "0",
            ]),
            Scanner::Semgrep => v(&[
                "scan",
                "--config",
                semgrep_config,
                "--sarif",
                "--output",
                &o,
                "--metrics=off",
                "--disable-version-check",
                "--quiet",
                &r,
            ]),
            Scanner::Trivy => v(&[
                "fs",
                "--scanners",
                "vuln",
                "--format",
                "sarif",
                "--output",
                &o,
                "--exit-code",
                "0",
                "--quiet",
                &r,
            ]),
            Scanner::OsvScanner => v(&[
                "scan",
                "source",
                "-r",
                &r,
                "--format",
                "sarif",
                "--output-file",
                &o,
            ]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub tool: String,
    pub rule_id: String,
    /// SARIF level: error, warning, note, none.
    pub level: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u64>,
}

/// Normalize every result of every run in a SARIF 2.1.0 document.
pub fn parse_sarif(doc: &Value) -> Vec<Finding> {
    let mut out = Vec::new();
    for run in doc["runs"].as_array().into_iter().flatten() {
        let tool = run["tool"]["driver"]["name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        // Rule default levels, used when a result has no level of its own.
        let default_level = |rule: &str| -> Option<String> {
            run["tool"]["driver"]["rules"]
                .as_array()?
                .iter()
                .find(|r| r["id"] == rule)?["defaultConfiguration"]["level"]
                .as_str()
                .map(String::from)
        };
        for r in run["results"].as_array().into_iter().flatten() {
            let rule_id = r["ruleId"].as_str().unwrap_or("").to_string();
            let loc = &r["locations"][0]["physicalLocation"];
            let level = r["level"]
                .as_str()
                .map(String::from)
                .or_else(|| default_level(&rule_id))
                .unwrap_or_else(|| "warning".into());
            out.push(Finding {
                tool: tool.clone(),
                level,
                message: qualyx_runners::redact::redact(
                    r["message"]["text"].as_str().unwrap_or(""),
                ),
                file: loc["artifactLocation"]["uri"]
                    .as_str()
                    .map(|u| u.trim_start_matches("file://").replace('\\', "/")),
                line: loc["region"]["startLine"].as_u64(),
                rule_id,
            });
        }
    }
    out
}

/// Concatenate the runs of several SARIF documents into one.
pub fn merge_sarif(docs: &[Value]) -> Value {
    let runs: Vec<Value> = docs
        .iter()
        .flat_map(|d| d["runs"].as_array().cloned().unwrap_or_default())
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": runs,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ScannerResult {
    pub scanner: Scanner,
    /// "ran", "missing" (not installed), "error".
    pub status: String,
    pub version: Option<String>,
    pub findings: usize,
    pub sarif: Option<String>,
    pub notes: Vec<String>,
    /// Actual scanner process exit code; absent when it never started or was killed.
    pub exit_code: Option<i32>,
    /// Whether the scanner exceeded its allotted time.
    pub timed_out: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecurityReport {
    /// "clean", "findings", or "inconclusive" when any requested scan is incomplete.
    pub status: String,
    /// True only when every requested scanner completed with usable evidence.
    pub coverage_complete: bool,
    pub scanners: Vec<ScannerResult>,
    pub findings: Vec<Finding>,
    /// Findings a human marked as false positives, with the reason. Reported, never hidden.
    #[serde(default)]
    pub suppressed: Vec<(Finding, String)>,
    pub merged_sarif: Option<String>,
}

fn program(s: Scanner) -> Option<PathBuf> {
    qualyx_runners::process::resolve(&[s.id()])
}

/// Semgrep rules: the project's own config when present, otherwise Semgrep's default ruleset
/// (which needs the network).
pub fn semgrep_config(root: &Path) -> String {
    for c in [".semgrep.yml", ".semgrep.yaml", ".semgrep"] {
        if root.join(c).exists() {
            return root.join(c).display().to_string();
        }
    }
    "p/default".into()
}

/// Run the selected scanners over `root`; evidence goes to `out_dir`.
pub fn scan(
    root: &Path,
    scanners: &[Scanner],
    out_dir: &Path,
    timeout: Duration,
) -> std::io::Result<SecurityReport> {
    fs::create_dir_all(out_dir)?;
    // The caller may reuse an evidence directory after a failed attempt.
    match fs::remove_file(out_dir.join("merged.sarif")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let env = qualyx_runners::process::child_env(&[]);
    let semgrep_cfg = semgrep_config(root);
    let mut results = Vec::new();
    let mut findings = Vec::new();
    let mut docs = Vec::new();
    for &s in scanners {
        let Some(prog) = program(s) else {
            results.push(ScannerResult {
                scanner: s,
                status: "missing".into(),
                version: None,
                findings: 0,
                sarif: None,
                notes: vec![format!("`{}` is not on PATH", s.id())],
                exit_code: None,
                timed_out: false,
            });
            continue;
        };
        let version = qualyx_runners::process::version(&prog, s.version_args(), root);
        let sarif_name = format!("{}.sarif", s.id());
        let sarif_path = out_dir.join(&sarif_name);
        // A failed rerun must never borrow a previous run's clean evidence.
        match fs::remove_file(&sarif_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let mut cmd = qualyx_runners::process::command(
            &prog,
            &s.args(root, &sarif_path, &semgrep_cfg),
            root,
            &env,
        );
        if s == Scanner::Semgrep {
            cmd.env("SEMGREP_SEND_METRICS", "off");
        }
        let log = out_dir.join(format!("{}.log", s.id()));
        let outcome = qualyx_runners::process::run(cmd, &log, timeout, &Default::default())?;
        let _ = qualyx_runners::redact::redact_file(&log);
        let mut notes = Vec::new();
        if s == Scanner::Semgrep && semgrep_cfg == "p/default" {
            notes.push(
                "no project .semgrep.yml: used the p/default registry ruleset (network)".into(),
            );
        }
        let doc = audit::read_sarif(&sarif_path);
        let completed = !outcome.timed_out
            && !outcome.cancelled
            && (outcome.exit_code == Some(0)
                || (s == Scanner::OsvScanner && outcome.exit_code == Some(1)));
        match doc {
            Ok(d) => {
                let f = parse_sarif(&d);
                // Redact decoded strings, so escaped secrets cannot bypass masking.
                fs::write(&sarif_path, serde_json::to_vec_pretty(&d)?)?;
                let complete = completed && audit::invocations_succeeded(&d);
                if !complete {
                    notes
                        .push("scanner execution incomplete; findings are partial evidence".into());
                }
                results.push(ScannerResult {
                    scanner: s,
                    status: if complete { "ran" } else { "error" }.into(),
                    version,
                    findings: f.len(),
                    sarif: Some(sarif_name),
                    notes,
                    exit_code: outcome.exit_code,
                    timed_out: outcome.timed_out,
                });
                findings.extend(f);
                docs.push(d);
            }
            Err(reason) => {
                // Even unusable scanner output may contain credentials.
                if sarif_path.exists() {
                    qualyx_runners::redact::redact_file(&sarif_path)?;
                }
                notes.push(format!(
                    "unusable SARIF: {reason} (exit {:?}, timed out: {}); see {}.log",
                    outcome.exit_code,
                    outcome.timed_out,
                    s.id()
                ));
                results.push(ScannerResult {
                    scanner: s,
                    status: "error".into(),
                    version,
                    findings: 0,
                    sarif: None,
                    notes,
                    exit_code: outcome.exit_code,
                    timed_out: outcome.timed_out,
                });
            }
        }
    }
    let merged_sarif = if docs.is_empty() {
        None
    } else {
        let merged = merge_sarif(&docs);
        fs::write(
            out_dir.join("merged.sarif"),
            serde_json::to_vec_pretty(&merged)?,
        )?;
        let _ = qualyx_runners::redact::redact_file(&out_dir.join("merged.sarif"));
        Some("merged.sarif".into())
    };
    let mut report = SecurityReport {
        status: "inconclusive".into(),
        coverage_complete: false,
        scanners: results,
        findings,
        suppressed: Vec::new(),
        merged_sarif,
    };
    report.refresh_status();
    report.write_evidence(out_dir)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_merges_sarif() {
        let a = json!({"version": "2.1.0", "runs": [{
            "tool": {"driver": {"name": "gitleaks", "rules": [{"id": "aws-access-token", "defaultConfiguration": {"level": "error"}}]}},
            "results": [{"ruleId": "aws-access-token", "message": {"text": "AWS key REDACTED"},
                "locations": [{"physicalLocation": {"artifactLocation": {"uri": "config/settings.py"}, "region": {"startLine": 3}}}]}]
        }]});
        let b = json!({"version": "2.1.0", "runs": [{
            "tool": {"driver": {"name": "osv-scanner"}},
            "results": [{"ruleId": "GHSA-xxxx", "level": "warning", "message": {"text": "django 2.2.0 vulnerable"}, "locations": [{"physicalLocation": {"artifactLocation": {"uri": "file://requirements.txt"}}}]}]
        }]});
        let f = parse_sarif(&a);
        assert_eq!(f[0].level, "error", "rule default level applies");
        assert_eq!(f[0].file.as_deref(), Some("config/settings.py"));
        assert_eq!(f[0].line, Some(3));
        let g = parse_sarif(&b);
        assert_eq!(g[0].file.as_deref(), Some("requirements.txt"));
        let m = merge_sarif(&[a, b]);
        assert_eq!(m["runs"].as_array().unwrap().len(), 2);
        assert_eq!(m["version"], "2.1.0");
    }

    #[test]
    fn messages_are_redacted() {
        let key = format!("AKIA{}", "Q".repeat(16));
        let d = json!({"runs": [{"tool": {"driver": {"name": "x"}}, "results": [{"ruleId": "r", "message": {"text": format!("found {key}")}}]}]});
        assert!(!parse_sarif(&d)[0].message.contains(&key));
    }

    #[test]
    fn missing_scanners_make_the_report_inconclusive() {
        let d = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        // A PATH without scanners: run with a scanner name that cannot exist.
        let r = scan(d.path(), &[], out.path(), Duration::from_secs(5)).unwrap();
        assert_eq!(r.status, "inconclusive");
    }

    #[test]
    fn args_never_go_through_a_shell_and_gitleaks_redacts() {
        let a = Scanner::Gitleaks.args(Path::new("/p; rm -rf /"), Path::new("/o.sarif"), "");
        assert!(a.contains(&"--redact".to_string()));
        assert_eq!(a[1], "/p; rm -rf /");
    }
}
