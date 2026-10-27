# Status

Last updated: 2026-10-26.

| Milestone | State | Evidence |
|---|---|---|
| M0 — research & threat model | **done** | [integration matrix](m0/integration-matrix.md), [threat model](m0/threat-model.md), ADRs [0001](adr/0001-rust-workspace.md)–[0005](adr/0005-adapter-sdk.md) |
| M1 — deterministic vertical slice | **done** (Windows, Linux, macOS) | below |
| M2 — MCP + isolation | not started | — |

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

## Known limitations (M1)

- Discovery only looks at the project root. Monorepos and nested projects come later.
- Node: only a plain `node --test` script gets per-test evidence. Jest, Vitest, Mocha and the rest
  run via `npm test` and are graded on **exit code only**, which the report says explicitly.
- On timeout only the direct child process is killed. Grandchildren (e.g. spawned by `npm.cmd`)
  may survive. Process-tree kill is in M2.
- Tests run with the user's privileges (threat model T1). Isolation is in M2.
- The TRX file name chosen by `dotnet test` includes the user and machine name. It stays in the
  local, gitignored `.qualyx/` folder.
