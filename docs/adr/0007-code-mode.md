# ADR 0007: Optional Code Mode with a container boundary

Date: 2026-11-02. Status: implemented, opt-in; acceptance scope in `docs/code-mode.md`.

## Discovery and plan

Repository inspection found `qualyx-mcp` (rmcp stdio, typed tool schemas, root allowlist,
audit), `qualyx-api` (bearer-authenticated HTTP with a 16 KiB body cap),
`qualyx-app::jobs` (bounded asynchronous local jobs), PostgreSQL `Queue` and separate
leased team workers, evidence storage and integrity-checked artifact reads. Native
runners already support process/container isolation. No JavaScript engine or sandbox
for generated orchestration code existed. Existing team Compose mounts trusted project
copies and has a private backend; it is not a hostile JavaScript sandbox. CI covers
three native operating systems and Linux container integrations.

Reuse these services. Add one adapter/runtime crate, a container image, two optional
MCP tools and two optional HTTP endpoints. Preserve all existing CLI commands, MCP
tools, job contracts and runner behavior. Preserve unrelated README builder changes.
Measure direct calls before enabling the alternative and retain direct calls.

## Decision

Use rquickjs 0.14.0 (MIT, Rust minimum 1.87) with QuickJS-NG in a short-lived Linux
container. The engine is an implementation detail, **not the security boundary**.
Its binary is built only with the `sandbox` Cargo feature; ordinary native builds
do not compile the C JavaScript engine. Its main process has no OS/module APIs and
only a bounded JSON-lines host bridge. No network, credentials, host filesystem or
Docker socket enters the container. The trusted native host controls Docker and
authorizes every bridge message, including messages from a compromised engine.

| Candidate | Integration / async | Isolation / cost | Maintenance / license | Decision |
|---|---|---|---|---|
| rquickjs / QuickJS-NG | Rust bindings; native promises and synchronous serialized callbacks | Engine limits plus mandatory container; small startup and process RSS measured separately | Published 0.14.0; MIT bindings, upstream QuickJS licenses | Selected |
| Boa | Rust implementation; promises/job queue | Still requires OS isolation; removes C build but different runtime compatibility and integration | Active project, MIT/Unlicense | No second runtime dependency |
| V8 / deno_core | Rust bindings; async event loop | Still requires OS isolation; larger binary/build and runtime surface | The inspected denoland/deno_core repository is archived; MIT. Verify current upstream before adoption | Too much infrastructure for seven operations |
| Cloudflare Dynamic Workers | Managed isolation and host callbacks | Cloud deployment rather than self-hosted local runtime | Cloudflare infrastructure | Inspiration only; no dependency |

Sources: [rquickjs](https://github.com/DelSkayn/rquickjs),
[Boa](https://github.com/boa-dev/boa), [deno_core](https://github.com/denoland/deno_core),
[Cloudflare pattern](https://developers.cloudflare.com/agents/model-context-protocol/guides/build-codemode-mcp-server/),
[Cloudflare rationale](https://blog.cloudflare.com/code-mode-mcp/).
The engine alternatives were considered architecturally, not benchmarked locally.

Search is bounded structured literal matching over the real registry, not a second
untrusted JavaScript execution. This is simpler for the current small catalog. Keeping
search separate permits future progressive growth without embedding every schema in
the execute description. Small catalogs can be cheaper with a single code tool or
direct invocation; benchmarks report this honestly. The registry does not invent
individual-test discovery, framework filtering, or environment capabilities.

## Authorization and consequences

Read-only is default. Explicit operator standing approvals name `tests.run` and/or
`tests.cancel` at server startup. Tool arguments cannot grant approval. These grants
cover all configured roots; separate server instances are needed for different trust
domains. A client approval of the outer execute tool is not an inner operation grant.
There is no new RBAC or interactive approval service.

Each sandbox is disposable; no cross-request state or worker reuse. This trades Docker
startup latency for a small auditable lifecycle and avoids residual state. A future pool
requires evidence that cold starts dominate useful workflows and a safe reset design.
The default production path stays unchanged. Native Docker access is privileged host
authority and must belong only to the trusted operator, never to the JavaScript container.
The supplied team API container intentionally has no Docker socket: run the optional
orchestration interface as a native process beside a local Linux engine, or keep Code Mode
disabled in that deployment. No socket is added to existing Compose.

## Validation gates

Real raw-JSON-RPC MCP workflow through the image and existing runner, denial/approval
checks, malformed/large code, engine memory/CPU limits, host output limits, cancellation
and concurrency; unchanged direct MCP tests; workspace fmt/clippy/test. Benchmarks use
the same operations and environment, no provider, with explicit context proxies. This
is local trusted-operator isolation, not a claim of formal escape resistance or hostile
multi-tenant security.
