# Qualyx

[English](README.md)

Qualyx es una plataforma de ingeniería de calidad hecha en Rust. Busca las pruebas que ya tiene un
proyecto, las corre con el runner propio de cada lenguaje y entrega un reporte con evidencia: qué
comandos se ejecutaron, con qué versiones, qué pasó, qué falló y qué se saltó. Se puede usar desde
la terminal o desde cualquier agente compatible con MCP. La IA es opcional, y el modo
determinista funciona sin ningún modelo.

> **Estado:** en planeación. Todavía no hay código usable. El roadmap empieza en M0 (investigación
> y modelo de amenazas) y M1 (una CLI que descubre un repo, corre sus pruebas y genera un reporte).

## Documentos

- [Planeación](docs/PLANNING.md): arquitectura, agentes, integraciones, hitos y criterios de salida.
- [Prompt maestro](docs/MASTER_PROMPT.md): las instrucciones completas para los agentes que lo construyen.
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
