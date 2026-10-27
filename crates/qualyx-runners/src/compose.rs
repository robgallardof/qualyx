//! Throwaway test environments from a Compose file (databases, queues, HTTP dependencies).
//!
//! `up` starts the services under a unique project name and waits for health checks; every
//! published port is exported to the tests as `QUALYX_SVC_<SERVICE>_<PORT>=host:port`; `down`
//! removes containers and volumes. Teardown happens even when the tests fail.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

pub struct ComposeEnv {
    engine: PathBuf,
    file: PathBuf,
    pub project: String,
    /// `(VAR, host:port)` for every published port.
    pub env: Vec<(String, String)>,
}

fn env_name(service: &str, port: u64) -> String {
    let s: String = service
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("QUALYX_SVC_{s}_{port}")
}

/// Parse `compose ps --format json` (one JSON object per line, or a JSON array in older
/// releases) into exported variables.
pub fn published_env(ps_output: &str) -> Vec<(String, String)> {
    let trimmed = ps_output.trim();
    let items: Vec<Value> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).unwrap_or_default()
    } else {
        trimmed
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    };
    let mut out = Vec::new();
    for it in items {
        let service = it["Service"].as_str().unwrap_or_default();
        for p in it["Publishers"].as_array().into_iter().flatten() {
            let (target, published) = (
                p["TargetPort"].as_u64().unwrap_or(0),
                p["PublishedPort"].as_u64().unwrap_or(0),
            );
            if published == 0 {
                continue;
            }
            let host = match p["URL"].as_str().unwrap_or("") {
                "" | "0.0.0.0" | "::" => "127.0.0.1",
                h => h,
            };
            let var = env_name(service, target);
            if !out.iter().any(|(k, _)| k == &var) {
                out.push((var, format!("{host}:{published}")));
            }
        }
    }
    out
}

fn compose(engine: &Path, file: &Path, project: &str, args: &[&str]) -> Command {
    let mut c = Command::new(engine);
    c.arg("compose")
        .arg("-f")
        .arg(file)
        .args(["-p", project])
        .args(args);
    c
}

impl ComposeEnv {
    pub fn up(file: &Path, run_id: &str, timeout: Duration) -> Result<ComposeEnv, String> {
        let engine =
            crate::container::engine().ok_or("compose environments need docker or podman")?;
        let file = file
            .canonicalize()
            .map_err(|e| format!("{}: {e}", file.display()))?;
        let project = format!("qualyx-{}", run_id.to_ascii_lowercase());
        let mut env = ComposeEnv {
            engine,
            file,
            project,
            env: Vec::new(),
        };
        let secs = timeout.as_secs().to_string();
        let up = compose(
            &env.engine,
            &env.file,
            &env.project,
            &["up", "-d", "--wait", "--wait-timeout", &secs],
        )
        .output()
        .map_err(|e| e.to_string())?;
        if !up.status.success() {
            let msg = String::from_utf8_lossy(&up.stderr)
                .lines()
                .rev()
                .take(8)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            env.down();
            return Err(format!("compose up failed:\n{msg}"));
        }
        let ps = compose(
            &env.engine,
            &env.file,
            &env.project,
            &["ps", "--format", "json"],
        )
        .output()
        .map_err(|e| e.to_string())?;
        env.env = published_env(&String::from_utf8_lossy(&ps.stdout));
        Ok(env)
    }

    pub fn down(&mut self) {
        let _ = compose(
            &self.engine,
            &self.file,
            &self.project,
            &["down", "-v", "--remove-orphans"],
        )
        .output();
    }
}

impl Drop for ComposeEnv {
    fn drop(&mut self) {
        self.down();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_published_ports() {
        // Docker Compose v2 NDJSON shape.
        let ps = r#"{"Service":"web","Publishers":[{"URL":"127.0.0.1","TargetPort":80,"PublishedPort":49321,"Protocol":"tcp"},{"URL":"::","TargetPort":80,"PublishedPort":49321,"Protocol":"tcp"}]}
{"Service":"db-main","Publishers":[{"URL":"0.0.0.0","TargetPort":5432,"PublishedPort":55432,"Protocol":"tcp"},{"URL":"","TargetPort":9999,"PublishedPort":0,"Protocol":"tcp"}]}"#;
        assert_eq!(
            published_env(ps),
            vec![
                (
                    "QUALYX_SVC_WEB_80".to_string(),
                    "127.0.0.1:49321".to_string()
                ),
                (
                    "QUALYX_SVC_DB_MAIN_5432".to_string(),
                    "127.0.0.1:55432".to_string()
                ),
            ]
        );
        assert_eq!(published_env(r#"[{"Service":"x","Publishers":[{"URL":"127.0.0.1","TargetPort":1,"PublishedPort":2}]}]"#).len(), 1);
    }
}
