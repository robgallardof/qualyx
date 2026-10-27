# ADR 0005 — Runner adapter contract

- Status: accepted (2026-10-26)

## Decision
In-process Rust trait first. Out-of-process plugins (an MCP or JSON-RPC protocol) come only once a
third party needs one.

```rust
pub trait Runner {
    fn ecosystem(&self) -> Ecosystem;
    fn plan(&self, project: &ProjectProfile, out_dir: &Path) -> Invocation; // command + expected report
    fn collect(&self, outcome: &ProcessOutcome, out_dir: &Path) -> RunnerResult; // parse into TestCases
}
```

- `plan` is pure, so it's testable without running anything.
- `collect` tries the structured report first (JUnit/TRX). If none exists it parses stdout. If that
  fails it falls back to the exit code alone and marks the per-test detail as **inconclusive**.
  It never invents tests.
- Shared parsers (JUnit, TRX) live once in `qualyx-runners::report`.
