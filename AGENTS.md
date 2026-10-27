# AGENTS.md — Qualyx

Canonical instructions for any coding agent (Claude Code, Codex, OpenCode, Antigravity, Cursor…).
`CLAUDE.md` only points here.

## What this is
Qualyx is a Rust-first, local-first QA execution and orchestration platform (CLI + MCP, optional
API/dashboard) with a deterministic no-LLM mode. Status: **M0 and M1 done; next is M2 (MCP +
isolation)**. Read [`docs/STATUS.md`](docs/STATUS.md) first.

- Full brief: [`docs/MASTER_PROMPT.md`](docs/MASTER_PROMPT.md)
- Architecture, milestones, gates: [`docs/PLANNING.md`](docs/PLANNING.md)
- Decisions: [`docs/adr/`](docs/adr/) · threat model: [`docs/m0/threat-model.md`](docs/m0/threat-model.md)

## Layout and commands
- `crates/qualyx-core` domain types · `qualyx-discovery` · `qualyx-runners` · `qualyx-store` ·
  `qualyx-cli` (binary `qualyx`, composition root only).
- `fixtures/<ecosystem>/{passing,failing}`: seeded projects. The failing ones carry one known bug.
- `schemas/`: generated JSON Schemas. Regenerate with `cargo run -p qualyx-cli -- schema --out schemas`.
- Before committing: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`. The end-to-end tests need cargo, dotnet 10, node 24 and python + pytest.

## Rules
- Work milestone by milestone, starting at M0 → M1. Do not jump ahead or batch the roadmap.
- Never report green without logs. No fake tests, no mocks of the feature under test, no
  placeholders claimed as finished, no weakened assertions.
- Integrate mature tools (Playwright, k6, Semgrep…) through adapters; do not reimplement them.
- Deny-by-default for shell, network, writes, DAST/load and anything touching production.
- KISS, DRY, SOLID; modular monolith before microservices.
- Commit messages in English, detailed and sectioned, with no AI co-author trailer.
- User docs in English and Spanish where feasible.

## Naming
- Product: **Qualyx** · CLI binary: `qualyx` · crates: `qualyx-*` · MCP tools: `qa_*`.

## Commit language

All new Git commit subjects and bodies must be written in English, including merge
and squash messages. Keep the repository's existing message conventions.
This applies to all contributors and coding agents, and only to new commits.
Do not rewrite existing history to translate commit messages.
