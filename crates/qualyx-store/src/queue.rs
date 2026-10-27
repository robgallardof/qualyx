//! PostgreSQL-backed, bounded work queue. Claims are atomic; expired leases fail closed
//! instead of rerunning a job whose previous worker might still be executing tests.

use crate::{Result, StoreError};
use postgres::{Client, NoTls, Row};
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// A durable run visible to any API process sharing the database.
#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub run_id: String,
    pub project: PathBuf,
    pub state: String,
    pub cancel_requested: bool,
    pub manifest: Option<Value>,
    pub error: Option<String>,
    #[serde(skip)]
    pub request: String,
}

fn sql(e: postgres::Error) -> StoreError {
    StoreError::Io(std::io::Error::other(format!("queue: {e}")))
}

fn row(r: Row) -> Result<Job> {
    let manifest: Option<String> = r.get("manifest");
    Ok(Job {
        run_id: r.get("id"),
        project: PathBuf::from(r.get::<_, String>("project")),
        state: r.get("state"),
        cancel_requested: r.get("cancel_requested"),
        manifest: manifest.map(|s| serde_json::from_str(&s)).transpose()?,
        error: r.get("error"),
        request: r.get("request"),
    })
}

/// One synchronous queue connection; create it outside an async runtime worker thread.
pub struct Queue(Client);

impl Queue {
    /// Connect and apply versioned migrations under the shared advisory lock.
    pub fn connect(url: &str) -> Result<Self> {
        let mut client = Client::connect(url, NoTls).map_err(sql)?;
        crate::pg::migrate(&mut client)?;
        Ok(Self(client))
    }

    /// Enqueue without executing discovery or any project-supplied code in the API.
    pub fn enqueue(&mut self, project: &Path, request: &str) -> Result<String> {
        let mut tx = self.0.transaction().map_err(sql)?;
        tx.batch_execute("SELECT pg_advisory_xact_lock(4242424243)")
            .map_err(sql)?;
        let count: i64 = tx
            .query_one(
                "SELECT count(*) FROM jobs WHERE state IN ('queued','running')",
                &[],
            )
            .map_err(sql)?
            .get(0);
        if count >= 64 {
            return Err(StoreError::Io(std::io::Error::other(
                "team queue capacity reached (64)",
            )));
        }
        let id = crate::new_run_id(chrono::Utc::now());
        tx.execute(
            "INSERT INTO jobs (id,project,request,state) VALUES ($1,$2,$3,'queued')",
            &[&id, &project.to_string_lossy().as_ref(), &request],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(id)
    }

    /// Mark lost workers as errors. Never automatically re-execute potentially destructive tests.
    pub fn expire(&mut self) -> Result<u64> {
        self.0.execute("UPDATE jobs SET state='failed',error='worker lease expired; execution interrupted' WHERE state='running' AND lease_until < CURRENT_TIMESTAMP", &[]).map_err(sql)
    }

    /// Claim one allowed-root job, skipping rows already locked by another worker.
    pub fn claim(&mut self, owner: &str, roots: &[PathBuf]) -> Result<Option<Job>> {
        self.expire()?;
        let mut tx = self.0.transaction().map_err(sql)?;
        let candidates = tx.query("SELECT * FROM jobs WHERE state='queued' ORDER BY created_at,id LIMIT 64 FOR UPDATE SKIP LOCKED", &[]).map_err(sql)?;
        for candidate in candidates {
            let job = row(candidate)?;
            if !roots.iter().any(|root| job.project.starts_with(root)) {
                continue;
            }
            tx.execute("UPDATE jobs SET state='running',owner=$2,lease_until=CURRENT_TIMESTAMP + INTERVAL '15 seconds' WHERE id=$1", &[&job.run_id, &owner]).map_err(sql)?;
            tx.commit().map_err(sql)?;
            return Ok(Some(Job {
                state: "running".into(),
                ..job
            }));
        }
        tx.commit().map_err(sql)?;
        Ok(None)
    }

    /// Extend ownership and return cancellation. Missing/expired ownership is an error.
    pub fn heartbeat(&mut self, id: &str, owner: &str) -> Result<bool> {
        let r = self.0.query_opt("UPDATE jobs SET lease_until=CURRENT_TIMESTAMP + INTERVAL '15 seconds' WHERE id=$1 AND owner=$2 AND state='running' AND lease_until >= CURRENT_TIMESTAMP RETURNING cancel_requested", &[&id, &owner]).map_err(sql)?;
        r.map(|r| r.get(0))
            .ok_or_else(|| StoreError::Io(std::io::Error::other("worker lease lost")))
    }

    /// Finish only a live owned job; late workers cannot overwrite an interruption result.
    pub fn finish(
        &mut self,
        id: &str,
        owner: &str,
        manifest: Option<&Value>,
        error: Option<&str>,
    ) -> Result<()> {
        let serialized = manifest.map(serde_json::to_string).transpose()?;
        let state = if error.is_some() {
            "failed"
        } else {
            "finished"
        };
        let count = self.0.execute("UPDATE jobs SET state=$3,manifest=$4,error=$5 WHERE id=$1 AND owner=$2 AND state='running' AND lease_until >= CURRENT_TIMESTAMP", &[&id, &owner, &state, &serialized, &error]).map_err(sql)?;
        if count != 1 {
            return Err(StoreError::Io(std::io::Error::other(
                "worker lease lost before completion",
            )));
        }
        Ok(())
    }

    /// Retrieve one durable status without running project discovery.
    pub fn get(&mut self, id: &str) -> Result<Option<Job>> {
        self.expire()?;
        self.0
            .query_opt("SELECT * FROM jobs WHERE id=$1", &[&id])
            .map_err(sql)?
            .map(row)
            .transpose()
    }

    /// Recent jobs; callers must enforce their configured root allowlist on each row.
    pub fn list(&mut self) -> Result<Vec<Job>> {
        self.expire()?;
        self.0
            .query(
                "SELECT * FROM jobs ORDER BY (state IN ('queued','running')) DESC,created_at DESC,id DESC LIMIT 200",
                &[],
            )
            .map_err(sql)?
            .into_iter()
            .map(row)
            .collect()
    }

    /// Cancel queued jobs immediately or signal the current worker.
    pub fn cancel(&mut self, id: &str) -> Result<Option<bool>> {
        let r = self.0.query_opt("UPDATE jobs SET cancel_requested=TRUE,state=CASE WHEN state='queued' THEN 'cancelled' ELSE state END WHERE id=$1 AND state IN ('queued','running') RETURNING id", &[&id]).map_err(sql)?;
        if r.is_some() {
            return Ok(Some(true));
        }
        Ok(self.get(id)?.map(|_| false))
    }
}
