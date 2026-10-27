- [Status](docs/STATUS.md): executed checks, evidence and limits.
- [Planning](docs/PLANNING.md) and [master prompt](docs/MASTER_PROMPT.md): architecture and milestones.
- [Integration matrix](docs/m0/integration-matrix.md), [threat model](docs/m0/threat-model.md) and [decisions](docs/adr/).
- [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md): contribution rules.

End-to-end tests need Rust, .NET 10, Node 24 and Python with pytest. Browser/service suites have additional opt-in requirements. Load, DAST and production targets require explicit permission.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

This README uses [Readme Builder](https://github.com/robgallardof/readme-builder). Edit `readme.json` and `docs/readme/content/`, then follow the [rebuild instructions](docs/readme/README.md).

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. [README asset notices](docs/readme/THIRD-PARTY-NOTICES.md).
