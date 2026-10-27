# Qualyx

[Español](README.es.md)

Qualyx is a quality engineering platform written in Rust. It finds the tests a project already has,
runs them with each language's own runner, and reports back with evidence: which commands ran,
which versions were used, what passed, what failed and what was skipped. You can use it from a
terminal now, and from any agent that speaks MCP once M2 lands. AI is optional, and the
deterministic mode doesn't need a model at all.

> **Status:** M0 and M1 are done. The CLI discovers Rust, .NET, Node and Python projects, runs
> their tests and stores the evidence. See [docs/STATUS.md](docs/STATUS.md) for what's verified
> and what isn't yet.

## Try it

```sh
cargo install --path crates/qualyx-cli   # installs the `qualyx` binary

qualyx doctor  path/to/project   # which toolchains are installed and which ones the project needs
qualyx discover path/to/project  # what Qualyx found and which tool it will use
qualyx test    path/to/project   # run the tests; exit 0 green, 1 failures, 2 unproven
qualyx report  path/to/project   # latest run (--list, --run <id>, --format markdown|json)
```

Every run leaves its evidence in `.qualyx/runs/<run-id>/`: `manifest.json` (commands, exit codes,
tool versions, commit, every test), `report.md` and the native logs and reports. The `.qualyx/`
folder ignores itself, so it never ends up in git.

| Ecosystem | Tool | Evidence |
|---|---|---|
| Rust | `cargo test` | per test (libtest output) |
| .NET | `dotnet test` | per test (TRX) |
| Node | `npm test` → `node --test` | per test (JUnit) |
| Node, any other runner | `npm test` | exit code only, stated in the report |
| Python | `python -m pytest` | per test (JUnit) |

## Documents

- [Status](docs/STATUS.md): what's verified, with evidence, and the known limitations.
- [Planning](docs/PLANNING.md): architecture, agents, integrations, milestones and gates.
- [Master prompt](docs/MASTER_PROMPT.md): the full brief for the coding agents building it.
- [Integration matrix](docs/m0/integration-matrix.md), [threat model](docs/m0/threat-model.md),
  [ADRs](docs/adr/).
- [AGENTS.md](AGENTS.md): working rules for any agent that contributes.

## Principles

- It plugs in the good tools that already exist (Playwright, k6, Semgrep, Trivy, axe-core…)
  instead of rewriting them.
- It runs locally first: SQLite and files on disk, with Docker as an option.
- It only claims something works once there's evidence for it. Unsupported features get
  documented as unsupported.
- It's safe by default. Load, DAST and anything aimed at production need explicit permission.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
