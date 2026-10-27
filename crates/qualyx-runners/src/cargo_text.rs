//! Parser for `cargo test` human output (stdout and stderr interleaved in one log).
//!
//! Stable cargo has no machine-readable test report, so this reads the libtest text format:
//! `Running <target> (...)` / `Doc-tests <crate>` headers, `test <name> ... <outcome>` lines and
//! `---- <name> stdout ----` failure blocks.

use qualyx_core::{Status, TestCase};

use crate::report::truncate;

fn suite_from_running(rest: &str) -> String {
    let target = rest.split(" (").next().unwrap_or(rest).trim();
    let target = target.strip_prefix("unittests ").unwrap_or(target);
    target.replace('\\', "/")
}

fn parse_test_line(line: &str) -> Option<(String, Status, Option<String>)> {
    let rest = line.strip_prefix("test ")?;
    // Doc-test names contain spaces, so split on the last separator.
    let (name, outcome) = rest.rsplit_once(" ... ")?;
    let (status, reason) = match outcome.trim() {
        "ok" => (Status::Passed, None),
        "FAILED" => (Status::Failed, None),
        o if o.starts_with("ignored") => (
            Status::Skipped,
            o.strip_prefix("ignored, ").map(str::to_string),
        ),
        // `bench`, `ok (should panic)` variants and unknown outcomes are not tests we can grade.
        o if o.starts_with("ok") => (Status::Passed, None),
        _ => return None,
    };
    Some((name.to_string(), status, reason))
}

pub fn parse(log: &str) -> Vec<TestCase> {
    let mut tests: Vec<TestCase> = Vec::new();
    let mut suite: Option<String> = None;
    // (index into `tests`, captured lines) while inside a `---- name stdout ----` block.
    let mut block: Option<(Option<usize>, Vec<&str>)> = None;

    let flush = |tests: &mut Vec<TestCase>, block: &mut Option<(Option<usize>, Vec<&str>)>| {
        if let Some((Some(i), lines)) = block.take() {
            let msg = truncate(&lines.join("\n"));
            if !msg.is_empty() {
                tests[i].message = Some(msg);
            }
        }
    };

    for raw in log.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("Running ") {
            flush(&mut tests, &mut block);
            suite = Some(suite_from_running(rest));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Doc-tests ") {
            flush(&mut tests, &mut block);
            suite = Some(format!("doc-tests {}", rest.trim()));
            continue;
        }
        if let Some(name) = line
            .strip_prefix("---- ")
            .and_then(|l| l.strip_suffix(" stdout ----"))
        {
            flush(&mut tests, &mut block);
            let idx = tests
                .iter()
                .rposition(|t| t.name == name && t.suite == suite && t.status == Status::Failed);
            block = Some((idx, Vec::new()));
            continue;
        }
        if block.is_some() && (line == "failures:" || line.starts_with("test result:")) {
            flush(&mut tests, &mut block);
            continue;
        }
        if let Some((_, lines)) = block.as_mut() {
            lines.push(line);
            continue;
        }
        if let Some((name, status, reason)) = parse_test_line(line) {
            tests.push(TestCase {
                name,
                suite: suite.clone(),
                status,
                duration_ms: None,
                message: reason,
            });
        }
    }
    flush(&mut tests, &mut block);
    tests
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real `cargo test --no-fail-fast --color never` output, stdout and stderr combined.
    const FAILING: &str = r#"   Compiling fixture-rust-failing v0.0.0 (C:\tmp\rust\failing)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.40s
     Running unittests src\lib.rs (target\debug\deps\fixture_rust_failing-a709b6d4ea6d3a99.exe)

running 2 tests
test tests::adds_two_numbers ... ok
test tests::multiplies_two_numbers ... FAILED

failures:

---- tests::multiplies_two_numbers stdout ----

thread 'tests::multiplies_two_numbers' (126424) panicked at src\lib.rs:21:9:
assertion `left == right` failed
  left: -1
 right: 6
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    tests::multiplies_two_numbers

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: test failed, to rerun pass `--lib`
   Doc-tests fixture_rust_failing

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

error: 1 target failed:
    `--lib`
"#;

    #[test]
    fn parses_failures_with_messages() {
        let t = parse(FAILING);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].name, "tests::adds_two_numbers");
        assert_eq!(t[0].suite.as_deref(), Some("src/lib.rs"));
        assert_eq!(t[0].status, Status::Passed);
        assert_eq!(t[1].status, Status::Failed);
        let msg = t[1].message.as_deref().unwrap();
        assert!(msg.contains("left: -1"), "{msg}");
        assert!(msg.contains("right: 6"), "{msg}");
    }

    #[test]
    fn parses_ignored_with_reason_and_doc_tests() {
        let log = "     Running unittests src/lib.rs (target/debug/deps/x)\n\
                   test tests::skipped ... ignored, seeded skip\n\
                   test tests::plain_ignored ... ignored\n\
                   \x20  Doc-tests x\n\
                   test src/lib.rs - add (line 3) ... ok\n";
        let t = parse(log);
        assert_eq!(t[0].status, Status::Skipped);
        assert_eq!(t[0].message.as_deref(), Some("seeded skip"));
        assert_eq!(t[1].message, None);
        assert_eq!(t[2].name, "src/lib.rs - add (line 3)");
        assert_eq!(t[2].suite.as_deref(), Some("doc-tests x"));
    }

    #[test]
    fn same_test_name_in_two_targets_is_kept_apart() {
        let log = "     Running unittests src/lib.rs (a)\n\
                   test it ... ok\n\
                   \x20    Running tests/it.rs (b)\n\
                   test it ... FAILED\n\
                   ---- it stdout ----\n\
                   boom\n\
                   failures:\n";
        let t = parse(log);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].message, None);
        assert_eq!(t[1].suite.as_deref(), Some("tests/it.rs"));
        assert_eq!(t[1].message.as_deref(), Some("boom"));
    }

    #[test]
    fn compile_error_yields_no_tests() {
        let log = "error[E0425]: cannot find value `x` in this scope\nerror: could not compile `y`";
        assert!(parse(log).is_empty());
    }
}
