# Qualyx

[English](README.md)

Qualyx es una plataforma de ingeniería de calidad hecha en Rust. Busca las pruebas que ya tiene un
proyecto, las corre con el runner propio de cada lenguaje y entrega un reporte con evidencia: qué
comandos se ejecutaron, con qué versiones, qué pasó, qué falló y qué se saltó. Ya se puede usar
desde la terminal y desde agentes compatibles con MCP. La IA es
opcional, y el modo determinista funciona sin ningún modelo.

> **Estado:** M0–M6 están implementados con el alcance documentado. M7 está en curso:
> API HTTP autenticada, trabajadores separados, Compose, PostgreSQL y evidencia en S3.
> Consulta la evidencia en [docs/STATUS.md](docs/STATUS.md) y la configuración y límites
> en [la guía de M7](docs/m7/team.md).

## Pruébalo

```sh
cargo install --path crates/qualyx-cli   # instala el binario `qualyx`

qualyx doctor  ruta/al/proyecto   # qué toolchains hay instaladas y cuáles necesita el proyecto
qualyx discover ruta/al/proyecto  # qué encontró Qualyx y con qué herramienta lo va a correr
qualyx test    ruta/al/proyecto   # corre las pruebas; sale con 0 verde, 1 fallos, 2 sin probar
qualyx report  ruta/al/proyecto   # la última corrida (--list, --run <id>, --format markdown|json)
```

Cada corrida deja su evidencia en `.qualyx/runs/<run-id>/`: `manifest.json` (comandos, códigos de
salida, versiones, commit y cada prueba), `report.md` y los logs y reportes nativos. La carpeta
`.qualyx/` se ignora a sí misma, así que nunca termina en git.

| Ecosistema | Herramienta | Evidencia |
|---|---|---|
| Rust | `cargo test` | por prueba (salida de libtest) |
| .NET | `dotnet test` | por prueba (TRX) |
| Node | `npm test` → `node --test` | por prueba (JUnit) |
| Node, cualquier otro runner | `npm test` | solo código de salida, y el reporte lo dice |
| Python | `python -m pytest` | por prueba (JUnit) |

## Documentos

- [Estado](docs/STATUS.md): lo verificado, con su evidencia, y las limitaciones conocidas.
- [Planeación](docs/PLANNING.md): arquitectura, agentes, integraciones, hitos y criterios de salida.
- [Prompt maestro](docs/MASTER_PROMPT.md): las instrucciones completas para los agentes que lo construyen.
- [Matriz de integraciones](docs/m0/integration-matrix.md), [modelo de amenazas](docs/m0/threat-model.md),
  [ADRs](docs/adr/).
- [AGENTS.md](AGENTS.md): las reglas de trabajo para cualquier agente que contribuya.

## Principios

- Integra las buenas herramientas que ya existen (Playwright, k6, Semgrep, Trivy, axe-core…)
  en lugar de reescribirlas.
- Funciona primero en local: SQLite y archivos en disco, con Docker como opción.
- Solo dice que algo funciona cuando hay evidencia. Lo que no está soportado queda documentado
  como no soportado.
- Es seguro por defecto. Las pruebas de carga, el DAST y cualquier cosa dirigida a producción
  requieren permiso explícito.

## Licencia

Doble licencia: [MIT](LICENSE-MIT) o [Apache-2.0](LICENSE-APACHE), a tu elección.
