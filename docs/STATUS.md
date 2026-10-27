# Status

Last updated: 2026-10-27.

| Milestone | State | Evidence |
|---|---|---|
| M0 — research & threat model | **done** | [integration matrix](m0/integration-matrix.md), [threat model](m0/threat-model.md), ADRs [0001](adr/0001-rust-workspace.md)–[0005](adr/0005-adapter-sdk.md) |
| M1 — deterministic vertical slice | **done** (Windows, Linux, macOS) | below |
| M2 — MCP + isolation | **done** | below, [client matrix](m2/clients.md) |
| M3 — repository intelligence + agents | **done** | below, [benchmark](m3/benchmark.md) |
| M4 — browser + Computer Use | **done** (scope below) | below, [matrix](m4/browsers.md) |
| M5 — performance + security | **done** | below, [evidence](m5/assurance.md) |
| M6 — quality and integration depth | **done** (scope below) | [evidence](m6/quality.md) |
| M7 — team mode + release | **implemented within the scope below; remote release gates pending** | [team guide](m7/team.md), [ADR 0006](adr/0006-team-dispatch.md), acceptance below |

## M7 durable execution, deployment and packaging — 2026-10-27

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
| `cargo test --workspace --locked` | 178 passed, 0 failed, 1 ignored (explicit PostgreSQL test) |
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

At this local checkpoint, GitHub CI and three-OS release execution after pushing are
**NOT EXECUTED**; their workflow definitions are not results. Live provider/agent smoke,
optional browser/desktop/load/DAST/mutation/contract suites were not re-executed locally.
Existing opt-in tests can return early in the default harness, so its count is not tool evidence.
No tag, package registry upload or public release has been created.

Limits: no hostile multi-tenant sandbox/RBAC, remote PostgreSQL TLS, HA, signed releases,
formal SBOM or OTel. The provided trusted-project worker can access its project mounts and
backend network; environment filtering is not isolation from a malicious same-user test.
See SECURITY.md before remote deployment. Remote release results must be recorded separately.

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
