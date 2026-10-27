# M3 — Impact selection benchmark

> Gate: reproducible impact results; benchmark token/time against a full-rescan baseline.

**Setup.** A clean `git clone` of this repository at `c4b936c` (M3 plus the data-file fix), Windows 11 x86_64,
release `qualyx` binary, warm build (one full run first, so timings compare test work rather
than cold compilation). Each scenario appends a line to one file, runs
`qualyx test --changed-since HEAD` twice, then reverts. Reproduce with [`scripts/bench_m3.sh`](scripts/bench_m3.sh)
(set `QUALYX_BIN` to a release build).

| Scenario | Changed file | Selected packages | Confidence | Tests run | Time rep 1 / rep 2 | Log bytes | Exit |
|---|---|---|---|---|---|---|---|
| Full baseline | — | all 11 | — | 104 | 25.6 s / 25.6 s | 9 740 | 0 |
| Leaf crate | `crates/qualyx-llm/src/lib.rs` | llm + app, cli, mcp | exact | 38 (−63 %) | 28.2 s / 21.0 s | 4 445 / 3 725 | 0 |
| Shared crate | `crates/qualyx-store/src/lib.rs` | store + app, cli, mcp | exact | 34 (−67 %) | 24.1 s / 17.4 s | 4 395 / 3 491 | 0 |
| Test fixture | `fixtures/python/failing/tests/test_calculator.py` | cli, impact (+ app, mcp) | heuristic | 41 (−61 %) | 24.0 s / 18.5 s | 5 524 / 4 801 | 1 |
| Docs only | `README.md` | none | exact | 0 | 0.3 s / 0.3 s | 0 | 2 |

Rep 1 includes the incremental rebuild of the edited crate. Rep 2 is the steady state.

## Reading the numbers

- **Reproducible.** Both repetitions selected the same packages and the same test counts in
  every scenario.
- **Tests run drop 61–67 %. Wall time drops less (18–32 % at steady state).** Every package
  here is a dependency of `qualyx-cli`, whose end-to-end tests (real cargo/dotnet/node/pytest
  runs) dominate the time. Impact selection can't skip them without being unsound. Projects with
  independent packages gain more. This repo is close to a worst case.
- **Docs-only changes cost nothing** and are reported as *inconclusive* (exit 2), never as a pass.
- **The fixture scenario is a true positive.** The benchmark's edit (`// bench edit` appended to
  a Python file) made that fixture invalid Python. The heuristic selection picked
  `qualyx-cli`, whose e2e test then failed (exit 1). Before the data-file fix in this
  milestone, the same change selected **no** Rust tests and the break would have gone
  unnoticed. `qualyx-impact` is also selected because its own unit test mentions a `fixtures/`
  path. That's a harmless over-selection, recorded rather than hidden.

## Token economy (agents)

An agent that reads the full test log to learn the outcome spends roughly `bytes / 4` tokens
(an approximation, not a tokenizer count):

| What the agent reads | Bytes | ≈ tokens |
|---|---|---|
| Full run log (baseline) | 9 740 | ~2 400 |
| Impact-selected run log | 3 491 – 5 524 | ~900 – 1 400 |
| `qa_run_status` / summary JSON (status, counts, failing test names) | 115 – 154 | ~30 – 40 |

The MCP surface returns the summary by default and serves logs only on request
(`qa_get_artifact`, tail-limited). That's where most of the token saving comes from. Impact
selection roughly halves the rest.
