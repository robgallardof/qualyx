- [Estado](docs/STATUS.md): verificaciones ejecutadas, evidencia y límites.
- [Planeación](docs/PLANNING.md) y [prompt maestro](docs/MASTER_PROMPT.md): arquitectura e hitos.
- [Matriz de integraciones](docs/m0/integration-matrix.md), [modelo de amenazas](docs/m0/threat-model.md) y [decisiones](docs/adr/).
- [CONTRIBUTING.md](CONTRIBUTING.md) y [AGENTS.md](AGENTS.md): reglas para contribuir.

Las pruebas completas necesitan Rust, .NET 10, Node 24 y Python con pytest. Las de navegadores y servicios tienen requisitos adicionales. Las pruebas de carga, DAST y los destinos de producción requieren permiso explícito.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

Este README usa [Readme Builder](https://github.com/robgallardof/readme-builder). Edita `readme.json` y `docs/readme/content/` y sigue las [instrucciones de regeneración](docs/readme/README.md).

Doble licencia: [MIT](LICENSE-MIT) o [Apache-2.0](LICENSE-APACHE), a tu elección. [Avisos de los recursos del README](docs/readme/THIRD-PARTY-NOTICES.md).
