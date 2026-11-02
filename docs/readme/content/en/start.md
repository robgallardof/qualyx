Qualyx discovers a project's tests, runs its native tools and preserves the evidence. Use it from a terminal, an MCP agent or a shared API, and look back at the results in a web console. **Deterministic execution needs no AI model.**

<!-- kit:why -->
<!-- /kit:why -->

Core workflows are verified within the [documented scope](docs/STATUS.md); the [full roadmap remains incomplete](docs/READINESS.md). CI validates Windows, Linux and macOS, including real browsers and installation smoke tests. For a downloaded binary, follow [native installation](INSTALL.md).

Install Git and stable Rust, plus the native toolchain for your project:

```sh
git clone https://github.com/robgallardof/qualyx.git
cd qualyx
cargo install --locked --path crates/qualyx-cli
qualyx doctor path/to/project
qualyx discover path/to/project
qualyx test path/to/project
qualyx report path/to/project
```

<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/readme/diagrams/first-check-en-dark.svg" /><img src="assets/readme/diagrams/first-check-en-light.svg" alt="Qualyx workflow: install, discover ecosystems, run native tools, keep the evidence, review the result" width="100%" /></picture></p>

For a small real example, install Python and pytest, then run `qualyx test fixtures/python/passing`. The companion `fixtures/python/failing` deliberately fails. Tests execute project code: use repositories you trust.

| Exit code | Meaning |
|---|---|
| `0` | Passed |
| `1` | At least one test failed |
| `2` | Nothing proven: an error, or an inconclusive result such as finding no tests |

An empty project is never green. Native archives and checksums are built by the [package workflow](https://github.com/robgallardof/qualyx/actions/workflows/release.yml); this does not imply a published release.
