//! Local state (ADR 0002): `manifest.json` per run is the source of truth, SQLite indexes it.
//!
//! Layout under the project root:
//! ```text
//! .qualyx/
//!   .gitignore          ("*": never committed)
//!   config.toml
//!   qualyx.db
//!   runs/<run-id>/manifest.json, report.md, <ecosystem>.log, native reports
//! ```

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use qualyx_core::{RunManifest, Status};
use rusqlite::{Connection, OptionalExtension, params};

pub const DIR: &str = ".qualyx";

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "I/O error: {e}"),
            StoreError::Sql(e) => write!(f, "database error: {e}"),
            StoreError::Json(e) => write!(f, "invalid manifest: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sql(e)
    }
}
impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Paths of the `.qualyx` directory of one project.
#[derive(Debug, Clone)]
pub struct Layout {
    pub dir: PathBuf,
}

impl Layout {
    pub fn new(project_root: &Path) -> Layout {
        Layout {
            dir: project_root.join(DIR),
        }
    }
    pub fn db(&self) -> PathBuf {
        self.dir.join("qualyx.db")
    }
    pub fn config(&self) -> PathBuf {
        self.dir.join("config.toml")
    }
    pub fn run_dir(&self, run_id: &str) -> PathBuf {
        self.dir.join("runs").join(run_id)
    }

    /// Create `.qualyx/` with a self-ignoring `.gitignore`. Idempotent.
    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(self.dir.join("runs"))?;
        let ignore = self.dir.join(".gitignore");
        if !ignore.exists() {
            fs::write(ignore, "# Qualyx local state: never commit.\n*\n")?;
        }
        Ok(())
    }
}

/// Sortable, collision-resistant, path-safe run id: `20261026T213015123Z-<pid hex>`.
pub fn new_run_id(now: DateTime<Utc>) -> String {
    format!(
        "{}-{:x}",
        now.format("%Y%m%dT%H%M%S%3fZ"),
        std::process::id()
    )
}

/// Run ids are generated, but they also arrive from the CLI; reject anything path-like.
pub fn is_valid_run_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn write_manifest(run_dir: &Path, manifest: &RunManifest) -> Result<PathBuf> {
    let path = run_dir.join("manifest.json");
    fs::write(&path, serde_json::to_vec_pretty(manifest)?)?;
    Ok(path)
}

pub fn read_manifest(path: &Path) -> Result<RunManifest> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

/// One row of `qualyx report --list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub id: String,
    pub started_at: String,
    pub status: Status,
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
}

/// Migrations indexed by `PRAGMA user_version`; append only.
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE runs (
    id            TEXT PRIMARY KEY,
    project_root  TEXT NOT NULL,
    started_at    TEXT NOT NULL,
    finished_at   TEXT NOT NULL,
    status        TEXT NOT NULL,
    total         INTEGER NOT NULL,
    passed        INTEGER NOT NULL,
    failed        INTEGER NOT NULL,
    skipped       INTEGER NOT NULL,
    errors        INTEGER NOT NULL,
    git_commit    TEXT,
    manifest_path TEXT NOT NULL
);
CREATE TABLE runner_results (
    run_id      TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    ecosystem   TEXT NOT NULL,
    tool        TEXT NOT NULL,
    status      TEXT NOT NULL,
    exit_code   INTEGER,
    duration_ms INTEGER NOT NULL,
    total       INTEGER NOT NULL,
    passed      INTEGER NOT NULL,
    failed      INTEGER NOT NULL,
    skipped     INTEGER NOT NULL,
    errors      INTEGER NOT NULL
);
CREATE TABLE test_cases (
    run_id      TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    ecosystem   TEXT NOT NULL,
    suite       TEXT,
    name        TEXT NOT NULL,
    status      TEXT NOT NULL,
    duration_ms INTEGER,
    message     TEXT
);
CREATE INDEX test_cases_by_name ON test_cases(name, status);
"#];

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(Store { conn })
    }

    pub fn insert_run(&mut self, m: &RunManifest, manifest_path: &Path) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO runs VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                m.run_id,
                m.project_root.to_string_lossy(),
                m.started_at.to_rfc3339(),
                m.finished_at.to_rfc3339(),
                m.status.as_str(),
                m.counts.total,
                m.counts.passed,
                m.counts.failed,
                m.counts.skipped,
                m.counts.errors,
                m.git.as_ref().map(|g| g.commit.as_str()),
                manifest_path.to_string_lossy(),
            ],
        )?;
        for r in &m.runners {
            tx.execute(
                "INSERT INTO runner_results VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    m.run_id,
                    r.ecosystem.as_str(),
                    r.tool.as_str(),
                    r.status.as_str(),
                    r.exit_code,
                    r.duration_ms as i64,
                    r.counts.total,
                    r.counts.passed,
                    r.counts.failed,
                    r.counts.skipped,
                    r.counts.errors,
                ],
            )?;
            for t in &r.tests {
                tx.execute(
                    "INSERT INTO test_cases VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        m.run_id,
                        r.ecosystem.as_str(),
                        t.suite,
                        t.name,
                        t.status.as_str(),
                        t.duration_ms.map(|d| d as i64),
                        t.message,
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn latest_run_id(&self) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM runs ORDER BY started_at DESC, id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn manifest_path(&self, run_id: &str) -> Result<Option<PathBuf>> {
        Ok(self
            .conn
            .query_row(
                "SELECT manifest_path FROM runs WHERE id = ?1",
                [run_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(PathBuf::from))
    }

    pub fn list_runs(&self, limit: u32) -> Result<Vec<RunSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_at, status, total, passed, failed FROM runs
             ORDER BY started_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            let status: String = r.get(2)?;
            Ok(RunSummary {
                id: r.get(0)?,
                started_at: r.get(1)?,
                status: Status::parse(&status).unwrap_or(Status::Inconclusive),
                total: r.get(3)?,
                passed: r.get(4)?,
                failed: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use qualyx_core::*;

    fn manifest(id: &str, minute: u32, status: Status) -> RunManifest {
        let at = Utc.with_ymd_and_hms(2026, 10, 26, 12, minute, 0).unwrap();
        let tests = vec![TestCase {
            name: "adds".into(),
            suite: Some("lib".into()),
            status,
            duration_ms: Some(3),
            message: None,
        }];
        let counts = Counts::from_tests(&tests);
        RunManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            run_id: id.into(),
            qualyx_version: "0.0.0".into(),
            project_root: "/p".into(),
            git: None,
            os: "linux".into(),
            arch: "x86_64".into(),
            started_at: at,
            finished_at: at,
            status,
            counts,
            runners: vec![RunnerResult {
                ecosystem: Ecosystem::Rust,
                tool: TestTool::CargoTest,
                tool_version: None,
                command: vec!["cargo".into(), "test".into()],
                exit_code: Some(0),
                timed_out: false,
                started_at: at,
                finished_at: at,
                duration_ms: 10,
                status,
                detail: Detail::PerTest,
                counts,
                tests,
                artifacts: vec!["rust.log".into()],
                notes: vec![],
            }],
        }
    }

    #[test]
    fn stores_and_lists_runs_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("q.db")).unwrap();
        s.insert_run(
            &manifest("a", 1, Status::Passed),
            Path::new("a/manifest.json"),
        )
        .unwrap();
        s.insert_run(
            &manifest("b", 2, Status::Failed),
            Path::new("b/manifest.json"),
        )
        .unwrap();

        assert_eq!(s.latest_run_id().unwrap().as_deref(), Some("b"));
        let runs = s.list_runs(10).unwrap();
        assert_eq!(
            runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(runs[0].status, Status::Failed);
        assert_eq!(runs[0].failed, 1);
        assert_eq!(
            s.manifest_path("a").unwrap(),
            Some(PathBuf::from("a/manifest.json"))
        );
        assert_eq!(s.manifest_path("zzz").unwrap(), None);
    }

    #[test]
    fn duplicate_run_id_is_rejected_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("q.db")).unwrap();
        let m = manifest("a", 1, Status::Passed);
        s.insert_run(&m, Path::new("m")).unwrap();
        assert!(s.insert_run(&m, Path::new("m")).is_err());
        assert_eq!(s.list_runs(10).unwrap().len(), 1);
    }

    #[test]
    fn reopening_does_not_rerun_migrations() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("q.db");
        Store::open(&db).unwrap();
        Store::open(&db).unwrap();
    }

    #[test]
    fn manifest_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let m = manifest("a", 1, Status::Passed);
        let p = write_manifest(dir.path(), &m).unwrap();
        assert_eq!(read_manifest(&p).unwrap(), m);
    }

    #[test]
    fn run_ids_are_path_safe() {
        let id = new_run_id(Utc::now());
        assert!(is_valid_run_id(&id), "{id}");
        for bad in ["", "../x", "a/b", r"a\b", "a.b"] {
            assert!(!is_valid_run_id(bad), "{bad}");
        }
    }
}
