# M2 — MCP client compatibility

`qualyx mcp-config <client> --root <dir>` prints the configuration for each client from one
launch command (`qualyx mcp --root <dir>…`).

| Client | Version tested | OS | Transport | Evidence | Status |
|---|---|---|---|---|---|
| Claude Code | 2.1.311 | Windows 11 | stdio | `claude -p … --mcp-config mcp.json --strict-mcp-config --allowedTools mcp__qualyx__qa_discover` called `qa_discover` on a Python fixture and answered `"python" "pytest"` (3 turns, 2026-10-26) | **verified** |
| OpenAI Codex CLI | 0.164.0 | Windows 11 | stdio | `codex exec -c mcp_servers.qualyx.command=… -c mcp_servers.qualyx.args=[…]` logged `mcp: qualyx/qa_discover (completed)` and answered `python pytest` (2026-10-26) | **verified** |
| Generic MCP client | protocol 2025-06-18 | Windows, Linux, macOS (CI) | stdio | `crates/qualyx-cli/tests/mcp.rs`: raw JSON-RPC, independent of the SDK | **verified** |
| OpenCode | — | — | stdio | config snippet generated, not installed on the test machine | unverified |
| Cursor | — | — | stdio | `.cursor/mcp.json` snippet generated | unverified |
| VS Code | — | — | stdio | `.vscode/mcp.json` snippet generated | unverified |
| Antigravity | — | — | — | no documented public MCP surface was confirmed; use the generic `mcpServers` snippet | unverified |

## Notes

- Codex reads `-c` values as TOML. On Windows, pass paths as TOML literal strings
  (`'C:\path'`), or use the `config.toml` snippet, which escapes them.
- Tool annotations are set (`readOnlyHint` on every read tool). Clients that honour them can
  auto-approve reads and ask before `qa_run`, which executes the project's own test code.
- Invalid arguments come back as a tool result with `isError: true`. The MCP spec allows this,
  and it lets the model see the reason.
