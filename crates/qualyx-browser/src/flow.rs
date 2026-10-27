//! Declarative browser flows (JSON) executed through any [`Driver`], with evidence.
//!
//! ```json
//! {"name": "login", "steps": [
//!   {"action": "goto", "url": "/login.html"},
//!   {"action": "fill", "selector": "#user", "value": "demo"},
//!   {"action": "click", "selector": "button[type=submit]"},
//!   {"action": "expect_text", "selector": "#status", "contains": "Welcome"},
//!   {"action": "screenshot", "name": "after-login"}
//! ]}
//! ```

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ConsoleEntry, Driver, Result, err, wait_until};

const DEFAULT_TIMEOUT_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Goto {
        url: String,
    },
    Fill {
        selector: String,
        value: String,
    },
    Click {
        selector: String,
    },
    ExpectText {
        selector: Option<String>,
        contains: String,
        timeout_ms: Option<u64>,
    },
    WaitFor {
        selector: String,
        timeout_ms: Option<u64>,
    },
    Screenshot {
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    pub index: usize,
    pub step: Step,
    pub passed: bool,
    pub ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowReport {
    pub flow: String,
    pub browser: String,
    pub version: Option<String>,
    pub protocol: String,
    pub passed: bool,
    pub steps: Vec<StepReport>,
    pub console: Vec<ConsoleEntry>,
    pub network_failures: Vec<String>,
    pub final_url: Option<String>,
    pub screenshots: Vec<String>,
}

fn js(v: &str) -> String {
    serde_json::to_string(v).expect("string serializes")
}

fn element(selector: &str) -> String {
    format!(
        "(() => {{ const e = document.querySelector({s}); if (!e) throw new Error('no element matches ' + {s}); return e; }})()",
        s = js(selector)
    )
}

/// Absolute URLs pass through; relative ones resolve against `base`.
pub fn resolve_url(base: Option<&str>, url: &str) -> Result<String> {
    if url.contains("://") || url.starts_with("about:") || url.starts_with("data:") {
        return Ok(url.to_string());
    }
    let base = base.ok_or_else(|| {
        err(format!(
            "relative url `{url}` needs a base (serve a directory or pass --base-url)"
        ))
    })?;
    Ok(format!(
        "{}/{}",
        base.trim_end_matches('/'),
        url.trim_start_matches('/')
    ))
}

fn run_step(d: &mut dyn Driver, step: &Step, base: Option<&str>) -> Result<()> {
    match step {
        Step::Goto { url } => d.navigate(&resolve_url(base, url)?),
        Step::Fill { selector, value } => d
            .eval(&format!(
                "(() => {{ const e = {el}; e.focus(); e.value = {v}; \
                 e.dispatchEvent(new Event('input', {{bubbles: true}})); \
                 e.dispatchEvent(new Event('change', {{bubbles: true}})); return true; }})()",
                el = element(selector),
                v = js(value)
            ))
            .map(drop),
        Step::Click { selector } => d
            .eval(&format!("({}).click(), true", element(selector)))
            .map(drop),
        Step::WaitFor {
            selector,
            timeout_ms,
        } => {
            let probe = format!("document.querySelector({}) !== null", js(selector));
            let ok = wait_until(
                Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)),
                || Ok(d.eval(&probe)? == Value::Bool(true)),
            )?;
            if ok {
                Ok(())
            } else {
                Err(err(format!("`{selector}` did not appear")))
            }
        }
        Step::ExpectText {
            selector,
            contains,
            timeout_ms,
        } => {
            let read = match selector {
                Some(s) => format!(
                    "(document.querySelector({}) || {{}}).innerText || ''",
                    js(s)
                ),
                None => "document.body ? document.body.innerText : ''".to_string(),
            };
            let mut last = String::new();
            let ok = wait_until(
                Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)),
                || {
                    last = d.eval(&read)?.as_str().unwrap_or_default().to_string();
                    Ok(last.contains(contains.as_str()))
                },
            )?;
            if ok {
                Ok(())
            } else {
                let shown: String = last.chars().take(200).collect();
                Err(err(format!(
                    "expected text containing {contains:?}, found {shown:?}"
                )))
            }
        }
        Step::Screenshot { .. } => Ok(()),
    }
}

fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn save_png(d: &mut dyn Driver, out_dir: &Path, name: &str) -> Option<String> {
    let png = d.screenshot_png().ok()?;
    let file = format!("{}.png", safe_name(name));
    fs::write(out_dir.join(&file), png).ok()?;
    Some(file)
}

/// Run every step, stopping at the first failure (screenshot taken there). Evidence files
/// land in `out_dir`.
pub fn run(
    flow: &Flow,
    d: &mut dyn Driver,
    browser: &crate::Browser,
    out_dir: &Path,
    base: Option<&str>,
) -> FlowReport {
    let _ = fs::create_dir_all(out_dir);
    let mut steps = Vec::new();
    let mut screenshots = Vec::new();
    let mut passed = true;
    for (index, step) in flow.steps.iter().enumerate() {
        let t = Instant::now();
        let result = run_step(d, step, base);
        let mut shot = None;
        if let Step::Screenshot { name } = step {
            shot = save_png(d, out_dir, &format!("{index:02}-{name}"));
        }
        let error = result.err().map(|e| e.0);
        if error.is_some() {
            passed = false;
            shot = save_png(d, out_dir, &format!("{index:02}-failure"));
        }
        screenshots.extend(shot.clone());
        steps.push(StepReport {
            index,
            step: step.clone(),
            passed: error.is_none(),
            ms: t.elapsed().as_millis() as u64,
            error,
            screenshot: shot,
        });
        if !passed {
            break;
        }
    }
    if passed {
        screenshots.extend(save_png(d, out_dir, "final"));
    }
    let final_url = d
        .eval("location.href")
        .ok()
        .and_then(|v| v.as_str().map(String::from));
    FlowReport {
        flow: flow.name.clone(),
        browser: browser.id.clone(),
        version: browser.version.clone(),
        protocol: d.protocol().into(),
        passed,
        steps,
        console: d.take_console(),
        network_failures: d.take_network_failures(),
        final_url,
        screenshots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flows_strictly() {
        let f: Flow = serde_json::from_str(r##"{"name":"x","steps":[{"action":"goto","url":"/a"},{"action":"expect_text","contains":"hi"}]}"##).unwrap();
        assert_eq!(f.steps.len(), 2);
        assert!(
            serde_json::from_str::<Flow>(
                r##"{"name":"x","steps":[{"action":"goto","url":"/a","extra":1}]}"##
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<Flow>(
                r##"{"name":"x","steps":[{"action":"hover","selector":"a"}]}"##
            )
            .is_err()
        );
    }

    #[test]
    fn selectors_and_values_are_json_escaped() {
        let e = element("a[title=\"x'); alert(1)//\"]");
        assert!(e.contains(r#""a[title=\"x'); alert(1)//\"]""#), "{e}");
    }

    #[test]
    fn urls_resolve_against_base() {
        assert_eq!(
            resolve_url(Some("http://127.0.0.1:9/"), "/login.html").unwrap(),
            "http://127.0.0.1:9/login.html"
        );
        assert_eq!(
            resolve_url(None, "https://example.org").unwrap(),
            "https://example.org"
        );
        assert!(resolve_url(None, "/x").is_err());
    }
}
