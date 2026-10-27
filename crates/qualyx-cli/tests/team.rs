//! M7 end to end: HTTP API (auth, roots, async runs, artifacts, recovery) and the shared
//! PostgreSQL index (explicit ignored test, needs Docker).

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_qualyx");
const TOKEN: &str = "test-token-0123456789-abcdefghijklmnop";

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

struct Server(Child, String);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn serve(root: &Path) -> Server {
    let port = free_port();
    let child = Command::new(BIN)
        .args(["serve", "--bind", &format!("127.0.0.1:{port}"), "--root"])
        .arg(root)
        .env("QUALYX_API_TOKEN", TOKEN)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let t = Instant::now();
    while ureq::get(&format!("{base}/health")).call().is_err() {
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "server did not start"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    Server(child, base)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into()
}

fn get(url: &str, token: Option<&str>) -> (u16, Value) {
    let mut r = agent().get(url);
    if let Some(t) = token {
        r = r.header("authorization", format!("Bearer {t}"));
    }
    let mut resp = r.call().unwrap();
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

fn post(url: &str, body: &Value) -> (u16, Value) {
    let mut resp = agent()
        .post(url)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .send(body.to_string())
        .unwrap();
    let status = resp.status().as_u16();
    (
        status,
        serde_json::from_str(&resp.body_mut().read_to_string().unwrap()).unwrap(),
    )
}

fn q(p: &Path) -> String {
    p.display()
        .to_string()
        .replace('\\', "%5C")
        .replace(':', "%3A")
        .replace(' ', "%20")
}

#[test]
#[ignore = "requires an installed Chromium-family browser"]
fn dashboard_recovers_from_auth_empty_history_and_network_failure() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("app");
    copy(&fixture("python/passing"), &project);
    let mut server = serve(root.path());
    let browser = qualyx_browser::detect()
        .into_iter()
        .find(|b| b.family == qualyx_browser::Family::Chromium)
        .expect("installed Chromium-family browser");
    let mut driver = qualyx_browser::launch(&browser, true).unwrap();
    driver.navigate(&server.1).unwrap();
    driver
        .eval(&format!(
            "document.getElementById('project').value = {}",
            serde_json::to_string(&project).unwrap()
        ))
        .unwrap();

    fn load(driver: &mut dyn qualyx_browser::Driver) -> String {
        driver
            .eval("document.getElementById('f').requestSubmit(); true")
            .unwrap();
        let start = Instant::now();
        loop {
            if driver
                .eval("!document.getElementById('load').disabled")
                .unwrap()
                == true
            {
                return driver
                    .eval("document.getElementById('msg').textContent")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string();
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "dashboard stuck loading"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    assert_eq!(load(driver.as_mut()), "missing or invalid bearer token");
    driver
        .eval(&format!(
            "document.getElementById('token').value = {}",
            serde_json::to_string(TOKEN).unwrap()
        ))
        .unwrap();
    assert_eq!(load(driver.as_mut()), "No runs found for this project.");
    assert!(
        Command::new(BIN)
            .arg("test")
            .arg(&project)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(load(driver.as_mut()).contains("1 run(s) shown"));
    assert_eq!(
        driver
            .eval("document.querySelectorAll('#rows tr').length")
            .unwrap(),
        1
    );
    server.0.kill().unwrap();
    server.0.wait().unwrap();
    assert!(load(driver.as_mut()).contains("Could not load runs"));
    assert_eq!(
        driver
            .eval("document.querySelectorAll('#rows tr').length")
            .unwrap(),
        0
    );
    assert_eq!(
        driver
            .eval("document.getElementById('load').disabled")
            .unwrap(),
        false
    );
}

#[test]
fn refuses_missing_or_weak_tokens_on_every_bind() {
    for bind in ["0.0.0.0:0", "127.0.0.1:0"] {
        for token in [
            None,
            Some(""),
            Some("weak"),
            Some("a token with spaces is not permitted here"),
        ] {
            let mut command = Command::new(BIN);
            command
                .args(["serve", "--bind", bind, "--root", "."])
                .env_remove("QUALYX_API_TOKEN");
            if let Some(token) = token {
                command.env("QUALYX_API_TOKEN", token);
            }
            let output = command.output().unwrap();
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("QUALYX_API_TOKEN"));
        }
    }
}

#[test]
fn api_runs_tests_with_auth_roots_and_artifacts() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("app");
    copy(&fixture("python/failing"), &project);
    let s = serve(root.path());

    assert_eq!(get(&format!("{}/health", s.1), None).0, 200);
    let (status, _) = get(&format!("{}/", s.1), None);
    assert_eq!(status, 200, "dashboard");
    assert_eq!(get(&format!("{}/v1/runs", s.1), None).0, 401);
    assert_eq!(get(&format!("{}/v1/runs", s.1), Some("wrong")).0, 401);
    let (empty_status, empty_history) = get(
        &format!("{}/v1/runs?project={}", s.1, q(&project)),
        Some(TOKEN),
    );
    assert_eq!(empty_status, 200);
    assert_eq!(empty_history["runs"], serde_json::json!([]));
    assert!(
        !project.join(".qualyx").exists(),
        "history reads must not initialize state"
    );

    let origin = agent()
        .get(&format!("{}/v1/runs", s.1))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("origin", "https://hostile.example")
        .call()
        .unwrap();
    assert_eq!(origin.status().as_u16(), 403);
    let large = agent()
        .post(&format!("{}/v1/runs", s.1))
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .send(serde_json::json!({"project": "x".repeat(20_000)}).to_string())
        .unwrap();
    assert_eq!(large.status().as_u16(), 413);

    let outside = tempfile::tempdir().unwrap();
    let (st, body) = post(
        &format!("{}/v1/runs", s.1),
        &serde_json::json!({"project": outside.path()}),
    );
    assert_eq!(st, 403, "{body}");

    let (st, body) = post(
        &format!("{}/v1/runs", s.1),
        &serde_json::json!({"project": project}),
    );
    assert_eq!(st, 202, "{body}");
    let id = body["run_id"].as_str().unwrap().to_string();

    let t = Instant::now();
    let run = loop {
        let (_, v) = get(&format!("{}/v1/runs/{id}", s.1), Some(TOKEN));
        if v["state"] != "running" {
            break v;
        }
        assert!(t.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(300));
    };
    assert_eq!(run["state"], "finished", "{run}");
    assert_eq!(run["manifest"]["status"], "failed");

    let (st, log) = get(
        &format!(
            "{}/v1/runs/{id}/artifacts/pytest.junit.xml?project={}",
            s.1,
            q(&project)
        ),
        Some(TOKEN),
    );
    assert_eq!(st, 200);
    assert!(
        log.as_str()
            .unwrap()
            .contains("test_multiplies_two_numbers")
    );
    let (st, _) = get(
        &format!(
            "{}/v1/runs/{id}/artifacts/..%2F..%2Fqualyx.db?project={}",
            s.1,
            q(&project)
        ),
        Some(TOKEN),
    );
    assert_eq!(st, 404, "artifact traversal");

    let (_, list) = get(
        &format!("{}/v1/runs?project={}", s.1, q(&project)),
        Some(TOKEN),
    );
    assert_eq!(list["runs"][0]["run_id"], id.as_str());
}

#[test]
fn explicit_recovery_records_interrupted_runs() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("app");
    copy(&fixture("python/passing"), &project);
    assert_eq!(
        Command::new(BIN)
            .arg("test")
            .arg(&project)
            .output()
            .unwrap()
            .status
            .code(),
        Some(0)
    );
    // Simulate a crash mid-run: a run dir with a log but no manifest.
    let crashed = project.join(".qualyx/runs/20991231T235959000Z-dead");
    fs::create_dir_all(&crashed).unwrap();
    fs::write(crashed.join("python.log"), "collecting ...").unwrap();

    // Starting another front end must not rewrite a potentially active CLI run.
    let server = serve(root.path());
    assert!(!crashed.join("manifest.json").exists());
    drop(server);

    let recovery = Command::new(BIN)
        .arg("recover")
        .arg(&project)
        .output()
        .unwrap();
    assert!(
        recovery.status.success(),
        "{}",
        String::from_utf8_lossy(&recovery.stderr)
    );

    let s = serve(root.path());
    let (_, list) = get(
        &format!("{}/v1/runs?project={}", s.1, q(&project)),
        Some(TOKEN),
    );
    let runs = list["runs"].as_array().unwrap();
    let rec = runs
        .iter()
        .find(|r| r["run_id"] == "20991231T235959000Z-dead")
        .expect("recovered run listed");
    assert_eq!(rec["status"], "error", "{list}");
    assert!(
        runs.iter().any(|r| r["status"] == "passed"),
        "earlier run untouched"
    );
}

#[test]
#[ignore = "requires real Docker PostgreSQL; run with --ignored"]
fn shared_postgres_index() {
    let name = format!("qualyx-pg-test-{}", std::process::id());
    let up = Command::new("docker")
        .args([
            "run",
            "-d",
            "--rm",
            "--name",
            &name,
            "-e",
            "POSTGRES_PASSWORD=qualyx",
            "-p",
            "127.0.0.1::5432",
            "docker.io/library/postgres:17-alpine",
        ])
        .output()
        .unwrap();
    assert!(
        up.status.success(),
        "{}",
        String::from_utf8_lossy(&up.stderr)
    );
    struct Rm(String);
    impl Drop for Rm {
        fn drop(&mut self) {
            let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
        }
    }
    let _rm = Rm(name.clone());
    let port = String::from_utf8_lossy(
        &Command::new("docker")
            .args(["port", &name, "5432/tcp"])
            .output()
            .unwrap()
            .stdout,
    )
    .lines()
    .next()
    .and_then(|l| l.rsplit(':').next())
    .unwrap()
    .trim()
    .to_string();
    let url = format!("postgres://postgres:qualyx@127.0.0.1:{port}/postgres");
    let t = Instant::now();
    while !Command::new("docker")
        .args([
            "exec",
            &name,
            "pg_isready",
            "-h",
            "127.0.0.1",
            "-U",
            "postgres",
        ])
        .output()
        .unwrap()
        .status
        .success()
    {
        assert!(t.elapsed() < Duration::from_secs(60), "postgres not ready");
        std::thread::sleep(Duration::from_millis(500));
    }
    std::thread::sleep(Duration::from_secs(2));

    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    copy(&fixture("python/passing"), a.path());
    copy(&fixture("python/failing"), b.path());
    for (dir, code) in [(a.path(), 0), (b.path(), 1), (a.path(), 0)] {
        let o = Command::new(BIN)
            .arg("test")
            .arg(dir)
            .env("QUALYX_DATABASE_URL", &url)
            .output()
            .unwrap();
        assert_eq!(
            o.status.code(),
            Some(code),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
    let list = |dir: &Path| {
        String::from_utf8_lossy(
            &Command::new(BIN)
                .args(["report", "--list"])
                .arg(dir)
                .env("QUALYX_DATABASE_URL", &url)
                .output()
                .unwrap()
                .stdout,
        )
        .into_owned()
    };
    let la = list(a.path());
    assert_eq!(la.lines().count(), 2, "{la}");
    assert!(la.contains("passed"));
    let lb = list(b.path());
    assert_eq!(lb.lines().count(), 1, "projects stay separate: {lb}");
    assert!(lb.contains("failed"));
    assert!(
        !a.path().join(".qualyx/qualyx.db").exists(),
        "no SQLite file in team mode"
    );
    // The report of the latest run is read back through the shared index.
    let o = Command::new(BIN)
        .args(["report", "--format", "json"])
        .arg(b.path())
        .env("QUALYX_DATABASE_URL", &url)
        .output()
        .unwrap();
    let m: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(m["status"], "failed");
    let schema = Command::new("docker")
        .args([
            "exec",
            &name,
            "psql",
            "-U",
            "postgres",
            "-tAc",
            "SELECT version FROM qualyx_schema",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&schema.stdout).trim(),
        "2",
        "migrations applied once"
    );

    // Recreate the exact v1 schema boundary, then upgrade with existing run history.
    let downgrade_fixture = Command::new("docker")
        .args([
            "exec",
            &name,
            "psql",
            "-U",
            "postgres",
            "-c",
            "DROP TABLE jobs; UPDATE qualyx_schema SET version=1",
        ])
        .output()
        .unwrap();
    assert!(downgrade_fixture.status.success());
    assert_eq!(
        list(a.path()).lines().count(),
        2,
        "v1-to-v2 upgrade preserves history"
    );
    let version = Command::new("docker")
        .args([
            "exec",
            &name,
            "psql",
            "-U",
            "postgres",
            "-tAc",
            "SELECT version FROM qualyx_schema",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&version.stdout).trim(), "2");

    let newer = Command::new("docker")
        .args([
            "exec",
            &name,
            "psql",
            "-U",
            "postgres",
            "-c",
            "UPDATE qualyx_schema SET version = 999",
        ])
        .output()
        .unwrap();
    assert!(newer.status.success());
    let refused = Command::new(BIN)
        .args(["report", "--list"])
        .arg(a.path())
        .env("QUALYX_DATABASE_URL", &url)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("unsupported PostgreSQL schema version")
    );
}

#[test]
fn simultaneous_requests_respect_capacity_and_cancel() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("app");
    copy(&fixture("python/passing"), &project);
    fs::write(
        project.join("tests/test_slow.py"),
        "import time\ndef test_slow():\n    time.sleep(120)\n",
    )
    .unwrap();
    let server = serve(root.path());
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let responses = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let barrier = barrier.clone();
                let url = format!("{}/v1/runs", server.1);
                let project = project.clone();
                scope.spawn(move || {
                    barrier.wait();
                    post(
                        &url,
                        &serde_json::json!({"project": project, "timeout_secs": 30}),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        responses.iter().filter(|(st, _)| *st == 202).count(),
        4,
        "{responses:?}"
    );
    assert_eq!(
        responses.iter().filter(|(st, _)| *st == 409).count(),
        2,
        "{responses:?}"
    );
    let ids: Vec<_> = responses
        .iter()
        .filter(|(st, _)| *st == 202)
        .map(|(_, body)| body["run_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        4
    );
    for id in &ids {
        assert_eq!(
            post(
                &format!("{}/v1/runs/{id}/cancel", server.1),
                &serde_json::json!({})
            )
            .0,
            202
        );
    }
    let start = Instant::now();
    for id in &ids {
        loop {
            let (status, body) = get(&format!("{}/v1/runs/{id}", server.1), Some(TOKEN));
            assert_eq!(status, 200);
            if body["state"] != "running" {
                assert_eq!(body["cancel_requested"], true);
                assert_ne!(body["manifest"]["status"], "passed", "{body}");
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "cancellation timed out"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
