//! Shared run index in PostgreSQL for team mode (ADR 0002). Manifests and artifacts stay on
//! the filesystem (a shared volume); the database indexes them across projects.
//!
//! Connections use `NoTls`: team mode is meant for a private network (e.g. the Compose
//! network of `deploy/team`). TLS to an external database is not supported yet.

use std::path::{Path, PathBuf};

use postgres::{Client, NoTls};
use qualyx_core::{RunManifest, Status};

use crate::{Result, RunIndex, RunSummary, StoreError};

/// Applied in order inside a transaction; `qualyx_schema.version` records progress.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE runs (
    id            TEXT NOT NULL,
    project_root  TEXT NOT NULL,
    started_at    TIMESTAMPTZ NOT NULL,
    finished_at   TIMESTAMPTZ NOT NULL,
    status        TEXT NOT NULL,
    total         INTEGER NOT NULL,
    passed        INTEGER NOT NULL,
    failed        INTEGER NOT NULL,
    skipped       INTEGER NOT NULL,
    errors        INTEGER NOT NULL,
    git_commit    TEXT,
    manifest_path TEXT NOT NULL,
    PRIMARY KEY (project_root, id)
);
CREATE TABLE runner_results (
    project_root TEXT NOT NULL,
    run_id       TEXT NOT NULL,
    ecosystem    TEXT NOT NULL,
    tool         TEXT NOT NULL,
    status       TEXT NOT NULL,
    exit_code    INTEGER,
    duration_ms  BIGINT NOT NULL,
    total        INTEGER NOT NULL,
    passed       INTEGER NOT NULL,
    failed       INTEGER NOT NULL,
    skipped      INTEGER NOT NULL,
    errors       INTEGER NOT NULL,
    FOREIGN KEY (project_root, run_id) REFERENCES runs(project_root, id) ON DELETE CASCADE
);
CREATE TABLE test_cases (
    project_root TEXT NOT NULL,
    run_id       TEXT NOT NULL,
    ecosystem    TEXT NOT NULL,
    suite        TEXT,
    name         TEXT NOT NULL,
    status       TEXT NOT NULL,
    duration_ms  BIGINT,
    message      TEXT,
    FOREIGN KEY (project_root, run_id) REFERENCES runs(project_root, id) ON DELETE CASCADE
);
CREATE INDEX test_cases_by_name ON test_cases(project_root, name, status);
CREATE INDEX runs_by_time ON runs(project_root, started_at DESC);
"#,
    r#"
CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    project TEXT NOT NULL,
    request TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued','running','finished','failed','cancelled')),
    cancel_requested BOOLEAN NOT NULL DEFAULT FALSE,
    owner TEXT,
    lease_until TIMESTAMPTZ,
    manifest TEXT,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX jobs_pending ON jobs(created_at, id) WHERE state = 'queued';
"#,
];

fn sql(e: postgres::Error) -> StoreError {
    StoreError::Io(std::io::Error::other(format!("postgres: {e}")))
}

pub struct PgIndex {
    client: Client,
    project: String,
}

impl PgIndex {
    pub fn connect(url: &str, project_root: &Path) -> Result<PgIndex> {
        let mut client = Client::connect(url, NoTls).map_err(sql)?;
        migrate(&mut client)?;
        Ok(PgIndex {
            client,
            project: project_root.to_string_lossy().into_owned(),
        })
    }
}

/// Bring the schema up to date. Safe to call concurrently from several Qualyx processes.
pub fn migrate(client: &mut Client) -> Result<usize> {
    let mut tx = client.transaction().map_err(sql)?;
    // Serialize migrators across processes for the duration of the transaction.
    tx.batch_execute("SELECT pg_advisory_xact_lock(4242424242); CREATE TABLE IF NOT EXISTS qualyx_schema (version INTEGER NOT NULL); INSERT INTO qualyx_schema SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM qualyx_schema);").map_err(sql)?;
    let current: i32 = tx
        .query_one("SELECT version FROM qualyx_schema", &[])
        .map_err(sql)?
        .get(0);
    if current < 0 || current as usize > MIGRATIONS.len() {
        return Err(StoreError::Io(std::io::Error::other(
            "unsupported PostgreSQL schema version",
        )));
    }
    let mut applied = 0;
    for (i, m) in MIGRATIONS.iter().enumerate().skip(current.max(0) as usize) {
        tx.batch_execute(m).map_err(sql)?;
        tx.execute(
            "UPDATE qualyx_schema SET version = $1",
            &[&((i + 1) as i32)],
        )
        .map_err(sql)?;
        applied += 1;
    }
    tx.commit().map_err(sql)?;
    Ok(applied)
}

impl RunIndex for PgIndex {
    fn insert_run(&mut self, m: &RunManifest, manifest_path: &Path) -> Result<()> {
        let p = self.project.clone();
        let mut tx = self.client.transaction().map_err(sql)?;
        tx.execute(
            "INSERT INTO runs VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            &[
                &m.run_id,
                &p,
                &m.started_at,
                &m.finished_at,
                &m.status.as_str(),
                &(m.counts.total as i32),
                &(m.counts.passed as i32),
                &(m.counts.failed as i32),
                &(m.counts.skipped as i32),
                &(m.counts.errors as i32),
                &m.git.as_ref().map(|g| g.commit.clone()),
                &manifest_path.to_string_lossy().into_owned(),
            ],
        )
        .map_err(sql)?;
        for r in &m.runners {
            tx.execute(
                "INSERT INTO runner_results VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                &[
                    &p,
                    &m.run_id,
                    &r.ecosystem.as_str(),
                    &r.tool.as_str(),
                    &r.status.as_str(),
                    &r.exit_code,
                    &(r.duration_ms as i64),
                    &(r.counts.total as i32),
                    &(r.counts.passed as i32),
                    &(r.counts.failed as i32),
                    &(r.counts.skipped as i32),
                    &(r.counts.errors as i32),
                ],
            )
            .map_err(sql)?;
            for t in &r.tests {
                tx.execute(
                    "INSERT INTO test_cases VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                    &[
                        &p,
                        &m.run_id,
                        &r.ecosystem.as_str(),
                        &t.suite,
                        &t.name,
                        &t.status.as_str(),
                        &t.duration_ms.map(|d| d as i64),
                        &t.message,
                    ],
                )
                .map_err(sql)?;
            }
        }
        tx.commit().map_err(sql)
    }

    fn latest_run_id(&mut self) -> Result<Option<String>> {
        let p = self.project.clone();
        Ok(self
            .client
            .query_opt("SELECT id FROM runs WHERE project_root = $1 ORDER BY started_at DESC, id DESC LIMIT 1", &[&p])
            .map_err(sql)?
            .map(|r| r.get(0)))
    }

    fn manifest_path(&mut self, run_id: &str) -> Result<Option<PathBuf>> {
        let p = self.project.clone();
        Ok(self
            .client
            .query_opt(
                "SELECT manifest_path FROM runs WHERE project_root = $1 AND id = $2",
                &[&p, &run_id],
            )
            .map_err(sql)?
            .map(|r| PathBuf::from(r.get::<_, String>(0))))
    }

    fn list_runs(&mut self, limit: u32) -> Result<Vec<RunSummary>> {
        let p = self.project.clone();
        let rows = self
            .client
            .query(
                "SELECT id, started_at, status, total, passed, failed FROM runs WHERE project_root = $1 ORDER BY started_at DESC, id DESC LIMIT $2",
                &[&p, &(limit as i64)],
            )
            .map_err(sql)?;
        Ok(rows
            .iter()
            .map(|r| {
                let started: chrono::DateTime<chrono::Utc> = r.get(1);
                let status: String = r.get(2);
                RunSummary {
                    id: r.get(0),
                    started_at: started.to_rfc3339(),
                    status: Status::parse(&status).unwrap_or(Status::Inconclusive),
                    total: r.get::<_, i32>(3) as u32,
                    passed: r.get::<_, i32>(4) as u32,
                    failed: r.get::<_, i32>(5) as u32,
                }
            })
            .collect())
    }
}
