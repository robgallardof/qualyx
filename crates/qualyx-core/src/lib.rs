//! Domain types shared by every Qualyx crate.
//!
//! These types are the public contract: they are serialized into `manifest.json`, stored in
//! SQLite and exported as JSON Schemas under `schemas/`. Changing a field is a schema change.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Version of the `manifest.json` layout. Bump on breaking changes.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// A language/build ecosystem Qualyx knows how to discover and test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    Rust,
    Dotnet,
    Node,
    Python,
}

impl Ecosystem {
    pub fn as_str(self) -> &'static str {
        match self {
            Ecosystem::Rust => "rust",
            Ecosystem::Dotnet => "dotnet",
            Ecosystem::Node => "node",
            Ecosystem::Python => "python",
        }
    }
}

/// The native tool Qualyx drives for an ecosystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TestTool {
    /// `cargo test`, results parsed from its text output.
    CargoTest,
    /// `dotnet test` with the TRX logger.
    DotnetTest,
    /// `npm test` whose script is `node --test`, with the JUnit reporter.
    NodeTest,
    /// `npm test` with an unknown runner: only the exit code is trusted.
    NpmScript,
    /// `python -m pytest` with `--junitxml`.
    Pytest,
}

impl TestTool {
    pub fn as_str(self) -> &'static str {
        match self {
            TestTool::CargoTest => "cargo-test",
            TestTool::DotnetTest => "dotnet-test",
            TestTool::NodeTest => "node-test",
            TestTool::NpmScript => "npm-script",
            TestTool::Pytest => "pytest",
        }
    }
}

/// One ecosystem found in a project, with the evidence that identified it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DetectedEcosystem {
    pub ecosystem: Ecosystem,
    pub tool: TestTool,
    /// File that proves the ecosystem, relative to the project root (forward slashes).
    pub manifest: String,
    /// Optional target passed to the tool (e.g. a `.sln` for `dotnet test`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Human-readable caveats, e.g. why only the exit code is trusted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Result of `qualyx discover`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectProfile {
    pub root: PathBuf,
    pub ecosystems: Vec<DetectedEcosystem>,
}

/// Outcome of a single test, a runner or a whole run.
///
/// `Inconclusive` means Qualyx could not prove pass or fail, and it never counts as green.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Passed,
    Failed,
    Skipped,
    Error,
    Inconclusive,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Passed => "passed",
            Status::Failed => "failed",
            Status::Skipped => "skipped",
            Status::Error => "error",
            Status::Inconclusive => "inconclusive",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        Some(match s {
            "passed" => Status::Passed,
            "failed" => Status::Failed,
            "skipped" => Status::Skipped,
            "error" => Status::Error,
            "inconclusive" => Status::Inconclusive,
            _ => return None,
        })
    }

    /// Worst-first ordering used to aggregate many statuses into one.
    fn severity(self) -> u8 {
        match self {
            Status::Error => 4,
            Status::Failed => 3,
            Status::Inconclusive => 2,
            Status::Passed => 1,
            Status::Skipped => 0,
        }
    }

    /// Aggregate statuses: the worst one wins. An empty set is inconclusive.
    pub fn worst(statuses: impl IntoIterator<Item = Status>) -> Status {
        statuses
            .into_iter()
            .max_by_key(|s| s.severity())
            .unwrap_or(Status::Inconclusive)
    }
}

/// Where the native tools run (ADR 0004).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Isolation {
    /// Local subprocess with the user's privileges and a filtered environment.
    #[default]
    Process,
    /// A throwaway container over a copy of the project (Docker-compatible engine).
    Container,
}

impl Isolation {
    pub fn as_str(self) -> &'static str {
        match self {
            Isolation::Process => "process",
            Isolation::Container => "container",
        }
    }
}

/// How much of a runner's result is backed by per-test evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Detail {
    /// Every test was parsed from a structured report or the tool's output.
    PerTest,
    /// Only the process exit code is known.
    ExitCodeOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TestCase {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Failure/skip message, truncated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Counts {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub errors: u32,
}

impl Counts {
    pub fn from_tests(tests: &[TestCase]) -> Counts {
        let mut c = Counts::default();
        for t in tests {
            c.total += 1;
            match t.status {
                Status::Passed => c.passed += 1,
                Status::Failed => c.failed += 1,
                Status::Skipped => c.skipped += 1,
                Status::Error | Status::Inconclusive => c.errors += 1,
            }
        }
        c
    }

    pub fn add(&mut self, other: Counts) {
        self.total += other.total;
        self.passed += other.passed;
        self.failed += other.failed;
        self.skipped += other.skipped;
        self.errors += other.errors;
    }
}

/// Everything known about one native tool invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunnerResult {
    pub ecosystem: Ecosystem,
    pub tool: TestTool,
    /// First line of `<tool> --version`, if it could be obtained.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    /// Exact program and arguments executed (no shell involved).
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// Stopped on request (`qa_cancel`, Ctrl-C) before finishing.
    #[serde(default)]
    pub cancelled: bool,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub status: Status,
    pub detail: Detail,
    pub counts: Counts,
    pub tests: Vec<TestCase>,
    /// Artifact paths relative to the run directory (logs, JUnit, TRX).
    pub artifacts: Vec<String>,
    /// Why the status is what it is, when it isn't obvious from the tests.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Source control state of the project when the run started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GitInfo {
    pub commit: String,
    pub dirty: bool,
}

/// The evidence record of one `qualyx test` run. Written once as `manifest.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunManifest {
    pub schema_version: u32,
    pub run_id: String,
    pub qualyx_version: String,
    pub project_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
    pub os: String,
    pub arch: String,
    #[serde(default)]
    pub isolation: Isolation,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub status: Status,
    pub counts: Counts,
    pub runners: Vec<RunnerResult>,
    /// SHA-256 of every runner artifact, keyed by its path in the run dir. Lets a reader
    /// detect evidence edited after the run (threat model T5).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub artifact_sha256: BTreeMap<String, String>,
}

impl RunManifest {
    /// Process exit code for `qualyx test`: 0 green, 1 test failures, 2 anything unproven.
    pub fn exit_code(&self) -> i32 {
        match self.status {
            Status::Passed | Status::Skipped => 0,
            Status::Failed => 1,
            Status::Error | Status::Inconclusive => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_status_wins() {
        assert_eq!(
            Status::worst([Status::Passed, Status::Failed, Status::Skipped]),
            Status::Failed
        );
        assert_eq!(
            Status::worst([Status::Failed, Status::Error]),
            Status::Error
        );
        assert_eq!(Status::worst([]), Status::Inconclusive);
    }

    #[test]
    fn status_round_trips_through_str() {
        for s in [
            Status::Passed,
            Status::Failed,
            Status::Skipped,
            Status::Error,
            Status::Inconclusive,
        ] {
            assert_eq!(Status::parse(s.as_str()), Some(s));
        }
    }

    #[test]
    fn counts_classify_every_status() {
        let t = |status| TestCase {
            name: "t".into(),
            suite: None,
            status,
            duration_ms: None,
            message: None,
        };
        let c = Counts::from_tests(&[
            t(Status::Passed),
            t(Status::Failed),
            t(Status::Skipped),
            t(Status::Error),
        ]);
        assert_eq!(
            c,
            Counts {
                total: 4,
                passed: 1,
                failed: 1,
                skipped: 1,
                errors: 1
            }
        );
    }
}
