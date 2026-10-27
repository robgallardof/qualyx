La CLI y MCP usan SQLite por defecto. El modo equipo agrega API HTTP autenticada, panel de lectura, trabajos en PostgreSQL, trabajadores separados y almacenamiento opcional de evidencia compatible con S3.

| Modo | Para qué sirve | Requisitos |
|---|---|---|
| CLI / MCP local | Un proyecto, evidencia local | Herramientas nativas |
| API local | Hasta cuatro ejecuciones simultáneas | Token y raíces de proyecto permitidas |
| API de equipo | Cola persistente y trabajadores independientes | PostgreSQL; S3 opcional |

La [guía bilingüe de equipo](docs/m7/team.md) incluye Compose, credenciales, SDK de Python, cancelación, respaldos y recuperación. Léela antes de desplegar `deploy/team/compose.yaml`.

Cada trabajador procesa un trabajo a la vez. La cola admite hasta 64 trabajos pendientes/en ejecución. Si vence la sesión de un trabajador, el trabajo falla sin repetir automáticamente sus efectos externos. Un trabajo terminado no significa pruebas aprobadas: revisa `manifest.status`.

**Límite del despliegue:** solo copias de proyectos de confianza. La imagen incluye Python/pytest; otros ecosistemas necesitan una imagen propia. No incluye RBAC por usuario, aislamiento multiusuario hostil, TLS remoto para PostgreSQL, alta disponibilidad, versiones firmadas ni SBOM formal. Lee [SECURITY.md](SECURITY.md) antes de exponerlo remotamente.
