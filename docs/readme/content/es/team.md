La CLI y MCP usan SQLite por defecto. El modo equipo agrega API HTTP autenticada, panel de lectura, trabajos en PostgreSQL, trabajadores separados y almacenamiento opcional de evidencia compatible con S3.

| Modo | Para qué sirve | Requisitos |
|---|---|---|
| CLI / MCP local | Un proyecto, evidencia local | Herramientas nativas |
| API local | Hasta cuatro ejecuciones simultáneas | Token y raíces de proyecto permitidas |
| API de equipo | Cola persistente y trabajadores independientes | PostgreSQL; S3 opcional |

### Revisa resultados en la consola

`qualyx serve` también sirve una consola web de solo lectura en `/`: historial de ejecuciones, conteo de pruebas correctas y fallidas, búsqueda, filtros y el reporte de cada ejecución. Está hecha con Next.js y shadcn/ui, tiene tema claro y oscuro y está en español e inglés. Va incluida dentro del binario `qualyx`, así que no necesitas un servidor de Node.js. Nunca inicia ni cancela ejecuciones: eso lo hace tu terminal o tu agente MCP.

```sh
export QUALYX_API_TOKEN="$(openssl rand -hex 24)"   # 32+ caracteres imprimibles
qualyx serve --root /ruta/a/proyectos               # http://127.0.0.1:7878
```

Abre `http://127.0.0.1:7878`, escribe la ruta de un proyecto dentro de `--root` y el token. El token solo vive en la memoria de esa pestaña.

La [guía bilingüe de equipo](docs/m7/team.md) incluye Compose, credenciales, SDK de Python, cancelación, respaldos y recuperación. Léela antes de desplegar `deploy/team/compose.yaml`.

Cada trabajador procesa un trabajo a la vez. La cola admite hasta 64 trabajos pendientes/en ejecución. Si vence la sesión de un trabajador, el trabajo falla sin repetir automáticamente sus efectos externos. Un trabajo terminado no significa pruebas aprobadas: revisa `manifest.status`.

**Límite del despliegue:** solo copias de proyectos de confianza. La imagen incluye Python/pytest; otros ecosistemas necesitan una imagen propia. No incluye RBAC por usuario, aislamiento multiusuario hostil, TLS remoto para PostgreSQL, alta disponibilidad, versiones firmadas ni SBOM formal. Lee [SECURITY.md](SECURITY.md) antes de exponerlo remotamente.
