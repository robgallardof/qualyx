# Product readiness / Preparación del producto

Reviewed 2026-10-27 against `MASTER_PROMPT.md`, `PLANNING.md`, the shipped commands,
integration matrices, local regressions and GitHub results. This is a readiness assessment,
not an exhaustive penetration test or a claim that all code paths were exercised.

**The supported core workflows are usable; the complete original roadmap is not finished.**
Historical milestone gates validate their documented subsets. They do not supersede the
broader product requirements below. See [STATUS](STATUS.md) for executed checks.

| Area | Working scope | Still needed for the broader plan |
|---|---|---|
| Installation and local tests | Native packages; Rust, .NET, Node and Python runners; evidence/reporting | Java/Go adapters and fixtures; nested-project discovery; richer Jest/Vitest per-test evidence |
| MCP and clients | Generic protocol tests; recorded Claude Code/Codex smoke | OpenCode + DeepSeek proof; other named clients remain unverified |
| Repository intelligence | Git/hash index, Rust dependency impact, conservative fallback | Broader language/symbol analysis and measured agent-vs-deterministic pilot comparisons |
| Optional AI | Configurable compatible-provider adapters with contract tests | Live provider verification, tool-calling/context capability probes and quality/cost benchmarks |
| Browser and desktop | Real browser login/accessibility tests; Windows read-only desktop inspection | Desktop input/consent workflow; interactive macOS/X11 validation; Wayland portal work; branded forks remain unknown |
| Performance | k6 workload/regression comparison and target controls | Lighthouse, profiling, DB timing and trace correlation |
| Security | Real scanners, SARIF, coverage-aware audit reports and review skill | Independent product security review; no guarantee that scanners detect authorization/business-logic defects |
| Test depth | Compose, Schemathesis, axe-core, Rust mutation and flaky detection | Fuzzing, non-Rust mutation, consumer-driven Pact contracts and reliability/chaos tooling |
| Team services | Authenticated API, PostgreSQL queue, workers, S3, cancellation/recovery | Hostile-repository isolation and per-user authorization are not supported; TLS database connections/HA remain absent |
| Distribution | Three-platform package builds and installation smoke; draft-release workflow | Formal SBOM, signing/provenance and an actual reviewed public release |

OpenTelemetry/dashboard expansion is optional in M7; optional integrations must not be
counted as blockers for local deterministic use. Conversely, full Computer Use and the
live provider/client evidence are explicit ambitions of the original product, not completed
merely by documenting their absence.

## Flow fixes in this review

- SDK 0.1.1 refuses redirects instead of forwarding bearer credentials or replaying requests.
  Real local HTTP servers reproduced the previous leak and verify the fix for five redirect codes.
- Packages include README assets, guides, policies, SDK and the audit skill, plus native
  installation instructions. The extracted package is checked for missing local documentation links.
- First-use history returns an empty list without creating project state. The dashboard
  distinguishes empty history, authorization failures and connection failures, clears stale rows,
  bounds request time and re-enables retry. Inputs have accessible names and status announcements.
- CI explicitly executes the CLI accessibility regression, SDK HTTP tests, package checks and
  the dashboard browser flow; these no longer depend on a default suite silently skipping them.

## Suggested next increments

1. Establish a real pilot using the supported deterministic/CLI/MCP/team workflows; record
   failures and expected outcomes rather than adding integrations solely to fill a table.
2. Complete a live provider/client path (including OpenCode + DeepSeek or an authorized local
   provider) and the missing capability probes. This needs a chosen endpoint/model and access.
3. Implement desktop interaction and consent per OS as a dedicated milestone with interactive
   host tests. Do not expose unaudited global input through the existing read-only companion.
4. Add fuzzing and release supply-chain evidence, then the remaining adapters according to pilot
   demand. Preserve the trusted-project deployment boundary until isolation is actually proven.

## En español

El núcleo funciona dentro de su alcance comprobado, pero el plan completo sigue pendiente.
Esta revisión mejora instalación, seguridad del SDK, primer uso del panel y cobertura de CI.
No implementa todavía control interactivo de escritorio, proveedores de IA verificados,
fuzzing ni todas las integraciones. Los hitos históricos deben leerse junto con estos límites.
