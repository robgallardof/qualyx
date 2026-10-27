//! Deterministic failure triage (the "Failure triage" agent role without an LLM):
//! group failures by a normalized signature, classify them, pull out source locations and
//! build the exact command that re-runs each failing test on its own.

use std::path::Path;
use std::sync::OnceLock;

use qualyx_core::{Ecosystem, RunManifest, RunnerResult, Status, TestCase, TestTool};
use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Assertion,
    Panic,
    Exception,
    Timeout,
    Cancelled,
    BuildOrCrash,
    MissingTool,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Repro {
    pub test: String,
    /// Program and arguments, run from the project root. No shell needed.
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FailureGroup {
    pub ecosystem: Ecosystem,
    pub category: Category,
    pub signature: String,
    pub tests: Vec<String>,
    pub locations: Vec<String>,
    pub sample_message: String,
    pub reproduce: Vec<Repro>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Triage {
    pub run_id: String,
    pub status: Status,
    pub groups: Vec<FailureGroup>,
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid triage regex"))
}

/// Collapse volatile details so the same bug in many tests shares one signature.
pub fn signature(message: &str) -> String {
    static NUM: OnceLock<Regex> = OnceLock::new();
    static PATH: OnceLock<Regex> = OnceLock::new();
    static THREAD: OnceLock<Regex> = OnceLock::new();
    let line = message
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("thread '") && !l.starts_with("note:"))
        .unwrap_or("");
    let s = re(&THREAD, r"\(\d+\)").replace_all(line, "");
    let s = re(&PATH, r"(?:[A-Za-z]:)?[\\/]?(?:[\w.\-]+[\\/])+[\w.\-]+").replace_all(&s, "<path>");
    let s = re(&NUM, r"-?\b\d+(?:\.\d+)?\b").replace_all(&s, "N");
    s.chars().take(160).collect()
}

pub fn locations(text: &str) -> Vec<String> {
    static LOC: OnceLock<Regex> = OnceLock::new();
    let r = re(
        &LOC,
        r#"(?:File "(?P<py>[^"]+)", line (?P<pyl>\d+))|(?:in (?P<cs>(?:[A-Za-z]:)?[^\s:]+\.(?:cs|fs|vb)):line (?P<csl>\d+))|(?P<f>[\w./\\\-]+\.(?:rs|py|js|mjs|cjs|ts|tsx|cs|fs|go|java)):(?P<l>\d+)"#,
    );
    let mut out: Vec<String> = Vec::new();
    for c in r.captures_iter(text) {
        let loc = if let (Some(f), Some(l)) = (c.name("py"), c.name("pyl")) {
            format!("{}:{}", f.as_str(), l.as_str())
        } else if let (Some(f), Some(l)) = (c.name("cs"), c.name("csl")) {
            format!("{}:{}", f.as_str(), l.as_str())
        } else {
            format!("{}:{}", &c["f"], &c["l"])
        };
        let loc = loc.replace('\\', "/");
        // Skip runtime internals; keep the project's own frames.
        if !loc.starts_with("node:")
            && !loc.contains("/rustc/")
            && !loc.contains("site-packages")
            && !out.contains(&loc)
        {
            out.push(loc);
        }
    }
    out.truncate(5);
    out
}

fn categorize(msg: &str) -> Category {
    let m = msg.to_ascii_lowercase();
    if m.contains("assert")
        || m.contains("expected")
            && (m.contains("actual") || m.contains("received") || m.contains("!=="))
    {
        Category::Assertion
    } else if m.contains("panicked at") {
        Category::Panic
    } else if m.contains("exception") || m.contains("error:") || m.contains("traceback") {
        Category::Exception
    } else {
        Category::Unknown
    }
}

fn runner_category(r: &RunnerResult) -> Category {
    let notes = r.notes.join("\n").to_ascii_lowercase();
    if r.cancelled {
        Category::Cancelled
    } else if r.timed_out {
        Category::Timeout
    } else if notes.contains("not found on path") {
        Category::MissingTool
    } else {
        Category::BuildOrCrash
    }
}

fn regex_escape(s: &str) -> String {
    regex::escape(s)
}

/// Command that re-runs exactly one test, or `None` when Qualyx can't name it precisely.
pub fn reproduce(root: &Path, r: &RunnerResult, t: &TestCase) -> Option<Vec<String>> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match r.tool {
        TestTool::CargoTest => {
            let suite = t.suite.as_deref().unwrap_or("src/lib.rs");
            let mut cmd = v(&["cargo", "test"]);
            // Keep the package selection the run used, if any.
            let pkgs: Vec<&String> = r
                .command
                .windows(2)
                .filter(|w| w[0] == "-p")
                .map(|w| &w[1])
                .collect();
            if pkgs.len() == 1 {
                cmd.extend(["-p".to_string(), pkgs[0].clone()]);
            }
            if suite.starts_with("doc-tests") {
                cmd.push("--doc".into());
            } else if let Some(name) = suite
                .strip_prefix("tests/")
                .and_then(|s| s.strip_suffix(".rs"))
            {
                cmd.extend(["--test".to_string(), name.to_string()]);
            } else if suite.ends_with("main.rs") || suite.starts_with("src/bin/") {
                cmd.push("--bins".into());
            } else {
                cmd.push("--lib".into());
            }
            cmd.extend(["--".to_string(), t.name.clone(), "--exact".to_string()]);
            Some(cmd)
        }
        TestTool::Pytest => {
            // classname "pkg.mod" or "pkg.mod.Class": find the longest prefix that is a file.
            let parts: Vec<&str> = t.suite.as_deref()?.split('.').collect();
            (1..=parts.len()).rev().find_map(|n| {
                let file = format!("{}.py", parts[..n].join("/"));
                root.join(&file).is_file().then(|| {
                    let mut node = file;
                    for c in &parts[n..] {
                        node.push_str("::");
                        node.push_str(c);
                    }
                    node.push_str("::");
                    node.push_str(&t.name);
                    v(&["python", "-m", "pytest", &node])
                })
            })
        }
        TestTool::NodeTest => Some(vec![
            "node".into(),
            "--test".into(),
            format!("--test-name-pattern=^{}$", regex_escape(&t.name)),
        ]),
        TestTool::DotnetTest => {
            let mut cmd = v(&["dotnet", "test"]);
            if let Some(target) = r
                .command
                .iter()
                .skip_while(|a| *a != "test")
                .nth(1)
                .filter(|a| !a.starts_with('-'))
            {
                cmd.push(target.clone());
            }
            cmd.extend([
                "--filter".to_string(),
                format!("FullyQualifiedName={}", t.name),
            ]);
            Some(cmd)
        }
        TestTool::NpmScript => None,
    }
}

pub fn triage(m: &RunManifest) -> Triage {
    let mut groups: Vec<FailureGroup> = Vec::new();
    for r in &m.runners {
        let failing: Vec<&TestCase> = r
            .tests
            .iter()
            .filter(|t| matches!(t.status, Status::Failed | Status::Error))
            .collect();
        if failing.is_empty()
            && matches!(
                r.status,
                Status::Failed | Status::Error | Status::Inconclusive
            )
        {
            let text = r.notes.join("\n");
            groups.push(FailureGroup {
                ecosystem: r.ecosystem,
                category: if r.detail == qualyx_core::Detail::ExitCodeOnly
                    && r.status == Status::Failed
                {
                    Category::Unknown
                } else {
                    runner_category(r)
                },
                signature: signature(&text),
                tests: vec![],
                locations: locations(&text),
                sample_message: text.chars().take(800).collect(),
                reproduce: vec![Repro {
                    test: format!("{} suite", r.ecosystem.as_str()),
                    command: r.command.clone(),
                }],
            });
            continue;
        }
        for t in failing {
            let msg = t.message.as_deref().unwrap_or("");
            let sig = signature(msg);
            let repro = reproduce(&m.project_root, r, t).map(|command| Repro {
                test: t.name.clone(),
                command,
            });
            let locs = locations(msg);
            match groups
                .iter_mut()
                .find(|g| g.ecosystem == r.ecosystem && g.signature == sig && !g.tests.is_empty())
            {
                Some(g) => {
                    g.tests.push(t.name.clone());
                    g.reproduce.extend(repro);
                    for l in locs {
                        if !g.locations.contains(&l) {
                            g.locations.push(l);
                        }
                    }
                }
                None => groups.push(FailureGroup {
                    ecosystem: r.ecosystem,
                    category: categorize(msg),
                    signature: sig,
                    tests: vec![t.name.clone()],
                    locations: locs,
                    sample_message: msg.chars().take(800).collect(),
                    reproduce: repro.into_iter().collect(),
                }),
            }
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.tests.len()));
    Triage {
        run_id: m.run_id.clone(),
        status: m.status,
        groups,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_ignore_volatile_details() {
        let a = "thread 'x' (1234) panicked at src/lib.rs:21:9:\nassertion `left == right` failed\n  left: -1";
        let b = "thread 'y' (99) panicked at src/other.rs:3:1:\nassertion `left == right` failed\n  left: 7";
        assert_eq!(signature(a), signature(b));
        assert_eq!(signature("expected 6, got -1"), "expected N, got N");
    }

    #[test]
    fn finds_locations_in_every_format() {
        let t = "panicked at src\\lib.rs:21:9\n   at A.B() in C:\\p\\Tests.cs:line 9\n  File \"tests/test_calc.py\", line 14, in test_x\n   at Failing.Tests.M() in /src/CalculatorTests.cs:line 17\n    at file:///C:/p/test/calc.test.js:9:45\n    at node:internal/test_runner/test:1325:25";
        let l = locations(t);
        assert!(l.contains(&"src/lib.rs:21".to_string()), "{l:?}");
        assert!(l.contains(&"tests/test_calc.py:14".to_string()), "{l:?}");
        assert!(
            l.contains(&"/src/CalculatorTests.cs:17".to_string()),
            "{l:?}"
        );
        assert!(l.contains(&"C:/p/Tests.cs:9".to_string()), "{l:?}");
        assert!(
            l.iter().any(|x| x.ends_with("test/calc.test.js:9")),
            "{l:?}"
        );
        assert!(!l.iter().any(|x| x.starts_with("node:")), "{l:?}");
    }

    #[test]
    fn categories() {
        assert_eq!(categorize("assert -1 == 6"), Category::Assertion);
        assert_eq!(categorize("Expected: 6\nActual: -1"), Category::Assertion);
        assert_eq!(
            categorize("thread 'a' panicked at src/x.rs:1:1:\nindex out of bounds"),
            Category::Panic
        );
        assert_eq!(
            categorize("System.NullReferenceException: Object reference"),
            Category::Exception
        );
    }
}
