//! Browser automation for E2E evidence (M4).
//!
//! - `detect` finds installed browsers and their versions per OS.
//! - Three protocol drivers behind one [`Driver`] trait: Chrome DevTools Protocol (Chrome,
//!   Chromium, Edge, Brave, Opera, Vivaldi), WebDriver BiDi (Firefox, Zen) and classic
//!   WebDriver (Safari through `safaridriver`).
//! - [`flow`] runs a JSON flow (goto / fill / click / expect_text / wait_for / screenshot)
//!   and keeps screenshots, console output and failed requests as evidence.
//! - [`serve`] is a loopback-only static server for local pages under test.
//!
//! Nothing here claims a browser works until [`smoke`] has driven it for real.

pub mod bidi;
pub mod cdp;
pub mod detect;
pub mod flow;
pub mod serve;
pub mod webdriver;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use detect::{Browser, Family, detect};

#[derive(Debug)]
pub struct BrowserError(pub String);

impl std::fmt::Display for BrowserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BrowserError {}

pub fn err(e: impl std::fmt::Display) -> BrowserError {
    BrowserError(e.to_string())
}

pub type Result<T> = std::result::Result<T, BrowserError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleEntry {
    pub level: String,
    pub text: String,
}

/// What a protocol driver must offer to run flows.
pub trait Driver {
    /// Protocol name recorded as evidence ("cdp", "webdriver-bidi", "webdriver").
    fn protocol(&self) -> &'static str;
    fn navigate(&mut self, url: &str) -> Result<()>;
    /// Evaluate a JS expression (awaiting promises) and return its JSON value.
    fn eval(&mut self, expression: &str) -> Result<Value>;
    fn screenshot_png(&mut self) -> Result<Vec<u8>>;
    /// Console messages and uncaught errors seen so far (drained).
    fn take_console(&mut self) -> Vec<ConsoleEntry>;
    /// Requests that failed or returned HTTP >= 400 (drained). Empty when unsupported.
    fn take_network_failures(&mut self) -> Vec<String>;
}

/// Launch the right driver for an installed browser.
pub fn launch(b: &Browser, headless: bool) -> Result<Box<dyn Driver>> {
    match b.family {
        Family::Chromium => Ok(Box::new(cdp::Cdp::launch(&b.path, headless)?)),
        Family::Firefox => Ok(Box::new(bidi::Bidi::launch(&b.path, headless)?)),
        Family::Safari => Ok(Box::new(webdriver::Safari::launch()?)),
    }
}

/// Poll `cond` until true or `timeout`.
pub(crate) fn wait_until(
    timeout: Duration,
    mut cond: impl FnMut() -> Result<bool>,
) -> Result<bool> {
    let start = std::time::Instant::now();
    loop {
        if cond()? {
            return Ok(true);
        }
        if start.elapsed() >= timeout {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SmokeResult {
    pub browser: String,
    pub version: Option<String>,
    pub protocol: Option<String>,
    /// "verified" (driven for real), "unsupported" (driver failed) — with the reason.
    pub status: String,
    pub detail: String,
}

/// Drive the browser through a local page: navigate, evaluate, screenshot.
pub fn smoke(b: &Browser) -> SmokeResult {
    let base = SmokeResult {
        browser: b.id.clone(),
        version: b.version.clone(),
        protocol: None,
        status: "unsupported".into(),
        detail: String::new(),
    };
    let server =
        match serve::Server::start_html("<!doctype html><title>qualyx-smoke</title><p id=p>ok</p>")
        {
            Ok(s) => s,
            Err(e) => {
                return SmokeResult {
                    detail: format!("local server: {e}"),
                    ..base
                };
            }
        };
    let run = || -> Result<(String, usize)> {
        let mut d = launch(b, true)?;
        d.navigate(&server.url("/"))?;
        let title = d.eval("document.title")?;
        let png = d.screenshot_png()?;
        if title != "qualyx-smoke" {
            return Err(err(format!("unexpected title {title}")));
        }
        Ok((d.protocol().to_string(), png.len()))
    };
    match run() {
        Ok((protocol, png)) => SmokeResult {
            protocol: Some(protocol),
            status: "verified".into(),
            detail: format!("navigate + evaluate + screenshot ({png} bytes PNG)"),
            ..base
        },
        Err(e) => SmokeResult {
            detail: e.0,
            ..base
        },
    }
}
