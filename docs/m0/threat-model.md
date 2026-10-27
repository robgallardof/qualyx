# M0 — Threat model

Method: STRIDE over the M1–M2 surface. Re-evaluate at every milestone that adds a trust boundary.

## Assets
Source code of the target repo, developer credentials (Git, cloud, API keys, browser sessions),
the host machine, network targets, run evidence (which must stay trustworthy), LLM API budget.

## Trust boundaries
1. **User → Qualyx CLI**: trusted operator.
2. **Agent client → Qualyx MCP** (M2): the request is trusted, but its *arguments* may come from
   prompt-injected text.
3. **Qualyx → target repository**: **untrusted**. Running its tests runs arbitrary code
   (build scripts, `conftest.py`, `build.rs`, npm lifecycle scripts, MSBuild targets).
4. **Qualyx → external tools** (k6, Semgrep, MCP servers): semi-trusted, pinned versions.
5. **Qualyx → network targets** (load/DAST, M5): must be explicitly authorized.
6. **Qualyx → LLM providers** (M3): data leaves the machine.
7. **HTTP API → PostgreSQL queue → workers → S3** (M7): the bearer credential grants
   access to configured trusted projects. Queue claims/leases separate control from execution;
   dedicated worker containers limit host exposure. They are not hostile multi-tenant sandboxes.
   See [SECURITY.md](../../SECURITY.md) for backend-network, credential and filesystem limits.

## Threats and mitigations

| # | Threat | STRIDE | Mitigation | Milestone |
|---|---|---|---|---|
| T1 | Malicious repo code runs during `qualyx test` | E | Documented as equivalent to running the project's own test command. Never run on repos you wouldn't run yourself. Container runner as an option. | M1 doc, M2 container |
| T2 | Repo config makes Qualyx execute arbitrary commands (e.g. a custom command in `.qualyx/config.toml`) | E | M1 only runs a **fixed command table per ecosystem**. No shell strings, args passed as vectors, no `sh -c`. | M1 |
| T3 | Prompt injection from repo files/tests reaching an agent that calls Qualyx tools | T/E | MCP tools take structured args, deny-by-default capabilities, path allowlist (workspace root), approval gates for writes/network. | M2 |
| T4 | Secrets leaked into logs/artifacts | I | Env scrubbing for child processes (allowlist), redaction of known token patterns in stored logs, artifacts kept under `.qualyx/` (gitignored). | M1 partial (artifacts local), M2 redaction |
| T5 | Evidence tampering (claiming green) | T/R | Manifest records commands, exit codes, tool versions, commit and timestamps. Logs stored next to it. SHA-256 of artifacts in M2. | M1 manifest, M2 hashes |
| T6 | Path traversal via run IDs / artifact names | T | Run IDs are generated (timestamp + random), never user-provided paths. Artifacts resolved under the run dir only. | M1 |
| T7 | Runaway / hanging test processes | D | Per-runner timeout (default 30 min, configurable), kill on timeout, status `error`. | M1 timeout, M2 cancel |
| T8 | Load/DAST against unauthorized or production targets | E/D | Target allowlist, traffic ceilings, abort conditions, explicit `--authorize`. Off by default. | M5 |
| T9 | Supply chain of external tools/MCP servers | T | Version pinning, checksum/signature verification, opt-in registry, trust review per integration. | M2+ |
| T10 | Source code sent to LLM providers | I | LLM off by default, per-provider opt-in, redaction, local providers (Ollama/vLLM) supported. | M3 |
| T11 | Browser sessions / cookies exposure | I | Isolated profiles per run, never the user's main profile unless explicitly chosen. | M4 |
| T12 | Two writers in one worktree | T | One writer per worktree, enforced by the scheduler. | M3 |

## Explicit residual risk for M1
`qualyx test` runs the target project's tests **with the user's privileges**, just like running
`cargo test` or `npm test` by hand. Isolation is an M2 deliverable.
