# M6 — Quality and integration depth

> Gate: known seeded bugs caught; false positives and skipped tests explicitly reported.

Every check below has a seeded defect it must catch and a control it must stay quiet on.
All of them ran for real on Windows 11 (`crates/qualyx-cli/tests/quality.rs`,
`crates/qualyx-browser/tests/real_browsers.rs`). CI repeats the tool-backed ones on Ubuntu.

| Check | Tool (version) | Seeded defect | Caught | Control |
|---|---|---|---|---|
| Flaky detection | `qualyx test --repeat N` | `fixtures/flaky`: a test that alternates pass/fail | `test_alternates` flagged | `test_stable` not flagged |
| Flaky history | `qualyx flaky` | — | same-commit, clean-tree runs only | dirty or different commits never compared (unit test) |
| Independent reviewer | `qualyx review --base REF` | boundary assertions removed, `#[ignore]` added, a test deleted | `assertions-removed`, `skip-added`, `test-removed` | adding a new test gives no findings |
| Mutation testing | cargo-mutants 27.1.0 | `fixtures/mutation/weak` never tests the 18 boundary | 1 missed mutant: `replace is_adult -> bool with true` | `fixtures/mutation/strong`: 0 missed |
| API contract | Schemathesis 4.32.5 | `fixtures/api/server.py` returns 500 for ids > 1000 | `GET /items/{id}` 500 `{"error": "boom"}` with a curl repro | unauthorized schema URL refused before any request |
| Accessibility | axe-core 4.14.0 (npm, SHA-512 verified) in Chrome/Edge | `a11y-bad.html`: no alt, no label, low contrast | `image-alt`, `label`, `color-contrast` | `login.html`: 0 violations |
| Compose environments | Docker Compose | — | nginx started, port exported as `QUALYX_SVC_WEB_80`, the test hit it | no containers left after the run |

## Reported explicitly, never hidden

- **Skipped tests:** `report.md` has a `### Skipped (N)` section with each test and its reason.
  The manifest keeps them with status `skipped`. A run where *everything* is skipped is
  `inconclusive`.
- **Security false positives:** `.qualyx/suppressions.toml` entries (`tool`, `rule_id`,
  optional `file`, mandatory `reason`) move a finding to `suppressed` with its reason in
  `security-report.json`. An entry without a reason is a config error.
- **Unseeded findings:** Schemathesis also reported `TRACE /items/0 → 501` from Python's
  `http.server`. It's real (an undocumented method returns a server error), so Qualyx reports
  it next to the seeded bug instead of filtering it out.
- **Flaky, not failed:** a repeat batch that disagrees exits 1 and names the flaky tests. The
  individual runs stay in history.

## Not covered in M6

- **Fuzzing:** `cargo-fuzz` needs a nightly toolchain. Nothing is wired yet, and nothing is
  claimed.
- **Mutation outside Rust** (mutmut, Stryker, PIT): not integrated.
- **Pact (consumer-driven contracts):** not integrated. Schemathesis covers provider-side
  schema conformance only.
- **Testcontainers libraries:** Qualyx manages Compose environments around any test runner.
  It doesn't drive language-specific Testcontainers APIs.
