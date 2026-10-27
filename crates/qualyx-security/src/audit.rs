//! Evidence-based audit summaries: coverage is separate from candidate finding counts.
//! Scanner text is untrusted evidence, never a confirmed exploit or an instruction.

use std::{fs, io, path::Path};

use serde_json::Value;

use crate::SecurityReport;

/// Read a usable SARIF result set; arbitrary JSON must not become a clean scan.
pub(crate) fn read_sarif(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|_| "report not readable".to_string())?;
    let mut doc: Value =
        serde_json::from_slice(&bytes).map_err(|_| "report is not valid JSON".to_string())?;
    if doc["version"] != "2.1.0" {
        return Err("expected SARIF 2.1.0".into());
    }
    let runs = doc["runs"]
        .as_array()
        .filter(|r| !r.is_empty())
        .ok_or("expected a nonempty runs array")?;
    for run in runs {
        if run["tool"]["driver"]["name"]
            .as_str()
            .is_none_or(str::is_empty)
        {
            return Err("missing scanner identity".into());
        }
        let results = run["results"].as_array().ok_or("missing results array")?;
        for result in results {
            if !result.is_object() || result["message"]["text"].as_str().is_none() {
                return Err("result missing a text message".into());
            }
        }
        if let Some(invocations) = run.get("invocations") {
            let invocations = invocations.as_array().ok_or("invalid invocations array")?;
            for invocation in invocations {
                if invocation["executionSuccessful"].as_bool().is_none() {
                    return Err("invocation missing executionSuccessful".into());
                }
            }
        }
    }
    redact_values(&mut doc);
    Ok(doc)
}

fn redact_values(value: &mut Value) {
    match value {
        Value::String(s) => *s = qualyx_runners::redact::redact(s),
        Value::Array(items) => items.iter_mut().for_each(redact_values),
        Value::Object(items) => items.values_mut().for_each(redact_values),
        _ => {}
    }
}

/// SARIF can contain partial results even when the scanner reports execution failure.
pub(crate) fn invocations_succeeded(doc: &Value) -> bool {
    doc["runs"].as_array().into_iter().flatten().all(|run| {
        run["invocations"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|invocation| {
                invocation["executionSuccessful"] != false
                    && invocation["toolExecutionNotifications"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .all(|notice| notice["level"] != "error")
            })
    })
}

fn cell(text: &str) -> String {
    qualyx_runners::redact::redact(text)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace('`', "&#96;")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
        .replace('*', "&#42;")
        .replace(['\r', '\n'], " ")
}

impl SecurityReport {
    /// Recompute the verdict after review/suppression without erasing coverage gaps.
    pub fn refresh_status(&mut self) {
        self.coverage_complete =
            !self.scanners.is_empty() && self.scanners.iter().all(|s| s.status == "ran");
        self.status = if !self.coverage_complete {
            "inconclusive"
        } else if self.findings.is_empty() {
            "clean"
        } else {
            "findings"
        }
        .into();
    }

    /// Persist machine-readable evidence and a human audit handoff after review changes.
    pub fn write_evidence(&self, out: &Path) -> io::Result<()> {
        let mut value = serde_json::to_value(self)?;
        redact_values(&mut value);
        fs::write(
            out.join("security-report.json"),
            serde_json::to_vec_pretty(&value)?,
        )?;
        fs::write(out.join("security-audit.md"), self.audit_markdown())
    }

    /// Render scope, execution receipts and candidates without claiming exploit validation.
    pub fn audit_markdown(&self) -> String {
        let mut text = format!(
            "# Security audit evidence\n\nStatus: **{}**. Coverage complete: **{}**.\n\n\
             Scope is the selected scanners and their configured rules/exclusions. Clean means no unsuppressed findings in that scope, not proof of application security.\n\n\
             ## Execution receipts\n\n| Scanner | Version | Status | Exit | Timed out | Findings | Notes |\n|---|---|---|---|---|---|---|\n",
            cell(&self.status),
            self.coverage_complete
        );
        for s in &self.scanners {
            text.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                s.scanner.id(),
                cell(s.version.as_deref().unwrap_or("unknown")),
                cell(&s.status),
                s.exit_code.map_or("none".into(), |n| n.to_string()),
                s.timed_out,
                s.findings,
                cell(&s.notes.join("; "))
            ));
        }
        text.push_str("\n## Candidates requiring review\n\nScanner findings are claims, not confirmed exploits. For each candidate record the reachable entry point, trust boundary, controls, impact, counterevidence and a reproduction or explicit proof gap. Preserve partial findings when coverage is incomplete.\n\n");
        for (index, f) in self.findings.iter().enumerate() {
            text.push_str(&format!(
                "- Candidate {}: [{}] {} / {} at {}:{} — {}\n",
                index + 1,
                cell(&f.level),
                cell(&f.tool),
                cell(&f.rule_id),
                cell(f.file.as_deref().unwrap_or("unknown")),
                f.line.map_or("?".into(), |n| n.to_string()),
                cell(&f.message)
            ));
        }
        text.push_str("\n## Reviewed suppressions\n\n");
        for (f, reason) in &self.suppressed {
            text.push_str(&format!(
                "- {} / {} at {}:{} — reason: {}\n",
                cell(&f.tool),
                cell(&f.rule_id),
                cell(f.file.as_deref().unwrap_or("unknown")),
                f.line.map_or("?".into(), |n| n.to_string()),
                cell(reason)
            ));
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Finding, Scanner, ScannerResult};
    use serde_json::json;

    fn report() -> SecurityReport {
        SecurityReport {
            status: String::new(),
            coverage_complete: false,
            scanners: vec![ScannerResult {
                scanner: Scanner::Gitleaks,
                status: "ran".into(),
                version: None,
                findings: 0,
                sarif: None,
                notes: vec![],
                exit_code: Some(0),
                timed_out: false,
            }],
            findings: vec![],
            suppressed: vec![],
            merged_sarif: None,
        }
    }

    #[test]
    fn partial_coverage_cannot_turn_green_after_suppression() {
        let mut r = report();
        r.refresh_status();
        assert_eq!(r.status, "clean");
        let f = Finding {
            tool: "gitleaks".into(),
            rule_id: "key".into(),
            level: "error".into(),
            message: "candidate".into(),
            file: None,
            line: None,
        };
        r.findings.push(f.clone());
        r.refresh_status();
        assert_eq!(r.status, "findings");
        let mut missing = r.scanners[0].clone();
        missing.scanner = Scanner::Semgrep;
        missing.status = "missing".into();
        r.scanners.push(missing);
        r.refresh_status();
        assert_eq!(r.status, "inconclusive");
        assert_eq!(r.findings.len(), 1);
        r.findings.clear();
        r.suppressed.push((f, "synthetic fixture".into()));
        r.refresh_status();
        assert!(!r.coverage_complete);
        assert_eq!(r.status, "inconclusive");
        let dir = tempfile::tempdir().unwrap();
        r.write_evidence(dir.path()).unwrap();
        let stored: Value =
            serde_json::from_slice(&fs::read(dir.path().join("security-report.json")).unwrap())
                .unwrap();
        assert_eq!(stored["status"], "inconclusive");
        assert_eq!(stored["suppressed"].as_array().unwrap().len(), 1);
        assert!(
            fs::read_to_string(dir.path().join("security-audit.md"))
                .unwrap()
                .contains("synthetic fixture")
        );
    }

    #[test]
    fn malformed_sarif_is_not_a_clean_scan() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.sarif");
        for doc in [
            json!({}),
            json!({"version":"2.1.0","runs":[]}),
            json!({"version":"2.1.0","runs":[{"tool":{"driver":{"name":"x"}},"results":[{}]}]}),
        ] {
            fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
            assert!(read_sarif(&path).is_err());
        }
        let mut doc = json!({"version":"2.1.0","runs":[{"tool":{"driver":{"name":"x"}},"results":[],"invocations":[{"executionSuccessful":false}]}]});
        fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        assert!(!invocations_succeeded(&read_sarif(&path).unwrap()));
        doc["runs"][0]["invocations"][0]["executionSuccessful"] = json!(true);
        fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        assert!(invocations_succeeded(&read_sarif(&path).unwrap()));
    }

    #[test]
    fn audit_text_does_not_render_scanner_html_or_leak_known_secrets() {
        let mut r = report();
        let key = format!("AKIA{}", "Q".repeat(16));
        r.scanners[0]
            .notes
            .push(format!("<img src=x> | {key}\n# injected"));
        let text = r.audit_markdown();
        assert!(!text.contains("<img"));
        assert!(!text.contains(&key));
        assert!(!text.contains("\n# injected"));
    }
}
