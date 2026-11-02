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

Para instalar o actualizar desde este repositorio, ejecuta `cargo install --path crates/qualyx-cli --locked --force` y construye el contenedor con `docker build -f deploy/code/Dockerfile -t qualyx-code-sandbox:local .`. Apunta la entrada MCP al binario instalado (`~/.cargo/bin/qualyx`, o `qualyx.exe` en Windows), en lugar de `target/debug`.

Agrega estas variables al campo `env` de esa entrada MCP y reinicia el servidor en tu cliente:

```json
{
  "QUALYX_CODE_MODE": "1",
  "QUALYX_CODE_ONLY": "1",
  "QUALYX_CODE_IMAGE": "qualyx-code-sandbox:local"
}
```

Esto anuncia `qa_code_search` y `qa_code_execute`. Para aprobar el envío y la cancelación de pruebas en los directorios configurados, el administrador puede agregar `QUALYX_CODE_APPROVALS=tests.run,tests.cancel`. Omite `QUALYX_CODE_ONLY` para conservar las herramientas directas en la misma sesión. Codex admite `.codex/config.toml` en proyectos de confianza; conserva las rutas del equipo y los permisos en local. Consulta la [configuración y los ejemplos](docs/code-mode.md).

<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/diagrams/code-mode-es-dark.svg" /><img src="assets/readme/diagrams/code-mode-es-light.svg" alt="Code Mode: buscar capacidades, componer JavaScript aislado, autorizar cada llamada, enviar trabajos asíncronos existentes y consultar la evidencia después; las llamadas sin permiso se rechazan" width="100%" /></picture></p>

<!-- kit:tools -->
<!-- /kit:tools -->

El repositorio incluye una [skill de auditoría de seguridad](.agents/skills/qualyx-security-audit/SKILL.md) que detectan los clientes que leen `.agents/skills`. No se edita ninguna configuración global de agentes. Las funciones con IA siguen siendo opcionales.

<!-- kit:agent-links -->
<!-- /kit:agent-links -->
