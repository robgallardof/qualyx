//! Classic W3C WebDriver over HTTP, used for Safari through Apple's `safaridriver`.
//! Safari has no headless mode and exposes no console log through WebDriver.

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

use crate::{ConsoleEntry, Driver, Result, err, wait_until};

// Safari supports one WebDriver session at a time. Keep the permit for the
// driver's entire lifetime so parallel callers cannot break each other's pairing.
static SAFARI_SESSION: Mutex<()> = Mutex::new(());

pub struct Safari {
    child: Child,
    base: String,
    session: String,
    _session_guard: MutexGuard<'static, ()>,
}

fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")
        .map_err(err)?
        .local_addr()
        .map_err(err)?
        .port())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn parse(mut r: ureq::http::Response<ureq::Body>) -> Result<Value> {
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().map_err(err)?;
    let v: Value = serde_json::from_str(&text).map_err(err)?;
    if status >= 400 {
        return Err(err(format!(
            "WebDriver {status}: {}",
            v["value"]["message"].as_str().unwrap_or(&text)
        )));
    }
    Ok(v["value"].clone())
}

impl Safari {
    pub fn launch() -> Result<Safari> {
        let session_guard = SAFARI_SESSION
            .lock()
            .map_err(|_| err("Safari session lock poisoned"))?;
        let port = free_port()?;
        let child = Command::new("safaridriver")
            .args(["-p", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| err(format!("cannot start safaridriver: {e}")))?;
        let base = format!("http://127.0.0.1:{port}");
        let ready = wait_until(Duration::from_secs(15), || {
            Ok(agent().get(&format!("{base}/status")).call().is_ok())
        })?;
        if !ready {
            return Err(err(
                "safaridriver did not start (run `safaridriver --enable` once)",
            ));
        }
        let mut s = Safari {
            child,
            base,
            session: String::new(),
            _session_guard: session_guard,
        };
        let v = s.post(
            "/session",
            json!({"capabilities": {"alwaysMatch": {"browserName": "safari"}}}),
        )?;
        s.session = v["sessionId"]
            .as_str()
            .ok_or_else(|| err("no sessionId"))?
            .to_string();
        Ok(s)
    }

    fn post(&self, path: &str, body: Value) -> Result<Value> {
        parse(
            agent()
                .post(&format!("{}{path}", self.base))
                .header("content-type", "application/json")
                .send(body.to_string())
                .map_err(err)?,
        )
    }

    fn get(&self, path: &str) -> Result<Value> {
        parse(
            agent()
                .get(&format!("{}{path}", self.base))
                .call()
                .map_err(err)?,
        )
    }
}

impl Driver for Safari {
    fn protocol(&self) -> &'static str {
        "webdriver"
    }

    fn navigate(&mut self, url: &str) -> Result<()> {
        self.post(
            &format!("/session/{}/url", self.session),
            json!({"url": url}),
        )?;
        Ok(())
    }

    fn eval(&mut self, expression: &str) -> Result<Value> {
        let script = "const done = arguments[arguments.length - 1];\
            Promise.resolve().then(() => (0, eval)(arguments[0]))\
            .then(v => done({ok: v === undefined ? null : v}), e => done({err: String(e)}));";
        let v = self.post(
            &format!("/session/{}/execute/async", self.session),
            json!({"script": script, "args": [expression]}),
        )?;
        match v.get("err").and_then(Value::as_str) {
            Some(e) => Err(err(e)),
            None => Ok(v["ok"].clone()),
        }
    }

    fn screenshot_png(&mut self) -> Result<Vec<u8>> {
        let v = self.get(&format!("/session/{}/screenshot", self.session))?;
        base64::engine::general_purpose::STANDARD
            .decode(v.as_str().unwrap_or_default())
            .map_err(err)
    }

    fn take_console(&mut self) -> Vec<ConsoleEntry> {
        Vec::new()
    }

    fn take_network_failures(&mut self) -> Vec<String> {
        Vec::new()
    }
}

impl Drop for Safari {
    fn drop(&mut self) {
        let _ = agent()
            .delete(&format!("{}/session/{}", self.base, self.session))
            .call();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
