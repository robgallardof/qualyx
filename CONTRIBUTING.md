# Contributing

Read [AGENTS.md](AGENTS.md), the [plan](docs/PLANNING.md) and the current
[evidence](docs/STATUS.md). Work on one milestone increment at a time. Tests must use real
dependencies; an unavailable optional tool is not a verified integration.

Before submitting, run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --locked -p qualyx-cli
python scripts/test_team.py
docker build -t qualyx:team .
python scripts/test_team_compose.py
```

The team tests require Docker and Python/pytest. They create and remove temporary containers
and fixture projects. Other optional integrations have separate opt-in flags documented in
their test modules; report them as NOT EXECUTED unless actually run. Record environment,
commands, limitations and results in `docs/STATUS.md`. Write detailed English commits with
no co-author trailer. Never commit generated evidence, credentials or real user data.

Release packaging: `python scripts/package.py --binary target/release/qualyx` (append `.exe`
on Windows). It verifies the extracted binary on seeded passing and failing Python fixtures.
The release workflow builds three native targets and creates a draft only for version tags.
Review checksums, dependency inventory, security findings and CI before publishing manually.

En español: consulta las reglas y el estado, añade pruebas contra dependencias reales y
registra qué ejecutaste. No presentes una integración opcional como validada si no la corriste.
