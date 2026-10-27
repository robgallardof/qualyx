# Qualyx

[Español](README.es.md)

Qualyx is a quality engineering platform written in Rust. It finds the tests a project already has,
runs them with each language's own runner, and reports back with evidence: which commands ran,
which versions were used, what passed, what failed and what was skipped. You can use it from a
terminal or from any agent that speaks MCP. AI is optional, and the deterministic mode doesn't
need a model at all.

> **Status:** planning. There's no usable code yet. The roadmap starts at M0 (research and threat
> model) and M1 (a CLI that discovers a repo, runs its tests and writes a report).

## Documents

- [Planning](docs/PLANNING.md): architecture, agents, integrations, milestones and gates.
- [Master prompt](docs/MASTER_PROMPT.md): the full brief for the coding agents building it.
- [AGENTS.md](AGENTS.md): working rules for any agent that contributes.

## Principles

- It plugs in the good tools that already exist (Playwright, k6, Semgrep, Trivy, axe-core…)
  instead of rewriting them.
- It runs locally first: SQLite and files on disk, with Docker as an option.
- It only claims something works once there's evidence for it. Unsupported features get
  documented as unsupported.
- It's safe by default. Load, DAST and anything aimed at production need explicit permission.
