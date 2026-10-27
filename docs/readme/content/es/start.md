Qualyx descubre las pruebas de un proyecto, ejecuta sus herramientas nativas y conserva la evidencia. Úsalo desde la terminal, un agente MCP o una API compartida. **La ejecución determinista no necesita un modelo de IA.**

Los flujos principales están verificados con el [alcance documentado](docs/STATUS.md); el [plan completo sigue pendiente](docs/READINESS.md). CI valida Windows, Linux y macOS, incluidos navegadores reales y pruebas de instalación. Para un binario descargado, sigue la [instalación nativa](INSTALL.md).

Instala Git y Rust estable, además de las herramientas nativas de tu proyecto:

```sh
git clone https://github.com/robgallardof/qualyx.git
cd qualyx
cargo install --locked --path crates/qualyx-cli
qualyx doctor ruta/al/proyecto
qualyx discover ruta/al/proyecto
qualyx test ruta/al/proyecto
qualyx report ruta/al/proyecto
```

Para un ejemplo real pequeño, instala Python y pytest y ejecuta `qualyx test fixtures/python/passing`. El proyecto `fixtures/python/failing` falla intencionalmente. Las pruebas ejecutan código del proyecto: usa repositorios de confianza.

Códigos de salida: **0** aprobado · **1** fallos · **2** sin demostrar. Un proyecto vacío nunca aparece aprobado. El [flujo de paquetes](https://github.com/robgallardof/qualyx/actions/workflows/release.yml) genera archivos nativos y checksums; esto no implica una versión pública publicada.
