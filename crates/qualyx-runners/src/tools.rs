//! One adapter per native test tool (ADR 0005): `plan` is pure, `collect` parses evidence.

use std::fs;
use std::path::Path;

use qualyx_core::{Detail, DetectedEcosystem, Status, TestCase, TestTool};

use crate::{cargo_text, report};

/// What to execute. `program` candidates are tried in order on `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: &'static [&'static str],
    pub args: Vec<String>,
    /// Arguments appended to the resolved program to print its version.
    pub version_args: &'static [&'static str],
    pub detail: Detail,
}

/// Parsed evidence from one invocation. `artifacts` are file names inside the run dir.
#[derive(Debug, Default)]
pub struct Collected {
    pub tests: Vec<TestCase>,
    pub artifacts: Vec<String>,
    pub notes: Vec<String>,
}

/// Maps an artifact file name to the path the tool must write it to. On the host that is
/// inside the run dir; in a container it is under the mounted output directory.
pub type OutPath<'a> = &'a dyn Fn(&str) -> String;

pub trait Runner: Sync {
    fn plan(&self, detected: &DetectedEcosystem, out: OutPath<'_>) -> Invocation;
    fn collect(&self, log: &str, out_dir: &Path) -> Collected;
}

pub fn runner_for(tool: TestTool) -> &'static dyn Runner {
    match tool {
        TestTool::CargoTest => &CargoTest,
        TestTool::DotnetTest => &DotnetTest,
        TestTool::NodeTest => &NodeTest,
        TestTool::NpmScript => &NpmScript,
        TestTool::Pytest => &Pytest,
    }
}

/// Parse one JUnit file written by the tool into `Collected`.
fn collect_junit(out_dir: &Path, file: &str) -> Collected {
    let mut c = Collected::default();
    match fs::read_to_string(out_dir.join(file)) {
        Ok(xml) => {
            c.artifacts.push(file.to_string());
            match report::parse_junit(&xml) {
                Ok(tests) => c.tests = tests,
                Err(e) => c.notes.push(format!("{file} could not be parsed: {e}")),
            }
        }
        Err(_) => c.notes.push(format!("{file} was not produced")),
    }
    c
}

pub struct CargoTest;

impl Runner for CargoTest {
    fn plan(&self, detected: &DetectedEcosystem, _: OutPath<'_>) -> Invocation {
        let mut args: Vec<String> = ["test", "--no-fail-fast", "--color", "never"]
            .map(String::from)
            .to_vec();
        for p in &detected.packages {
            args.extend(["-p".to_string(), p.clone()]);
        }
        Invocation {
            program: &["cargo"],
            args,
            version_args: &["--version"],
            detail: Detail::PerTest,
        }
    }

    fn collect(&self, log: &str, _: &Path) -> Collected {
        Collected {
            tests: cargo_text::parse(log),
            ..Default::default()
        }
    }
}

pub struct DotnetTest;

const TRX_DIR: &str = "dotnet-trx";

impl Runner for DotnetTest {
    fn plan(&self, detected: &DetectedEcosystem, out: OutPath<'_>) -> Invocation {
        let mut args = vec!["test".to_string()];
        args.extend(detected.target.clone());
        args.extend([
            "--nologo".into(),
            "--results-directory".into(),
            out(TRX_DIR),
            "--logger".into(),
            "trx".into(),
        ]);
        Invocation {
            program: &["dotnet"],
            args,
            version_args: &["--version"],
            detail: Detail::PerTest,
        }
    }

    fn collect(&self, _: &str, out_dir: &Path) -> Collected {
        let mut c = Collected::default();
        let dir = out_dir.join(TRX_DIR);
        let mut files: Vec<_> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "trx"))
            .collect();
        files.sort();
        if files.is_empty() {
            c.notes.push("no TRX report was produced".into());
        }
        for f in files {
            let name = format!("{TRX_DIR}/{}", f.file_name().unwrap().to_string_lossy());
            match fs::read_to_string(&f)
                .map_err(|e| e.to_string())
                .and_then(|x| report::parse_trx(&x).map_err(|e| e.to_string()))
            {
                Ok(tests) => c.tests.extend(tests),
                Err(e) => c.notes.push(format!("{name} could not be parsed: {e}")),
            }
            c.artifacts.push(name);
        }
        c
    }
}

pub struct NodeTest;

const NODE_JUNIT: &str = "node.junit.xml";

impl Runner for NodeTest {
    fn plan(&self, _: &DetectedEcosystem, out: OutPath<'_>) -> Invocation {
        // Appended to the `node --test` script; the spec reporter keeps the log readable.
        Invocation {
            program: &["npm"],
            args: vec![
                "test".into(),
                "--".into(),
                "--test-reporter=spec".into(),
                "--test-reporter-destination=stdout".into(),
                "--test-reporter=junit".into(),
                format!("--test-reporter-destination={}", out(NODE_JUNIT)),
            ],
            version_args: &["--version"],
            detail: Detail::PerTest,
        }
    }

    fn collect(&self, _: &str, out_dir: &Path) -> Collected {
        collect_junit(out_dir, NODE_JUNIT)
    }
}

pub struct NpmScript;

impl Runner for NpmScript {
    fn plan(&self, _: &DetectedEcosystem, _: OutPath<'_>) -> Invocation {
        Invocation {
            program: &["npm"],
            args: vec!["test".into()],
            version_args: &["--version"],
            detail: Detail::ExitCodeOnly,
        }
    }

    fn collect(&self, _: &str, _: &Path) -> Collected {
        Collected {
            notes: vec![
                "no verified adapter for this runner; only the exit code is evidence".into(),
            ],
            ..Default::default()
        }
    }
}

pub struct Pytest;

const PYTEST_JUNIT: &str = "pytest.junit.xml";

impl Runner for Pytest {
    fn plan(&self, _: &DetectedEcosystem, out: OutPath<'_>) -> Invocation {
        Invocation {
            program: &["python", "python3"],
            args: vec![
                "-m".into(),
                "pytest".into(),
                "-p".into(),
                "no:cacheprovider".into(),
                format!("--junitxml={}", out(PYTEST_JUNIT)),
            ],
            version_args: &["-m", "pytest", "--version"],
            detail: Detail::PerTest,
        }
    }

    fn collect(&self, _: &str, out_dir: &Path) -> Collected {
        collect_junit(out_dir, PYTEST_JUNIT)
    }
}

/// Decide a runner's status from its evidence. Never green without proof.
pub fn grade(
    detail: Detail,
    tests: &[TestCase],
    exit_code: Option<i32>,
    timed_out: bool,
    notes: &mut Vec<String>,
) -> Status {
    if timed_out {
        notes.push("timed out and was killed".into());
        return Status::Error;
    }
    let ok = exit_code == Some(0);
    if detail == Detail::ExitCodeOnly {
        return if ok { Status::Passed } else { Status::Failed };
    }
    if tests.is_empty() {
        if ok {
            notes.push("the tool succeeded but reported no tests".into());
            return Status::Inconclusive;
        }
        notes.push(
            "the tool failed before reporting any test (build error or crash); see the log".into(),
        );
        return Status::Error;
    }
    match Status::worst(tests.iter().map(|t| t.status)) {
        Status::Skipped => {
            notes.push("every test was skipped".into());
            Status::Inconclusive
        }
        Status::Passed if !ok => {
            notes.push(format!(
                "exit code {exit_code:?} but no failing test was reported; see the log"
            ));
            Status::Error
        }
        s => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qualyx_core::Ecosystem;

    fn det(tool: TestTool, target: Option<&str>) -> DetectedEcosystem {
        DetectedEcosystem {
            ecosystem: Ecosystem::Rust,
            tool,
            manifest: "x".into(),
            target: target.map(String::from),
            notes: vec![],
            packages: vec![],
        }
    }

    fn tc(status: Status) -> TestCase {
        TestCase {
            name: "t".into(),
            suite: None,
            status,
            duration_ms: None,
            message: None,
        }
    }

    #[test]
    fn dotnet_passes_target_and_trx_logger() {
        let inv = DotnetTest.plan(&det(TestTool::DotnetTest, Some("A.sln")), &|f| {
            format!("out/{f}")
        });
        assert_eq!(inv.args[0..2], ["test", "A.sln"]);
        assert!(inv.args.windows(2).any(|w| w == ["--logger", "trx"]));
    }

    #[test]
    fn pytest_writes_junit_into_run_dir() {
        let inv = Pytest.plan(&det(TestTool::Pytest, None), &|f| format!("out/{f}"));
        assert!(
            inv.args
                .iter()
                .any(|a| a.starts_with("--junitxml=") && a.ends_with("pytest.junit.xml"))
        );
    }

    #[test]
    fn missing_report_is_noted() {
        let dir = tempfile::tempdir().unwrap();
        let c = NodeTest.collect("", dir.path());
        assert!(c.tests.is_empty());
        assert_eq!(c.notes, ["node.junit.xml was not produced"]);
    }

    #[test]
    fn grading_rules() {
        let mut n = vec![];
        assert_eq!(
            grade(
                Detail::PerTest,
                &[tc(Status::Passed)],
                Some(0),
                false,
                &mut n
            ),
            Status::Passed
        );
        assert_eq!(
            grade(
                Detail::PerTest,
                &[tc(Status::Passed), tc(Status::Failed)],
                Some(1),
                false,
                &mut n
            ),
            Status::Failed
        );
        // Passing tests but a non-zero exit: something else broke.
        assert_eq!(
            grade(
                Detail::PerTest,
                &[tc(Status::Passed)],
                Some(1),
                false,
                &mut n
            ),
            Status::Error
        );
        // Nothing ran.
        assert_eq!(
            grade(Detail::PerTest, &[], Some(0), false, &mut n),
            Status::Inconclusive
        );
        assert_eq!(
            grade(Detail::PerTest, &[], Some(101), false, &mut n),
            Status::Error
        );
        assert_eq!(
            grade(
                Detail::PerTest,
                &[tc(Status::Skipped)],
                Some(0),
                false,
                &mut n
            ),
            Status::Inconclusive
        );
        assert_eq!(
            grade(
                Detail::PerTest,
                &[tc(Status::Passed)],
                Some(0),
                true,
                &mut n
            ),
            Status::Error
        );
        assert_eq!(
            grade(Detail::ExitCodeOnly, &[], Some(0), false, &mut n),
            Status::Passed
        );
        assert_eq!(
            grade(Detail::ExitCodeOnly, &[], Some(1), false, &mut n),
            Status::Failed
        );
    }
}
