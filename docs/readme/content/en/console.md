Tests run from your terminal or your agent. To look back at them, `qualyx serve` gives you a small web console:

<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/console-en-dark.png" /><img src="assets/screenshots/console-en-light.png" alt="Qualyx console: eight runs of a project, six passed and two with failures, with the report link on each row" width="100%" /></picture></p>

- **History at a glance:** how many runs passed, failed or are still going, with search by run ID and a status filter.
- **The evidence, not a summary:** open any run's `report.md` exactly as Qualyx wrote it.
- **Light or dark, English or Spanish:** it follows your system and browser, and you can switch both.
- **Read-only on purpose:** it never starts or cancels anything, so leaving it open is harmless.

```sh
export QUALYX_API_TOKEN="$(openssl rand -hex 24)"   # 32+ printable characters
qualyx serve --root /path/to/projects               # then open http://127.0.0.1:7878
```

Enter a project path under `--root` and the token. The token lives only in that tab and is gone when you reload. The console is built into the `qualyx` binary, so there is nothing else to install or run: no Node.js, no separate server.
