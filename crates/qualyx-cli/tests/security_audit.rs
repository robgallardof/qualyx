//! Real CLI audit regression: invalid Semgrep rules and reviewed Gitleaks candidates.
//! Requires actual Gitleaks and Semgrep installations; CI runs this explicitly.

use std::{fs, process::Command};

use serde_json::Value;

#[test]
fn missing_requested_scanner_returns_inconclusive_from_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let empty_path = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_qualyx"))
        .arg("security")
        .arg(dir.path())
        .args(["--scanner", "semgrep", "--json"])
        .env("PATH", empty_path.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "inconclusive");
    assert_eq!(report["coverage_complete"], false);
    assert_eq!(report["scanners"][0]["status"], "missing");
}

#[test]
#[ignore = "requires real Gitleaks and Semgrep on PATH"]
fn suppressing_candidates_cannot_hide_a_failed_scanner() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("app.py"), "print('hello')\n").unwrap();
    fs::write(dir.path().join(".semgrep.yml"), "rules: [invalid").unwrap();
    // A local rule makes the seeded candidate deterministic without storing any secret.
    fs::write(dir.path().join(".gitleaks.toml"),
        "title = 'audit regression'\n[[rules]]\nid = 'synthetic-marker'\ndescription = 'test marker'\nregex = '''QUALYX_SYNTHETIC_MARKER_123456'''\n").unwrap();
    fs::write(
        dir.path().join("sample.txt"),
        "QUALYX_SYNTHETIC_MARKER_123456\n",
    )
    .unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_qualyx"))
            .arg("security")
            .arg(dir.path())
            .args(["--scanner", "gitleaks", "--scanner", "semgrep", "--json"])
            .output()
            .unwrap()
    };
    let initial = run();
    assert_eq!(
        initial.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let report: Value = serde_json::from_slice(&initial.stdout).unwrap();
    assert!(!report["findings"].as_array().unwrap().is_empty());
    assert_eq!(report["coverage_complete"], false);
    fs::write(dir.path().join(".qualyx/suppressions.toml"),
        "[[suppress]]\ntool = 'gitleaks'\nrule_id = 'synthetic-marker'\nreason = 'intentional regression marker'\n").unwrap();
    let reviewed = run();
    assert_eq!(reviewed.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&reviewed.stdout).unwrap();
    assert_eq!(report["status"], "inconclusive");
    assert!(report["findings"].as_array().unwrap().is_empty());
    assert!(!report["suppressed"].as_array().unwrap().is_empty());
    for run in fs::read_dir(dir.path().join(".qualyx/runs")).unwrap() {
        let audit = run.unwrap().path().join("security/security-audit.md");
        assert!(fs::read_to_string(audit).unwrap().contains("inconclusive"));
    }
}
