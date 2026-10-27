# AGENTS.md — Qualyx

Canonical instructions for any coding agent (Claude Code, Codex, OpenCode, Antigravity, Cursor…).
`CLAUDE.md` only points here.

## What this is
Qualyx is a Rust-first, local-first QA execution and orchestration platform (CLI + MCP, optional
API/dashboard) with a deterministic no-LLM mode. Status: **planning — no code yet**.

- Full brief: [`docs/MASTER_PROMPT.md`](docs/MASTER_PROMPT.md)
- Architecture, milestones, gates: [`docs/PLANNING.md`](docs/PLANNING.md)

## Rules
- Work milestone by milestone, starting at M0 → M1. Do not jump ahead or batch the roadmap.
- Never report green without logs. No fake tests, no mocks of the feature under test, no
  placeholders claimed as finished, no weakened assertions.
- Integrate mature tools (Playwright, k6, Semgrep…) through adapters; do not reimplement them.
- Deny-by-default for shell, network, writes, DAST/load and anything touching production.
- KISS, DRY, SOLID; modular monolith before microservices.
- Commit messages in Spanish, detailed and sectioned, with no AI co-author trailer.
- User docs in English and Spanish where feasible.

## Naming
- Product: **Qualyx** · CLI binary: `qualyx` · crates: `qualyx-*` · MCP tools: `qa_*`.
