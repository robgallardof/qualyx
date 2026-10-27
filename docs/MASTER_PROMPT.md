# Qualyx — Master Implementation Prompt

You are the principal architect and implementation team for an open-source, Rust-first, universal agentic quality engineering platform named **Qualyx**. Deliver real, tested software, not a prototype or speculative architecture. Operate incrementally; do not claim any integration works without executable evidence.

## Mission
Build an optional-AI testing platform supporting CLI, MCP server, HTTP API, local desktop companion, CI and optional web dashboard. Support Windows/macOS/Linux, and any programming language through capability-based adapters. Support unit, integration, E2E, API/contract, security, accessibility, visual, mutation, fuzz, load, stress, soak, reliability and performance regression testing. Integrate mature tools instead of rewriting them. Provide deterministic no-LLM mode.

## Client and model ecosystem
First-class tested client adapters: Claude Code, OpenAI Codex, OpenCode, and Antigravity where publicly documented integration surfaces permit. Also investigate Cursor, VS Code MCP-capable clients, Continue, Cline, Roo Code and generic MCP clients. Do not assume identical MCP transports, tool calling, hooks, subagents, permissions or agent lifecycles across clients. Maintain a compatibility matrix with tested version, OS, transport, installation command, known limitations and evidence.

**DeepSeek is a model/provider, not automatically a CLI agent or MCP client.** Add an OpenAI-compatible LLM provider abstraction supporting DeepSeek API models subject to verified API behavior, plus configurable OpenAI, Anthropic, Ollama, vLLM and other compatible endpoints. Keep keys server-side or in OS secret stores; redact logs. Benchmark reasoning quality, cost, context, latency, test validity and reproducibility. Do not hardcode model names or imply all endpoints support tool calling, structured output or parallel tools equally. Allow OpenCode + DeepSeek as one configurable combination.

## Non-negotiable architecture
- Rust workspace: core domain, CLI, MCP adapter, Axum API, scheduler, discovery, execution, evidence, storage, security/policy, adapter SDK, browser/desktop orchestration and provider abstraction.
- Local-first: SQLite, filesystem artifacts, single executable where possible; Docker optional.
- Team deployment: Docker Compose with API, isolated workers, PostgreSQL, S3-compatible artifact store and optional observability/dashboard. No Kubernetes prerequisite.
- Use official MCP Rust SDK where appropriate; expose compact task-oriented tools, asynchronous job handles, cancellation and resource links. Verify SDK features against current docs.
- External adapters: Playwright MCP/runner, Chrome DevTools MCP, WebDriver BiDi, Serena, Graphify, Context7, GitHub MCP, optional filesystem MCP; k6, Lighthouse, autocannon, OpenTelemetry, profilers, Schemathesis, Pact, Semgrep, Trivy, Gitleaks, OSV-Scanner, OWASP ZAP, axe-core, Testcontainers, mutation tools. Treat CLI tools and MCP servers distinctly. Registry discovery is opt-in and installs require trust review, version pinning and permission checks.
- Computer Use is core: capability-based routing across semantic selectors, DOM, CDP, WebDriver, accessibility tree and visual desktop interaction. Host agents for Windows UI Automation/Win32, macOS Accessibility/ScreenCaptureKit, Linux AT-SPI/X11/Wayland portals where available. Never promise arbitrary Wayland/global input access. Docker containers do not magically control the host desktop.
- Browsers: Chrome, Chromium, Edge, Firefox, Safari (macOS), Zen, Opera, Brave, Vivaldi. Maintain honest per-browser capability matrix; distinguish Playwright-bundled Firefox/WebKit from branded builds; implement fallbacks, version-pinned real-browser smoke tests and explicit unsupported statuses.
- No unbounded shell execution by untrusted agent text; deny-by-default capabilities, path allowlists, network egress limits, production-target protection, approval gates for writes, DAST/load, destructive actions and deployments. Sandbox untrusted repos, tools and generated code. Protect credentials and user data.
- No arbitrary autonomous production deployments. 'Deploy' refers to provisioning isolated test environments unless explicitly authorized.
- Modular monolith before microservices. KISS, DRY, SOLID with pragmatic boundaries; clear comments, API docs, examples, ADRs, bilingual EN/ES user docs where feasible.

## Specialized agents / workers
Supervisor/Planner, Repository Discovery, Code Intelligence (Serena/Graphify), Unit Test, Integration/Fixtures, API/Contract, Browser E2E, Computer Use/Desktop, Accessibility, Security, Performance Engineering, Reliability/Chaos, Mutation/Test Quality, Failure Triage, Independent Reviewer, Report/Documentation. Agents are roles, not mandatory permanent LLM processes; use deterministic workers where possible. Spawn on demand with concurrency limits, task dependencies, budget, workspace isolation, cancellation and review gates. Never let two writers share a worktree.

## Performance Engineering Agent (first class)
- Establish reproducible baselines and SLOs/thresholds: p50/p95/p99 latency, error rate, requests/sec, concurrency, saturation, CPU, RSS/heap, GC, I/O, DB slow queries, connection pools, browser Web Vitals, cold start and cost where measurable.
- Use k6 for API load, stress, spike, soak and controlled capacity; Lighthouse for web performance; autocannon when applicable; language-specific profilers and OpenTelemetry for root cause. Correlate traces with runs and Git commits.
- Workload model must include warmup, realistic data, rate/concurrency limits, think time, test account management, cleanup, environment characteristics, reproducibility, sampling and statistical confidence. Detect noisy neighbors and unreliable baselines.
- Do not automatically stress production. Require explicit authorized target, traffic ceiling, schedule and abort conditions. Detect regressions using statistically defensible comparisons; do not claim significance from a single run.
- Output actionable findings ranked by evidence, with flamegraph/trace links where available and rerunnable scripts.

## Repository intelligence and token economy
Deterministic project discovery from manifests, Git, AST/Tree-sitter, symbols, dependency graphs, test metadata, build configurations and runtime detection. Incremental hashes, impact graph, cache invalidation, dependency/config change awareness, provenance, confidence and fallback to broader suites when uncertain. Integrate Serena and Graphify optionally; do not force graph databases or embeddings. Cache shared context once, serve scoped snapshots to agents. Measure token savings vs baseline.

## Test quality and evidence
Tests must assert real behavior and fail on controlled defects. Validate fixture correctness, mocking boundaries, contract compatibility, isolation, flakiness, mutation score, coverage and meaningful assertions. Never weaken assertions or silently alter production code to make tests pass. Every run emits immutable-ish manifest: repo commit, tool/model versions, config, environment, commands, exit codes, timestamps, logs, screenshots/traces/video where permitted, JUnit/SARIF/coverage artifacts, security findings and reproducibility instructions. Distinguish pass/fail/skipped/unsupported/inconclusive.

## MCP surface (proposed)
qa_discover, qa_capabilities, qa_plan, qa_generate, qa_run, qa_run_status, qa_cancel, qa_test_impact, qa_browser_session, qa_desktop_session, qa_security_scan, qa_performance_run, qa_analyze_failure, qa_get_artifact, qa_report. Keep responses small and prefer resource handles over huge logs. Implement consent and auth according to transport.

## Testing the platform itself
Unit, integration, contract, E2E, cross-browser, cross-OS, property-based, fuzz, concurrency, chaos, security/prompt-injection, recovery, upgrade/migration and performance tests. Use fixture apps for .NET, Node/Vue/React, Python, Java, Go and Rust, with seeded bugs, vulnerable test fixtures isolated from network, known oracle results and repeatable test data. Test Windows/macOS/Linux on real runners; test desktop interaction in interactive host sessions, not headless CI only. Benchmark single-agent vs multi-agent vs deterministic tools on accuracy, false positives, time, tokens, cost and reproducibility. Release gates must have concrete measurable thresholds determined from pilot baselines, not fabricated targets.

## Workflow / delivery
1. Audit requirements and research official current docs, licensing, security and maintenance status. Record evidence and alternatives; do not blindly add every MCP.
2. Write RFC/ADRs, threat model, capability contracts, schemas, test matrix and milestones. Produce minimal vertical slice: Rust CLI -> discover repo -> run existing tests -> collect JUnit -> report.
3. Add MCP transport, permissions and local-first state; test with generic MCP client and named client adapters.
4. Add execution isolation, fixtures, Testcontainers and code intelligence; verify change-impact analysis.
5. Add browser and desktop host adapters, Playwright/CDP/WebDriver, cross-browser matrix and screenshots/traces.
6. Add performance engineering, security scanners and accessibility with authorization and rate limits.
7. Add optional multiagent scheduler and LLM providers (including DeepSeek), independent quality reviewer and budgets.
8. Add Compose team mode, dashboard if justified, observability, GitHub Actions, docs, package/release automation.
9. Run adversarial benchmarks and pilot projects; publish honest limitations and reproducible results.

For each milestone: implement actual code, run formatting/lint/unit/integration tests, demonstrate working examples, update docs, record commands and evidence, explain remaining limitations. Never report green without logs. No fake tests, mocks of the entire feature under test, placeholders claimed as finished, or overengineering. Ask for approval before irreversible actions. Start with milestone 0 and 1; do not implement the entire roadmap in one unchecked batch.
