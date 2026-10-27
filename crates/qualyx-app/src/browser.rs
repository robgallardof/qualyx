//! Browser flows and desktop probes as application services, with evidence on disk.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use qualyx_browser::{Browser, SmokeResult, flow};
use serde::Serialize;

use crate::{Layout, discover, load_config};

#[derive(Debug, Clone, Serialize)]
pub struct BrowserEntry {
    #[serde(flatten)]
    pub browser: Browser,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke: Option<SmokeResult>,
}

/// Installed browsers, optionally driven through a smoke test each.
pub fn browsers(smoke: bool) -> Vec<BrowserEntry> {
    qualyx_browser::detect()
        .into_iter()
        .map(|b| BrowserEntry {
            smoke: smoke.then(|| qualyx_browser::smoke(&b)),
            browser: b,
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct BrowseRequest {
    pub flow_file: PathBuf,
    /// Browser ids (`chrome`, `firefox`…); empty means every installed one.
    pub browsers: Vec<String>,
    /// Directory served on loopback as the flow's base URL.
    pub serve_dir: Option<PathBuf>,
    pub base_url: Option<String>,
    pub headed: bool,
    /// Only loopback URLs and origins listed in `[browser] allowed_origins` may be visited.
    pub restrict_origins: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowseReport {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub passed: bool,
    pub flows: Vec<flow::FlowReport>,
    /// Browsers that could not be launched, with the reason.
    pub unavailable: Vec<(String, String)>,
}

fn is_loopback(url: &str) -> bool {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or("");
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    let host = if host.starts_with('[') {
        host.split(']').next().map(|h| &h[1..]).unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn origin_allowed(url: &str, allowed: &[String]) -> bool {
    url.starts_with("about:")
        || is_loopback(url)
        || allowed
            .iter()
            .any(|a| url == a || url.starts_with(&format!("{}/", a.trim_end_matches('/'))))
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct BrowserConfig {
    allowed_origins: Vec<String>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct ConfigWithBrowser {
    browser: BrowserConfig,
}

/// Run a flow in each requested browser; evidence under `.qualyx/runs/<id>/browser/<id>/`.
pub fn browse(project: &Path, req: &BrowseRequest) -> Result<BrowseReport> {
    let root = discover(project)?.root;
    let layout = Layout::new(&root);
    layout.ensure()?;
    let _ = load_config(&layout)?; // validate the config file even if it has no [browser]
    let flow: flow::Flow = serde_json::from_slice(
        &fs::read(&req.flow_file)
            .with_context(|| format!("cannot read {}", req.flow_file.display()))?,
    )
    .with_context(|| format!("invalid flow {}", req.flow_file.display()))?;

    let server = match &req.serve_dir {
        Some(d) => {
            Some(qualyx_browser::serve::Server::start_dir(d).map_err(|e| anyhow::anyhow!(e))?)
        }
        None => None,
    };
    let base = server
        .as_ref()
        .map(|s| s.url("/"))
        .or_else(|| req.base_url.clone());

    if req.restrict_origins {
        let allowed = fs::read_to_string(layout.config())
            .ok()
            .and_then(|t| toml::from_str::<ConfigWithBrowser>(&t).ok())
            .unwrap_or_default()
            .browser
            .allowed_origins;
        for step in &flow.steps {
            if let flow::Step::Goto { url } = step {
                let full =
                    flow::resolve_url(base.as_deref(), url).map_err(|e| anyhow::anyhow!(e))?;
                if !origin_allowed(&full, &allowed) {
                    bail!("{full} is not loopback and not in [browser] allowed_origins");
                }
            }
        }
    }

    let installed = qualyx_browser::detect();
    let chosen: Vec<Browser> = if req.browsers.is_empty() {
        installed
    } else {
        req.browsers
            .iter()
            .map(|id| {
                installed
                    .iter()
                    .find(|b| &b.id == id)
                    .cloned()
                    .with_context(|| format!("browser `{id}` is not installed"))
            })
            .collect::<Result<_>>()?
    };
    if chosen.is_empty() {
        bail!("no supported browser is installed");
    }

    let run_id = qualyx_store::new_run_id(chrono::Utc::now());
    let run_dir = layout.run_dir(&run_id);
    fs::create_dir_all(&run_dir)?;
    let mut flows = Vec::new();
    let mut unavailable = Vec::new();
    for b in &chosen {
        match qualyx_browser::launch(b, !req.headed) {
            Ok(mut d) => {
                let out = run_dir.join("browser").join(&b.id);
                let mut report = flow::run(&flow, d.as_mut(), b, &out, base.as_deref());
                report.screenshots = report
                    .screenshots
                    .iter()
                    .map(|s| format!("browser/{}/{s}", b.id))
                    .collect();
                flows.push(report);
            }
            Err(e) => unavailable.push((b.id.clone(), e.0)),
        }
    }
    let passed = !flows.is_empty() && flows.iter().all(|f| f.passed);
    let report = BrowseReport {
        run_id,
        run_dir: run_dir.clone(),
        passed,
        flows,
        unavailable,
    };
    fs::write(
        run_dir.join("browser-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

/// axe-core audit of `url` (or `page` under a served `serve_dir`) in each chosen browser.
pub fn a11y(
    project: &Path,
    url: Option<&str>,
    serve_dir: Option<&Path>,
    page: &str,
    browsers: &[String],
    restrict_origins: bool,
) -> Result<Vec<qualyx_browser::a11y::A11yReport>> {
    let root = discover(project)?.root;
    let server = match serve_dir {
        Some(d) => {
            Some(qualyx_browser::serve::Server::start_dir(d).map_err(|e| anyhow::anyhow!(e))?)
        }
        None => None,
    };
    let target = match (&server, url) {
        (Some(s), _) => s.url(page),
        (None, Some(u)) => u.to_string(),
        (None, None) => bail!("give a URL or a directory to serve"),
    };
    if restrict_origins {
        let allowed = fs::read_to_string(Layout::new(&root).config())
            .ok()
            .and_then(|t| toml::from_str::<ConfigWithBrowser>(&t).ok())
            .unwrap_or_default()
            .browser
            .allowed_origins;
        if !origin_allowed(&target, &allowed) {
            bail!("{target} is not loopback and not in [browser] allowed_origins");
        }
    }
    let axe = qualyx_browser::a11y::axe_source(Some(&root)).map_err(|e| anyhow::anyhow!(e))?;
    let installed = qualyx_browser::detect();
    let chosen: Vec<Browser> = if browsers.is_empty() {
        installed.into_iter().take(1).collect()
    } else {
        browsers
            .iter()
            .map(|id| {
                installed
                    .iter()
                    .find(|b| &b.id == id)
                    .cloned()
                    .with_context(|| format!("browser `{id}` is not installed"))
            })
            .collect::<Result<_>>()?
    };
    if chosen.is_empty() {
        bail!("no supported browser is installed");
    }
    let mut out = Vec::new();
    for b in &chosen {
        let mut d = qualyx_browser::launch(b, true).map_err(|e| anyhow::anyhow!(e))?;
        d.navigate(&target).map_err(|e| anyhow::anyhow!(e))?;
        out.push(
            qualyx_browser::a11y::audit(
                d.as_mut(),
                &target,
                &axe,
                qualyx_browser::a11y::DEFAULT_TAGS,
            )
            .map_err(|e| anyhow::anyhow!(e))?,
        );
    }
    let layout = Layout::new(&root);
    layout.ensure()?;
    let dir = layout.run_dir(&qualyx_store::new_run_id(chrono::Utc::now()));
    fs::create_dir_all(&dir)?;
    fs::write(
        dir.join("a11y-report.json"),
        serde_json::to_vec_pretty(&out)?,
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_policy() {
        let allowed = vec!["https://staging.example.com".to_string()];
        for ok in [
            "http://127.0.0.1:8080/x",
            "http://localhost/",
            "http://[::1]:3000/a",
            "about:blank",
            "https://staging.example.com/login",
        ] {
            assert!(origin_allowed(ok, &allowed), "{ok}");
        }
        for bad in [
            "https://example.com/",
            "http://127.0.0.1.evil.com/",
            "http://user@evil.com/",
            "https://staging.example.com.evil.com/",
            "http://169.254.169.254/latest",
        ] {
            assert!(!origin_allowed(bad, &allowed), "{bad}");
        }
    }
}
