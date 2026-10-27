//! Flaky tests: the same test, on the same code, both passing and failing.
//!
//! "Same code" means runs that share a git commit with a clean working tree, or runs of one
//! `qualyx test --repeat` invocation (same code by construction).

use std::collections::BTreeMap;

use qualyx_core::{RunManifest, Status};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FlakyTest {
    pub ecosystem: String,
    pub suite: Option<String>,
    pub name: String,
    pub passed: u32,
    pub failed: u32,
    /// Commit (or "repeat") the conflicting runs share.
    pub code: String,
    pub runs: Vec<String>,
}

/// `same_code` = true when the manifests are known to be the same code (a repeat batch).
pub fn detect(manifests: &[RunManifest], same_code: bool) -> Vec<FlakyTest> {
    type Key = (String, String, Option<String>, String);
    let mut seen: BTreeMap<Key, (u32, u32, Vec<String>)> = BTreeMap::new();
    for m in manifests {
        let code = if same_code {
            "repeat".to_string()
        } else {
            match &m.git {
                Some(g) if !g.dirty => g.commit.clone(),
                _ => continue, // uncommitted or non-git runs can't be compared safely
            }
        };
        for r in &m.runners {
            for t in &r.tests {
                let e = seen
                    .entry((
                        code.clone(),
                        r.ecosystem.as_str().to_string(),
                        t.suite.clone(),
                        t.name.clone(),
                    ))
                    .or_default();
                match t.status {
                    Status::Passed => e.0 += 1,
                    Status::Failed | Status::Error => e.1 += 1,
                    _ => continue,
                }
                if !e.2.contains(&m.run_id) {
                    e.2.push(m.run_id.clone());
                }
            }
        }
    }
    seen.into_iter()
        .filter(|(_, (p, f, _))| *p > 0 && *f > 0)
        .map(
            |((code, ecosystem, suite, name), (passed, failed, runs))| FlakyTest {
                ecosystem,
                suite,
                name,
                passed,
                failed,
                code,
                runs,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use qualyx_core::*;

    fn manifest(id: &str, commit: Option<(&str, bool)>, results: &[(&str, Status)]) -> RunManifest {
        let tests: Vec<TestCase> = results
            .iter()
            .map(|(n, s)| TestCase {
                name: n.to_string(),
                suite: None,
                status: *s,
                duration_ms: None,
                message: None,
            })
            .collect();
        let now = chrono::Utc::now();
        RunManifest {
            schema_version: 1,
            run_id: id.into(),
            qualyx_version: "t".into(),
            project_root: ".".into(),
            git: commit.map(|(c, d)| GitInfo {
                commit: c.into(),
                dirty: d,
            }),
            os: "x".into(),
            arch: "x".into(),
            isolation: Isolation::Process,
            selection: None,
            services: vec![],
            started_at: now,
            finished_at: now,
            status: Status::Passed,
            counts: Counts::from_tests(&tests),
            runners: vec![RunnerResult {
                ecosystem: Ecosystem::Python,
                tool: TestTool::Pytest,
                tool_version: None,
                command: vec![],
                exit_code: Some(0),
                timed_out: false,
                cancelled: false,
                started_at: now,
                finished_at: now,
                duration_ms: 0,
                status: Status::Passed,
                detail: Detail::PerTest,
                counts: Counts::from_tests(&tests),
                tests,
                artifacts: vec![],
                notes: vec![],
            }],
            artifact_sha256: Default::default(),
        }
    }

    #[test]
    fn mixed_results_on_the_same_commit_are_flaky() {
        let ms = [
            manifest(
                "a",
                Some(("c1", false)),
                &[("t1", Status::Passed), ("t2", Status::Passed)],
            ),
            manifest(
                "b",
                Some(("c1", false)),
                &[("t1", Status::Failed), ("t2", Status::Passed)],
            ),
        ];
        let f = detect(&ms, false);
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].name.as_str(), f[0].passed, f[0].failed), ("t1", 1, 1));
    }

    #[test]
    fn different_commits_or_dirty_trees_are_not_compared() {
        let ms = [
            manifest("a", Some(("c1", false)), &[("t1", Status::Passed)]),
            manifest("b", Some(("c2", false)), &[("t1", Status::Failed)]),
            manifest("c", Some(("c1", true)), &[("t1", Status::Failed)]),
            manifest("d", None, &[("t1", Status::Failed)]),
        ];
        assert!(detect(&ms, false).is_empty());
        // In a repeat batch the same manifests are comparable.
        assert_eq!(detect(&ms, true).len(), 1);
    }
}
