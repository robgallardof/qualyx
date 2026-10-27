# M7 — Durable team execution and release

The team path separates the authenticated API from serial workers through a durable PostgreSQL
queue. Workers archive run evidence in S3-compatible storage. The local CLI and MCP workflows
continue to work without team services. See [STATUS](../STATUS.md) for executed gates and
[SECURITY](../../SECURITY.md) for the trusted-project deployment boundary.

## Run the supplied Compose stack

Build once: `docker build -t qualyx:team .`. Set these environment variables in the shell
that runs Compose (generate new private values; never commit an environment file):

- `QUALYX_PROJECTS_DIR`: absolute directory containing trusted fixture/project checkouts.
- `QUALYX_API_TOKEN`: 32+ printable ASCII characters without spaces.
- `QUALYX_POSTGRES_PASSWORD`: a random hex password (URL-safe).
- `QUALYX_S3_ACCESS_KEY` and `QUALYX_S3_SECRET_KEY`: private S3 credentials.
- Optional `QUALYX_API_PORT`: host port, default 7878.

```powershell
$env:QUALYX_PROJECTS_DIR = 'C:/work/qa-projects'
$env:QUALYX_API_TOKEN = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$env:QUALYX_POSTGRES_PASSWORD = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(24))
$env:QUALYX_S3_ACCESS_KEY = 'qualyx'
$env:QUALYX_S3_SECRET_KEY = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
docker compose -f deploy/team/compose.yaml up -d --wait
```

The API publishes only `127.0.0.1:7878`. PostgreSQL and SeaweedFS S3 are private backend
services; only the API also joins the frontend network. The worker has no API credential,
Docker socket or external network route. It runs as UID 10001, with a read-only root filesystem,
dropped capabilities, no-new-privileges, 1 GiB memory and 256-process limits. Give UID 10001
write permission to the specific project copies/evidence directories on Linux. The API mounts
those projects read-only. **Use disposable trusted project copies**, not valuable working trees.

The supplied image contains Python 3.13 and pytest 8.3.5. Other ecosystems require a worker
image with their native toolchains installed; no universal runtime support is claimed for this
small image. Package restores cannot reach the internet on its internal network: preinstall
project dependencies in a custom image. Scale serial workers with
`docker compose -f deploy/team/compose.yaml up -d --scale worker=2`.

[SeaweedFS mini](https://github.com/seaweedfs/seaweedfs/wiki/Quick-Start-with-weed-mini)
provides the S3 endpoint and initial bucket from environment variables. Its image is pinned to
4.17 plus a manifest digest. This is a single-node deployment, not highly available storage.

## Native processes

```sh
qualyx serve --team --root /work/projects --bind 127.0.0.1:7878
qualyx worker --root /work/projects
```

Both require `QUALYX_DATABASE_URL=postgres://...`. The API also needs `QUALYX_API_TOKEN`.
Workers default to the existing container runner. `--allow-process` explicitly permits native
execution for trusted projects (used inside the dedicated Compose worker). `--once` handles
at most one available job, then exits. A worker processes one job at a time; add workers for
concurrency. Team requests cap ecosystem concurrency at eight and timeouts at 1–1800 seconds.
Repository config cannot opt service credentials into the team test environment.

Without `--team`, `serve` retains its local four-run execution registry. The default CLI and
MCP still use SQLite unless `QUALYX_DATABASE_URL` selects the shared PostgreSQL index.

## HTTP v1 and SDK

`/health` and the read-only dashboard `/` are public. `/v1` requires a bearer credential on
every bind, including loopback. Origin-bearing API requests are refused and JSON bodies are
limited to 16 KiB. Use a trusted TLS proxy before remote exposure; there is no OAuth/CORS/RBAC.

| Method | Route | Result |
|---|---|---|
| POST | `/v1/runs` | 202 with `run_id` and `state: queued` in team mode; JSON `project`, optional `changed_since`, `jobs`, `timeout_secs` |
| GET | `/v1/runs` | Team `jobs` and `in_flight`; optional encoded `project` and `limit` (max 200) |
| GET | `/v1/runs/{id}` | Durable team state, cancellation flag, manifest or error |
| POST | `/v1/runs/{id}/cancel` | Cancel queued job or signal worker; finished jobs are unchanged |
| GET | `/v1/runs/{id}/artifacts/{name}?project=...` | Known text artifact, last 200,000 bytes; filesystem or S3 fallback |

The local API's listing uses `runs` for stored history, and its polling registry lives only
for that process. Team status survives API restarts. An accepted or finished job is not a
passing test: check `manifest.status` (`passed`, `failed`, `error`, `inconclusive`).

Install the standard-library Python SDK v0.1.0 with `python -m pip install ./sdk/python`:

```python
import os
from qualyx_client import QualyxClient
client = QualyxClient('http://127.0.0.1:7878', os.environ['QUALYX_API_TOKEN'])
run = client.start('/projects/my-app')
print(client.status(run['run_id']))
# Poll until finished/failed/cancelled, then inspect manifest.status.
```

Project paths are canonicalized under configured roots. Workers recheck roots before execution.
All processes need the same absolute project paths. Team queue admission is bounded to 64
queued/running jobs; excess requests receive 503. Local mode rejects excess running jobs with
409. API instances only expose queued jobs inside their own configured roots.

## Ownership, cancellation and recovery

PostgreSQL migration 2 adds jobs to the original run index. Migrations are transactional under
an advisory lock and reject newer schemas. Workers claim via `FOR UPDATE SKIP LOCKED`, extend
15-second leases while executing and poll cancellation each second. Expired leases become
`failed`; they are never automatically requeued. A late worker cannot overwrite a lost lease.
A test may already have performed external side effects before interruption: no exactly-once
side-effect guarantee is made. Review before explicitly starting another run.

`qualyx recover path/to/project` is for **offline local evidence recovery**. Stop every process
using that project first. It records log-bearing incomplete run directories as errors; it does
not resume jobs or repair an index entry for an already written manifest. API startup never
rewrites potentially active evidence. Team lost-worker recovery is handled by lease expiry.

## Evidence and backups

Set `QUALYX_S3_BUCKET`, `QUALYX_S3_ACCESS_KEY`, `QUALYX_S3_SECRET_KEY`, optional
`QUALYX_S3_ENDPOINT` and `QUALYX_S3_REGION` on worker/API processes. HTTPS is default; private
Compose HTTP requires explicit `QUALYX_S3_ALLOW_HTTP=1`. Configured upload failures fail the
job instead of claiming successful archiving. Keys include a project SHA-256 and run ID.
Manifests, reports and declared native artifacts are archived; native reads verify their hashes.
Missing local files fall back to S3. The database index and object archive must both be backed up.

`docker compose ... down` retains named volumes. `down --volumes` destroys them: use that
only for disposable tests. Stop new jobs and wait for workers before backing up the PostgreSQL
and object volumes as one consistent set, together with project snapshots and credentials.
Restore into an isolated stack at the same absolute project paths, verify known artifacts and
run a passing/failing fixture before allowing new jobs. Do not treat `health` as integrity validation.

## Verification and release

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo test -p qualyx-cli --test team -- --include-ignored --nocapture
cargo build --locked -p qualyx-cli
python scripts/test_team.py
docker build -t qualyx:team .
python scripts/test_team_compose.py
cargo build --locked --release -p qualyx-cli
python scripts/package.py --binary target/release/qualyx
```

Append `.exe` on Windows. The real-service tests exercise restart, single claim with two workers,
seeded pass/fail, SDK calls, cancellation, queue limits, credential filtering, tamper refusal,
lease expiry and S3 recovery. Compose acceptance recreates all services while preserving
PostgreSQL/object volumes. Temporary projects and containers are removed after tests.

CI runs native tests and package smoke on Windows/Linux/macOS, plus team services on Linux.
`release.yml` builds native release archives on main/tag pushes, tests the extracted installation,
and includes SHA-256 checksums and dependency inventories. Version tags prepare a **draft**
release; publication remains manual. An unexecuted workflow is not a passing release gate.
OTel, a full formal SBOM, signed releases, remote database TLS, high availability, per-user
authorization and hostile multi-tenant execution are not implemented by this milestone.

## En español

M7 separa la API y los trabajadores mediante una cola PostgreSQL y respalda la evidencia en
S3. Compose conserva los datos al recrear servicios; publica únicamente la API en localhost.
El trabajador incluido soporta Python y ejecuta proyectos de confianza dentro de su contenedor,
sin la credencial de la API. No es aislamiento para repositorios hostiles. La cancelación es
cooperativa; perder un trabajador marca el trabajo como fallido y no vuelve a ejecutarlo.
Configura credenciales privadas, permisos de escritura para UID 10001 y copias descartables
de los proyectos. Para otros lenguajes, prepara una imagen con sus herramientas y dependencias.
Los paquetes se validan antes de generar el borrador de un lanzamiento; publicarlo es manual.
