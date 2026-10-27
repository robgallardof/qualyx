//! End-to-end: the real `qualyx` binary against the seeded fixtures with real native tools.
//!
//! Requires cargo, dotnet, node/npm and python+pytest on PATH (CI installs all of them).
//! A missing tool makes the test fail loudly instead of being skipped silently.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qualyx_core::{Detail, Ecosystem, RunManifest, Status};

const BIN: &str = env!("CARGO_BIN_EXE_qualyx");

fn fixture(ecosystem: &str, kind: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(ecosystem)
        .join(kind)
}

/// Copy a fixture into a temp dir so runs never write into the repository.
fn copy_dir(from: &Path, to: &Path) {
    const SKIP: &[&str] = &[
        "target",
        "bin",
        "obj",
        "node_modules",
        ".qualyx",
        "__pycache__",
        ".pytest_cache",
    ];
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if SKIP.contains(&name.to_string_lossy().as_ref()) {
            continue;
        }
        let (src, dst) = (entry.path(), to.join(&name));
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            fs::copy(&src, &dst).unwrap();
        }
    }
}

struct Outcome {
    exit: i32,
    manifest: RunManifest,
    _dir: tempfile::TempDir,
}

fn run_fixture(ecosystem: &str, kind: &str) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture(ecosystem, kind), dir.path());
    let test = Command::new(BIN)
        .arg("test")
        .arg(dir.path())
        .output()
        .unwrap();
    let report = Command::new(BIN)
        .args(["report", "--format", "json"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(
        report.status.success(),
        "report failed: {}",
        String::from_utf8_lossy(&report.stderr)
    );
    let manifest: RunManifest = serde_json::from_slice(&report.stdout).unwrap();
    Outcome {
        exit: test.status.code().unwrap(),
        manifest,
        _dir: dir,
    }
}

fn names_with(m: &RunManifest, status: Status) -> Vec<String> {
    let mut v: Vec<String> = m.runners[0]
        .tests
        .iter()
        .filter(|t| t.status == status)
        .map(|t| t.name.clone())
        .collect();
    v.sort();
    v
}

fn assert_passing(ecosystem: Ecosystem, kind: &str, passed: &[&str], skipped: &[&str]) {
    let o = run_fixture(ecosystem.as_str(), kind);
    let r = &o.manifest.runners;
    assert_eq!(r.len(), 1, "{:#?}", o.manifest);
    assert_eq!(r[0].ecosystem, ecosystem);
    assert_eq!(r[0].detail, Detail::PerTest);
    assert_eq!(o.manifest.status, Status::Passed, "notes: {:?}", r[0].notes);
    assert_eq!(o.exit, 0);
    assert_eq!(names_with(&o.manifest, Status::Passed), passed);
    assert_eq!(names_with(&o.manifest, Status::Skipped), skipped);
    assert!(r[0].tool_version.is_some());
}

fn assert_failing(ecosystem: Ecosystem, passed: &str, failed: &str, message_contains: &str) {
    let o = run_fixture(ecosystem.as_str(), "failing");
    let r = &o.manifest.runners[0];
    assert_eq!(o.manifest.status, Status::Failed, "notes: {:?}", r.notes);
    assert_eq!(o.exit, 1);
    assert_eq!(names_with(&o.manifest, Status::Passed), [passed]);
    assert_eq!(names_with(&o.manifest, Status::Failed), [failed]);
    let failure = r.tests.iter().find(|t| t.status == Status::Failed).unwrap();
    let msg = failure.message.as_deref().unwrap_or_default();
    assert!(msg.contains(message_contains), "message was: {msg}");
}

#[test]
fn rust_passing() {
    assert_passing(
        Ecosystem::Rust,
        "passing",
        &["tests::adds_two_numbers", "tests::multiplies_two_numbers"],
        &["tests::skipped_on_purpose"],
    );
}

#[test]
fn rust_failing() {
    assert_failing(
        Ecosystem::Rust,
        "tests::adds_two_numbers",
        "tests::multiplies_two_numbers",
        "left: -1",
    );
}

#[test]
fn dotnet_passing() {
    assert_passing(
        Ecosystem::Dotnet,
        "passing",
        &[
            "Passing.CalculatorTests.AddsTwoNumbers",
            "Passing.CalculatorTests.MultipliesTwoNumbers",
        ],
        &["Passing.CalculatorTests.SkippedOnPurpose"],
    );
}

#[test]
fn dotnet_failing() {
    assert_failing(
        Ecosystem::Dotnet,
        "Failing.CalculatorTests.AddsTwoNumbers",
        "Failing.CalculatorTests.MultipliesTwoNumbers",
        "Expected: 6",
    );
}

#[test]
fn node_passing() {
    assert_passing(
        Ecosystem::Node,
        "passing",
        &["adds two numbers", "multiplies two numbers"],
        &["skipped on purpose"],
    );
}

#[test]
fn node_failing() {
    assert_failing(
        Ecosystem::Node,
        "adds two numbers",
        "multiplies two numbers",
        "-1 !== 6",
    );
}

#[test]
fn python_passing() {
    assert_passing(
        Ecosystem::Python,
        "passing",
        &["test_adds_two_numbers", "test_multiplies_two_numbers"],
        &["test_skipped_on_purpose"],
    );
}

#[test]
fn python_failing() {
    assert_failing(
        Ecosystem::Python,
        "test_adds_two_numbers",
        "test_multiplies_two_numbers",
        "assert -1 == 6",
    );
}

#[test]
fn empty_project_is_inconclusive_not_green() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(BIN)
        .arg("test")
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn run_ids_cannot_escape_the_run_store() {
    let o = run_fixture("python", "passing");
    let out = Command::new(BIN)
        .args(["report", "--run", "../../etc", "--format", "json"])
        .arg(&o.manifest.project_root)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid run id"));
}

#[test]
fn evidence_files_exist_for_every_artifact() {
    let o = run_fixture("node", "failing");
    let run_dir = o
        .manifest
        .project_root
        .join(".qualyx/runs")
        .join(&o.manifest.run_id);
    assert!(run_dir.join("manifest.json").is_file());
    assert!(run_dir.join("report.md").is_file());
    for a in &o.manifest.runners[0].artifacts {
        assert!(run_dir.join(a).is_file(), "missing artifact {a}");
    }
}
