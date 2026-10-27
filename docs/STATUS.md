# Status

Last updated: 2026-10-26.

| Milestone | State | Evidence |
|---|---|---|
| M0 — research & threat model | **done** | [integration matrix](m0/integration-matrix.md), [threat model](m0/threat-model.md), ADRs [0001](adr/0001-rust-workspace.md)–[0005](adr/0005-adapter-sdk.md) |
| M1 — deterministic vertical slice | **done** (Windows, Linux, macOS) | below |
| M2 — MCP + isolation | **done** | below, [client matrix](m2/clients.md) |
| M3 — repository intelligence + agents | **done** | below, [benchmark](m3/benchmark.md) |
| M4 — browser + Computer Use | **done** (scope below) | below, [matrix](m4/browsers.md) |

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

## Known limitations (M1)

- Discovery only looks at the project root. Monorepos and nested projects come later.
- Node: only a plain `node --test` script gets per-test evidence. Jest, Vitest, Mocha and the rest
  run via `npm test` and are graded on **exit code only**, which the report says explicitly.
- ~~On timeout only the direct child process is killed~~ (fixed in M2: process-tree kill).
- In `process` isolation, tests still run with the user's privileges (threat model T1). Use
  `--isolation container` for untrusted code.
- The TRX file name chosen by `dotnet test` includes the user and machine name. It stays in the
  local, gitignored `.qualyx/` folder.
