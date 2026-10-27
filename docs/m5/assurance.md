# M5 — Performance and security evidence

> Gate: seeded performance regression and seeded vulnerability detected with evidence; unsafe
> target blocked.

Tools were verified locally on Windows 11 with the official release binaries, each checked
against its release's SHA-256 file. CI repeats this on Ubuntu (`assurance` job) with the same
pinned versions.

| Tool | Version | License | How Qualyx runs it |
|---|---|---|---|
| k6 | 2.3.0 | AGPL-3.0 | external process, generated script |
| Gitleaks | 8.30.1 | MIT | `gitleaks dir --redact --report-format sarif` |
| Semgrep | 1.182.1 | LGPL-2.1 | `semgrep scan --sarif --metrics=off` (project rules, else `p/default`) |
| Trivy | 0.76.1 | Apache-2.0 | `trivy fs --scanners vuln --format sarif` |
| OSV-Scanner | 2.7.0 | Apache-2.0 | `osv-scanner scan source --format sarif` |
| OWASP ZAP | `ghcr.io/zaproxy/zaproxy:stable` | Apache-2.0 | container, `zap-baseline.py` (spider + passive rules only) |

## Performance

`crates/qualyx-perf/tests/k6_regression.rs`: k6 hits an in-process server whose latency is
seeded (3 ms, then 15 ms). Each side runs twice (4 VUs, 1 s warm-up + 4 s measured).

| Comparison | Median | Ratio | One-sided Mann–Whitney p | Verdict |
|---|---|---|---|---|
| baseline vs seeded slowdown | 3.62 ms → 15.47 ms | ×4.27 | ≈ 0 | **regression** |
| baseline vs unchanged rerun (A/A) | — | ×0.99 | 1.0 | no-regression |

How a verdict is reached:

- Only **main-phase** request durations count. Warm-up is a separate k6 scenario, filtered out
  of the raw `--out json` stream.
- A regression needs **both** p < 0.01 **and** a median slowdown above 10 %. With fewer than
  100 requests per side, fewer than 2 repeats, or p95 varying more than 25 % between repeats,
  the verdict is **inconclusive**, never a pass or fail.
- Per-request samples within a run are autocorrelated. The repeat-to-repeat CV check exists
  because the p-value alone overstates confidence.
- Abort condition: `http_req_failed{phase:main}` above the configured rate stops the run
  (`abortOnFail`). Pointed at a dead port with a 60 s duration, the run ends in seconds.

## Security

For coverage semantics, evidence receipts and the reusable repository skill, see
[Security audit workflow](security-audit.md). A clean subset must not hide a failed requested scanner.

`crates/qualyx-security/tests/real_scanners.rs` builds a project at runtime (no secret is ever
committed) with an AWS-key-shaped token, `subprocess.call(cmd, shell=True)` and
`django==2.2.0`:

| Scanner | Findings | Seeded issue found |
|---|---|---|
| Gitleaks | 1 | the key in `config/settings.py:1`. The key text appears in **no** evidence file |
| Semgrep | 1 | `qualyx-subprocess-shell-true` at `app.py:4` (project rule file) |
| Trivy | 43 | Django 2.2.0 advisories |
| OSV-Scanner | 75 | Django 2.2.0 advisories |

All four SARIF runs are merged into `merged.sarif` (SARIF 2.1.0). A clean control project
reports `clean` for Gitleaks and Semgrep.

`crates/qualyx-security/tests/zap_baseline.rs`: ZAP baseline against a local page with no
security headers reported CSP not set (10038), missing anti-clickjacking (10020), missing
X-Content-Type-Options (10021), absent anti-CSRF tokens (10202) and more. On Docker Desktop
the container reaches the host's loopback through `host.docker.internal`. On Linux it uses
`--network=host`.

## Unsafe targets blocked

| Case | Where | Result |
|---|---|---|
| `https://example.com`, `169.254.169.254`, `127.0.0.1.evil.com`, `user@evil.com`, `file://` | `qualyx-perf` unit tests | refused |
| Allowed non-loopback target without `rate_per_sec` | unit test | refused (traffic must be bounded) |
| VUs/RPS/duration above `[performance]` ceilings (50 / 200 / 300 s by default) | unit + MCP test | refused |
| `qa_performance_run` against a production URL | MCP compliance test | refused before k6 is looked up |
| DAST against a non-authorized URL | `zap_baseline.rs` | refused before Docker starts, and nothing is written |
