//! Accessibility audits with axe-core injected into a real browser (any [`Driver`]).
//!
//! axe-core is MPL-2.0. Qualyx doesn't bundle it: it uses the project's
//! `node_modules/axe-core` or downloads the pinned version from the npm registry, verifies the
//! registry's SHA-512 integrity, and caches it.

use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};

use crate::{Driver, Result, err};

pub const AXE_VERSION: &str = "4.14.0";
/// `dist.integrity` published by the npm registry for axe-core@4.14.0.
pub const AXE_INTEGRITY: &str = "sha512-9WTZxEjsZ7b13TH8JPmbV2z8CHbl80/2hm3XPEG4JgNdQLK81IBRXmSxHfMAOkSqQeRxT/0dwNDz2GOm3zzpcQ==";
pub const DEFAULT_TAGS: &[&str] = &["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    pub id: String,
    pub impact: Option<String>,
    pub help: String,
    pub nodes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A11yReport {
    pub url: String,
    pub axe_version: String,
    pub axe_source: String,
    pub tags: Vec<String>,
    pub violations: Vec<Violation>,
    pub passes: u64,
    /// Rules axe could not decide; they need a human look, not a pass.
    pub incomplete: u64,
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .or_else(|| std::env::var_os("LOCALAPPDATA"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("qualyx")
}

/// Verify an npm `sha512-<base64>` integrity string.
pub fn integrity_ok(bytes: &[u8], integrity: &str) -> bool {
    let Some(b64) = integrity.strip_prefix("sha512-") else {
        return false;
    };
    base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes)) == b64
}

/// axe-core source and where it came from.
pub fn axe_source(project: Option<&Path>) -> Result<(String, String)> {
    if let Some(p) = project {
        let local = p.join("node_modules/axe-core/axe.min.js");
        if let Ok(s) = std::fs::read_to_string(&local) {
            return Ok((s, format!("project {}", local.display())));
        }
    }
    let cached = cache_dir().join(format!("axe-core-{AXE_VERSION}.min.js"));
    if let Ok(s) = std::fs::read_to_string(&cached) {
        return Ok((s, format!("cache {}", cached.display())));
    }
    let url = format!("https://registry.npmjs.org/axe-core/-/axe-core-{AXE_VERSION}.tgz");
    let mut tgz = Vec::new();
    ureq::get(&url)
        .call()
        .map_err(|e| err(format!("download {url}: {e}")))?
        .into_body()
        .into_reader()
        .read_to_end(&mut tgz)
        .map_err(err)?;
    if !integrity_ok(&tgz, AXE_INTEGRITY) {
        return Err(err(format!(
            "integrity mismatch for {url}; refusing to use it"
        )));
    }
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&tgz[..]));
    for entry in archive.entries().map_err(err)? {
        let mut entry = entry.map_err(err)?;
        if entry.path().map_err(err)?.to_string_lossy() == "package/axe.min.js" {
            let mut s = String::new();
            entry.read_to_string(&mut s).map_err(err)?;
            let _ = std::fs::create_dir_all(cache_dir());
            let _ = std::fs::write(&cached, &s);
            return Ok((s, format!("npm {url} (sha512 verified)")));
        }
    }
    Err(err("axe.min.js not found in the axe-core package"))
}

/// Inject axe-core into the current page and run it for `tags`.
pub fn audit(
    d: &mut dyn Driver,
    url: &str,
    axe: &(String, String),
    tags: &[&str],
) -> Result<A11yReport> {
    d.eval(&format!("{};true", axe.0))?;
    let tags_js = serde_json::to_string(tags).map_err(err)?;
    let v = d.eval(&format!(
        "axe.run(document, {{runOnly: {{type: 'tag', values: {tags_js}}}, resultTypes: ['violations']}}).then(r => ({{\
           version: axe.version,\
           violations: r.violations.map(v => ({{id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.map(n => n.target.join(' '))}})),\
           passes: r.passes.length, incomplete: r.incomplete.length }}))"
    ))?;
    let violations: Vec<Violation> =
        serde_json::from_value(v["violations"].clone()).map_err(err)?;
    Ok(A11yReport {
        url: url.into(),
        axe_version: v["version"].as_str().unwrap_or(AXE_VERSION).into(),
        axe_source: axe.1.clone(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        violations,
        passes: v["passes"].as_u64().unwrap_or(0),
        incomplete: v["incomplete"].as_u64().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrity_check() {
        let data = b"hello";
        let good = format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(data))
        );
        assert!(integrity_ok(data, &good));
        assert!(!integrity_ok(b"tampered", &good));
        assert!(!integrity_ok(data, "sha1-abc"));
    }
}
