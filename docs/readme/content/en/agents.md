Qualyx runs as a local MCP server (`qualyx mcp --root <dir>`, stdio). `qualyx mcp-config <client>` prints the exact entry for your client; nothing is written for you, so you can review it before adding it.

| Client | Print the entry | Where it goes | Remove it | Tested |
|---|---|---|---|---|
| Claude Code | `qualyx mcp-config claude --root <dir>` | `.mcp.json`, or `claude mcp add qualyx -- …` | `claude mcp remove qualyx` | Verified on 2.1.311 |
| Codex CLI | `qualyx mcp-config codex --root <dir>` | `~/.codex/config.toml` | Delete the `[mcp_servers.qualyx]` table | Verified on 0.164.0 |
| OpenCode | `qualyx mcp-config opencode --root <dir>` | `opencode.json` | Delete `mcp.qualyx` | Snippet only, not run |
| Cursor | `qualyx mcp-config cursor --root <dir>` | `.cursor/mcp.json` | Delete `mcpServers.qualyx` | Snippet only, not run |
| VS Code | `qualyx mcp-config vscode --root <dir>` | `.vscode/mcp.json` | Delete `servers.qualyx` | Snippet only, not run |
| Any MCP client | `qualyx mcp-config generic --root <dir>` | The client's `mcpServers` | Delete `mcpServers.qualyx` | Raw JSON-RPC tests in CI |

```sh
qualyx mcp-config claude --root /absolute/path/to/project   # review, then paste or run the printed command
claude mcp list                                             # qualyx should show as connected
```

Pass `--root` more than once to allow several projects. Read tools are marked read-only, so clients that honor annotations can approve them on their own and ask before `qa_run`, which executes the project's code.

Optional [self-hosted Code Mode](docs/code-mode.md) adds progressive capability search and isolated JavaScript composition over the same jobs. It is disabled by default, requires a local Linux container engine, and grants read-only access until the operator explicitly approves write capabilities. Direct tools remain available.

To install or update from this checkout, run `cargo install --path crates/qualyx-cli --locked --force`, then build the sandbox with `docker build -f deploy/code/Dockerfile -t qualyx-code-sandbox:local .`. Point your MCP entry at the installed binary (`~/.cargo/bin/qualyx`, or `qualyx.exe` on Windows), rather than `target/debug`.

Add these variables to that MCP entry's `env`, then restart the server in your client:

```json
{
  "QUALYX_CODE_MODE": "1",
  "QUALYX_CODE_ONLY": "1",
  "QUALYX_CODE_IMAGE": "qualyx-code-sandbox:local"
}
```

This advertises `qa_code_search` and `qa_code_execute`. To approve test submission and cancellation for the configured roots, the operator can also set `QUALYX_CODE_APPROVALS=tests.run,tests.cancel`. Leave `QUALYX_CODE_ONLY` unset to keep direct tools in the same session. Codex supports a trusted project's `.codex/config.toml`; keep machine-specific paths and approvals local. See the [configuration and examples](docs/code-mode.md).

<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/diagrams/code-mode-en-dark.svg" /><img src="assets/readme/diagrams/code-mode-en-light.svg" alt="Code Mode: discover capabilities, compose isolated JavaScript, authorize each call, submit existing asynchronous jobs and retrieve evidence later; unauthorized calls are refused" width="100%" /></picture></p>

<!-- kit:tools -->
<!-- /kit:tools -->

The repository also ships a [security audit skill](.agents/skills/qualyx-security-audit/SKILL.md) that clients reading `.agents/skills` pick up. No global agent configuration is edited. AI-assisted features stay optional.

<!-- kit:agent-links -->
<!-- /kit:agent-links -->
