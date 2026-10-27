//! Human-readable views of a [`RunManifest`]: a terminal summary and a Markdown report.

use std::fmt::Write;

use qualyx_core::{Detail, RunManifest, Status};

fn icon(s: Status) -> &'static str {
    match s {
        Status::Passed => "PASS",
        Status::Failed => "FAIL",
        Status::Skipped => "SKIP",
        Status::Error => "ERROR",
        Status::Inconclusive => "INCONCLUSIVE",
    }
}

fn exit(code: Option<i32>) -> String {
    code.map_or_else(|| "none".to_string(), |c| c.to_string())
}

fn counts_line(c: &qualyx_core::Counts) -> String {
    format!(
        "{} tests: {} passed, {} failed, {} skipped, {} errors",
        c.total, c.passed, c.failed, c.skipped, c.errors
    )
}

/// Compact terminal summary.
pub fn summary(m: &RunManifest) -> String {
    let mut s = String::new();
    for r in &m.runners {
        let detail = match r.detail {
            Detail::PerTest => counts_line(&r.counts),
            Detail::ExitCodeOnly => format!("exit code {} (no per-test detail)", exit(r.exit_code)),
        };
        let _ = writeln!(
            s,
            "[{}] {} ({}) — {detail}",
            icon(r.status),
            r.ecosystem.as_str(),
            r.tool.as_str()
        );
        for t in r
            .tests
            .iter()
            .filter(|t| matches!(t.status, Status::Failed | Status::Error))
        {
            let suite = t
                .suite
                .as_deref()
                .map(|x| format!("{x} › "))
                .unwrap_or_default();
            let first = t
                .message
                .as_deref()
                .and_then(|m| m.lines().find(|l| !l.trim().is_empty()));
            let _ = writeln!(
                s,
                "    ✗ {suite}{}{}",
                t.name,
                first
                    .map(|l| format!(" — {}", l.trim()))
                    .unwrap_or_default()
            );
        }
        for n in &r.notes {
            let _ = writeln!(s, "    note: {n}");
        }
    }
    if m.runners.is_empty() {
        let _ = writeln!(s, "No supported ecosystem was detected; nothing ran.");
    }
    let _ = writeln!(
        s,
        "\n{} — {} — run {}",
        icon(m.status),
        counts_line(&m.counts),
        m.run_id
    );
    s
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}

/// Full Markdown report, also written as `report.md` next to the manifest.
pub fn markdown(m: &RunManifest) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Qualyx run `{}`\n", m.run_id);
    let _ = writeln!(s, "- **Status:** {}", icon(m.status));
    let _ = writeln!(s, "- **Totals:** {}", counts_line(&m.counts));
    let _ = writeln!(s, "- **Project:** `{}`", m.project_root.display());
    if let Some(g) = &m.git {
        let _ = writeln!(
            s,
            "- **Commit:** `{}`{}",
            g.commit,
            if g.dirty {
                " (uncommitted changes)"
            } else {
                ""
            }
        );
    }
    let _ = writeln!(
        s,
        "- **Started:** {} · **Finished:** {}",
        m.started_at.to_rfc3339(),
        m.finished_at.to_rfc3339()
    );
    let _ = writeln!(
        s,
        "- **Host:** {} / {} · Qualyx {}",
        m.os, m.arch, m.qualyx_version
    );

    for r in &m.runners {
        let _ = writeln!(
            s,
            "\n## {} — {} ({})\n",
            r.ecosystem.as_str(),
            icon(r.status),
            r.tool.as_str()
        );
        let _ = writeln!(s, "- **Command:** `{}`", r.command.join(" "));
        if let Some(v) = &r.tool_version {
            let _ = writeln!(s, "- **Tool version:** {v}");
        }
        let _ = writeln!(
            s,
            "- **Exit code:** {}{} · **Duration:** {} ms",
            exit(r.exit_code),
            if r.timed_out { " (timed out)" } else { "" },
            r.duration_ms
        );
        let _ = writeln!(
            s,
            "- **Evidence:** {}",
            if r.detail == Detail::PerTest {
                "per test"
            } else {
                "exit code only"
            }
        );
        let _ = writeln!(
            s,
            "- **Artifacts:** {}",
            r.artifacts
                .iter()
                .map(|a| format!("`{a}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        for n in &r.notes {
            let _ = writeln!(s, "- **Note:** {n}");
        }
        if !r.tests.is_empty() {
            let _ = writeln!(s, "\n| Status | Suite | Test | ms |\n|---|---|---|---|");
            for t in &r.tests {
                let _ = writeln!(
                    s,
                    "| {} | {} | {} | {} |",
                    icon(t.status),
                    cell(t.suite.as_deref().unwrap_or("")),
                    cell(&t.name),
                    t.duration_ms.map(|d| d.to_string()).unwrap_or_default()
                );
            }
        }
        let failures: Vec<_> = r
            .tests
            .iter()
            .filter(|t| t.message.is_some() && matches!(t.status, Status::Failed | Status::Error))
            .collect();
        if !failures.is_empty() {
            let _ = writeln!(s, "\n### Failures\n");
            for t in failures {
                let _ = writeln!(
                    s,
                    "**{}**\n\n```text\n{}\n```\n",
                    t.name,
                    t.message.as_deref().unwrap_or("")
                );
            }
        }
    }
    s
}
