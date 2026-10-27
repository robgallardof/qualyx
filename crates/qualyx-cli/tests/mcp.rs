//! MCP compliance and safety, speaking raw JSON-RPC 2.0 over stdio to the real binary
//! (independent of the SDK the server is built with).

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_qualyx");

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start(root: &Path) -> Client {
        let mut child = Command::new(BIN)
            .args(["mcp", "--root"])
            .arg(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Client {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let init = c.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "qualyx-compliance-test", "version": "0"}
            }),
        );
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        assert!(
            init["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("qa_run")
        );
        c.notify("notifications/initialized", json!({}));
        c
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(
                self.stdout.read_line(&mut line).unwrap() > 0,
                "server closed stdout"
            );
            let v: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("non-JSON line {line:?}: {e}"));
            assert_eq!(v["jsonrpc"], "2.0");
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    /// Call a tool; returns (is_error, parsed text content).
    fn call(&mut self, name: &str, args: Value) -> (bool, Value) {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let result = &r["result"];
        assert!(result.is_object(), "protocol error for {name}: {r}");
        let text = result["content"][0]["text"].as_str().unwrap_or_default();
        let body = serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()));
        (result["isError"].as_bool().unwrap_or(false), body)
    }

    fn wait_finished(&mut self, run_id: &str, limit: Duration) -> Value {
        let t = Instant::now();
        loop {
            let (err, s) = self.call("qa_run_status", json!({"run_id": run_id}));
            assert!(!err, "{s}");
            if s["state"] != "running" {
                return s;
            }
            assert!(
                t.elapsed() < limit,
                "run {run_id} still running after {limit:?}"
            );
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn python_project(dir: &Path, body: &str) {
    fs::create_dir_all(dir.join("tests")).unwrap();
    fs::write(dir.join("pytest.ini"), "[pytest]\n").unwrap();
    fs::write(dir.join("tests/test_x.py"), body).unwrap();
}

#[test]
fn lists_tools_with_schemas() {
    let root = tempfile::tempdir().unwrap();
    let mut c = Client::start(root.path());
    let r = c.request("tools/list", json!({}));
    let tools = r["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "qa_a11y",
            "qa_analyze_failure",
            "qa_browser_session",
            "qa_cancel",
            "qa_capabilities",
            "qa_desktop_session",
            "qa_discover",
            "qa_doctor",
            "qa_flaky",
            "qa_get_artifact",
            "qa_list_runs",
            "qa_performance_run",
            "qa_report",
            "qa_review",
            "qa_run",
            "qa_run_status",
            "qa_security_scan",
            "qa_test_impact",
            "qa_verify"
        ]
    );
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["description"].as_str().is_some_and(|d| !d.is_empty()));
    }
    let run = tools.iter().find(|t| t["name"] == "qa_run").unwrap();
    assert_eq!(run["inputSchema"]["required"], json!(["path"]));
    let discover = tools.iter().find(|t| t["name"] == "qa_discover").unwrap();
    assert_eq!(discover["annotations"]["readOnlyHint"], true);
}

#[test]
fn unknown_method_and_bad_params_are_protocol_errors() {
    let root = tempfile::tempdir().unwrap();
    let mut c = Client::start(root.path());
    let r = c.request("no/such/method", json!({}));
    assert!(r["error"]["code"].is_i64(), "{r}");
    // Invalid arguments may surface as a JSON-RPC error or as a tool result with isError
    // (both are valid MCP); what matters is that nothing runs and the reason is reported.
    let r = c.request(
        "tools/call",
        json!({"name": "qa_discover", "arguments": {"wrong": 1}}),
    );
    let tool_error = r["result"]["isError"] == true
        && r["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("path");
    assert!(r["error"]["code"].is_i64() || tool_error, "{r}");
    let r = c.request(
        "tools/call",
        json!({"name": "no_such_tool", "arguments": {}}),
    );
    assert!(
        r["error"]["code"].is_i64() || r["result"]["isError"] == true,
        "{r}"
    );
}

#[test]
fn run_status_report_artifact_verify_end_to_end() {
    let root = tempfile::tempdir().unwrap();
    python_project(
        root.path(),
        "def test_ok():\n    assert 1 == 1\n\ndef test_bad():\n    assert 2 + 2 == 5\n",
    );
    let mut c = Client::start(root.path());
    let path = root.path().display().to_string();

    let (err, d) = c.call("qa_discover", json!({"path": path}));
    assert!(!err, "{d}");
    assert_eq!(d["ecosystems"][0]["ecosystem"], "python");

    let (err, started) = c.call("qa_run", json!({"path": path}));
    assert!(!err, "{started}");
    assert_eq!(started["state"], "running");
    let run_id = started["run_id"].as_str().unwrap().to_string();

    let s = c.wait_finished(&run_id, Duration::from_secs(120));
    assert_eq!(s["state"], "finished");
    assert_eq!(s["status"], "failed");
    assert_eq!(s["counts"]["passed"], 1);
    assert_eq!(s["failures"][0]["name"], "test_bad");

    let (err, rep) = c.call("qa_report", json!({"path": path, "format": "markdown"}));
    assert!(!err);
    assert!(rep.as_str().unwrap().contains("test_bad"));

    let (err, log) = c.call(
        "qa_get_artifact",
        json!({"path": path, "run_id": run_id, "name": "pytest.junit.xml"}),
    );
    assert!(!err, "{log}");
    assert!(log.as_str().unwrap().contains("testcase"));

    let (err, tri) = c.call(
        "qa_analyze_failure",
        json!({"path": path, "run_id": run_id}),
    );
    assert!(!err, "{tri}");
    assert_eq!(tri["groups"][0]["tests"][0], "test_bad");
    assert!(
        tri["groups"][0]["reproduce"][0]["command"]
            .to_string()
            .contains("test_bad")
    );

    let (err, v) = c.call("qa_verify", json!({"path": path, "run_id": run_id}));
    assert!(!err);
    assert_eq!(v["intact"], true);

    let (_, runs) = c.call("qa_list_runs", json!({"path": path}));
    assert_eq!(runs[0]["run_id"], run_id.as_str());

    // The audit trail recorded the calls.
    let audit = fs::read_to_string(root.path().join(".qualyx/mcp-audit.jsonl")).unwrap();
    assert!(
        audit
            .lines()
            .any(|l| l.contains("\"qa_run\"") && l.contains("allowed"))
    );
}

#[test]
fn cancel_stops_a_long_run_quickly() {
    let root = tempfile::tempdir().unwrap();
    python_project(
        root.path(),
        "import time\n\ndef test_slow():\n    time.sleep(120)\n",
    );
    let mut c = Client::start(root.path());
    let (_, started) = c.call("qa_run", json!({"path": root.path().display().to_string()}));
    let run_id = started["run_id"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_secs(3));
    let (err, r) = c.call("qa_cancel", json!({"run_id": run_id}));
    assert!(!err, "{r}");
    let t = Instant::now();
    let s = c.wait_finished(&run_id, Duration::from_secs(30));
    assert!(t.elapsed() < Duration::from_secs(30));
    assert_eq!(s["status"], "error");
    assert!(s["runners"][0]["notes"].to_string().contains("cancelled"));
}

#[test]
fn paths_outside_roots_and_artifact_traversal_are_denied() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    python_project(root.path(), "def test_ok():\n    assert True\n");
    let mut c = Client::start(root.path());

    let (err, msg) = c.call(
        "qa_run",
        json!({"path": outside.path().display().to_string()}),
    );
    assert!(err);
    assert!(msg.as_str().unwrap().contains("outside the allowed roots"));
    // `..` from inside the root is resolved before the check.
    let sneaky = root
        .path()
        .join("..")
        .join(outside.path().file_name().unwrap());
    let (err, _) = c.call("qa_discover", json!({"path": sneaky.display().to_string()}));
    assert!(err);

    let path = root.path().display().to_string();
    let (_, started) = c.call("qa_run", json!({"path": path}));
    let run_id = started["run_id"].as_str().unwrap().to_string();
    c.wait_finished(&run_id, Duration::from_secs(120));
    for name in [
        "../../../etc/passwd",
        "../qualyx.db",
        "C:\\Windows\\win.ini",
    ] {
        let (err, msg) = c.call(
            "qa_get_artifact",
            json!({"path": path, "run_id": run_id, "name": name}),
        );
        assert!(err, "{name} was readable: {msg}");
    }
    let (err, _) = c.call("qa_run", json!({"path": path, "isolation": "vm"}));
    assert!(err);

    let audit = fs::read_to_string(root.path().join(".qualyx/mcp-audit.jsonl")).unwrap();
    assert!(audit.lines().any(|l| l.contains("denied")));
}

#[test]
fn browser_flows_cannot_reach_remote_origins() {
    let root = tempfile::tempdir().unwrap();
    let flow = root.path().join("remote.json");
    fs::write(
        &flow,
        r#"{"name":"x","steps":[{"action":"goto","url":"http://169.254.169.254/latest/meta-data"}]}"#,
    )
    .unwrap();
    let mut c = Client::start(root.path());
    let (err, msg) = c.call(
        "qa_browser_session",
        json!({"path": root.path().display().to_string(), "flow_path": flow.display().to_string()}),
    );
    assert!(err);
    assert!(msg.as_str().unwrap().contains("not loopback"), "{msg}");
    // A flow file outside the roots is refused before it is even read.
    let outside = tempfile::tempdir().unwrap();
    let f2 = outside.path().join("f.json");
    fs::write(&f2, "{}").unwrap();
    let (err, msg) = c.call(
        "qa_browser_session",
        json!({"path": root.path().display().to_string(), "flow_path": f2.display().to_string()}),
    );
    assert!(
        err && msg.as_str().unwrap().contains("outside the allowed roots"),
        "{msg}"
    );
}

#[test]
fn desktop_session_is_read_only() {
    let root = tempfile::tempdir().unwrap();
    let mut c = Client::start(root.path());
    let (err, probe) = c.call("qa_desktop_session", json!({"action": "probe"}));
    assert!(!err, "{probe}");
    assert_eq!(probe["input"], "not-implemented");
    let (err, _) = c.call("qa_desktop_session", json!({"action": "click"}));
    assert!(err);
}

#[test]
fn load_tests_against_unauthorized_targets_are_blocked() {
    let root = tempfile::tempdir().unwrap();
    let w = root.path().join("prod.toml");
    fs::write(
        &w,
        "name = \"prod\"\ntarget = \"https://example.com/checkout\"\nvus = 5\n",
    )
    .unwrap();
    let mut c = Client::start(root.path());
    let (err, msg) = c.call(
        "qa_performance_run",
        json!({"path": root.path().display().to_string(), "workload_path": w.display().to_string()}),
    );
    assert!(err);
    assert!(msg.as_str().unwrap().contains("not loopback"), "{msg}");

    // Loopback but beyond the configured ceiling.
    fs::write(
        &w,
        "name = \"big\"\ntarget = \"http://127.0.0.1:9/\"\nvus = 5000\n",
    )
    .unwrap();
    let (err, msg) = c.call(
        "qa_performance_run",
        json!({"path": root.path().display().to_string(), "workload_path": w.display().to_string()}),
    );
    assert!(err && msg.as_str().unwrap().contains("vus"), "{msg}");
}
