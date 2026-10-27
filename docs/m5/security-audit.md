# Security audit workflow / Flujo de auditoría

Qualyx already runs Gitleaks, Semgrep, Trivy and OSV-Scanner and merges SARIF. This increment
adds explicit coverage, scanner execution receipts and a Markdown handoff for source review.
It takes inspiration from Codex Security's separation of candidate triage and validation;
it does not install, invoke or claim certification from that plugin.

## Run and interpret

```sh
qualyx security path/to/project --json
```

The existing CLI and `qa_security_scan` MCP tool share the implementation. Evidence now
includes `security-audit.md`, `security-report.json`, individual SARIF/log files and merged SARIF.
The Markdown report labels findings as candidates; automatic scanner results do not establish
reachability, exploitability or business impact. It preserves reviewed suppression reasons.

| Result | Meaning | CLI exit |
|---|---|---|
| `clean`, `coverage_complete: true` | All selected scanners completed; no unsuppressed findings | 0 |
| `findings`, `coverage_complete: true` | All selected scanners completed; candidates need review | 1 |
| `inconclusive`, `coverage_complete: false` | At least one requested scanner missing, failed, timed out or emitted unusable evidence | 2 |

Incomplete runs retain any usable partial findings. Suppressing all candidates cannot turn
incomplete coverage green. Each scanner records its version, exit code, timeout and notes.
Stale per-scanner SARIF is removed before a rerun. Arbitrary JSON, missing result sets and
failed SARIF invocations cannot establish a clean scan. OSV's finding exit code 1 is accepted;
other unexpected process exits are incomplete even if they left a report.

Coverage means completion of the **selected** tools with their rules and exclusions. It does
not mean every file/language/security class was analyzed. Semgrep can use project rules or a
network registry; dependency scanners may require vulnerability database downloads. These
tools do not replace manual authorization, business-logic or deployment review.

## Reusable skill

The repository includes [qualyx-security-audit](../../.agents/skills/qualyx-security-audit/SKILL.md).
Clients supporting repository `.agents/skills` can discover it; other clients can load the
file explicitly. No global agent configuration is edited. The skill guides boundary analysis,
candidate validation, counterevidence, reproducible tests and coverage reporting. It is guidance
for a client agent, not executable instructions automatically run by the Qualyx binary.

Validation uses the existing real-scanner CI job plus regressions for malformed SARIF, partial
coverage after suppressions, invalid Semgrep rules with stale evidence, timeouts and redaction.
See [STATUS](../STATUS.md) for which checks actually ran.

## En español

El análisis ahora diferencia **sin hallazgos**, **hallazgos por revisar** y **cobertura
incompleta**. Si falta o falla un escáner solicitado, sale con código 2 aunque los demás no
encuentren problemas. Conserva los hallazgos parciales y las exclusiones revisadas con su motivo.
Cada ejecución genera `security-audit.md` con el resultado de las herramientas y los candidatos.

La skill del repositorio guía la revisión del código, las fronteras de confianza, las
precondiciones y la reproducción del problema. Un hallazgo del escáner no se presenta como
vulnerabilidad confirmada sin evidencia. Un resultado limpio no certifica toda la aplicación.
La skill no instala herramientas ni cambia la configuración global del agente.
