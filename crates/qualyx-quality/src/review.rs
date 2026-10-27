//! Independent reviewer: reads the diff of test files and flags changes that weaken tests
//! (master prompt: "never weaken assertions"). Deterministic; it reports, it never edits.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Fewer assertion lines than before in a test file.
    AssertionsRemoved,
    /// A skip/ignore marker was added.
    SkipAdded,
    /// A test definition disappeared.
    TestRemoved,
    /// An assertion line was replaced by a different one (expected value may have changed).
    AssertionChanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewFinding {
    pub kind: Kind,
    pub file: String,
    pub detail: String,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Review {
    pub base: String,
    pub test_files: usize,
    pub findings: Vec<ReviewFinding>,
}

fn re(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).expect("valid review regex"))
}

fn is_assertion(l: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"\bassert(_eq|_ne|_matches)?!|^\s*assert\b|self\.assert\w+\(|\bexpect\(|\bassert\.\w+\(|\bAssert\.\w+\(|\.should\b|\bassertThat\(").is_match(l)
}

fn is_skip(l: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"#\[ignore|@pytest\.mark\.(skip|xfail)|@unittest\.skip|pytest\.skip\(|\b(test|it|describe)\.skip\(|\bx(it|describe)\(|\.todo\(|Skip\s*=|\[Ignore|@Disabled").is_match(l)
}

fn is_test_def(l: &str) -> bool {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"#\[test\]|#\[tokio::test|^\s*(async\s+)?def\s+test_|^\s*(test|it)\(\s*['`]|\[(Fact|Theory|Test|TestMethod)\]|@Test\b").is_match(l)
}

pub fn is_test_file(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);
    p.contains("/tests/")
        || p.starts_with("tests/")
        || p.contains("/test/")
        || p.starts_with("test/")
        || p.contains("__tests__")
        || name.starts_with("test_")
        || name.ends_with("_test.py")
        || name.ends_with("_test.go")
        || name.ends_with("_tests.rs")
        || name.contains(".test.")
        || name.contains(".spec.")
        || name.ends_with("tests.cs")
        || name.ends_with("test.cs")
        || name.ends_with("test.java")
}

/// Review a unified diff (as printed by `git diff -U0`).
pub fn review_diff(diff: &str, base: &str) -> Review {
    let mut findings = Vec::new();
    let mut files = 0;
    // Split into per-file chunks.
    let mut chunks: Vec<(String, Vec<&str>)> = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.split(" b/").nth(1).unwrap_or(rest).to_string();
            chunks.push((path, Vec::new()));
        } else if let Some((_, lines)) = chunks.last_mut() {
            lines.push(line);
        }
    }
    for (file, lines) in chunks {
        if !is_test_file(&file) {
            continue;
        }
        files += 1;
        let removed: Vec<&str> = lines
            .iter()
            .filter(|l| l.starts_with('-') && !l.starts_with("---"))
            .map(|l| &l[1..])
            .collect();
        let added: Vec<&str> = lines
            .iter()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .map(|l| &l[1..])
            .collect();
        let pick = |v: &[&str], f: fn(&str) -> bool| {
            v.iter()
                .filter(|l| f(l))
                .map(|l| l.trim().to_string())
                .collect::<Vec<_>>()
        };

        let (ra, aa) = (pick(&removed, is_assertion), pick(&added, is_assertion));
        if ra.len() > aa.len() {
            findings.push(ReviewFinding {
                kind: Kind::AssertionsRemoved,
                file: file.clone(),
                detail: format!("{} assertion line(s) removed, {} added", ra.len(), aa.len()),
                removed: ra.clone(),
                added: aa.clone(),
            });
        } else if !ra.is_empty() && ra.len() == aa.len() && ra != aa {
            findings.push(ReviewFinding { kind: Kind::AssertionChanged, file: file.clone(), detail: "assertions rewritten; check the expected values still describe correct behaviour".into(), removed: ra.clone(), added: aa.clone() });
        }
        let skips = pick(&added, is_skip);
        let unskips = pick(&removed, is_skip);
        if skips.len() > unskips.len() {
            findings.push(ReviewFinding {
                kind: Kind::SkipAdded,
                file: file.clone(),
                detail: format!(
                    "{} skip/ignore marker(s) added",
                    skips.len() - unskips.len()
                ),
                removed: unskips,
                added: skips,
            });
        }
        let (rt, at) = (pick(&removed, is_test_def), pick(&added, is_test_def));
        if rt.len() > at.len() {
            findings.push(ReviewFinding {
                kind: Kind::TestRemoved,
                file: file.clone(),
                detail: format!(
                    "{} test definition(s) removed, {} added",
                    rt.len(),
                    at.len()
                ),
                removed: rt,
                added: at,
            });
        }
    }
    Review {
        base: base.into(),
        test_files: files,
        findings,
    }
}

/// Review `base...HEAD` plus uncommitted changes of the repository at `root`.
pub fn review_git(root: &Path, base: &str) -> Result<Review, String> {
    if base.starts_with('-') {
        return Err(format!("invalid base `{base}`"));
    }
    let run = |args: &[&str]| -> Result<String, String> {
        let o = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .map_err(|e| e.to_string())?;
        if o.status.success() {
            Ok(String::from_utf8_lossy(&o.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
        }
    };
    run(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{base}^{{commit}}"),
    ])
    .map_err(|_| format!("unknown git revision `{base}`"))?;
    let committed = run(&[
        "diff",
        "-U0",
        "--no-color",
        "--no-ext-diff",
        &format!("{base}...HEAD"),
    ])?;
    let working = run(&["diff", "-U0", "--no-color", "--no-ext-diff", "HEAD"])?;
    let mut r = review_diff(&format!("{committed}\n{working}"), base);
    r.findings.dedup_by(|a, b| {
        a.file == b.file && a.kind == b.kind && a.removed == b.removed && a.added == b.added
    });
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEAKENING: &str = "diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -3 +3 @@
-    age >= 18
+    age > 18
diff --git a/tests/limits.rs b/tests/limits.rs
--- a/tests/limits.rs
+++ b/tests/limits.rs
@@ -4,2 +4 @@
-    assert!(!is_adult(17));
-    assert!(is_adult(18));
+    assert!(is_adult(30));
@@ -9,0 +9 @@
+#[ignore]
diff --git a/tests/test_api.py b/tests/test_api.py
--- a/tests/test_api.py
+++ b/tests/test_api.py
@@ -10,3 +9,0 @@
-def test_rejects_negative():
-    with pytest.raises(ValueError):
-        parse(-1)
";

    #[test]
    fn flags_every_kind_of_weakening() {
        let r = review_diff(WEAKENING, "main");
        let kinds: Vec<(Kind, &str)> = r
            .findings
            .iter()
            .map(|f| (f.kind, f.file.as_str()))
            .collect();
        assert!(
            kinds.contains(&(Kind::AssertionsRemoved, "tests/limits.rs")),
            "{kinds:?}"
        );
        assert!(
            kinds.contains(&(Kind::SkipAdded, "tests/limits.rs")),
            "{kinds:?}"
        );
        assert!(
            kinds.contains(&(Kind::TestRemoved, "tests/test_api.py")),
            "{kinds:?}"
        );
        assert_eq!(r.test_files, 2, "src/lib.rs is not a test file");
    }

    #[test]
    fn adding_tests_is_not_a_finding() {
        let d = "diff --git a/tests/x.rs b/tests/x.rs\n+++ b/tests/x.rs\n@@ -0,0 +1,4 @@\n+#[test]\n+fn new_case() {\n+    assert_eq!(f(2), 4);\n+}\n";
        assert!(review_diff(d, "main").findings.is_empty());
    }

    #[test]
    fn changed_expected_value_is_flagged_for_review() {
        let d = "diff --git a/test/calc.test.js b/test/calc.test.js\n@@ -5 +5 @@\n-  assert.equal(multiply(2, 3), 6);\n+  assert.equal(multiply(2, 3), -1);\n";
        let r = review_diff(d, "main");
        assert_eq!(r.findings[0].kind, Kind::AssertionChanged);
    }

    #[test]
    fn recognises_test_files() {
        for p in [
            "tests/a.rs",
            "src/foo_tests.rs",
            "app/test_x.py",
            "web/a.spec.ts",
            "Calc/CalculatorTests.cs",
            "pkg/__tests__/a.js",
        ] {
            assert!(is_test_file(p), "{p}");
        }
        for p in ["src/lib.rs", "app/models.py", "README.md"] {
            assert!(!is_test_file(p), "{p}");
        }
    }
}
