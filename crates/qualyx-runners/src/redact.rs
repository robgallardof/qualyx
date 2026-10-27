//! Secret redaction for stored logs and messages (threat model T4).
//!
//! Two layers: well-known token shapes, and the literal values of credential-looking
//! environment variables of the Qualyx process itself.

use std::sync::OnceLock;

use regex::Regex;

pub const MASK: &str = "[REDACTED]";

fn patterns() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(concat!(
            r"gh[pousr]_[A-Za-z0-9]{36,}",
            r"|github_pat_[A-Za-z0-9_]{40,}",
            r"|glpat-[A-Za-z0-9_\-]{20,}",
            r"|sk-(?:ant-|proj-)?[A-Za-z0-9_\-]{20,}",
            r"|xox[abprs]-[A-Za-z0-9\-]{10,}",
            r"|AKIA[0-9A-Z]{16}",
            r"|AIza[0-9A-Za-z_\-]{35}",
            r"|npm_[A-Za-z0-9]{36}",
            r"|eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}",
            r"|-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            r"|(?i:(?:password|passwd|secret|token|api[_-]?key)\s*[=:]\s*)[^\s'\x22]{6,}",
        ))
        .expect("valid redaction regex")
    })
}

fn env_secrets() -> &'static Vec<String> {
    static VALUES: OnceLock<Vec<String>> = OnceLock::new();
    VALUES.get_or_init(|| {
        let mut v: Vec<String> = std::env::vars()
            .filter(|(k, val)| {
                !crate::process::is_allowed_env(k) && looks_secret_name(k) && val.len() >= 8
            })
            .map(|(_, val)| val)
            .collect();
        // Longest first so overlapping values are masked whole.
        v.sort_by_key(|s| std::cmp::Reverse(s.len()));
        v
    })
}

fn looks_secret_name(k: &str) -> bool {
    let k = k.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "_KEY",
        "AUTH",
        "CREDENTIAL",
    ]
    .iter()
    .any(|m| k.contains(m))
}

/// Mask secrets in `text`.
pub fn redact(text: &str) -> String {
    let mut out = patterns().replace_all(text, MASK).into_owned();
    for s in env_secrets() {
        if out.contains(s.as_str()) {
            out = out.replace(s.as_str(), MASK);
        }
    }
    out
}

/// Rewrite a log file in place with secrets masked. Returns whether anything was masked.
pub fn redact_file(path: &std::path::Path) -> std::io::Result<bool> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let clean = redact(&text);
    if clean != text {
        std::fs::write(path, clean)?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_known_token_shapes() {
        // Built at runtime so no literal secret-shaped string lives in the repository.
        let gh = format!("ghp_{}", "a".repeat(36));
        let aws = format!("AKIA{}", "B".repeat(16));
        let text = format!("token {gh} and key {aws}\npassword=hunter22 ok");
        let r = redact(&text);
        assert!(!r.contains(&gh) && !r.contains(&aws), "{r}");
        assert!(!r.contains("hunter22"), "{r}");
        assert!(r.contains("ok"));
    }

    #[test]
    fn masks_private_key_blocks() {
        let k = format!(
            "-----BEGIN RSA PRIVATE KEY-----\n{}\n-----END RSA PRIVATE KEY-----",
            "x".repeat(40)
        );
        assert_eq!(redact(&format!("a\n{k}\nb")), format!("a\n{MASK}\nb"));
    }

    #[test]
    fn leaves_ordinary_output_alone() {
        let s = "test tests::adds ... ok\nassert -1 == 6";
        assert_eq!(redact(s), s);
    }
}
