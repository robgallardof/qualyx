# Self-hosted Code Mode

Code Mode is optional orchestration over the existing Qualyx services. No model, paid
API, Cloudflare account or remote sandbox is required. It is **disabled by default**.
See [ADR 0007](adr/0007-code-mode.md) for repository findings, alternatives and boundaries.

## Run locally

Windows/macOS: start Docker Desktop using Linux containers. Linux: use a local Docker
engine with its default seccomp profile. Build from the repository root:

```sh
cargo build -p qualyx-cli --locked
docker build -f deploy/code/Dockerfile -t qualyx-code-sandbox:local .
```

To reinstall an optimized binary from the current checkout, run:

```sh
cargo install --path crates/qualyx-cli --locked --force
```

Point the MCP entry's `command` at the installed binary in `~/.cargo/bin/` (`qualyx.exe`
on Windows). Rebuild the sandbox image with the command above after runtime changes,
then restart the MCP server in the client. `--version` remains the workspace version;
reinstalling a newer source revision does not itself change that version string.
Codex accepts project-scoped `.codex/config.toml` in a trusted checkout. Keep local
absolute paths and standing approvals out of commits. See the
[official MCP configuration guide](https://learn.chatgpt.com/docs/extend/mcp).

The [Code Mode workflow](../assets/readme/diagrams/code-mode-en.archify.json) and its
[Spanish counterpart](../assets/readme/diagrams/code-mode-es.archify.json) are editable
Archify sources. The README shows exported light/dark SVGs; the flow describes the full
job lifecycle across later polling/result requests, not a single blocked JavaScript program.

Compose can build the same image with `docker compose -f deploy/code/compose.yaml --profile code build`.
It has no socket or project mount. Do not add a Docker socket to the existing team API.
An MCP configuration example is in [deploy/code/mcp.example.json](../deploy/code/mcp.example.json).

PowerShell, read-only:

```powershell
$env:QUALYX_CODE_MODE = '1'
target/debug/qualyx mcp --root C:/projects/my-project
```

Linux/macOS, read-only:

```sh
QUALYX_CODE_MODE=1 target/debug/qualyx mcp --root /projects/my-project
```

For an explicitly approved test workflow, the **operator** sets
`QUALYX_CODE_APPROVALS=tests.run,tests.cancel` before starting the server. Approvals are
standing grants over its allowed roots. Do not set these for arbitrary remote callers.
Local jobs always use the existing container runner. HTTP team mode enqueues through
the existing PostgreSQL queue; workers enforce their own isolation policy. Code Mode
does not execute suites inside its JavaScript container.

`QUALYX_CODE_IMAGE` selects a trusted prebuilt image (default `qualyx-code-sandbox:local`).
Use an immutable image ID or digest in deployment. Images are never pulled by execute.
The image must implement the supplied protocol and must not contain operator secrets.
No unconfined, privileged or in-process fallback exists if Docker/image startup fails.

## Interfaces

When enabled, standard MCP stdio adds `qa_code_search` and `qa_code_execute`. Existing
tools remain available. No client-side JavaScript runtime or protocol extension is needed.

Set `QUALYX_CODE_ONLY=1` as well to advertise only the two Code Mode tools in a separate
opt-in MCP session. This is needed to realize a smaller tool-schema context. The default
enabled session advertises both interfaces, which increases its catalog size. Keep a
direct session for existing workflows; the benchmark compares direct-only to code-only.

Raw MCP compatibility is tested; named client products have not all been re-tested.

Search arguments:

```json
{"query":"pytest","read_only":true,"limit":3,"offset":0}
```

Search matches identifiers, descriptions, categories and schema text, with an optional
exact `permission` filter. It returns input/output schemas, permission, version, side
effect and async metadata, `total` and `next_offset`. This catalog describes supported
project ecosystem discovery, not individual test discovery. There are seven operations:
`tests.discover`, `tests.run`, `tests.status`, `tests.results`, `tests.cancel`,
`artifacts.list`, `artifacts.read`. Schemas are derived from the same strict Rust types
used for input validation; unknown fields are refused. Outputs are validated with the
corresponding Rust result types before sending them back to JavaScript.

Execute takes `{"code":"<async function body>"}`. Example body:

```javascript
const project = await qualyx.call("tests.discover", {path: "/projects/demo"});
if (!project.ecosystems.length) return {message: "No supported native suite"};
const job = await qualyx.call("tests.run", {path: project.root, timeout_secs: 120});
return {run_id: job.run_id, state: job.state};
```

Poll `tests.status` in a later request or use existing direct `qa_run_status`. Read the
persisted result through `tests.results`, list evidence via `artifacts.list`, and fetch
known integrity-checked evidence through `artifacts.read`. Runs survive the completion
of orchestration code. Cancelling an orchestration request stops its sandbox; cancelling
an already submitted test job requires `tests.cancel` or the existing job cancellation API.

HTTP adds authenticated `POST /v1/code/search` and `POST /v1/code/execute` with the same
JSON arguments and existing bearer/Origin/body-size rules. It shares local jobs with
`/v1/runs`; under `serve --team` it shares the existing durable queue and root allowlist.
The ordinary HTTP `/health` remains a host health check. There is no separate HTTP cancellation endpoint for
an in-flight Code Mode program; HTTP disconnect cancellation is not guaranteed. Use MCP
request cancellation or drop the library future, or let the execution deadline stop it.
Each sandbox must send a ready handshake before capability calls. A running API alone
does not prove Docker readiness.

Library callers can construct `qualyx_codemode::Host` with explicit `Config`, `Roots`
and the existing `Jobs`. No additional CLI subcommand is needed; MCP/HTTP are the
long-lived adapters that preserve asynchronous job ownership.

## Limits and audit

| Resource | Bound |
|---|---|
| Source | 16,384 UTF-8 bytes; HTTP's existing 16 KiB JSON body cap may be lower after escaping |
| Search | 256-byte query, up to 7 entries/page, default 3 |
| Container | Non-root 65532, read-only, no mounts/network, all capabilities dropped, no-new-privileges, default seccomp |
| Container resources | 128 MiB memory including swap ceiling, 0.5 CPU, 16 PIDs, 64 open files, no core dumps |
| Engine | 16 MiB JS allocation limit, 256 KiB stack, 10-second interrupt deadline |
| Host execution | 15-second deadline including startup and bridge work, then bounded cleanup (up to four 5-second removal attempts after errors) |
| Bridge | 32 calls/request; messages at most 65,536 bytes; oversize results refused |
| Concurrency/rate | 2 executions, no waiting queue; 30 search/execute requests/minute per host |
| Artifact reads | 8,192-byte tail, using existing manifest membership/hash checks |

Final results carry execution/correlation identity, duration, startup time, call count,
runtime RSS/CPU ticks when available and `truncated:false`. Oversized results fail
instead of returning malformed truncated JSON. Artifact text can contain the existing
explicit omitted-byte marker. No host environment or raw credentials are sent to JS;
existing redaction runs on capability results. Do not treat heuristic secret redaction
as permission to store secrets in project evidence.

`<first-root>/.qualyx/code-audit.jsonl` records start, readiness, capability identifiers,
decisions, dispatch duration, completion, timeout and cancellation. It excludes source,
arguments, result bodies, raw errors and credentials. Audit opening/writing failure
refuses execution. Correlation ID equals execution ID; returned job IDs associate work
with existing evidence. Container cleanup runs on completion/cancellation. Keep the host
alive for cleanup; forcible host termination/engine failure can leave containers that the
operator must remove by `qualyx-code-` name. No Docker daemon-wide garbage collection runs.

Native evidence/discovery services still perform blocking filesystem/database work;
cancelling a request cannot preempt an already running host I/O call. Once a job submission
has been accepted its independent lifecycle is intentional. Rate and concurrency limits
are per host process, not a distributed quota. There is no RBAC, hostile tenant support,
durable approval workflow, worker reuse, named-client matrix or automatic production enablement.

## Verification and measurement

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
python scripts/test_code_mode.py --team
python scripts/test_code_mode.py --benchmark --repetitions 3
```

The Python acceptance starts real MCP processes and the actual container, exercises
limits/denials/cancellation, runs the Python seeded-failure fixture using the existing
native runner, and reads persisted failure evidence. It never substitutes mocked tool
results. Docker and image are mandatory; missing prerequisites fail rather than skip.

The benchmark compares direct tools and composition for one operation, dependent calls,
and discovery/composition across the actual small catalog. Raw JSON is written under
`artifacts/code-mode-benchmark.json`. Model-visible bytes and bytes/4 are **context
proxies**, not measured model tokens. No provider latency or model behavior is included.
Process CPU ticks/RSS omit Docker daemon/worker overhead; direct process CPU/RSS is not
measured. Throughput is sequential workflow rate, not a saturation/load claim. Direct
tools remain the appropriate choice when their lower latency/context costs win.

### Recorded local results (2026-11-02)

Windows host, Docker Linux engine 29.8.2, local cached image, three repetitions per
approach/scenario. Setup, server initialization and one seeded pytest run to produce
evidence are excluded from measured workflow latency. No model/provider is involved.
These are small-sample medians, not latency SLOs or saturation measurements.
Raw samples: [code-mode-benchmark.json](code-mode-benchmark.json).

| Scenario | Interface | Median ms | Visible bytes | Bytes/4 proxy | Round trips | Sequential workflows/s |
|---|---|---:|---:|---:|---:|---:|
| Single discovery | Direct | 1.956 | 11,495 | 2,874 | 1 | 511.143 |
| Single discovery | Code only | 862.204 | 2,215 | 554 | 1 | 1.160 |
| Dependent discoveries | Direct | 2.836 | 11,887 | 2,972 | 2 | 352.560 |
| Dependent discoveries | Code only | 891.080 | 2,215 | 554 | 1 | 1.122 |
| Catalog + discover/results/report | Direct | 9.713 | 15,387 | 3,847 | 3 | 102.958 |
| Catalog + discover/results/report | Code only | 827.308 | 19,775 | 4,944 | 2 | 1.209 |

Direct tool schemas occupy 11,103 serialized bytes; the two Code Mode schemas occupy
1,455. The final scenario deliberately searches all seven actual capabilities before
composing three distinct operations, so schema discovery outweighs any context saving.
Direct and composed workflows retrieve the same seeded failure and report. The direct
report tool formats a summary; the bridge reads the underlying stored manifest and selects
the same status. These adapters' intermediate response shapes differ, as recorded in
their visible-byte totals. Both interfaces keep using the same evidence services.

Median sandbox startup was 361–411 ms, and total host bridge dispatch was 3–15 ms across
the three scenarios (audit precision: milliseconds). Successful sandbox process peak RSS
was 3,588–3,760 KiB; CPU consumption was 2–3 kernel ticks. JavaScript allocator usage is
also in the raw samples. Docker daemon/host/worker resource costs are excluded. All 18
measured workflows completed with zero unexpected errors. These results support retaining
direct invocation and avoiding a worker pool until a useful workload justifies its complexity.

## Español

Code Mode es opcional y está desactivado por defecto. Requiere Docker con contenedores
Linux; no requiere servicios de pago ni un modelo. Compila la imagen y activa
`QUALYX_CODE_MODE=1` antes de iniciar MCP o HTTP. Por defecto solo permite lectura.
El administrador puede aprobar `tests.run` y `tests.cancel` mediante
`QUALYX_CODE_APPROVALS`; el JavaScript generado no puede aprobarse a sí mismo.
Cada ejecución usa un contenedor nuevo, sin red, montajes ni credenciales. Las pruebas
se envían al sistema de trabajos existente y devuelven identificadores asíncronos.
Usa los comandos de verificación anteriores. Los límites y restricciones indicados
aplican también al modo local; esto no introduce seguridad multiusuario ni RBAC.
