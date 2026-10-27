Cada ejecución escribe en `.qualyx/runs/<run-id>/`: `manifest.json`, `report.md`, reportes nativos y logs. El manifiesto registra comandos, versiones, commit, códigos de salida y el detalle disponible de las pruebas. `.qualyx/` se excluye a sí misma de Git.

```sh
qualyx report ruta/al/proyecto --list
qualyx report ruta/al/proyecto --run <run-id> --format json
qualyx report ruta/al/proyecto --format markdown
```

| Ecosistema | Runner nativo | Evidencia |
|---|---|---|
| Rust | `cargo test` | Pruebas individuales de libtest |
| .NET | `dotnet test` | Pruebas individuales de TRX |
| Node | `npm test` → `node --test` | Pruebas individuales de JUnit |
| Node, otros runners | `npm test` | Solo código de salida, indicado en el reporte |
| Python | `python -m pytest` | Pruebas individuales de JUnit |

Los adaptadores opcionales cubren [navegadores y accesibilidad](docs/m4/browsers.md), [carga y seguridad](docs/m5/assurance.md), y [mutación, contratos y servicios](docs/m6/quality.md). Requieren sus herramientas y permisos; una ejecución normal no activa todos los adaptadores.
