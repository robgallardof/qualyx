//! Authorized DAST with OWASP ZAP's baseline scan (spider + passive rules only, no attacks),
//! run from the official container image.
//!
//! Targets must be loopback or listed in `[security] dast_allowed_targets`: scanning a system
//! you don't own is not a feature (threat model T8).

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::Finding;

pub const ZAP_IMAGE: &str = "ghcr.io/zaproxy/zaproxy:stable";

fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    Some(if host.starts_with('[') {
        host.split(']').next()?.trim_start_matches('[')
    } else {
        host.split(':').next()?
    })
}

fn is_loopback(url: &str) -> bool {
    matches!(host_of(url), Some("127.0.0.1" | "localhost" | "::1"))
}

pub fn authorize(target: &str, allowed: &[String]) -> Result<(), String> {
    if !(target.starts_with("http://") || target.starts_with("https://")) {
        return Err(format!("target must be http(s): {target}"));
    }
    let listed = allowed.iter().any(|a| {
        let a = a.trim_end_matches('/');
        target == a || target.starts_with(&format!("{a}/"))
    });
    if is_loopback(target) || listed {
        Ok(())
    } else {
        Err(format!(
            "{target} is not loopback and not in [security] dast_allowed_targets"
        ))
    }
}

/// How the container reaches a loopback target on the host.
fn container_target(target: &str) -> (String, Vec<String>) {
    if !is_loopback(target) {
        return (target.to_string(), Vec::new());
    }
    if cfg!(target_os = "linux") {
        (target.to_string(), vec!["--network=host".into()])
    } else {
        // Docker Desktop (Windows/macOS) proxies host.docker.internal to the host.
        let h = host_of(target).unwrap_or("127.0.0.1");
        let h = if h == "::1" { "[::1]" } else { h };
        (target.replacen(h, "host.docker.internal", 1), Vec::new())
    }
}

fn level(riskcode: &str) -> &'static str {
    match riskcode {
        "3" => "error",
        "2" => "warning",
        _ => "note",
    }
}

/// ZAP's JSON report (`-J`) -> findings.
pub fn parse_report(v: &Value) -> Vec<Finding> {
    let mut out = Vec::new();
    for site in v["site"].as_array().into_iter().flatten() {
        for a in site["alerts"].as_array().into_iter().flatten() {
            let instances = a["instances"].as_array().map(Vec::len).unwrap_or(0);
            out.push(Finding {
                tool: "zap".into(),
                rule_id: a["pluginid"].as_str().unwrap_or("").into(),
                level: level(a["riskcode"].as_str().unwrap_or("0")).into(),
                message: format!(
                    "{} ({} instance(s))",
                    a["alert"].as_str().unwrap_or("alert"),
                    instances
                ),
                file: a["instances"][0]["uri"].as_str().map(String::from),
                line: None,
            });
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct DastReport {
    pub target: String,
    pub image: String,
    pub status: String,
    pub findings: Vec<Finding>,
    pub notes: Vec<String>,
}

/// Run the ZAP baseline scan against an authorized target; evidence goes to `out_dir`.
pub fn baseline(
    target: &str,
    allowed: &[String],
    out_dir: &Path,
    timeout: Duration,
) -> Result<DastReport, String> {
    authorize(target, allowed)?;
    let engine = qualyx_runners::container::engine().ok_or("DAST needs docker or podman")?;
    fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        // The ZAP image runs as an unprivileged user that must write the report.
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(out_dir, fs::Permissions::from_mode(0o777));
    }
    let (inner, mut net) = container_target(target);
    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
    ];
    args.append(&mut net);
    args.extend([
        "-v".into(),
        format!("{}:/zap/wrk", out_dir.display()),
        ZAP_IMAGE.into(),
        "zap-baseline.py".into(),
        "-t".into(),
        inner.clone(),
        "-J".into(),
        "zap-report.json".into(),
        "-I".into(), // findings are reported by Qualyx, not as a failing exit code
    ]);
    let env = qualyx_runners::process::child_env(&[]);
    let cmd = qualyx_runners::process::command(&engine, &args, out_dir, &env);
    let outcome =
        qualyx_runners::process::run(cmd, &out_dir.join("zap.log"), timeout, &Default::default())
            .map_err(|e| e.to_string())?;
    let report: Value = fs::read(out_dir.join("zap-report.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| {
            format!(
                "ZAP produced no report (exit {:?}); see zap.log",
                outcome.exit_code
            )
        })?;
    let findings = parse_report(&report);
    let mut notes = vec!["baseline scan: spider + passive rules, no active attacks".to_string()];
    if inner != target {
        notes.push(format!("scanned as {inner} from inside the container"));
    }
    Ok(DastReport {
        target: target.into(),
        image: ZAP_IMAGE.into(),
        status: if findings.is_empty() {
            "clean".into()
        } else {
            "findings".into()
        },
        findings,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_authorized_targets() {
        assert!(authorize("http://127.0.0.1:8080/", &[]).is_ok());
        for t in [
            "https://example.com",
            "http://10.0.0.5/",
            "http://169.254.169.254/",
            "ftp://127.0.0.1/",
        ] {
            assert!(authorize(t, &[]).is_err(), "{t}");
        }
        assert!(
            authorize(
                "https://qa.example.com/app",
                &["https://qa.example.com".into()]
            )
            .is_ok()
        );
    }

    #[test]
    fn parses_zap_alerts() {
        let v = json!({"site": [{"alerts": [
            {"pluginid": "10038", "alert": "Content Security Policy (CSP) Header Not Set", "riskcode": "2", "instances": [{"uri": "http://h/x"}, {"uri": "http://h/y"}]},
            {"pluginid": "10021", "alert": "X-Content-Type-Options Header Missing", "riskcode": "1", "instances": [{"uri": "http://h/x"}]}
        ]}]});
        let f = parse_report(&v);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].level, "warning");
        assert!(f[0].message.contains("2 instance"));
        assert_eq!(f[1].level, "note");
    }

    #[test]
    fn loopback_is_rewritten_off_linux() {
        let (t, net) = container_target("http://127.0.0.1:9000/a");
        if cfg!(target_os = "linux") {
            assert_eq!(t, "http://127.0.0.1:9000/a");
            assert_eq!(net, ["--network=host"]);
        } else {
            assert_eq!(t, "http://host.docker.internal:9000/a");
        }
    }
}
