Qualyx corre como servidor MCP local (`qualyx mcp --root <dir>`, stdio). `qualyx mcp-config <cliente>` imprime la entrada exacta para tu cliente; no escribe nada por ti, así que puedes revisarla antes de agregarla.

| Cliente | Imprimir la entrada | Dónde va | Quitarla | Probado |
|---|---|---|---|---|
| Claude Code | `qualyx mcp-config claude --root <dir>` | `.mcp.json`, o `claude mcp add qualyx -- …` | `claude mcp remove qualyx` | Verificado en 2.1.311 |
| Codex CLI | `qualyx mcp-config codex --root <dir>` | `~/.codex/config.toml` | Borra la tabla `[mcp_servers.qualyx]` | Verificado en 0.164.0 |
| OpenCode | `qualyx mcp-config opencode --root <dir>` | `opencode.json` | Borra `mcp.qualyx` | Solo se genera, sin probar |
| Cursor | `qualyx mcp-config cursor --root <dir>` | `.cursor/mcp.json` | Borra `mcpServers.qualyx` | Solo se genera, sin probar |
| VS Code | `qualyx mcp-config vscode --root <dir>` | `.vscode/mcp.json` | Borra `servers.qualyx` | Solo se genera, sin probar |
| Cualquier cliente MCP | `qualyx mcp-config generic --root <dir>` | El `mcpServers` del cliente | Borra `mcpServers.qualyx` | Pruebas JSON-RPC en CI |

```sh
qualyx mcp-config claude --root /ruta/absoluta/al/proyecto   # revisa y pega o corre el comando impreso
claude mcp list                                              # qualyx debe aparecer conectado
```

Pasa `--root` varias veces para permitir varios proyectos. Las herramientas de lectura están marcadas como de solo lectura: los clientes que respetan esas marcas pueden aprobarlas solos y pedir permiso antes de `qa_run`, que ejecuta el código del proyecto.

El [Code Mode autoalojado](docs/code-mode.md) opcional permite descubrir capacidades y componer llamadas mediante JavaScript aislado sobre los mismos trabajos. Está desactivado por defecto, requiere un motor local de contenedores Linux y solo permite lectura hasta que el administrador apruebe capacidades de escritura. Las herramientas directas siguen disponibles.

<!-- kit:tools -->
<!-- /kit:tools -->

El repositorio incluye una [skill de auditoría de seguridad](.agents/skills/qualyx-security-audit/SKILL.md) que detectan los clientes que leen `.agents/skills`. No se edita ninguna configuración global de agentes. Las funciones con IA siguen siendo opcionales.

<!-- kit:agent-links -->
<!-- /kit:agent-links -->
