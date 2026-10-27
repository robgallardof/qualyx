//! Chrome DevTools Protocol driver for Chromium-family browsers.

use std::collections::HashMap;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::{ConsoleEntry, Driver, Result, err, wait_until};

const CALL_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Cdp {
    child: Child,
    _profile: tempfile::TempDir,
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
    console: Vec<ConsoleEntry>,
    failures: Vec<String>,
    request_urls: HashMap<String, String>,
}

pub(crate) fn set_read_timeout(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>, d: Duration) {
    if let MaybeTlsStream::Plain(s) = ws.get_mut() {
        let _ = s.set_read_timeout(Some(d));
    }
}

pub(crate) fn is_timeout(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

impl Cdp {
    pub fn launch(path: &Path, headless: bool) -> Result<Cdp> {
        let profile = tempfile::tempdir().map_err(err)?;
        let mut args = vec![
            "--remote-debugging-port=0".to_string(),
            format!("--user-data-dir={}", profile.path().display()),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--disable-extensions".into(),
            "--disable-background-networking".into(),
            "--disable-sync".into(),
            "--disable-gpu".into(),
            "--mute-audio".into(),
            "--window-size=1280,800".into(),
        ];
        if headless {
            args.push("--headless=new".into());
        }
        args.push("about:blank".into());
        let mut child = Command::new(path)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| err(format!("cannot start {}: {e}", path.display())))?;

        // Chromium writes the chosen port to DevToolsActivePort in the profile.
        let port_file = profile.path().join("DevToolsActivePort");
        let mut port = None;
        let ok = wait_until(Duration::from_secs(30), || {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(err(format!("browser exited early ({status})")));
            }
            port = std::fs::read_to_string(&port_file)
                .ok()
                .and_then(|t| t.lines().next()?.trim().parse::<u16>().ok());
            Ok(port.is_some())
        })?;
        let port = port
            .filter(|_| ok)
            .ok_or_else(|| err("DevTools port never appeared"))?;

        let mut page_ws = None;
        wait_until(Duration::from_secs(15), || {
            let list: Option<Value> = ureq::get(&format!("http://127.0.0.1:{port}/json/list"))
                .call()
                .ok()
                .and_then(|mut r| r.body_mut().read_to_string().ok())
                .and_then(|t| serde_json::from_str(&t).ok());
            page_ws = list.and_then(|l| {
                l.as_array()?
                    .iter()
                    .find(|t| t["type"] == "page")
                    .and_then(|t| t["webSocketDebuggerUrl"].as_str().map(String::from))
            });
            Ok(page_ws.is_some())
        })?;
        let url = page_ws.ok_or_else(|| err("no page target"))?;
        let (mut ws, _) =
            tungstenite::connect(url.as_str()).map_err(|e| err(format!("CDP connect: {e}")))?;
        set_read_timeout(&mut ws, Duration::from_millis(200));
        let mut cdp = Cdp {
            child,
            _profile: profile,
            ws,
            next_id: 1,
            console: Vec::new(),
            failures: Vec::new(),
            request_urls: HashMap::new(),
        };
        for domain in ["Page.enable", "Runtime.enable", "Network.enable"] {
            cdp.call(domain, json!({}))?;
        }
        Ok(cdp)
    }

    fn on_event(&mut self, method: &str, p: &Value) {
        match method {
            "Runtime.consoleAPICalled" => {
                let text = p["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|a| {
                        a["value"].as_str().map(String::from).unwrap_or_else(|| {
                            a["description"]
                                .as_str()
                                .map(String::from)
                                .unwrap_or_else(|| a["value"].to_string())
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                self.console.push(ConsoleEntry {
                    level: p["type"].as_str().unwrap_or("log").into(),
                    text,
                });
            }
            "Runtime.exceptionThrown" => {
                let d = &p["exceptionDetails"];
                let text = d["exception"]["description"]
                    .as_str()
                    .or(d["text"].as_str())
                    .unwrap_or("uncaught exception");
                self.console.push(ConsoleEntry {
                    level: "exception".into(),
                    text: text.into(),
                });
            }
            "Network.requestWillBeSent" => {
                if let (Some(id), Some(url)) =
                    (p["requestId"].as_str(), p["request"]["url"].as_str())
                {
                    self.request_urls.insert(id.into(), url.into());
                }
            }
            "Network.responseReceived" => {
                let status = p["response"]["status"].as_u64().unwrap_or(0);
                if status >= 400 {
                    self.failures.push(format!(
                        "HTTP {status} {}",
                        p["response"]["url"].as_str().unwrap_or("")
                    ));
                }
            }
            "Network.loadingFailed" => {
                let url = p["requestId"]
                    .as_str()
                    .and_then(|id| self.request_urls.get(id))
                    .cloned()
                    .unwrap_or_default();
                if p["canceled"] != true {
                    self.failures.push(format!(
                        "{} {url}",
                        p["errorText"].as_str().unwrap_or("failed")
                    ));
                }
            }
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
                    if v["id"] == json!(id) {
                        if let Some(e) = v.get("error") {
                            return Err(err(format!(
                                "{method}: {}",
                                e["message"].as_str().unwrap_or("error")
                            )));
                        }
                        return Ok(v["result"].clone());
                    }
                    if let Some(m) = v["method"].as_str() {
                        let m = m.to_string();
                        self.on_event(&m, &v["params"]);
                    }
                }
                Ok(_) => {}
                Err(e) if is_timeout(&e) => {}
                Err(e) => return Err(err(format!("CDP connection: {e}"))),
            }
        }
    }
}

impl Driver for Cdp {
    fn protocol(&self) -> &'static str {
        "cdp"
    }

    fn navigate(&mut self, url: &str) -> Result<()> {
        let r = self.call("Page.navigate", json!({"url": url}))?;
        if let Some(e) = r["errorText"].as_str() {
            return Err(err(format!("navigation to {url} failed: {e}")));
        }
        let ok = wait_until(Duration::from_secs(30), || {
            Ok(self.eval("document.readyState")? == "complete")
        })?;
        if !ok {
            return Err(err(format!("{url} did not finish loading")));
        }
        Ok(())
    }

    fn eval(&mut self, expression: &str) -> Result<Value> {
        let r = self.call(
            "Runtime.evaluate",
            json!({"expression": expression, "awaitPromise": true, "returnByValue": true}),
        )?;
        if let Some(d) = r.get("exceptionDetails") {
            let text = d["exception"]["description"]
                .as_str()
                .or(d["text"].as_str())
                .unwrap_or("script error");
            return Err(err(text));
        }
        Ok(r["result"]["value"].clone())
    }

    fn screenshot_png(&mut self) -> Result<Vec<u8>> {
        let r = self.call("Page.captureScreenshot", json!({"format": "png"}))?;
        base64::engine::general_purpose::STANDARD
            .decode(r["data"].as_str().unwrap_or_default())
            .map_err(err)
    }

    fn take_console(&mut self) -> Vec<ConsoleEntry> {
        // Pick up events that arrived after the last command.
        let _ = self.call("Runtime.evaluate", json!({"expression": "0"}));
        std::mem::take(&mut self.console)
    }

    fn take_network_failures(&mut self) -> Vec<String> {
        std::mem::take(&mut self.failures)
    }
}

impl Drop for Cdp {
    fn drop(&mut self) {
        let _ = self.ws.send(Message::text(
            json!({"id": 0, "method": "Browser.close"}).to_string(),
        ));
        let _ = self.ws.close(None);
        std::thread::sleep(Duration::from_millis(200));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
