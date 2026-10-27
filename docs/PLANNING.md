# Qualyx — Implementation Planning

Status: original product design and acceptance baseline. Implemented subsets and executed
checks are tracked in [STATUS](STATUS.md); outstanding scope is tracked in [READINESS](READINESS.md).
This plan is not a claim that every listed integration has shipped.

## Definition of product
Universal Rust-first QA execution/orchestration platform, available as CLI + MCP + optional API/desktop host/dashboard, supporting deterministic and optional agentic execution.

## Architecture boundaries
- **Control plane**: policy, discovery, planning, scheduling, task state, budgets, reports.
- **Execution plane**: isolated runners and host desktop companions.
- **Knowledge plane**: project index, Git diffs, test graph, optional Serena/Graphify, documentation.
- **Integration plane**: MCP gateway/registry, language runners, browser automation, model providers.
- **Evidence plane**: artifacts, provenance, run manifests, telemetry and reports.

## Agents
| Role | Inputs | Outputs | Tools | Mandatory LLM? |
|---|---|---|---|---|
| Supervisor | goal, policies, capabilities | task DAG | scheduler | No |
| Discovery | repo snapshot | project profile | Git/manifests/AST | No |
| Code intelligence | changed files | symbols/impact | Serena/Graphify | No |
| Unit | code, test conventions | proposed tests | native runners | Optional |
| Integration | dependencies, fixtures | isolated integration runs | Testcontainers/Compose | Optional |
| API/contract | OpenAPI/AsyncAPI | contract results | Schemathesis/Pact | Optional |
| Browser | flows | E2E results | Playwright/CDP/WebDriver | Optional |
| Computer Use | authorized UI goal | interaction evidence | desktop host | Optional |
| Security | threat model | findings/SARIF | Semgrep/Trivy/Gitleaks/ZAP | Optional |
| Performance | SLO, target, workload | regression evidence | k6/Lighthouse/OTel | Optional |
| Accessibility | pages | WCAG findings | axe-core/Pa11y | No |
| Reliability | fault scenario | recovery evidence | Toxiproxy | Optional |
| Quality reviewer | tests, defects | quality assessment | mutation/coverage | Optional |
| Failure triage | failed run | diagnosis and reproduction | logs/traces | Optional |

## Tool selection policy
Tier A (baseline): Git, native runners, Playwright, k6, OpenTelemetry, Semgrep/Trivy/Gitleaks, Testcontainers where applicable.
Tier B (high-value optional): Chrome DevTools MCP, Serena, Graphify, Context7, GitHub MCP, WebDriver BiDi, axe-core, Schemathesis, Pact.
Tier C (scenario-driven): ZAP, Lighthouse, autocannon, OSV-Scanner, profilers, Toxiproxy, mutation/fuzz tools, Appium/Maestro.
All third-party integrations require license, security, maintenance and compatibility review. Use adapters for CLI tools, not fake MCP wrappers by default.

## Client/provider compatibility
| Integration | Type | Required approach |
|---|---|---|
| Claude Code | Agent client | MCP stdio/HTTP and client-specific setup verified |
| Codex | Agent client | MCP/client adapter verified |
| OpenCode | Agent client | MCP and custom model provider setup verified |
| Antigravity | Agent client | Investigate supported public integration, fallback generic MCP/CLI |
| Cursor/Continue/Cline/Roo | Optional clients | Feature-detect MCP and approvals |
| DeepSeek | LLM provider | OpenAI-compatible adapter with feature probes and API key handling |
| Ollama/vLLM | Local LLM serving | Configurable endpoint, capabilities probe |
| OpenAI/Anthropic | LLM providers | Official adapters where practical |

## Browser/OS validation
Browser candidates: Chrome, Chromium, Edge, Firefox, Safari/macOS, Zen, Opera, Brave, Vivaldi. Classify automation mode as verified protocol / fallback visual / unsupported. Keep exact tested versions and artifacts. Test OS Windows, macOS, Linux X11 and Wayland separately; Wayland requires compositor-mediated permissions. Test interactive desktop hosts on real machines/VMs.

## Milestones and acceptance
### M0 — Research & threat model
- Matrix: tools, licenses, clients, OS, browsers, runtimes, known limitations.
- Threat model for untrusted repos, prompt injection, shell access, browser sessions, secrets, network targets.
- ADRs for Rust workspace, state store, MCP transport, worker isolation and adapter SDK.
**Gate:** each integration has owner, verified source, status and explicit fallback.

### M1 — Deterministic vertical slice
- `qualyx init`, `qualyx doctor`, `qualyx discover`, `qualyx test`, `qualyx report` proposed CLI.
- Discover at least Rust, .NET, Node and Python sample projects; run native tests.
- SQLite run store and normalized result schema; stable artifact paths.
**Gate:** clean checkout -> install -> run -> report on all 3 OS, with passing and failing fixture projects.

### M2 — MCP + isolation
- MCP server with scoped tools, async run IDs, cancellation, timeouts and audit trail.
- Local subprocess isolation and optional container runner; permissions and approval workflow.
- Generic MCP compliance tests; client-specific smoke tests.
**Gate:** safe execution, cancellation, error recovery and denial tests.

### M3 — Repository intelligence + agents
- Incremental index, impact analysis, optional Serena/Graphify, Context7.
- Supervisor task DAG and optional agent roles with worktree isolation.
- LLM abstraction: DeepSeek, OpenAI, Anthropic, Ollama/vLLM via tested adapters.
**Gate:** reproducible impact results; benchmark token/time against full-rescan baseline.

### M4 — Browser + Computer Use
- Playwright, CDP, DevTools, WebDriver; native host desktop agent.
- Browser matrix and OS matrix; screenshots, network, console, traces.
- Visual fallback with explicit uncertainty and accessibility-first routing.
**Gate:** reproducible sample login/form flows; no unverified 'all browsers' claims.

### M5 — Performance + security
- Security audit follow-up: explicit scanner coverage and execution receipts, candidate
  validation workflow and repository skill. See [security audit](m5/security-audit.md).
- k6 workloads (smoke/load/stress/spike/soak), Lighthouse, autocannon where useful.
- p50/p95/p99, throughput, errors, saturation, GC, DB query timing, traces.
- Semgrep, Trivy, Gitleaks, OSV and authorized ZAP; SARIF output.
- Production safeguards, rate ceilings, stop conditions, secret redaction.
**Gate:** seeded performance regression and seeded vulnerability detected with evidence; unsafe target blocked.

### M6 — Quality and integration depth
- Testcontainers/Compose, API/contract, accessibility, mutation, fuzz, flaky analysis.
- Evidence manifests and independent reviewer; no assertion weakening.
**Gate:** known seeded bugs caught, false positives and skipped tests explicitly reported.

### M7 — Team mode + release
- Compose API/worker/Postgres/object storage; optional dashboard, OTel.
- CI integrations, package releases, versioned SDK, docs, examples, contribution guide.
**Gate:** installation smoke tests, migrations, recovery, cross-platform releases and security review.

## Core quality requirements
- Full Rust unit/integration/contract tests and adapter integration tests.
- Fixtures with actual databases/services; mocks only at justified boundaries.
- E2E with real browsers and host desktop where available.
- Performance baselines before optimization; measure scheduler overhead and multiagent value.
- Reproducible artifact manifests, SBOM, supply-chain scans and pinned versions.
- CI on Windows/macOS/Linux, with separate interactive host test runners.
- Clearly document unavailable features rather than silently skipping.

## Performance Agent work breakdown
1. Discover application topology, target environment and telemetry availability.
2. Define authorized target, SLO, workload, limits, warmup, sample size and abort thresholds.
3. Generate k6 scripts/fixtures; use Lighthouse for browser pages and optional autocannon for Node HTTP.
4. Execute baseline and comparison runs with environment metadata and noise checks.
5. Correlate p95/p99, throughput, errors, CPU/memory/GC, DB spans and flamegraphs.
6. Classify regression with uncertainty; propose code/infra changes separately.
7. Re-run after approved change; publish reproducible evidence.

## Immediate backlog
- Define JSON schemas for `ProjectProfile`, `Capability`, `Task`, `Run`, `Evidence`, `Finding`, `ToolDescriptor`, `AgentDescriptor`, `PolicyDecision`.
- Rust workspace skeleton and smoke CLI.
- Four sample fixture repositories with seeded pass/fail cases.
- Integration matrix tests for CLI/MCP and OS.
- Security policy defaults and signed/version-pinned external tool install plan.
- Proof of concept for OpenCode + DeepSeek and Claude Code/Codex MCP, each with measured results.

## Explicit non-goals for MVP
- Claiming universal browser coverage without real test results.
- Reimplementing Playwright, k6, Semgrep, Serena or Graphify.
- Unlimited autonomous agents, unrestricted shell or network, production stress by default.
- Mandatory vector database, graph database, Kubernetes, dashboard or paid LLM.
