//! M3 end to end: impact-based selection, parallel ecosystems and triage whose reproduction
//! commands are executed for real.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qualyx_core::{RunManifest, Status};
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_qualyx");

/// Serializes .NET builds within this test binary (concurrent first-run restores on a fresh
/// macOS runner failed once in CI run 40448534920).
static DOTNET: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn fixture(e: &str, k: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(e)
        .join(k)
}

fn copy_into(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dst = to.join(entry.file_name());
        if entry.path().is_dir() {
            fs::create_dir_all(&dst).unwrap();
            copy_into(&entry.path(), &dst);
        } else {
            fs::copy(entry.path(), dst).unwrap();
        }
    }
}

fn qualyx(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(BIN).args(args).arg(dir).output().unwrap()
}

fn manifest(dir: &Path) -> RunManifest {
    serde_json::from_slice(&qualyx(dir, &["report", "--format", "json"]).stdout).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

/// Rust + Python at the same root, committed.
fn polyglot() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    copy_into(&fixture("rust", "passing"), d.path());
    copy_into(&fixture("python", "passing"), d.path());
    fs::write(
        d.path().join(".gitignore"),
        "target/\n.qualyx/\n__pycache__/\n.pytest_cache/\n",
    )
    .unwrap();
    git(d.path(), &["init", "-q"]);
    git(d.path(), &["add", "-A"]);
    git(d.path(), &["commit", "-q", "-m", "init"]);
    d
}

#[test]
fn changed_since_runs_only_affected_ecosystems() {
    let d = polyglot();
    let test_file = d.path().join("tests/test_calculator.py");
    let body = fs::read_to_string(&test_file).unwrap();
    fs::write(
        &test_file,
        body + "\n\ndef test_added():\n    assert True\n",
    )
    .unwrap();

    let impact: Value =
        serde_json::from_slice(&qualyx(d.path(), &["impact", "--base", "HEAD", "--json"]).stdout)
            .unwrap();
    assert_eq!(
        impact["changed_files"],
        serde_json::json!(["tests/test_calculator.py"])
    );

    let out = qualyx(d.path(), &["test", "--changed-since", "HEAD"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let m = manifest(d.path());
    let ran: Vec<&str> = m.runners.iter().map(|r| r.ecosystem.as_str()).collect();
    assert_eq!(ran, ["python"]);
    let sel = m.selection.unwrap();
    assert!(
        sel.contains("rust skipped") && sel.contains("python all"),
        "{sel}"
    );
    assert_eq!(m.counts.passed, 3);
}

#[test]
fn docs_only_change_runs_nothing_and_is_not_green() {
    let d = polyglot();
    fs::write(d.path().join("NOTES.md"), "x").unwrap();
    let out = qualyx(d.path(), &["test", "--changed-since", "HEAD"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(manifest(d.path()).runners.is_empty());
}

#[test]
fn parallel_jobs_run_every_ecosystem() {
    let d = polyglot();
    let out = qualyx(d.path(), &["test", "--jobs", "2"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let m = manifest(d.path());
    assert_eq!(m.runners.len(), 2);
    assert!(m.runners.iter().all(|r| r.status == Status::Passed));
    // Discovery order is kept regardless of finishing order.
    assert_eq!(m.runners[0].ecosystem.as_str(), "rust");
}

#[test]
fn index_based_impact_without_git() {
    let d = tempfile::tempdir().unwrap();
    copy_into(&fixture("python", "passing"), d.path());
    assert!(qualyx(d.path(), &["index"]).status.success());
    let out = qualyx(d.path(), &["test", "--changed-since", "index"]);
    assert_eq!(out.status.code(), Some(2), "nothing changed, nothing ran");
    fs::write(
        d.path().join("tests/test_calculator.py"),
        "def test_x():\n    assert 1\n",
    )
    .unwrap();
    let out = qualyx(d.path(), &["test", "--changed-since", "index"]);
    assert_eq!(out.status.code(), Some(0));
}

/// Run the failing fixture, triage it, then execute the reproduction command and check it
/// fails on the same test and only that test.
fn triage_reproduces(e: &str, expected_test: &str, sibling: &str) {
    let d = tempfile::tempdir().unwrap();
    copy_into(&fixture(e, "failing"), d.path());
    assert_eq!(qualyx(d.path(), &["test"]).status.code(), Some(1));
    let t: Value = serde_json::from_slice(&qualyx(d.path(), &["triage", "--json"]).stdout).unwrap();
    let group = &t["groups"][0];
    assert_eq!(group["tests"][0], expected_test, "{t:#}");
    if e != "node" {
        assert!(!group["locations"].as_array().unwrap().is_empty(), "{t:#}");
    }
    let cmd: Vec<String> =
        serde_json::from_value(group["reproduce"][0]["command"].clone()).unwrap();
    let program = which::which(&cmd[0]).unwrap();
    let out = Command::new(program)
        .args(&cmd[1..])
        .current_dir(d.path())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "repro passed: {cmd:?}\n{text}");
    let short = expected_test.rsplit(['.', ':']).next().unwrap();
    assert!(
        text.contains(short),
        "repro output lacks {short}: {cmd:?}\n{text}"
    );
    assert!(
        !text.contains(sibling),
        "repro ran more than one test: {cmd:?}\n{text}"
    );
}

#[test]
fn triage_repro_rust() {
    triage_reproduces("rust", "tests::multiplies_two_numbers", "adds_two_numbers");
}

#[test]
fn triage_repro_python() {
    triage_reproduces(
        "python",
        "test_multiplies_two_numbers",
        "test_adds_two_numbers",
    );
}

#[test]
fn triage_repro_node() {
    triage_reproduces("node", "multiplies two numbers", "adds two numbers");
}

#[test]
fn triage_repro_dotnet() {
    let _serial = DOTNET.lock().unwrap_or_else(|e| e.into_inner());
    triage_reproduces(
        "dotnet",
        "Failing.CalculatorTests.MultipliesTwoNumbers",
        "AddsTwoNumbers",
    );
}
