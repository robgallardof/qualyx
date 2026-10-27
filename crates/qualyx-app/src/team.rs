//! Separate durable workers. The API only enqueues requests; worker ownership is leased,
//! cancellation is polled, and lost ownership never becomes a passing result.

use crate::{TestRequest, jobs::Roots};
use anyhow::{Context, Result, bail};
use qualyx_core::Isolation;
use qualyx_runners::CancelToken;
use qualyx_store::{Layout, queue::Queue};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// Worker roots and execution policy. Process mode requires explicit operator consent.
pub struct WorkerConfig {
    pub roots: Vec<PathBuf>,
    pub allow_process: bool,
    pub once: bool,
}

fn run_loop(cfg: WorkerConfig, stop: CancelToken) -> Result<()> {
    let url = std::env::var("QUALYX_DATABASE_URL").context("worker needs QUALYX_DATABASE_URL")?;
    let roots = Roots::new(&cfg.roots)?;
    let owner = qualyx_store::new_run_id(chrono::Utc::now());
    let mut queue = Queue::connect(&url)?;
    while !stop.is_cancelled() {
        let Some(job) = queue.claim(&owner, roots.list())? else {
            if cfg.once {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
            continue;
        };
        let cancel = CancelToken::default();
        let runner_cancel = cancel.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let allow_process = cfg.allow_process;
        let roots = roots.clone();
        let execution = job.clone();
        let handle = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| -> Result<serde_json::Value> {
                let project = roots
                    .check(&execution.project.to_string_lossy())
                    .map_err(anyhow::Error::msg)?;
                let mut req: TestRequest = serde_json::from_str(&execution.request)?;
                if req.compose.is_some() {
                    bail!("team jobs cannot start arbitrary Compose services");
                }
                req.isolation = Some(if allow_process {
                    Isolation::Process
                } else {
                    Isolation::Container
                });
                req.timeout_secs = Some(req.timeout_secs.unwrap_or(600).clamp(1, 1800));
                req.jobs = req.jobs.clamp(1, 8);
                let mut prepared = crate::prepare(&project, &req, runner_cancel.clone())?;
                prepared.run_id = execution.run_id;
                prepared.run_dir = Layout::new(&project).run_dir(&prepared.run_id);
                // Repository config cannot opt worker credentials into child environments.
                prepared.opts.env_passthrough.clear();
                let dir = prepared.run_dir.clone();
                let manifest = prepared.execute(|_| {})?;
                crate::archive::upload(&manifest, &dir)?;
                Ok(serde_json::to_value(manifest)?)
            })
            .unwrap_or_else(|_| Err(anyhow::anyhow!("worker execution panicked")));
            let _ = tx.send(result);
        });
        let mut lease_error = None;
        let result = loop {
            if stop.is_cancelled() {
                cancel.cancel();
            }
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(result) => break result,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(anyhow::anyhow!("worker execution disconnected"));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    match queue.heartbeat(&job.run_id, &owner) {
                        Ok(true) => cancel.cancel(),
                        Ok(false) => {}
                        Err(e) => {
                            cancel.cancel();
                            lease_error = Some(e);
                        }
                    }
                }
            }
        };
        let _ = handle.join();
        if let Some(e) = lease_error {
            return Err(e.into());
        }
        match result {
            Ok(manifest) => queue.finish(&job.run_id, &owner, Some(&manifest), None)?,
            Err(e) => queue.finish(
                &job.run_id,
                &owner,
                None,
                Some(&qualyx_runners::redact::redact(&format!("{e:#}"))),
            )?,
        }
        if cfg.once {
            return Ok(());
        }
    }
    Ok(())
}

/// Run a serial worker; Ctrl-C/SIGTERM requests cancellation and waits for evidence cleanup.
pub fn worker(cfg: WorkerConfig) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let stop = CancelToken::default();
        let worker_stop = stop.clone();
        let mut work = tokio::task::spawn_blocking(move || run_loop(cfg, worker_stop));
        tokio::select! {
            result = &mut work => result?,
            _ = shutdown() => { stop.cancel(); work.await? }
        }
    })
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
