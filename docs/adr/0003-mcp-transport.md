# ADR 0003 — MCP transport and SDK

- Status: accepted for M2 (2026-10-26)

## Decision
- Official Rust SDK `rmcp` (modelcontextprotocol/rust-sdk, 3.8 at decision time, Apache-2.0).
- **stdio first**, since every target client supports it. Streamable HTTP second, for team mode.
- Long work returns a run ID right away. `qa_run_status`, `qa_cancel` and resource links serve
  logs/artifacts instead of inlining them.
- Tools take structured, validated arguments. Nothing gets passed to a shell.

## Consequences
M1 must keep the run pipeline callable as a library (not only from `main`) so MCP can reuse it.
