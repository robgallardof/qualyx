# Status

Last updated: 2026-10-27.

## Web console (Next.js) — 2026-10-27

The single-file `dashboard.html` is replaced by a read-only console in `web/dashboard`:
Next.js 16 static export, shadcn/ui (Radix), Tailwind 4, light/dark/system theme,
English/Spanish (browser language by default), React Hook Form + Zod validation, history
search/filters, outcome counts, the run report in a dialog and an MCP setup guide. The export is
committed in `crates/qualyx-api/dashboard`; `build.rs` embeds it, so `cargo build` and the Docker
image need no Node. The API serves it with `nosniff`, `no-referrer` and a same-origin CSP
(`frame-ancestors 'none'`); `/_next/static` is cached as immutable.

- EXECUTED: `npm run typecheck`, 5 Node unit tests for response parsing (native and team
  payloads, invalid payloads rejected), filtering, summaries and shell quoting; two clean
  `npm run build` runs produced byte-identical exports; fmt, strict workspace clippy and the
  locked workspace suite (185 passed, 0 failed); the rewritten real-Chromium regression
  `dashboard_recovers_from_auth_empty_history_and_network_failure` (weak token refused
  client-side, 401, empty history, one passed run, API loss with rows cleared and retry enabled).
  Visual review in Chromium: desktop light/dark, Spanish report dialog, 390 px mobile.
- CI: new `dashboard` job (typecheck, unit tests, build, drift check of the committed export).
- NOT EXECUTED: Firefox/Safari rendering of the console; there is no `npm run dev` proxy, because
  cross-origin requests are refused by design. Rebuild and use `qualyx serve` to try changes.

## Browser CI flakiness — 2026-10-27

Windows run 40557076110 failed when Firefox discarded its startup window during
`browsingContext.create`; launch now retries only that "no such frame" error (93ca4b5). The
Safari driver start/session creation now retries with a fresh `safaridriver` (96df60d). Run
40560345896 passed on all three operating systems.

## macOS browser CI repair — 2026-10-27

Run 40551659939 passed Windows, Linux, lint, team deployment and assurance, but macOS
failed when parallel browser regressions competed for Safari's single WebDriver session.
Safari had already passed accessibility and smoke checks in that same job; the failing
login flow could not start its driver. Serialize the real-browser test binary with
`--test-threads=1`, retaining every browser, test and assertion. Apple's documented
[single-session limit](https://developer.apple.com/documentation/webkit/about-webdriver-for-safari)
requires this scheduling constraint. Native Safari verification requires the hosted macOS job;
it is NOT EXECUTED locally on Windows. Local fmt, strict workspace clippy and the full
locked workspace test suite passed. Follow-up CI results will be checked after publication.

**Product readiness:** the original roadmap is not fully delivered. Milestone states below
describe tested subsets, not universal completion. See the [current readiness assessment](READINESS.md)
for implementation gaps, verification gaps and the next increments.

## Product flow review — 2026-10-27

Reviewed installation/distribution, local history, dashboard, SDK/API, team acceptance,
security and the milestone coverage. See [READINESS](READINESS.md) for remaining product gaps.

- Fixed a reproduced SDK credential leak: urllib followed redirects and copied Authorization
  to the redirected destination. SDK 0.1.1 rejects redirects (301/302/303/307/308) for both GET
  and POST. The real two-server regression failed before the fix and passed afterwards.
- Native packages now include tracked guides, README assets, policy documents, SDK and audit
  skill, plus bilingual native installation instructions. Extracted documentation links are
  validated; deleting a required image makes the regression fail. No local caches are copied.
- Empty history is a successful empty list, without creating `.qualyx`. A real-browser test
  caught the previous first-use error. The dashboard now handles auth errors, empty history,
  populated history and API loss, clears stale rows, bounds requests and enables retries.
- CI now explicitly invokes the CLI accessibility and dashboard browser regressions, plus
  SDK HTTP and package documentation checks. Default skipped tests are not integration proof.
- Hardened team acceptance startup to wait for PostgreSQL TCP readiness rather than its
  temporary init socket; UTF-8 diagnostic output no longer masks failures on Windows.
  The initial acceptance attempt failed at API database startup; a repeat passed. The final
  TCP-readiness version also passed the complete real PostgreSQL/S3/worker acceptance run,
  including restart, atomic claim, cancellation, lease loss and archive recovery.

Executed locally: fmt, strict workspace clippy and the full workspace suite (185 passed,
0 failed, 3 explicitly ignored); all six team Rust tests with ignored tests enabled, including
real PostgreSQL and Chromium dashboard; real CLI accessibility against the seeded page;
four Python SDK/package regressions; extracted Windows package smoke with pass/fail fixtures;
SDK 0.1.1 wheel build; actionlint and pinned Readme Builder drift/link checks.

The preceding security change `c34c23a` also completed
[CI](https://github.com/robgallardof/qualyx/actions/runs/40546999740) and
[all three packages](https://github.com/robgallardof/qualyx/actions/runs/40546999714).
New remote CI for this flow increment, other OS desktop interaction, live LLM/provider smoke
and independent penetration testing are **NOT EXECUTED** at this local checkpoint.

## Security audit follow-up — 2026-10-27

The original M5 covered scanner execution/SARIF, not a complete audit workflow. Added
[audit guidance](m5/security-audit.md) and the repository-local
[`qualyx-security-audit` skill](../.agents/skills/qualyx-security-audit/SKILL.md), inspired by
candidate triage and evidence validation. No global agent settings or plugins were installed.
This is an incremental security improvement, not closure of the remaining original roadmap.

- `security-report.json` now exposes `coverage_complete`, scanner exit codes and timeouts.
  Any requested missing/failed scanner makes the verdict inconclusive (CLI exit 2), while
  keeping usable partial findings. Reviewed suppressions cannot erase coverage gaps.
- Scanner output must contain usable SARIF 2.1.0 results. Unexpected process exits, failed
  invocations and timeouts cannot produce a clean verdict. Reruns discard old SARIF evidence.
- `security-audit.md` records execution receipts, candidate findings and suppression reasons.
  Candidates require code-path/boundary review and reproduction or an explicit proof gap;
  scanner output never automatically confirms an exploit. Decoded evidence strings are redacted.
- EXECUTED: real Gitleaks 8.30.1, Semgrep 1.182.1, Trivy 0.76.1 and OSV-Scanner 2.7.0:
  four integration tests passed, including seeded findings/redaction, clean controls, invalid
  Semgrep rules with stale evidence, and a real scanner timeout.
- EXECUTED: real CLI regression with Gitleaks/Semgrep; suppression of the seeded candidate
  retained exit 2 for failed Semgrep. The missing-scanner CLI regression also passed.
- EXECUTED: skill frontmatter/scaffold validation, pinned Readme Builder drift/link checks
  (0 findings), and actionlint. CI explicitly runs the new real-scanner CLI regression.
- EXECUTED: `cargo fmt --all --check`, strict workspace clippy and `cargo test --workspace
  --locked`: 185 passed, 0 failed, 2 explicitly ignored (PostgreSQL and the separate real-scanner
  CLI test). Default opt-in integration tests can return early; real scanner evidence above
  comes from the separate enabled run, not the workspace count.
- Live agent invocation of the new skill, an independent penetration test and new remote CI
  on this security change are **NOT EXECUTED** at this local checkpoint.

Compatibility: the security status strings are unchanged, but a partial scan that previously
returned `clean` or `findings` now returns `inconclusive`; consumers must inspect retained
findings as well as coverage. The new JSON fields are additive. Existing threat-model and
single-tenant trusted-project deployment limits still apply.

| Milestone | State | Evidence |
|---|---|---|
| M0 — research & threat model | **done** | [integration matrix](m0/integration-matrix.md), [threat model](m0/threat-model.md), ADRs [0001](adr/0001-rust-workspace.md)–[0005](adr/0005-adapter-sdk.md) |
| M1 — deterministic vertical slice | **done** (Windows, Linux, macOS) | below |
| M2 — MCP + isolation | **done** | below, [client matrix](m2/clients.md) |
| M3 — repository intelligence + agents | **done** | below, [benchmark](m3/benchmark.md) |
| M4 — browser + Computer Use | **done** (scope below) | below, [matrix](m4/browsers.md) |
| M5 — performance + security | **done** | below, [evidence](m5/assurance.md) |
| M6 — quality and integration depth | **done** (scope below) | [evidence](m6/quality.md) |
| M7 — team mode + release | **done within the scope below** | [team guide](m7/team.md), [ADR 0006](adr/0006-team-dispatch.md), acceptance below |

## M7 durable execution, deployment and packaging — 2026-10-27

README follow-up: regenerated English and Spanish entry points with Readme Builder
`6172887`, committed authored JSON/Markdown and local light/dark SVGs with license notices.
Builder drift check and local-link audit passed (0 findings). Browser proof-sheet review
loaded all 34 assets and checked desktop and 375px layouts without horizontal overflow.
This verifies local components, not GitHub's final Markdown renderer. Quickstart, evidence,
MCP, team deployment boundaries and contributor instructions now precede the closing banner.

CI follow-up: the first Linux acceptance run of `4374ea5` exposed a fixture mistake:
pytest's `testpaths = ["tests"]` excluded newly written slow/credential tests at the root.
Moved them under `tests/`, asserted the credential test appears in the native results and
waited for the slow test's execution marker before cancellation/crash. The original assertion
is preserved; earlier local success did not prove those extra test bodies ran.
The same CI run also exposed non-canonical recovery identity on macOS/Windows runners
and Linux host cleanup of UID-10001-created bind-mount evidence. Recovery now canonicalizes
its project root; Compose acceptance moves/relaxes only disposable worker-owned evidence
as that worker user. It keeps the non-root deployment and all original assertions.
On `00f098b`, M7/Compose, Linux/macOS tests and all three release packages passed. Windows
then exposed a pre-existing Firefox BiDi startup race (`getTree` returned no initial context).
The driver now creates its own top-level context using the W3C command rather than assuming
the startup window is ready. Real-browser coverage repeats Firefox launch/navigation three
times and retains all seeded accessibility/login assertions. Local Firefox is unavailable;
that integration is validated by the Windows/Linux CI hosts, not a protocol mock.
The next macOS run exposed Safari's documented one-session limit under parallel callers.
Safari now holds a process-wide permit for the full driver lifetime; the same parallel
real-browser suite remains the regression gate. Other programs/processes must still
coordinate access to Safari on that host. Reference: [Apple WebDriver](https://developer.apple.com/documentation/safari-developer-tools/webdriver).

Implemented:

- `serve --team` dispatches durable PostgreSQL jobs without preparing/executing projects in
  the API. Separate serial `worker` processes claim atomically, heartbeat 15-second leases,
  poll cancellation and cannot overwrite a lost lease. Queue capacity: 64. Lost jobs fail
  rather than silently rerunning external side effects. Local API mode remains available.
- PostgreSQL schema v2 upgrades v1 transactionally while preserving run history; unknown
  versions are refused. API views and worker execution enforce configured canonical roots.
- Optional signed S3 archive via Apache object_store 0.14.2; project-scoped keys, manifest,
  report and native artifacts. Missing local evidence falls back to S3; native hashes are
  checked. Configured archive failures fail the job. Explicit offline recovery remains separate.
- Shipped Compose API/worker/PostgreSQL/SeaweedFS, persistent database/object volumes,
  loopback-only API exposure, private backend, non-root restricted worker, read-only API
  project mount and separated credentials. The supplied image supports Python/pytest;
  other native toolchains require a custom image. This is for trusted project copies.
- Python HTTP v1 SDK 0.1.0, bilingual guide, contribution/security policies, updated threat
  model, native package/checksum/install smoke and three-OS release workflow. Main pushes
  validate packages; version tags create drafts only. OTel/dashboard expansion is optional.

Executed on Windows 11 and Linux containers through Docker Desktop:

| Check | Result |
|---|---|
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed |
| `cargo test --workspace --locked` | 179 passed, 0 failed, 1 ignored (explicit PostgreSQL test) |
| `cargo test -p qualyx-cli --test team -- --include-ignored --nocapture` | 5 passed, including real PostgreSQL, v1-to-v2 upgrade, newer-schema refusal |
| `python scripts/test_team.py` | passed: real PostgreSQL/S3/API/worker processes; SDK, restart, two-worker single claim, pass/fail, queued/running cancel, lease expiry, capacity, credential filtering and tamper/S3 recovery |
| `docker build -t qualyx:team .` + `python scripts/test_team_compose.py` | passed: non-root worker, credential separation, service recreation, persistent database/object volumes and S3 fallback |
| Native release build + `scripts/package.py` | passed on Windows: extracted installation, doctor, seeded pass/fail and SHA-256 archive |
| Python SDK wheel build | passed |
| `rhysd/actionlint` (image digest `b1934ee5f1c509618f2508e6eb47ee0d3520686341fec936f3b79331f9315667`) | passed for CI/release workflow definitions |
| Trivy 0.69.3 source/dependency and secret scan (current downloaded DB) | 0 HIGH/CRITICAL dependency findings, 0 secret findings; fixtures, target and Git history excluded |

Evidence logs and local packages are generated under ignored `target/` and `artifacts/`.
Acceptance removes its temporary containers/projects. The scanner result is scoped, not a
claim that lower-severity findings or image/OS vulnerabilities do not exist.

Remote gates on code commit `78d1d9f014cec8754e3cf318b1f1db10e152b0b9` **passed**:
[CI run 40466395666](https://github.com/robgallardof/qualyx/actions/runs/40466395666)
completed all six jobs: lint, Windows/Linux/macOS workspace and real-browser suites,
assurance (real load/security/mutation/contract integrations), and team Compose deployment.
[Package run 40466395847](https://github.com/robgallardof/qualyx/actions/runs/40466395847)
built and smoke-tested all three native packages; draft publication was correctly skipped.
Live provider/agent and desktop smoke were not re-executed in this continuation.
Default opt-in tests can return early, so the workspace count alone is not external-tool evidence.
No tag, package registry upload or public release has been created.

Limits: no hostile multi-tenant sandbox/RBAC, remote PostgreSQL TLS, HA, signed releases,
formal SBOM or OTel. The provided trusted-project worker can access its project mounts and
backend network; environment filtering is not isolation from a malicious same-user test.
See SECURITY.md before remote deployment. The remote results validate this scope, not the
omitted capabilities.

## M7 continuation — 2026-10-27

Resumed the uncommitted M7 implementation after M6 (`4e6cff4`), using the planning pack
`qa-command-center-planning.zip` and the matching `docs/PLANNING.md`.

- HTTP `/v1` run API and read-only dashboard, canonical project-root allowlist, cancellation,
  artifact retrieval and shared CLI/MCP job services. Credentials are required on every bind:
  32+ printable ASCII characters, constant-time comparison. Origin-bearing API requests and
  JSON bodies over 16 KiB are refused.
- Fixed the race between checking capacity and registering jobs. Concurrent requests cannot
  exceed the four-run API limit. Completed polling history is bounded; persisted runs remain
  available. Run IDs now include a sequence so simultaneous requests cannot share a directory.
- Shared PostgreSQL index selected with `QUALYX_DATABASE_URL`; default remains SQLite.
  Project-scoped queries, transactional migrations, advisory migrator lock and refusal of
  unsupported schema versions. Filesystem evidence remains authoritative.
- Recovery is an explicit **offline** `qualyx recover` operation. Server startup leaves
  incomplete evidence alone because another process may still be running. Added regression
  coverage for that distinction. Recovery does not resume work.
- EXECUTED on Windows: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`.
  The workspace harness reports 176 passed, 0 failed, 1 ignored (PostgreSQL).
- EXECUTED separately: `cargo test -p qualyx-cli --test team -- --include-ignored --nocapture`:
  5 passed, including real HTTP child processes, simultaneous requests/cancellation, Python
  fixtures and a real Docker PostgreSQL 17 container. Project separation, repeated schema
  initialization and newer-schema refusal passed. The temporary container was removed.
- The PostgreSQL test is explicitly ignored by default instead of returning success without
  execution. Added its explicit invocation to Linux CI; that new CI run is **NOT EXECUTED**.
- NOT EXECUTED in this continuation: live LLM/client smoke, real browser/desktop runs, optional
  scanner/load/mutation/contract/Compose suites, Linux/macOS runs, installation/release smoke
  and security review. Existing opt-in tests in those suites may return early in the default
  workspace harness; its pass count is not evidence that those tools ran.
- **Remaining M7:** separate isolated workers, Compose deployment, object storage, observability,
  package/release automation, installation smoke and the complete migration/recovery/security
  gate. This increment is a trusted-project API with a shared credential, not multi-user
  authorization. PostgreSQL TLS and artifact replication are not implemented.

## M1 gate

> Clean checkout → install → run → report on all 3 OSes, with passing and failing fixture projects.

Verified locally on **Windows 11 x86_64** (rustc 1.99.1, .NET SDK 10.0.402, Node 24.18.0,
npm 11.16.0, Python 3.14.6, pytest 8.3.5):

```text
cargo fmt --all --check                                   ok
cargo clippy --workspace --all-targets -- -D warnings     ok, 0 warnings
cargo test --workspace                                    46 passed, 0 failed
  qualyx-core 3 · qualyx-discovery 11 · qualyx-runners 15 · qualyx-store 5
  qualyx-cli: fixtures (end to end) 11 · schemas 1
```

The end-to-end tests run the real `qualyx` binary against copies of `fixtures/` with the real
toolchains:

| Fixture | Expected | Result |
|---|---|---|
| rust/passing | 2 passed, 1 skipped, exit 0 | ✔ |
| rust/failing | `multiplies_two_numbers` fails (`left: -1`), exit 1 | ✔ |
| dotnet/passing | 2 passed, 1 skipped, exit 0 | ✔ |
| dotnet/failing | `MultipliesTwoNumbers` fails (`Expected: 6`), exit 1 | ✔ |
| node/passing | 2 passed, 1 skipped, exit 0 | ✔ |
| node/failing | `multiplies two numbers` fails (`-1 !== 6`), exit 1 | ✔ |
| python/passing | 2 passed, 1 skipped, exit 0 | ✔ |
| python/failing | `test_multiplies_two_numbers` fails (`assert -1 == 6`), exit 1 | ✔ |
| empty dir | inconclusive, exit 2 (never green) | ✔ |
| `--run ../../etc` | rejected as invalid run id | ✔ |

Linux and macOS: the same suite passed in [CI](../.github/workflows/ci.yml) on commit `8142b11`
([run 40445737901](https://github.com/robgallardof/qualyx/actions/runs/40445737901)): lint,
`ubuntu-latest`, `windows-latest` and `macos-latest` all green, with .NET 10, Node 24,
Python 3.13 and pytest 8.3.5.

## M2 gate

> Safe execution, cancellation, error recovery and denial tests.

| Requirement | Evidence |
|---|---|
| MCP server, scoped tools, async run ids | `qualyx mcp --root <dir>` (rmcp 3.8, stdio): 10 tools. `qa_run` returns a run id at once, `qa_run_status` polls it |
| Generic MCP compliance | `tests/mcp.rs` speaks raw JSON-RPC: initialize, tools/list with schemas and annotations, protocol errors, tool errors |
| Cancellation | `qa_cancel` on a 120 s test: the run ends in seconds as `error` / "cancelled on request" (`cancel_stops_a_long_run_quickly`) |
| Process-tree kill | `process::tests::{timeout_kills_the_tree, cancellation_stops_the_process}` use a shell that spawns a grandchild |
| Denials | paths outside `--root` (including `..` tricks), artifact names outside the run, unknown isolation: all refused and audited (`paths_outside_roots_and_artifact_traversal_are_denied`) |
| Audit trail | `<root>/.qualyx/mcp-audit.jsonl`, one JSON line per call with the decision |
| Environment allowlist | children get toolchain variables only. Names with TOKEN/SECRET/KEY/AUTH/PASSWORD are always dropped (`child_does_not_see_secrets`) |
| Secret redaction | logs and failure messages are masked for known token shapes and for the values of the server's own credential variables |
| Evidence integrity | `artifact_sha256` in the manifest, `qualyx verify` / `qa_verify` |
| Container runner | `--isolation container`: a copy of the project, `--cap-drop=ALL`, `no-new-privileges`, pids/memory limits. Python and Node verified with Docker 29.8 locally and on ubuntu CI. Rust (`rust:1`) and .NET (`dotnet/sdk:10.0`) images are wired but **not yet verified** |
| Named clients | Claude Code 2.1.311 and Codex CLI 0.164.0 called `qa_discover` for real (see [clients](m2/clients.md)) |

## M3 gate

> Reproducible impact results; benchmark token/time against a full-rescan baseline.

| Requirement | Evidence |
|---|---|
| Incremental index, change detection | git (`base...HEAD` + working tree + untracked) or SHA-256 content index (`qualyx index`). Both tested end to end |
| Impact analysis | Rust: owning package via `cargo metadata` + transitive dependents. Data files select the packages that reference them by name. Unsure means the whole ecosystem, with a reason. 12 unit tests on real git workspaces |
| Reproducible results + benchmark | [benchmark](m3/benchmark.md): same selection on every repetition, 61–67 % fewer tests, docs-only changes run nothing |
| Supervisor task DAG | `qualyx-scheduler`: concurrency cap, budget, cancel, cycle detection, failure skips dependents. `qualyx test --jobs N` |
| Worktree isolation | `qualyx worktree create/list/remove` with an atomic single-writer lock |
| Failure triage role | `qualyx triage` / `qa_analyze_failure`. The reproduction command was executed in tests for Rust, Python, Node and .NET, and it re-fails exactly the broken test |
| LLM abstraction | `qualyx-llm`: openai-compatible (DeepSeek, OpenAI, Ollama, vLLM) and anthropic, contract-tested against a local HTTP server. **No live provider verified**: no API key or local model was available on the test machine. `qualyx llm-probe` checks a real endpoint |
| Serena / Graphify / Context7 | not wired in: Qualyx's impact analysis doesn't need them. They stay optional MCP servers an agent can run beside Qualyx (see the [integration matrix](m0/integration-matrix.md)) |

## M4 gate

> Reproducible sample login/form flows; no unverified "all browsers" claims.

| Requirement | Evidence |
|---|---|
| Protocol drivers | CDP (Chromium family), WebDriver BiDi (Firefox family, W3C), classic WebDriver (Safari) behind one `Driver` trait |
| Reproducible login/form flows | `fixtures/web/flows/login-ok.json` passes and `login-wrong-password.json` fails at step 4 with the real page text, a screenshot, console and the seeded 404, on Chrome 156 and Edge 156 |
| Browser matrix, no blanket claims | [matrix](m4/browsers.md): verified rows name the exact version and OS. Everything else is "unknown" or "see CI" |
| Playwright | `playwright.config.*` detected, JUnit parsed (CDATA included); passing/failing fixtures verified with 1.64.1 |
| Screenshots, console, network | per-step PNGs, `console`, `network_failures` (CDP, BiDi) in `browser-report.json` |
| Safety | MCP `qa_browser_session` only visits loopback or `[browser] allowed_origins` (cloud metadata IPs, `user@host` and look-alike hosts refused). Flow files and served dirs must be under the roots |
| Computer Use | read-only host companion: probe, UI Automation window snapshot and screenshot verified on Windows 11. **No input injection** (needs consent flows). Wayland is reported as unavailable |

## M5 gate

> Seeded performance regression and seeded vulnerability detected with evidence; unsafe target
> blocked.

| Requirement | Evidence |
|---|---|
| k6 workloads with warm-up, ceilings, abort | `qualyx perf run` (TOML workload), generated script, `abortOnFail` on the error rate |
| p50/p95/p99, throughput, errors | per repeat in `result.json`. Raw per-request latencies kept |
| Statistically defended regression | `qualyx perf compare`: seeded ×4.27 slowdown → regression (p ≈ 0). A/A → no-regression. Noisy or small samples → inconclusive |
| Semgrep, Trivy, Gitleaks, OSV + SARIF | `qualyx security`: all four ran for real, seeded secret, injection and vulnerable dependency all found. Merged SARIF 2.1.0 |
| Secret redaction | Gitleaks `--redact` plus Qualyx redaction. The seeded key is absent from every evidence file (asserted) |
| Authorized ZAP | `qualyx dast <url>`: passive baseline from the official image, loopback or `[security] dast_allowed_targets` only |
| Unsafe targets blocked | production URL, metadata IP, look-alike hosts, over-ceiling load: refused in CLI, library and MCP |
| Not covered | Lighthouse, autocannon, profilers/flamegraphs and DB timing aren't integrated yet. The [integration matrix](m0/integration-matrix.md) marks them planned |

## M6 gate

> Known seeded bugs caught; false positives and skipped tests explicitly reported.

See [m6/quality.md](m6/quality.md): flaky detection, the independent reviewer, mutation
testing (cargo-mutants), API contracts (Schemathesis), accessibility (axe-core) and Compose
environments each catch their seeded defect and stay quiet on their control. Skipped tests,
suppressed (reviewed) security findings and unseeded findings are listed in the reports.
Fuzzing, non-Rust mutation and Pact are **not** implemented.

## Known limitations (M1)

- Discovery only looks at the project root. Monorepos and nested projects come later.
- Node: only a plain `node --test` script gets per-test evidence. Jest, Vitest, Mocha and the rest
  run via `npm test` and are graded on **exit code only**, which the report says explicitly.
- ~~On timeout only the direct child process is killed~~ (fixed in M2: process-tree kill).
- In `process` isolation, tests still run with the user's privileges (threat model T1). Use
  `--isolation container` for untrusted code.
- The TRX file name chosen by `dotnet test` includes the user and machine name. It stays in the
  local, gitignored `.qualyx/` folder.
