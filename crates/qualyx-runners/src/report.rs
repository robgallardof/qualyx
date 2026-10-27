//! Parsers for structured test reports: JUnit XML (pytest, node:test, many others) and TRX
//! (`dotnet test`). Both produce normalized [`TestCase`]s and never invent tests.

use qualyx_core::{Status, TestCase};
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

/// Longest failure/skip message kept per test; the full text stays in the native report.
pub const MAX_MESSAGE_CHARS: usize = 2000;

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

fn err(e: impl std::fmt::Display) -> ParseError {
    ParseError(e.to_string())
}

pub fn truncate(s: &str) -> String {
    let s = s.trim();
    match s.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

fn attr(e: &BytesStart<'_>, name: &str) -> Result<Option<String>, ParseError> {
    match e.try_get_attribute(name).map_err(err)? {
        Some(a) => Ok(Some(
            a.normalized_value(XmlVersion::Implicit1_0)
                .map_err(err)?
                .into_owned(),
        )),
        None => Ok(None),
    }
}

/// Resolve `&#10;`, `&lt;` and friends, which quick-xml reports as separate events.
fn push_ref(out: &mut String, r: &BytesRef<'_>) -> Result<(), ParseError> {
    if let Some(c) = r.resolve_char_ref().map_err(err)? {
        out.push(c);
        return Ok(());
    }
    let name = r.xml10_content();
    out.push_str(match name.as_ref() {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "quot" => "\"",
        "apos" => "'",
        other => return Err(ParseError(format!("unknown entity &{other};"))),
    });
    Ok(())
}

fn reader(xml: &str) -> Reader<&[u8]> {
    // TRX files start with a UTF-8 BOM.
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    Reader::from_reader(xml.as_bytes())
}

fn seconds_to_ms(s: &str) -> Option<u64> {
    let secs: f64 = s.trim().parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| (secs * 1000.0).round() as u64)
}

/// Parse every `<testcase>` in a JUnit document, at any nesting depth.
pub fn parse_junit(xml: &str) -> Result<Vec<TestCase>, ParseError> {
    let mut r = reader(xml);
    let mut tests = Vec::new();
    let mut current: Option<TestCase> = None;
    // Text inside <failure>/<error>/<skipped>: usually the traceback with file:line.
    let mut body: Option<String> = None;

    loop {
        let event = r.read_event().map_err(err)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                match e.name().as_ref() {
                    "testcase" => {
                        let tc = TestCase {
                            name: attr(e, "name")?.unwrap_or_default(),
                            suite: attr(e, "classname")?.filter(|s| !s.is_empty()),
                            status: Status::Passed,
                            duration_ms: attr(e, "time")?.as_deref().and_then(seconds_to_ms),
                            message: None,
                        };
                        if empty {
                            tests.push(tc);
                        } else {
                            current = Some(tc);
                        }
                    }
                    tag @ ("failure" | "error" | "skipped") => {
                        if let Some(tc) = current.as_mut() {
                            tc.status = match tag {
                                "failure" => Status::Failed,
                                "error" => Status::Error,
                                _ => Status::Skipped,
                            };
                            tc.message = attr(e, "message")?.map(|m| truncate(&m));
                            if !empty {
                                body = Some(String::new());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(t) => {
                if let Some(b) = body.as_mut() {
                    b.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) => {
                if let Some(b) = body.as_mut() {
                    push_ref(b, &r)?;
                }
            }
            // Playwright and many other reporters wrap the failure detail in CDATA.
            Event::CData(c) => {
                if let Some(b) = body.as_mut() {
                    b.push_str(&c.into_inner());
                }
            }
            Event::End(e) => match e.name().as_ref() {
                "failure" | "error" | "skipped" => {
                    if let (Some(tc), Some(b)) = (current.as_mut(), body.take()) {
                        tc.message = merge(tc.message.take(), &b);
                    }
                }
                "testcase" => tests.extend(current.take()),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(tests)
}

/// The summary message first, then the detail (traceback) when it adds something.
fn merge(message: Option<String>, detail: &str) -> Option<String> {
    let detail = detail.trim().replace("\r\n", "\n").replace('\r', "");
    match message {
        Some(m) if detail.is_empty() || m.contains(&detail) => Some(m),
        Some(m) => Some(truncate(&format!("{m}\n\n{detail}"))),
        None if detail.is_empty() => None,
        None => Some(truncate(&detail)),
    }
}

/// `hh:mm:ss.fffffff` as written by the TRX logger.
fn trx_duration_ms(s: &str) -> Option<u64> {
    let mut parts = s.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let sec: f64 = parts.next()?.parse().ok()?;
    Some(((h * 3600.0 + m * 60.0 + sec) * 1000.0).round() as u64)
}

fn trx_status(outcome: &str) -> Status {
    match outcome {
        "Passed" | "PassedButRunAborted" => Status::Passed,
        "Failed" => Status::Failed,
        "NotExecuted" | "NotRunnable" => Status::Skipped,
        "Error" | "Timeout" | "Aborted" => Status::Error,
        _ => Status::Inconclusive,
    }
}

/// Parse the `<UnitTestResult>` entries of a TRX document.
pub fn parse_trx(xml: &str) -> Result<Vec<TestCase>, ParseError> {
    let mut r = reader(xml);
    let mut tests = Vec::new();
    let mut current: Option<TestCase> = None;
    let mut message: Option<String> = None;
    let mut stack: Option<String> = None;

    loop {
        let event = r.read_event().map_err(err)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => match e.name().as_ref() {
                "UnitTestResult" => {
                    let tc = TestCase {
                        name: attr(e, "testName")?.unwrap_or_default(),
                        suite: None,
                        status: trx_status(attr(e, "outcome")?.as_deref().unwrap_or("")),
                        duration_ms: attr(e, "duration")?.as_deref().and_then(trx_duration_ms),
                        message: None,
                    };
                    if matches!(event, Event::Empty(_)) {
                        tests.push(tc);
                    } else {
                        current = Some(tc);
                    }
                }
                "Message" if current.is_some() => message = Some(String::new()),
                "StackTrace" if current.is_some() => stack = Some(String::new()),
                _ => {}
            },
            Event::Text(t) => {
                if let Some(m) = message.as_mut().or(stack.as_mut()) {
                    m.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) => {
                if let Some(m) = message.as_mut().or(stack.as_mut()) {
                    push_ref(m, &r)?;
                }
            }
            Event::End(e) => match e.name().as_ref() {
                "Message" => {
                    if let (Some(tc), Some(m)) = (current.as_mut(), message.take()) {
                        tc.message = Some(truncate(&m.replace("\r\n", "\n").replace('\r', "")));
                    }
                }
                "StackTrace" => {
                    if let (Some(tc), Some(st)) = (current.as_mut(), stack.take()) {
                        tc.message = merge(tc.message.take(), &st);
                    }
                }
                "UnitTestResult" => tests.extend(current.take()),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(tests)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from real node v24 `--test-reporter=junit` output.
    const NODE_JUNIT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<testsuites>
	<testcase name="adds two numbers" time="0.000617" classname="test" file="calc.test.js"/>
	<testcase name="multiplies two numbers" time="0.000653" classname="test" file="calc.test.js" failure="x">
		<failure type="testCodeFailure" message="Expected values to be strictly equal:&#10;&#10;-1 !== 6">
[Error [ERR_TEST_FAILURE]: Expected values to be strictly equal: -1 !== 6]
		</failure>
	</testcase>
	<testcase name="skipped on purpose" time="0.000078" classname="test">
		<skipped type="skipped" message="seeded skip"/>
	</testcase>
	<!-- tests 3 -->
</testsuites>"#;

    // Real pytest 8 `--junitxml` output (single line).
    const PYTEST_JUNIT: &str = r#"<?xml version="1.0" encoding="utf-8"?><testsuites><testsuite name="pytest" errors="0" failures="0" skipped="1" tests="3" time="0.015"><testcase classname="tests.test_calculator" name="test_adds_two_numbers" time="0.000" /><testcase classname="tests.test_calculator" name="test_multiplies_two_numbers" time="0.002" /><testcase classname="tests.test_calculator" name="test_skipped_on_purpose" time="0.000"><skipped type="pytest.skip" message="seeded skip">tests/test_calculator.py:20: seeded skip</skipped></testcase></testsuite></testsuites>"#;

    #[test]
    fn parses_node_junit() {
        let t = parse_junit(NODE_JUNIT).unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].status, Status::Passed);
        assert_eq!(t[0].duration_ms, Some(1));
        assert_eq!(t[1].status, Status::Failed);
        let m = t[1].message.as_deref().unwrap();
        assert!(
            m.starts_with("Expected values to be strictly equal:\n\n-1 !== 6"),
            "{m}"
        );
        assert!(m.contains("ERR_TEST_FAILURE"), "{m}");
        assert_eq!(t[2].status, Status::Skipped);
    }

    #[test]
    fn parses_pytest_junit() {
        let t = parse_junit(PYTEST_JUNIT).unwrap();
        let names: Vec<_> = t.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "test_adds_two_numbers",
                "test_multiplies_two_numbers",
                "test_skipped_on_purpose"
            ]
        );
        assert_eq!(t[0].suite.as_deref(), Some("tests.test_calculator"));
        assert_eq!(t[1].duration_ms, Some(2));
        assert_eq!(t[2].status, Status::Skipped);
    }

    #[test]
    fn junit_failure_without_message_uses_body() {
        let xml = r#"<testsuite><testcase name="a"><failure>boom &amp; bust</failure></testcase></testsuite>"#;
        let t = parse_junit(xml).unwrap();
        assert_eq!(t[0].status, Status::Failed);
        assert_eq!(t[0].message.as_deref(), Some("boom & bust"));
    }

    #[test]
    fn junit_cdata_detail_is_kept() {
        // Shape of Playwright 1.64 `--reporter=junit` output.
        let xml = "<testsuites><testsuite><testcase name=\"signs in\" classname=\"login.spec.js\" time=\"5.14\">\
            <failure message=\"expect(locator).toHaveText(expected) failed\" type=\"expect.toHaveText\">\
            <![CDATA[    Expected: \"Welcome, demo\"\n    Received: \"\"\n    at tests/login.spec.js:24:41]]></failure>\
            </testcase></testsuite></testsuites>";
        let t = parse_junit(xml).unwrap();
        let m = t[0].message.as_deref().unwrap();
        assert!(
            m.starts_with("expect(locator).toHaveText(expected) failed"),
            "{m}"
        );
        assert!(
            m.contains("Expected: \"Welcome, demo\"") && m.contains("login.spec.js:24:41"),
            "{m}"
        );
    }

    #[test]
    fn junit_error_element_is_error() {
        let t = parse_junit(
            r#"<testsuite><testcase name="a"><error message="setup"/></testcase></testsuite>"#,
        )
        .unwrap();
        assert_eq!(t[0].status, Status::Error);
    }

    #[test]
    fn malformed_xml_is_an_error_not_an_empty_result() {
        assert!(parse_junit("<testsuite><testcase name=\"a\"></testsuite>").is_err());
    }

    // Trimmed from a real `dotnet test --logger trx` file (xunit, .NET 10), BOM included.
    const TRX: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>
<TestRun xmlns=\"http://microsoft.com/schemas/VisualStudio/TeamTest/2010\">
  <Results>
    <UnitTestResult testName=\"Failing.CalculatorTests.AddsTwoNumbers\" duration=\"00:00:00.0799723\" outcome=\"Passed\" />
    <UnitTestResult testName=\"Failing.CalculatorTests.MultipliesTwoNumbers\" duration=\"00:00:00.0018289\" outcome=\"Failed\">
      <Output>
        <ErrorInfo>
          <Message>Assert.Equal() Failure: Values differ&#xD;
Expected: 6&#xD;
Actual:   -1</Message>
          <StackTrace>   at Failing.CalculatorTests.MultipliesTwoNumbers()</StackTrace>
        </ErrorInfo>
      </Output>
    </UnitTestResult>
    <UnitTestResult testName=\"Passing.CalculatorTests.SkippedOnPurpose\" duration=\"00:00:00\" outcome=\"NotExecuted\" />
  </Results>
</TestRun>";

    #[test]
    fn parses_trx() {
        let t = parse_trx(TRX).unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].status, Status::Passed);
        assert_eq!(t[0].duration_ms, Some(80));
        assert_eq!(t[1].status, Status::Failed);
        let m = t[1].message.as_deref().unwrap();
        assert!(
            m.starts_with("Assert.Equal() Failure: Values differ\nExpected: 6\nActual:   -1"),
            "{m}"
        );
        assert!(
            m.contains("at Failing.CalculatorTests.MultipliesTwoNumbers()"),
            "{m}"
        );
        assert_eq!(t[2].status, Status::Skipped);
    }

    #[test]
    fn truncates_long_messages() {
        let long = "x".repeat(MAX_MESSAGE_CHARS + 10);
        assert_eq!(truncate(&long).chars().count(), MAX_MESSAGE_CHARS + 1);
    }
}
