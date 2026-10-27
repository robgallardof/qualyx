Genera una configuración, revísala y agrégala a tu cliente:

```sh
qualyx mcp-config codex --root /ruta/absoluta/al/proyecto
```

El servidor se inicia con `qualyx mcp --root /ruta/absoluta/al/proyecto`. MCP permite descubrir proyectos, ejecutar pruebas y consultar evidencia. Generar una configuración no demuestra compatibilidad: consulta versiones verificadas y clientes pendientes en la [matriz de clientes](docs/m2/clients.md), incluidos Claude Code, Codex, Cursor y OpenCode.

Las funciones asistidas por IA son opcionales. Mantén los permisos explícitos: ejecutar pruebas implica ejecutar código del proyecto.
