//! Optional Code Mode host. Untrusted JavaScript runs only in a restricted Linux container.
//! The stdin/stdout bridge is untrusted: authorization is repeated for every message.
pub mod registry;

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use qualyx_app::jobs::{Jobs, Roots};
use qualyx_core::Isolation;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Semaphore;

pub const FRAME_BYTES: usize = 65_536;
pub const MAX_CALLS: usize = 32;
pub const DEADLINE: Duration = Duration::from_secs(15);

/// Operator-owned configuration. Never deserialize policy from generated code or tool inputs.
#[derive(Clone)]
pub struct Config {
    pub enabled: bool,
    pub image: String,
    /// Explicit standing approvals for tests.run / tests.cancel. Empty means read-only.
    pub approved: Vec<String>,
    /// Existing durable team queue. Credentials never enter the sandbox.
    pub queue_url: Option<String>,
}
impl Config {
    pub fn from_env() -> Self {
        Self {
            enabled: std::env::var("QUALYX_CODE_MODE").as_deref() == Ok("1"),
            image: std::env::var("QUALYX_CODE_IMAGE")
                .unwrap_or_else(|_| "qualyx-code-sandbox:local".into()),
            approved: std::env::var("QUALYX_CODE_APPROVALS")
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
            queue_url: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Execute {
    /// JavaScript async function body. Use await qualyx.call("tests.discover", {path: ...}); return JSON.
    pub code: String,
}

#[derive(Clone)]
pub struct Host {
    config: Config,
    roots: Roots,
    jobs: Jobs,
    permits: Arc<Semaphore>,
    bridge_permits: Arc<Semaphore>,
    requests: Arc<Mutex<VecDeque<Instant>>>,
    audit: Arc<Mutex<std::fs::File>>,
}

impl Host {
    /// Audit failure prevents construction; Code Mode never executes without an audit sink.
    pub fn new(config: Config, roots: Roots, jobs: Jobs) -> Result<Self> {
        if config
            .approved
            .iter()
            .any(|s| !["tests.run", "tests.cancel"].contains(&s.as_str()))
        {
            bail!("unknown Code Mode approval");
        }
        if config.image.is_empty() || config.image.starts_with('-') {
            bail!("invalid sandbox image");
        }
        let root = roots
            .list()
            .first()
            .context("Code Mode needs an allowed root")?;
        let dir = root.join(".qualyx");
        std::fs::create_dir_all(&dir)?;
        let audit = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(dir.join("code-audit.jsonl"))?;
        Ok(Self {
            config,
            roots,
            jobs,
            permits: Arc::new(Semaphore::new(2)),
            bridge_permits: Arc::new(Semaphore::new(2)),
            requests: Arc::new(Mutex::new(VecDeque::new())),
            audit: Arc::new(Mutex::new(audit)),
        })
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn record(&self, id: &str, event: &str, detail: Value) -> Result<()> {
        use std::io::Write;
        let mut file = self
            .audit
            .lock()
            .map_err(|_| anyhow::anyhow!("audit lock poisoned"))?;
        writeln!(
            file,
            "{}",
            json!({"ts_unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis(),"execution_id":id,"correlation_id":id,"event":event,"detail":detail})
        )?;
        file.flush()?;
        Ok(())
    }

    fn admit(&self) -> Result<()> {
        if !self.enabled() {
            bail!("Code Mode is disabled");
        }
        let mut requests = self
            .requests
            .lock()
            .map_err(|_| anyhow::anyhow!("rate lock poisoned"))?;
        let now = Instant::now();
        while requests
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
        {
            requests.pop_front();
        }
        if requests.len() >= 30 {
            bail!("Code Mode rate limit (30 requests/minute)");
        }
        requests.push_back(now);
        Ok(())
    }

    pub fn search(&self, args: &registry::Search) -> Result<Value> {
        self.admit()?;
        registry::search(args)
    }

    /// Cancellation is by dropping this future (including MCP request cancellation).
    /// Cleanup kills the container, including if docker run is still starting.
    pub async fn execute(&self, args: Execute) -> Result<Value> {
        self.admit()?;
        if args.code.len() > 16_384 {
            bail!("code exceeds 16384 bytes");
        }
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .context("Code Mode concurrency limit (2)")?;
        let id = format!("qualyx-code-{}", uuid::Uuid::new_v4());
        self.record(
            &id,
            "start",
            json!({"queue_ms":0,"approvals":self.config.approved}),
        )?;
        let started = Instant::now();
        let mut cleanup = Cleanup {
            name: id.clone(),
            host: self.clone(),
            started,
            finished: false,
        };
        let result = tokio::time::timeout(DEADLINE, self.exchange(&id, &args.code, started)).await;
        // Remove before returning, not merely when the client connection is torn down.
        cleanup
            .remove(if matches!(&result, Ok(Ok(_))) { 1 } else { 4 })
            .await;
        let result = match result {
            Ok(r) => r,
            Err(_) => {
                self.record(&id, "timeout", json!({"source":"host"}))?;
                Err(anyhow::anyhow!("Code Mode execution timeout"))
            }
        };
        self.record(
            &id,
            "finish",
            json!({"success":result.is_ok(),"duration_ms":started.elapsed().as_millis(),
            "timeout":started.elapsed() >= DEADLINE,"truncated":false}),
        )?;
        cleanup.finished = true;
        result
    }

    async fn exchange(&self, id: &str, code: &str, started: Instant) -> Result<Value> {
        // No project mounts, environment forwarding, network, sockets, shell or module loader.
        let mut child = Command::new("docker")
            .args([
                "run",
                "--pull=never",
                "--rm",
                "--interactive",
                "--name",
                id,
                "--network=none",
                "--read-only",
                "--cap-drop=ALL",
                "--security-opt=no-new-privileges:true",
                "--user=65532:65532",
                "--pids-limit=16",
                "--memory=128m",
                "--memory-swap=128m",
                "--cpus=0.5",
                "--ulimit=nofile=64:64",
                "--ulimit=core=0:0",
                "--log-driver=none",
            ])
            .arg(&self.config.image)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("cannot start Docker sandbox")?;
        let mut input = child.stdin.take().context("sandbox stdin unavailable")?;
        let output = child.stdout.take().context("sandbox stdout unavailable")?;
        let mut output = BufReader::new(output);
        write_frame(&mut input, &json!({"code":code})).await?;
        let ready = read_frame(&mut output).await?;
        if ready != json!({"kind":"ready"}) {
            bail!("sandbox failed readiness handshake");
        }
        self.record(
            id,
            "ready",
            json!({"startup_ms":started.elapsed().as_millis()}),
        )?;
        let mut calls = 0;
        let startup_ms = started.elapsed().as_millis();
        loop {
            let msg = read_frame(&mut output).await?;
            match msg["kind"].as_str() {
                Some("call") => {
                    calls += 1;
                    if calls > MAX_CALLS {
                        bail!("capability call limit exceeded");
                    }
                    let capability = msg["capability"].as_str().context("missing capability")?;
                    let call_start = Instant::now();
                    // Do not send attacker-controlled identifiers, arguments, code, errors or results to audit.
                    let registered = registry::registry().iter().any(|c| c.id == capability);
                    self.record(id, "call", json!({"capability":if registered {capability} else {"unknown"},"call":calls}))?;
                    let host = self.clone();
                    let capability = capability.to_owned();
                    let value = msg.get("input").cloned().context("missing input")?;
                    // Blocking application services are existing bounded jobs / evidence readers.
                    // Cancelled blocking I/O retains its permit until it actually returns.
                    let bridge_permit = self
                        .bridge_permits
                        .clone()
                        .try_acquire_owned()
                        .context("host bridge busy")?;
                    let dispatch = tokio::task::spawn_blocking(move || {
                        let _permit = bridge_permit;
                        host.dispatch(&capability, value)
                    })
                    .await?;
                    self.record(id, "decision", json!({"allowed":dispatch.is_ok(),"call":calls,"duration_ms":call_start.elapsed().as_millis()}))?;
                    let reply = match dispatch {
                        Ok(v) => json!({"ok":true,"result":v}),
                        Err(_) => {
                            json!({"ok":false,"error":"capability denied, invalid input, unavailable, or result limit exceeded"})
                        }
                    };
                    write_frame(&mut input, &reply).await?;
                }
                Some("result") => {
                    let value = msg.get("result").cloned().context("missing JSON result")?;
                    let status = child.wait().await?;
                    if !status.success() {
                        bail!("sandbox failed");
                    }
                    let metrics = msg.get("metrics");
                    self.record(
                        id,
                        "runtime",
                        json!({"calls":calls,
                        "peak_rss_kib":metrics.and_then(|m| m["peak_rss_kib"].as_u64()),
                        "cpu_ticks":metrics.and_then(|m| m["cpu_ticks"].as_u64())}),
                    )?;
                    let result = json!({"execution_id":id,"result":value,"calls":calls,
                        "startup_ms":startup_ms,"runtime_metrics":msg.get("metrics"),
                        "duration_ms":started.elapsed().as_millis(),"truncated":false});
                    if serde_json::to_vec(&result)?.len() > FRAME_BYTES - 1024 {
                        bail!("final output limit exceeded");
                    }
                    return Ok(result);
                }
                Some("error") => {
                    if msg["reason"] == "runtime timeout" {
                        self.record(id, "timeout", json!({"source":"runtime"}))?;
                        bail!("Code Mode runtime timeout");
                    }
                    bail!("sandbox JavaScript failed or exceeded runtime limits");
                }
                _ => bail!("invalid sandbox protocol"),
            }
        }
    }

    /// Host authorization applies even to a compromised sandbox speaking arbitrary protocol.
    fn dispatch(&self, id: &str, input: Value) -> Result<Value> {
        let capability = registry::registry()
            .into_iter()
            .find(|c| c.id == id)
            .context("unknown capability")?;
        if !capability.read_only && !self.config.approved.iter().any(|s| s == id) {
            bail!("operator approval required");
        }
        let check = |path: &str| self.roots.check(path).map_err(anyhow::Error::msg);
        let result = match id {
            "tests.discover" => {
                let a: registry::Project = serde_json::from_value(input)?;
                serde_json::to_value(qualyx_app::discover(&check(&a.path)?)?)?
            }
            "tests.run" => {
                let a: registry::Run = serde_json::from_value(input)?;
                let path = check(&a.path)?;
                let timeout = a.timeout_secs.unwrap_or(1800);
                if !(1..=1800).contains(&timeout) {
                    bail!("invalid native test timeout");
                }
                let req = qualyx_app::TestRequest {
                    timeout_secs: Some(timeout),
                    isolation: Some(Isolation::Container),
                    ..Default::default()
                };
                if let Some(url) = &self.config.queue_url {
                    // Worker policy remains authoritative. The supplied team worker may explicitly
                    // allow process execution inside its own dedicated container.
                    let run_id = qualyx_store::queue::Queue::connect(url)?
                        .enqueue(&path, &serde_json::to_string(&req)?)?;
                    json!({"run_id":run_id,"state":"queued"})
                } else {
                    let (run_id, ecosystems) = self.jobs.start(&path, &req)?;
                    json!({"run_id":run_id,"state":"running","ecosystems":ecosystems})
                }
            }
            "tests.status" | "tests.cancel" => {
                let a: registry::Id = serde_json::from_value(input)?;
                if a.run_id.len() > 128 {
                    bail!("invalid run id");
                }
                if let Some(url) = &self.config.queue_url {
                    let mut q = qualyx_store::queue::Queue::connect(url)?;
                    let job = q.get(&a.run_id)?.context("unknown run")?;
                    check(&job.project.to_string_lossy())?;
                    if id == "tests.cancel" {
                        json!({"run_id":a.run_id,"cancel_requested":q.cancel(&a.run_id)?.is_some()})
                    } else {
                        json!({"run_id":job.run_id,"state":job.state,"cancel_requested":job.cancel_requested})
                    }
                } else {
                    let job = self.jobs.get(&a.run_id).context("unknown run")?;
                    check(&job.project.to_string_lossy())?;
                    if id == "tests.cancel" {
                        json!({"run_id":a.run_id,"cancel_requested":self.jobs.cancel(&a.run_id).is_some()})
                    } else {
                        json!({"run_id":job.run_id,"state":job.state,"cancel_requested":job.cancel_requested})
                    }
                }
            }
            "tests.results" | "artifacts.list" => {
                let a: registry::Stored = serde_json::from_value(input)?;
                let (manifest, _) = qualyx_app::load_run(&check(&a.path)?, Some(&a.run_id))?;
                if id == "tests.results" {
                    serde_json::to_value(manifest)?
                } else {
                    json!({"artifacts":manifest.runners.iter().flat_map(|r| r.artifacts.iter()).chain(["report.md".to_string(), "manifest.json".to_string()].iter()).cloned().collect::<Vec<_>>()})
                }
            }
            "artifacts.read" => {
                let a: registry::Artifact = serde_json::from_value(input)?;
                json!({"text":qualyx_app::read_artifact(&check(&a.path)?, &a.run_id, &a.name, 8192)?})
            }
            _ => bail!("unknown capability"),
        };
        registry::validate_output(&capability, &result)?;
        let text = qualyx_runners::redact::redact(&serde_json::to_string(&result)?);
        if text.len() > FRAME_BYTES - 1024 {
            bail!("capability result limit exceeded");
        }
        Ok(serde_json::from_str(&text)?)
    }
}

async fn write_frame(writer: &mut (impl AsyncWriteExt + Unpin), value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > FRAME_BYTES {
        bail!("output limit exceeded");
    }
    writer.write_all(&bytes).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn read_frame(reader: &mut (impl AsyncBufReadExt + Unpin)) -> Result<Value> {
    let mut bytes = Vec::new();
    let count = reader
        .take((FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .await?;
    if count == 0 || count > FRAME_BYTES || bytes.last() != Some(&b'\n') {
        bail!("invalid or oversized sandbox frame");
    }
    Ok(serde_json::from_slice(&bytes)?)
}

struct Cleanup {
    name: String,
    host: Host,
    started: Instant,
    finished: bool,
}
impl Cleanup {
    async fn remove(&self, attempts: usize) {
        for attempt in 0..attempts {
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                Command::new("docker")
                    .args(["rm", "--force", &self.name])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .status(),
            )
            .await;
            if attempt + 1 < attempts {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let name = self.name.clone();
        let _ = self.host.record(
            &name,
            "cancelled",
            json!({"duration_ms":self.started.elapsed().as_millis()}),
        );
        // A docker create/run request can still be in flight when its client is killed.
        // Retries cover normal startup races; daemon failure still needs operator cleanup.
        // No untrusted values become shell text.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                for _ in 0..4 {
                    let _ = tokio::time::timeout(
                        Duration::from_secs(3),
                        Command::new("docker")
                            .args(["rm", "--force", &name])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .kill_on_drop(true)
                            .status(),
                    )
                    .await;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host(dir: &std::path::Path, enabled: bool) -> Host {
        Host::new(
            Config {
                enabled,
                image: "local".into(),
                approved: vec![],
                queue_url: None,
            },
            Roots::new(&[dir.into()]).unwrap(),
            Jobs::new(2),
        )
        .unwrap()
    }
    #[tokio::test]
    async fn disabled_validation_permissions_and_limits() {
        let dir = tempfile::tempdir().unwrap();
        let h = host(dir.path(), false);
        assert!(
            h.execute(Execute {
                code: "return 1".into()
            })
            .await
            .unwrap_err()
            .to_string()
            .contains("disabled")
        );
        let h = host(dir.path(), true);
        assert!(h.dispatch("tests.run", json!({"path":dir.path()})).is_err());
        assert!(h.dispatch("tests.cancel", json!({"run_id":"x"})).is_err());
        assert!(h.dispatch("shell", json!({})).is_err());
        assert!(
            h.dispatch("tests.discover", json!({"path":dir.path(),"unknown":1}))
                .is_err()
        );
        let outside = tempfile::tempdir().unwrap();
        assert!(
            h.dispatch("tests.discover", json!({"path":outside.path()}))
                .is_err()
        );
        assert!(
            h.dispatch("tests.discover", json!({"path":dir.path()}))
                .unwrap()
                .is_object()
        );
        assert!(
            h.execute(Execute {
                code: " ".repeat(16385)
            })
            .await
            .is_err()
        );
        for _ in 0..29 {
            h.admit().unwrap();
        }
        assert!(h.admit().is_err());
    }
    #[tokio::test]
    async fn protocol_frames_are_bounded() {
        let mut bytes = std::io::Cursor::new(vec![b'x'; FRAME_BYTES + 2]);
        assert!(read_frame(&mut bytes).await.is_err());
        let mut bytes = std::io::Cursor::new(b"{\"ok\":true}\n");
        assert_eq!(read_frame(&mut bytes).await.unwrap(), json!({"ok":true}));
        assert!(
            write_frame(&mut tokio::io::sink(), &json!("x".repeat(FRAME_BYTES)))
                .await
                .is_err()
        );
    }
}
