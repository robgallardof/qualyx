# M0 — Integration matrix

Evidence: GitHub REST API `GET /repos/{owner}/{repo}` (`license.spdx_id`, `pushed_at`, `archived`) and,
where SPDX was `NOASSERTION`, the repository `LICENSE` file. Queried **2026-10-26**. Crate versions from
`cargo search` the same day. Re-run before each milestone; versions get pinned when adopted.

**Owner** is the maintainer of the adapter inside Qualyx. Until the team exists it is `@robgallardof`.

## Licensing rule

Qualyx is `MIT OR Apache-2.0`. Copyleft tools (**AGPL / GPL / LGPL**) are **only ever invoked as
separate processes** through their CLI or protocol. They are never linked, vendored or redistributed in
Qualyx binaries. Users install them themselves, and `qualyx doctor` reports whether they're present.

## Runtime libraries (linked into Qualyx)

| Crate | Version | License | Use | Milestone |
|---|---|---|---|---|
| clap | 4.6 | MIT/Apache-2.0 | CLI | M1 |
| rusqlite (bundled) | 0.41 | MIT | local run store | M1 ✔ |
| quick-xml | 0.42 | MIT | JUnit / TRX parsing | M1 ✔ |
| schemars | 1.2 | MIT | JSON Schemas of the domain types | M1 |
| rmcp (official MCP Rust SDK) | 3.8 | Apache-2.0 (transitioning from MIT) | MCP server | M2 ✔ |
| axum | — | MIT | HTTP API | M7 |
| opentelemetry-rust | — | Apache-2.0 | traces/metrics | M5 |
| testcontainers-rs | — | Apache-2.0 | not used: Compose environments instead | — |

## External tools (process / protocol boundary)

| Tool | License | Last push | Kind | Status | Fallback |
|---|---|---|---|---|---|
| cargo test | MIT/Apache-2.0 | — | native runner | **verified M1** | — |
| dotnet test (TRX) | MIT | — | native runner | **verified M1** | exit code only |
| pytest (`--junitxml`) | MIT | — | native runner | **verified M1** | exit code only |
| node --test (JUnit reporter) | MIT | — | native runner | **verified M1** | exit code only |
| vitest (`--reporter=junit`) | MIT | — | native runner | planned M1.x | exit code only |
| Playwright | Apache-2.0 | 2026-10-27 | runner | **verified M4** (1.64.1) | CDP |
| Playwright MCP | Apache-2.0 | 2026-10-22 | MCP | planned M4 | Playwright CLI |
| Chrome DevTools MCP | Apache-2.0 | 2026-10-26 | MCP | planned M4 | CDP direct |
| WebDriver BiDi | W3C spec | 2026-10-08 | protocol | **verified M4** (Firefox 157/158) | WebDriver classic |
| k6 | **AGPL-3.0** | 2026-10-26 | CLI | **verified M5** (2.3.0) | — |
| Lighthouse | Apache-2.0 | 2026-10-25 | CLI | planned (not in M5) | — |
| autocannon | MIT | 2026-05-16 | CLI | planned (not in M5) | k6 |
| Semgrep | **LGPL-2.1** | 2026-10-27 | CLI | **verified M5** (1.182.1) | — |
| Trivy | Apache-2.0 | 2026-10-26 | CLI | **verified M5** (0.76.1) | OSV-Scanner |
| Gitleaks | MIT | 2026-10-23 | CLI | **verified M5** (8.30.1) | — |
| OSV-Scanner | Apache-2.0 | 2026-10-27 | CLI | **verified M5** (2.7.0) | Trivy |
| OWASP ZAP | Apache-2.0 | 2026-10-26 | container | **verified M5** (baseline, authorized targets only) | — |
| axe-core | **MPL-2.0** | 2026-10-26 | JS in page (downloaded, SHA-512 verified, not bundled) | **verified M6** (4.14.0) | Pa11y |
| Pa11y | **LGPL-3.0** | 2026-10-26 | CLI | planned M6 | axe-core |
| Schemathesis | MIT | 2026-10-26 | CLI | **verified M6** (4.32.5) | — |
| Pact (reference) | MIT | 2026-10-22 | CLI/FFI | not integrated | Schemathesis (provider side) |
| Toxiproxy | MIT | 2026-10-20 | server | not integrated | — |
| Appium | Apache-2.0 | 2026-10-27 | server | scenario (Tier C) | Maestro |
| Maestro | Apache-2.0 | 2026-10-26 | CLI | scenario (Tier C) | Appium |
| Serena | **GPL-3.0-or-later** (SolidLSP MIT) | 2026-10-26 | MCP | planned M3, optional | built-in manifest/Git impact |
| Graphify (now Graphify-Labs/graphify) | Apache-2.0 | 2026-10-26 | CLI/MCP | planned M3, optional | built-in impact |
| Context7 | MIT | 2026-10-26 | MCP | planned M3, optional | — |
| GitHub MCP server | MIT | 2026-10-23 | MCP | planned M7 | `git` + REST |

| cargo-mutants | MIT | — | CLI | **verified M6** (27.1.0) | — |
| Docker Compose | Apache-2.0 | — | CLI | **verified M6** (test environments) | — |

## Agent clients

| Client | License | Integration surface | Status |
|---|---|---|---|
| Claude Code | proprietary | MCP stdio/HTTP, `CLAUDE.md` → `AGENTS.md` | recorded stdio smoke verified; [versions/evidence](../m2/clients.md) |
| OpenAI Codex | Apache-2.0 | MCP, `AGENTS.md` | recorded MCP smoke verified; [versions/evidence](../m2/clients.md) |
| OpenCode (now anomalyco/opencode) | MIT | MCP + custom providers, `AGENTS.md` | planned M2/M3 (with DeepSeek) |
| Antigravity | proprietary | **unverified** public integration surface | investigate in M2; fallback generic MCP / CLI |
| Cursor, Continue, Cline, Roo | various | MCP (feature-detect) | optional |

## LLM providers (M3, all optional)

DeepSeek (OpenAI-compatible), OpenAI, Anthropic, Ollama, vLLM. Every endpoint gets a capability
probe for tool calling, structured output and context size. Model names are never hard-coded.

## Known limitations (recorded up front)

- Branded browsers (Zen, Opera, Brave, Vivaldi) are Chromium/Firefox forks. Their automation status
  stays **unknown** until a pinned version passes a real smoke test.
- Safari is only automatable through `safaridriver` on macOS. Playwright's WebKit build isn't Safari.
- On Wayland, global input and capture go through compositor portals with user consent. There is
  no silent desktop control.
- Docker containers can't drive the host desktop. Desktop automation needs a host companion.
