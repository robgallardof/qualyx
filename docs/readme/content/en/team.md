Local CLI/MCP uses SQLite by default. Team mode adds an authenticated HTTP API, a read-only dashboard, PostgreSQL jobs, separate workers and optional S3-compatible evidence storage.

| Mode | Use it for | Needs |
|---|---|---|
| Local CLI / MCP | One checkout, local evidence | Native test tools |
| Local API | Up to four concurrent runs | Token and allowed project roots |
| Durable team API | Shared queue, independent workers | PostgreSQL; optional S3 |

### Review results in the console

`qualyx serve` also serves a read-only web console at `/`: run history, pass/fail counts, search and filters, and each run's report. It is built with Next.js and shadcn/ui, has light and dark themes and is available in English and Spanish. It is bundled inside the `qualyx` binary, so no Node.js server is needed. It never starts or cancels runs: your terminal or MCP agent does that.

```sh
export QUALYX_API_TOKEN="$(openssl rand -hex 24)"   # 32+ printable characters
qualyx serve --root /path/to/projects               # http://127.0.0.1:7878
```

Open `http://127.0.0.1:7878`, enter a project path under `--root` and the token. The token stays in that tab's memory only.

The [bilingual team guide](docs/m7/team.md) includes the complete Compose setup, credentials, Python SDK, cancellation, backups and recovery. Start there before deploying `deploy/team/compose.yaml`.

Each worker processes one job at a time. The queue holds at most 64 queued/running jobs. Expired worker leases fail jobs rather than automatically repeating external side effects. A finished job is not necessarily a passing test: inspect `manifest.status`.

**Deployment boundary:** trusted project copies only. The supplied image includes Python/pytest; other toolchains need a custom image. No per-user RBAC, hostile multi-tenant isolation, remote PostgreSQL TLS, HA, signed releases or formal SBOM is claimed. Read [SECURITY.md](SECURITY.md) before remote exposure.
