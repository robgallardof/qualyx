# ADR 0006 — Durable team dispatch and evidence archive

Status: accepted for M7's trusted-project scope, 2026-10-27.

## Decision

Keep the modular monolith and one executable. `serve --team` only enqueues requests;
`worker` processes them separately. PostgreSQL is the run index and bounded work queue,
using transactional claims (`FOR UPDATE SKIP LOCKED`), heartbeat leases and owner-checked
completion. A job lost to an expired lease fails and is never requeued automatically.
A local API mode and the existing CLI/MCP paths remain available without a team deployment.

Use the Apache object_store client for signed S3 operations and SeaweedFS mini as the supplied
single-node S3 service. Archive manifests, reports and declared native artifacts under a
project digest and run ID. Read missing local evidence from S3, verifying native hashes.
Filesystem evidence remains available for local operation; PostgreSQL indexes its identity.

Compose runs API, workers, PostgreSQL and object storage separately. Only the API is published
on loopback; worker/backend services use a private internal network. The supplied non-root
worker image includes Python/pytest and is intentionally for trusted disposable project copies.
A worker container is not a hostile multi-tenant execution sandbox: it can reach backend
services and all project mounts assigned to it. See SECURITY.md.

## Alternatives and consequences

A broker such as Redis would add a fifth service without a second queue consumer type.
PostgreSQL already supplies the required durability and atomic claim primitive. A thread-only
API cannot survive process restarts or separate execution credentials; it remains local mode.

Automatic retries could duplicate external test side effects after an ambiguous worker crash.
Fail-closed expiry requires an explicit new run after review instead. Lease ownership protects
the recorded result, not exactly-once external effects. Cancellation is cooperative.

S3 storage avoids tying evidence availability to one worker filesystem. It adds dependencies
and credential administration; a configured archive failure therefore fails the job explicitly.
PostgreSQL TLS, remote hostile workers, per-user RBAC, signed evidence and high availability
remain out of scope. Release archives are checksummed and installation-tested; tagged releases
are drafts until a human reviews them.

## Evidence

`scripts/test_team.py`, `scripts/test_team_compose.py`, the PostgreSQL integration test and
`docs/STATUS.md`. Official references: [PostgreSQL locking clauses](https://www.postgresql.org/docs/current/sql-select.html#SQL-FOR-UPDATE-SHARE),
[object_store](https://github.com/apache/arrow-rs-object-store),
[SeaweedFS mini](https://github.com/seaweedfs/seaweedfs/wiki/Quick-Start-with-weed-mini).
