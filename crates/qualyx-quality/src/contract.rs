//! API contract testing with Schemathesis (external process): property-based requests from an
//! OpenAPI schema, checked for server errors and response-schema conformance.
//!
//! It sends real requests, so targets follow the same authorization as load tests: loopback
//! or an explicitly allowed origin.

use std::fs;
use std::path::Path;
use std::time::Duration;

use qualyx_core::{Status, TestCase};
use serde::Serialize;

pub const DEFAULT_CHECKS: &str =
    "not_a_server_error,response_schema_conformance,status_code_conformance";

#[derive(Debug, Clone, Serialize)]
pub struct ContractReport {
    pub schema: String,
    pub tool_version: Option<String>,
    pub checks: String,
    /// "passed", "failed", "error".
    pub status: String,
    pub operations: Vec<TestCase>,
    pub exit_code: Option<i32>,
    pub notes: Vec<String>,
}

fn host_is_loopback(url: &str) -> bool {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or("");
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    let host = if host.starts_with('[') {
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

pub fn authorize(url: &str, allowed: &[String]) -> Result<(), String> {
    let listed = allowed.iter().any(|a| {
        let a = a.trim_end_matches('/');
        url == a || url.starts_with(&format!("{a}/"))
    });
    if (url.starts_with("http://") || url.starts_with("https://"))
        && (host_is_loopback(url) || listed)
    {
        Ok(())
    } else {
        Err(format!(
            "{url} is not loopback and not in [contract] allowed_targets"
        ))
    }
}

/// Run Schemathesis against `schema_url` (the API base is taken from it, or from `base_url`).
pub fn run(
    schema_url: &str,
    base_url: Option<&str>,
    allowed: &[String],
    max_examples: u32,
    out_dir: &Path,
    timeout: Duration,
) -> Result<ContractReport, String> {
    authorize(schema_url, allowed)?;
    if let Some(b) = base_url {
        authorize(b, allowed)?;
    }
    let st = qualyx_runners::process::resolve(&["schemathesis", "st"])
        .ok_or("schemathesis is not on PATH (pip install schemathesis)")?;
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let junit = out_dir.join("schemathesis.junit.xml");
    let mut args: Vec<String> = vec![
        "run".into(),
        schema_url.into(),
        "--checks".into(),
        DEFAULT_CHECKS.into(),
        "--max-examples".into(),
        max_examples.to_string(),
        "--report".into(),
        "junit".into(),
        "--report-junit-path".into(),
        junit.display().to_string(),
    ];
    if let Some(b) = base_url {
        args.extend(["--url".into(), b.into()]);
    }
    let env = qualyx_runners::process::child_env(&[]);
    let mut cmd = qualyx_runners::process::command(&st, &args, out_dir, &env);
    // Schemathesis prints Unicode; without UTF-8 mode it crashes when output is redirected on Windows.
    cmd.env("PYTHONUTF8", "1").env("PYTHONIOENCODING", "utf-8");
    let o = qualyx_runners::process::run(
        cmd,
        &out_dir.join("schemathesis.log"),
        timeout,
        &Default::default(),
    )
    .map_err(|e| e.to_string())?;
    let _ = qualyx_runners::redact::redact_file(&out_dir.join("schemathesis.log"));
    let version = qualyx_runners::process::version(&st, &["--version"], out_dir);
    let mut notes = Vec::new();
    let operations = match fs::read_to_string(&junit) {
        Ok(x) => qualyx_runners::report::parse_junit(&x).map_err(|e| e.to_string())?,
        Err(_) => {
            notes.push("no JUnit report produced; see schemathesis.log".into());
            Vec::new()
        }
    };
    let status = if operations.is_empty() {
        "error"
    } else if operations
        .iter()
        .any(|t| matches!(t.status, Status::Failed | Status::Error))
    {
        "failed"
    } else {
        "passed"
    };
    Ok(ContractReport {
        schema: schema_url.into(),
        tool_version: version,
        checks: DEFAULT_CHECKS.into(),
        status: status.into(),
        operations,
        exit_code: o.exit_code,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_targets_are_authorized() {
        assert!(authorize("http://127.0.0.1:8000/openapi.json", &[]).is_ok());
        assert!(authorize("https://api.example.com/openapi.json", &[]).is_err());
        assert!(
            authorize(
                "https://api.example.com/openapi.json",
                &["https://api.example.com".into()]
            )
            .is_ok()
        );
        assert!(authorize("file:///spec.json", &[]).is_err());
    }
}
