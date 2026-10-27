# ADR 0002 — Local state store

- Status: accepted (2026-10-26)

## Decision
- **SQLite** through `rusqlite` with the `bundled` feature, so no system library is needed.
  File: `<project>/.qualyx/qualyx.db`.
- Artifacts live on the filesystem: `<project>/.qualyx/runs/<run-id>/` holds `manifest.json`,
  `<runner>.stdout.log`, `<runner>.stderr.log` and native reports (JUnit XML, TRX).
- `manifest.json` is the source of truth. The DB is an index for listing and querying.
- Schema migrations use `PRAGMA user_version`, applied on open.
- Team mode (M7) adds a PostgreSQL implementation behind the same repository trait. No ORM.

## Alternatives
JSON files only (no querying), sled/redb (less tooling to inspect), Postgres now (breaks local-first).
