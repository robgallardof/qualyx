//! Real Gitleaks, Semgrep, Trivy and OSV-Scanner against a project seeded at runtime with a
//! secret, a command-injection pattern and a vulnerable dependency, plus a clean control.
//! Opt in with QUALYX_TEST_SCANNERS=1 (scanners on PATH; Trivy/OSV need the network).

use std::fs;
use std::path::Path;
use std::time::Duration;

use qualyx_security::{ALL, Scanner, scan};

const SEMGREP_RULES: &str = r#"rules:
  - id: qualyx-subprocess-shell-true
    languages: [python]
    severity: ERROR
    message: subprocess call with shell=True (command injection risk)
    pattern: subprocess.$FUNC(..., shell=True, ...)
"#;

fn enabled() -> bool {
    let on = std::env::var("QUALYX_TEST_SCANNERS").as_deref() == Ok("1");
    if !on {
        eprintln!("NOT RUN: set QUALYX_TEST_SCANNERS=1 with the scanners on PATH");
    }
    on
}

/// An AWS-access-key-shaped token built at runtime, so no secret lives in the repository.
fn fake_aws_key() -> String {
    const B32: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15 ^ std::process::id() as u64;
    let tail: String = (0..16)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            B32[(x % 32) as usize] as char
        })
        .collect();
    format!("AKIA{tail}")
}

fn seeded(dir: &Path, key: &str) {
    fs::create_dir_all(dir.join("config")).unwrap();
    fs::write(
        dir.join("config/settings.py"),
        format!("AWS_ACCESS_KEY_ID = \"{key}\"\n"),
    )
    .unwrap();
    fs::write(
        dir.join("app.py"),
        "import subprocess\n\ndef run(cmd):\n    return subprocess.call(cmd, shell=True)\n",
    )
    .unwrap();
    fs::write(dir.join("requirements.txt"), "django==2.2.0\n").unwrap();
    fs::write(dir.join(".semgrep.yml"), SEMGREP_RULES).unwrap();
}

#[test]
fn seeded_vulnerabilities_are_detected_with_redacted_evidence() {
    if !enabled() {
        return;
    }
    let project = tempfile::tempdir().unwrap();
    let key = fake_aws_key();
    seeded(project.path(), &key);
    let out = tempfile::tempdir().unwrap();
    let r = scan(project.path(), &ALL, out.path(), Duration::from_secs(900)).unwrap();
    for s in &r.scanners {
        eprintln!(
            "{:?} {} {:?} findings={} {:?}",
            s.scanner, s.status, s.version, s.findings, s.notes
        );
        assert_eq!(s.status, "ran", "{s:?}");
    }
    assert_eq!(r.status, "findings");
    assert!(r.coverage_complete);
    assert!(out.path().join("security-audit.md").is_file());
    let by = |tool: Scanner| {
        r.findings.iter().filter(move |f| {
            f.tool
                .to_ascii_lowercase()
                .contains(&tool.id().replace("-scanner", ""))
        })
    };

    let secret = by(Scanner::Gitleaks)
        .find(|f| {
            f.file
                .as_deref()
                .is_some_and(|p| p.ends_with("config/settings.py"))
        })
        .expect("gitleaks found the key");
    assert_eq!(secret.line, Some(1));
    assert!(
        by(Scanner::Semgrep)
            .any(|f| f.rule_id.ends_with("qualyx-subprocess-shell-true") && f.line == Some(4)),
        "{:#?}",
        r.findings
    );
    assert!(
        by(Scanner::OsvScanner).any(|f| f.message.to_lowercase().contains("django")),
        "{:#?}",
        r.findings
    );
    assert!(
        by(Scanner::Trivy).any(|f| f.message.to_lowercase().contains("django")),
        "{:#?}",
        r.findings
    );

    // The key never reaches stored evidence in clear.
    for entry in fs::read_dir(out.path()).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
        assert!(!text.contains(&key), "secret leaked into evidence");
    }
    let merged: serde_json::Value =
        serde_json::from_slice(&fs::read(out.path().join("merged.sarif")).unwrap()).unwrap();
    assert_eq!(merged["runs"].as_array().unwrap().len(), 4);
}

#[test]
fn clean_project_has_no_secret_or_sast_findings() {
    if !enabled() {
        return;
    }
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("app.py"),
        "def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    fs::write(project.path().join(".semgrep.yml"), SEMGREP_RULES).unwrap();
    let out = tempfile::tempdir().unwrap();
    let r = scan(
        project.path(),
        &[Scanner::Gitleaks, Scanner::Semgrep],
        out.path(),
        Duration::from_secs(300),
    )
    .unwrap();
    assert!(
        r.scanners.iter().all(|s| s.status == "ran"),
        "{:?}",
        r.scanners
    );
    assert_eq!(r.status, "clean", "{:#?}", r.findings);
    assert!(r.coverage_complete);
}

#[test]
fn broken_rules_cannot_reuse_stale_evidence_or_hide_behind_a_clean_scanner() {
    if !enabled() {
        return;
    }
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("app.py"), "print('hello')\n").unwrap();
    fs::write(project.path().join(".semgrep.yml"), "rules: [invalid").unwrap();
    let out = tempfile::tempdir().unwrap();
    fs::write(
        out.path().join("semgrep.sarif"),
        r#"{"version":"2.1.0","runs":[{"tool":{"driver":{"name":"semgrep"}},"results":[]}]}"#,
    )
    .unwrap();
    let report = scan(
        project.path(),
        &[Scanner::Gitleaks, Scanner::Semgrep],
        out.path(),
        Duration::from_secs(120),
    )
    .unwrap();
    assert_eq!(report.scanners[0].status, "ran");
    assert_eq!(report.scanners[1].status, "error");
    assert!(!report.coverage_complete);
    assert_eq!(report.status, "inconclusive");
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(out.path().join("security-report.json")).unwrap())
            .unwrap();
    assert_eq!(stored["status"], "inconclusive");
}

#[test]
fn timed_out_scanner_is_inconclusive() {
    if !enabled() {
        return;
    }
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("app.py"), "print('hello')\n").unwrap();
    fs::write(project.path().join(".semgrep.yml"), SEMGREP_RULES).unwrap();
    let out = tempfile::tempdir().unwrap();
    let report = scan(
        project.path(),
        &[Scanner::Semgrep],
        out.path(),
        Duration::ZERO,
    )
    .unwrap();
    assert!(report.scanners[0].timed_out);
    assert_eq!(report.status, "inconclusive");
    assert!(!report.coverage_complete);
}
