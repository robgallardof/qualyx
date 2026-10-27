# Security and deployment boundary

Use GitHub's private vulnerability reporting on this repository for sensitive reports;
do not include credentials or exploit real third-party targets in a public issue.

M7 is a single-tenant trusted-project deployment. One bearer credential controls its API
roots; there is no per-user RBAC or hostile multi-tenant isolation. Process mode executes
repository tests with the worker user's privileges. A worker container separates execution
from the API and host but shares its configured projects and backend network. Tests can
read those mounted projects and may reach backend services. Use dedicated deployments per
trust boundary and never run hostile repositories in the supplied trusted-project image.
For untrusted tests use an externally hardened execution environment; this Compose stack
is not that environment.

- The API requires a 32+ character credential on every bind, rejects Origin requests and
  limits JSON bodies. Compose publishes it on host loopback only. Use TLS at a trusted
  reverse proxy before any remote exposure. No CORS/OAuth/browser mutation API is supported.
- Database and S3 are private Compose services with no host-published ports. PostgreSQL
  uses `NoTls`; S3 HTTP is explicitly enabled only for this private network. Configure HTTPS
  for external S3. The supplied worker has no API token or Docker socket and runs non-root
  with capabilities dropped, a read-only root filesystem, and memory/process limits.
- Worker-native test environments drop service credentials; repository `env_passthrough`
  cannot opt them back in for team jobs. This is hygiene, not protection from a hostile
  same-user process reading worker files or inspecting its process environment.
- Work claims are transactional. Lost leases are errors and never automatically rerun.
  There is no exactly-once guarantee for external side effects: a killed test may have acted
  already. Retrying requires a new explicit run request and operator review.
- Filesystem and S3 artifact reads validate names and native artifact hashes. Back up the
  PostgreSQL and object volumes together; keep filesystem evidence where practical.
- Object/database credentials, project snapshots and evidence may contain sensitive data.
  Restrict host filesystem access and apply retention/access policy outside this initial stack.

Security evidence: [M7 guide](docs/m7/team.md), [threat model](docs/m0/threat-model.md).
The release workflow prepares drafts; creating a draft is not a security certification.

En español: el despliegue está diseñado para proyectos de confianza de un solo equipo.
No aísla repositorios hostiles entre sí. Mantén credenciales privadas, publica solo detrás
de TLS cuando sea necesario y separa despliegues por frontera de confianza.
