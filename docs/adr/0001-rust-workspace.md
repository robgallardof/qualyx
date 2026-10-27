# ADR 0001 — Rust workspace layout

- Status: accepted (2026-10-26)

## Context
The plan asks for a modular monolith with clear boundaries: core domain, discovery, execution,
storage, CLI, MCP later. Too many crates early slows the work down. Too few blurs the boundaries.

## Decision
One Cargo workspace under `crates/`, split by reason to change:

| Crate | Responsibility | Depends on |
|---|---|---|
| `qualyx-core` | domain types (`ProjectProfile`, `Run`, `TestCase`, `Status`, `RunManifest`…) and JSON Schemas | serde, schemars |
| `qualyx-discovery` | detect ecosystems from manifests on disk | core |
| `qualyx-runners` | run native test tools, parse JUnit/TRX/text into `TestCase`s | core |
| `qualyx-store` | SQLite run store and artifact directory layout | core |
| `qualyx-cli` | binary `qualyx`, the composition root | all |

The MCP adapter (`qualyx-mcp`, M2) and the API (`qualyx-api`, M7) will be new crates that reuse the
same services. The CLI owns no business logic beyond wiring.

## Consequences
Every boundary can be tested on its own. Adding an ecosystem touches discovery and runners only.
