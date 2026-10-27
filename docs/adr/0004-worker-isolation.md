# ADR 0004 — Worker isolation

- Status: accepted (2026-10-26)

## Decision
1. **M1:** local subprocess with a fixed command per ecosystem, args as a vector (no shell),
   working dir = project root, timeout with kill. Runs with the user's privileges (see threat model T1).
2. **M2:** environment allowlist for children, cancellation, and an optional container runner
   (Docker/Podman) that mounts the repo read-only plus a writable output dir.
3. **M3:** one git worktree per writing agent.

Host desktop automation can never run in a container (it needs a host companion, M4).
