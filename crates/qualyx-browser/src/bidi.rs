//! WebDriver BiDi driver (W3C) for Firefox-family browsers, spoken directly over WebSocket.

use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Map, Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::cdp::{is_timeout, set_read_timeout};
use crate::{ConsoleEntry, Driver, Result, err};

const CALL_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Bidi {
    child: Child,
    _profile: tempfile::TempDir,
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    context: String,
    console: Vec<ConsoleEntry>,
    failures: Vec<String>,
}

/// Firefox's startup window was swapped out under the call ("no such frame").
fn is_discarded(e: &crate::BrowserError) -> bool {
    e.to_string().contains("no such frame")
}

/// BiDi RemoteValue -> plain JSON.
pub fn remote_to_json(v: &Value) -> Value {
    match v["type"].as_str() {
        Some("string" | "boolean") => v["value"].clone(),
        Some("number") => match &v["value"] {
            Value::String(s) => Value::String(s.clone()), // NaN, Infinity, -0
            n => n.clone(),
        },
        Some("array") => Value::Array(
            v["value"]
                .as_array()
                .into_iter()
                .flatten()
                .map(remote_to_json)
                .collect(),
        ),
        Some("object") => {
            let mut m = Map::new();
            for pair in v["value"].as_array().into_iter().flatten() {
                if let Some(k) = pair[0].as_str() {
                    m.insert(k.into(), remote_to_json(&pair[1]));
                }
            }
            Value::Object(m)
        }
        _ => Value::Null,
    }
}

impl Bidi {
    /// Launch Firefox and own a fresh browsing context. A startup window that stays
    /// discarded is not recoverable in place, so that case gets a fresh process.
    pub fn launch(path: &Path, headless: bool) -> Result<Bidi> {
        let mut attempt = 1;
        loop {
            match Self::launch_once(path, headless) {
                Err(e) if attempt < 3 && is_discarded(&e) => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_secs(1));
                }
                r => return r,
            }
        }
    }

    fn launch_once(path: &Path, headless: bool) -> Result<Bidi> {
        let profile = tempfile::tempdir().map_err(err)?;
        let mut args = vec![
            "--remote-debugging-port=0".to_string(),
            "--profile".into(),
            profile.path().display().to_string(),
            "--no-remote".into(),
            "--new-instance".into(),
        ];
        if headless {
            args.push("--headless".into());
        }
        args.push("about:blank".into());
        let mut child = Command::new(path)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| err(format!("cannot start {}: {e}", path.display())))?;

        // Firefox announces "WebDriver BiDi listening on ws://127.0.0.1:PORT" on stderr.
        let stderr = child.stderr.take().ok_or_else(|| err("no stderr"))?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if let Some(i) = line.find("ws://") {
                    let _ = tx.send(line[i..].trim().to_string());
                }
            }
        });
        let base = rx.recv_timeout(Duration::from_secs(30)).map_err(|_| {
            let _ = child.kill();
            err("Firefox never announced a WebDriver BiDi endpoint")
        })?;
        let url = format!("{}/session", base.trim_end_matches('/'));
        let (mut ws, _) =
            tungstenite::connect(url.as_str()).map_err(|e| err(format!("BiDi connect: {e}")))?;
        set_read_timeout(&mut ws, Duration::from_millis(200));
        let mut b = Bidi {
            child,
            _profile: profile,
            ws,
            next_id: 1,
            context: String::new(),
            console: Vec::new(),
            failures: Vec::new(),
        };
        b.call("session.new", json!({"capabilities": {}}))?;
        // session.new can finish before Firefox's startup window exists. Own an
        // explicit context instead of racing the initial getTree snapshot.
        // While that startup window is still being swapped out, create can fail
        // with "no such frame" (DiscardedBrowsingContextError); it settles quickly.
        let settle = Instant::now() + Duration::from_secs(10);
        let created = loop {
            match b.call("browsingContext.create", json!({"type": "window"})) {
                Err(e) if Instant::now() < settle && is_discarded(&e) => {
                    std::thread::sleep(Duration::from_millis(250));
                }
                r => break r?,
            }
        };
        b.context = created["context"]
            .as_str()
            .ok_or_else(|| err("browsingContext.create returned no context"))?
            .to_string();
        b.call("session.subscribe", json!({"events": ["log.entryAdded"]}))?;
        // Network events are newer in Firefox; absence only means no network evidence.
        let _ = b.call(
            "session.subscribe",
            json!({"events": ["network.responseCompleted", "network.fetchError"]}),
        );
        Ok(b)
    }

    fn on_event(&mut self, method: &str, p: &Value) {
        match method {
            "log.entryAdded" => self.console.push(ConsoleEntry {
                level: p["level"].as_str().unwrap_or("info").into(),
                text: p["text"].as_str().unwrap_or_default().into(),
            }),
            "network.responseCompleted" => {
                let status = p["response"]["status"].as_u64().unwrap_or(0);
                if status >= 400 {
                    self.failures.push(format!(
                        "HTTP {status} {}",
                        p["request"]["url"].as_str().unwrap_or("")
                    ));
                }
            }
            "network.fetchError" => self.failures.push(format!(
                "{} {}",
                p["errorText"].as_str().unwrap_or("failed"),
                p["request"]["url"].as_str().unwrap_or("")
            )),
            _ => {}
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"id": id, "method": method, "params": params});
        self.ws.send(Message::text(msg.to_string())).map_err(err)?;
        let deadline = Instant::now() + CALL_TIMEOUT;
        loop {
            if Instant::now() > deadline {
                return Err(err(format!("{method} timed out")));
            }
            match self.ws.read() {
                Ok(Message::Text(t)) => {
                    let v: Value = serde_json::from_str(t.as_str()).map_err(err)?;
                    match v["type"].as_str() {
                        Some("event") => {
                            let m = v["method"].as_str().unwrap_or_default().to_string();
                            self.on_event(&m, &v["params"]);
                        }
                        _ if v["id"] == json!(id) => {
                            if v["type"] == "error" {
                                return Err(err(format!(
                                    "{method}: {} {}",
                                    v["error"].as_str().unwrap_or(""),
                                    v["message"].as_str().unwrap_or("")
                                )));
                            }
                            return Ok(v["result"].clone());
                        }
                        _ => {}
                    }
                }
                Ok(_) => {}
                Err(e) if is_timeout(&e) => {}
                Err(e) => return Err(err(format!("BiDi connection: {e}"))),
            }
        }
    }
}

impl Driver for Bidi {
    fn protocol(&self) -> &'static str {
        "webdriver-bidi"
    }

    fn navigate(&mut self, url: &str) -> Result<()> {
        let ctx = self.context.clone();
        self.call(
            "browsingContext.navigate",
            json!({"context": ctx, "url": url, "wait": "complete"}),
        )?;
        Ok(())
    }

    fn eval(&mut self, expression: &str) -> Result<Value> {
        let ctx = self.context.clone();
        let r = self.call(
            "script.evaluate",
            json!({"expression": expression, "target": {"context": ctx}, "awaitPromise": true, "resultOwnership": "none"}),
        )?;
        if r["type"] == "exception" {
            let d = &r["exceptionDetails"];
            return Err(err(d["text"].as_str().unwrap_or("script error")));
        }
        Ok(remote_to_json(&r["result"]))
    }

    fn screenshot_png(&mut self) -> Result<Vec<u8>> {
        let ctx = self.context.clone();
        let r = self.call("browsingContext.captureScreenshot", json!({"context": ctx}))?;
        base64::engine::general_purpose::STANDARD
            .decode(r["data"].as_str().unwrap_or_default())
            .map_err(err)
    }

    fn take_console(&mut self) -> Vec<ConsoleEntry> {
        let _ = self.eval("0");
        std::mem::take(&mut self.console)
    }

    fn take_network_failures(&mut self) -> Vec<String> {
        std::mem::take(&mut self.failures)
    }
}

impl Drop for Bidi {
    fn drop(&mut self) {
        let _ = self.ws.send(Message::text(
            json!({"id": 0, "method": "browser.close", "params": {}}).to_string(),
        ));
        let _ = self.ws.close(None);
        std::thread::sleep(Duration::from_millis(300));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_remote_values() {
        let v = json!({"type": "object", "value": [["a", {"type": "number", "value": 1}], ["b", {"type": "array", "value": [{"type": "string", "value": "x"}, {"type": "null"}]}]]});
        assert_eq!(remote_to_json(&v), json!({"a": 1, "b": ["x", null]}));
        assert_eq!(remote_to_json(&json!({"type": "undefined"})), Value::Null);
    }
}
