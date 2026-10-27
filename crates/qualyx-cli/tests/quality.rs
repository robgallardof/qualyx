//! M6 end to end: every check must catch its seeded bug and stay quiet on its control.
//! Tool-dependent tests are opt-in: QUALYX_TEST_MUTANTS, QUALYX_TEST_SCHEMATHESIS,
//! QUALYX_TEST_DOCKER, QUALYX_TEST_BROWSERS.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_qualyx");

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dst = to.join(e.file_name());
        if e.path().is_dir() {
            copy(&e.path(), &dst);
        } else {
            fs::copy(e.path(), dst).unwrap();
        }
    }
}

fn qualyx(args: &[&str], dir: &Path) -> Output {
    Command::new(BIN).args(args).arg(dir).output().unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn opt_in(var: &str) -> bool {
    let on = std::env::var(var).as_deref() == Ok("1");
    if !on {
        eprintln!("NOT RUN: set {var}=1");
    }
    on
}

#[test]
fn repeat_detects_the_seeded_flaky_test_only() {
    let d = tempfile::tempdir().unwrap();
    copy(&fixture("flaky"), d.path());
    let o = qualyx(&["test", "--repeat", "2"], d.path());
    let t = text(&o);
    assert_eq!(o.status.code(), Some(1), "{t}");
    assert!(t.contains("FLAKY python test_alternates"), "{t}");
    assert!(!t.contains("FLAKY python test_stable"), "{t}");
}

#[test]
fn report_lists_skipped_tests_explicitly() {
    let d = tempfile::tempdir().unwrap();
    copy(&fixture("python/passing"), d.path());
    assert_eq!(qualyx(&["test"], d.path()).status.code(), Some(0));
    let md = String::from_utf8_lossy(&qualyx(&["report", "--format", "markdown"], d.path()).stdout)
        .into_owned();
    assert!(md.contains("### Skipped (1)"), "{md}");
    assert!(
        md.contains("test_skipped_on_purpose") && md.contains("seeded skip"),
        "{md}"
    );
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

#[test]
fn reviewer_flags_weakened_tests_and_accepts_new_ones() {
    let d = tempfile::tempdir().unwrap();
    copy(&fixture("mutation/strong"), d.path());
    fs::create_dir_all(d.path().join("tests")).unwrap();
    fs::write(
        d.path().join("tests/limits.rs"),
        "use fixture_mutation_strong::is_adult;\n\n#[test]\nfn boundary() {\n    assert!(!is_adult(17));\n    assert!(is_adult(18));\n}\n\n#[test]\nfn adult() {\n    assert!(is_adult(40));\n}\n",
    )
    .unwrap();
    git(d.path(), &["init", "-q"]);
    git(d.path(), &["add", "-A"]);
    git(d.path(), &["commit", "-q", "-m", "init"]);

    // Control: adding a test is fine.
    let mut f = fs::read_to_string(d.path().join("tests/limits.rs")).unwrap();
    f.push_str("\n#[test]\nfn child() {\n    assert!(!is_adult(3));\n}\n");
    fs::write(d.path().join("tests/limits.rs"), &f).unwrap();
    let o = qualyx(&["review", "--base", "HEAD", "--json"], d.path());
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["findings"], serde_json::json!([]), "{v:#}");
    assert_eq!(o.status.code(), Some(0));
    git(d.path(), &["commit", "-qam", "add child test"]);

    // Weakening: drop the boundary assertions and silence another test.
    let weakened = f
        .replace(
            "    assert!(!is_adult(17));\n    assert!(is_adult(18));\n",
            "    assert!(is_adult(30));\n",
        )
        .replace("#[test]\nfn adult()", "#[test]\n#[ignore]\nfn adult()");
    fs::write(d.path().join("tests/limits.rs"), weakened).unwrap();
    let o = qualyx(&["review", "--base", "HEAD", "--json"], d.path());
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let kinds: Vec<&str> = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"assertions-removed") && kinds.contains(&"skip-added"),
        "{v:#}"
    );
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn mutation_testing_separates_weak_and_strong_suites() {
    if !opt_in("QUALYX_TEST_MUTANTS") {
        return;
    }
    let weak = tempfile::tempdir().unwrap();
    copy(&fixture("mutation/weak"), weak.path());
    let o = qualyx(&["mutation", "--json"], weak.path());
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o)));
    assert_eq!(o.status.code(), Some(1));
    assert!(v["missed"].as_u64().unwrap() >= 1, "{v:#}");
    assert!(
        v["missed_mutants"].to_string().contains("is_adult"),
        "{v:#}"
    );

    let strong = tempfile::tempdir().unwrap();
    copy(&fixture("mutation/strong"), strong.path());
    let o = qualyx(&["mutation", "--json"], strong.path());
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o)));
    assert_eq!(v["missed"], 0, "{v:#}");
    assert_eq!(o.status.code(), Some(0));
}

struct Api(Child, u16);

impl Drop for Api {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn start_api() -> Api {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let python = which::which("python")
        .or_else(|_| which::which("python3"))
        .unwrap();
    let child = Command::new(python)
        .arg(fixture("api/server.py"))
        .arg(port.to_string())
        .spawn()
        .unwrap();
    let t = Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "fixture API did not start"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    Api(child, port)
}

#[test]
fn contract_testing_catches_the_seeded_server_error() {
    if !opt_in("QUALYX_TEST_SCHEMATHESIS") {
        return;
    }
    let api = start_api();
    let project = tempfile::tempdir().unwrap();
    let o = Command::new(BIN)
        .args([
            "contract",
            &format!("http://127.0.0.1:{}/openapi.json", api.1),
            "--max-examples",
            "100",
            "--json",
            "--project",
        ])
        .arg(project.path())
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o)));
    assert_eq!(v["status"], "failed", "{v:#}");
    assert!(
        v["operations"].to_string().contains("boom"),
        "seeded 500 not reported: {v:#}"
    );
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn contract_testing_refuses_unauthorized_targets() {
    let project = tempfile::tempdir().unwrap();
    let o = Command::new(BIN)
        .args([
            "contract",
            "https://api.example.com/openapi.json",
            "--project",
        ])
        .arg(project.path())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(text(&o).contains("not loopback"), "{}", text(&o));
}

#[test]
fn compose_environment_is_created_used_and_removed() {
    if !opt_in("QUALYX_TEST_DOCKER") {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    copy(&fixture("compose"), d.path());
    let compose = d.path().join("compose.yaml");
    let o = Command::new(BIN)
        .arg("test")
        .arg(d.path())
        .arg("--compose")
        .arg(&compose)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let m: Value =
        serde_json::from_slice(&qualyx(&["report", "--format", "json"], d.path()).stdout).unwrap();
    assert!(
        m["services"][0]
            .as_str()
            .unwrap()
            .starts_with("QUALYX_SVC_WEB_80=127.0.0.1:"),
        "{m:#}"
    );
    assert_eq!(m["counts"]["passed"], 1);
    let project = format!(
        "qualyx-{}",
        m["run_id"].as_str().unwrap().to_ascii_lowercase()
    );
    let ps = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("label=com.docker.compose.project={project}"),
            "-q",
        ])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&ps.stdout).trim().is_empty(),
        "containers left behind"
    );
}

#[test]
fn a11y_command_reports_seeded_violations() {
    if !opt_in("QUALYX_TEST_BROWSERS") {
        return;
    }
    let project = tempfile::tempdir().unwrap();
    let o = Command::new(BIN)
        .args(["a11y", "/a11y-bad.html", "--json", "--serve"])
        .arg(fixture("web/site"))
        .arg("--project")
        .arg(project.path())
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o)));
    let ids = v[0]["violations"].to_string();
    for seeded in ["image-alt", "label", "color-contrast"] {
        assert!(ids.contains(seeded), "{seeded}: {ids}");
    }
    assert_eq!(o.status.code(), Some(1));
}
