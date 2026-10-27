Every run writes `.qualyx/runs/<run-id>/`: `manifest.json`, `report.md`, native reports and logs. The manifest records commands, tool versions, commit, exit codes and available test details. `.qualyx/` ignores itself in Git.

```sh
qualyx report path/to/project --list
qualyx report path/to/project --run <run-id> --format json
qualyx report path/to/project --format markdown
```

| Ecosystem | Native runner | Evidence |
|---|---|---|
| Rust | `cargo test` | Individual tests from libtest |
| .NET | `dotnet test` | Individual tests from TRX |
| Node | `npm test` → `node --test` | Individual tests from JUnit |
| Node, other runners | `npm test` | Exit code only, explicitly reported |
| Python | `python -m pytest` | Individual tests from JUnit |

Optional adapters cover [browsers and accessibility](docs/m4/browsers.md), [load and security](docs/m5/assurance.md), and [mutation, contracts and services](docs/m6/quality.md). They require their own tools and permissions; a default run does not execute every adapter.

[Security audits](docs/m5/security-audit.md) include explicit scanner coverage, execution receipts and a reusable agent skill. Missing or failed scans are inconclusive; scanner findings remain candidates until reviewed.
