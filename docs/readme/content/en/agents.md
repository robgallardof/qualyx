Print a configuration, review it, then add it to your client:

```sh
qualyx mcp-config codex --root /absolute/path/to/project
```

The server command is `qualyx mcp --root /absolute/path/to/project`. MCP exposes discovery, execution and evidence to compatible clients. Configuration output does not prove client compatibility: see the [tested client matrix](docs/m2/clients.md) for verified versions and unverified clients, including Claude Code, Codex, Cursor and OpenCode.

AI-assisted features are optional. Keep permissions explicit: running tests executes the project's own code.
