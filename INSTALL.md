# Use a native Qualyx package / Usar un paquete nativo

Extract the archive for your operating system and architecture. Compare its SHA-256 with
the adjacent `.sha256` file. Keep the extracted directory together: it includes the binary,
guides, README images, Python SDK and security audit skill.

From the extracted folder:

```sh
./qualyx --version
./qualyx doctor /absolute/path/to/your/project
./qualyx discover /absolute/path/to/your/project
./qualyx test /absolute/path/to/your/project
./qualyx report /absolute/path/to/your/project
```

On Windows PowerShell use `./qualyx.exe` instead. You can add the extracted directory to
your PATH if desired. Rust is not required to run the binary; the project under test still
needs its native test tools (for example Python/pytest or .NET). `doctor` shows availability.

Exit codes: 0 passed, 1 test failures, 2 unproven. Tests execute the project's code: use a
trusted checkout. Run evidence is stored under that project's `.qualyx/`, not beside the binary.

The packaged [README](README.md), [team guide](docs/m7/team.md), [security policy](SECURITY.md)
and [audit skill](.agents/skills/qualyx-security-audit/SKILL.md) are available locally.
Install the optional Python SDK from this directory with `python -m pip install ./sdk/python`.
The SDK requires Python 3.10+ and refuses redirects on authenticated requests.

Source-build, Compose-build, contributor and fixture commands in the guides require a
source checkout. This archive is a native installation, not the full build workspace.
Get the source from [the repository](https://github.com/robgallardof/qualyx).

## En español

Extrae el paquete de tu sistema y arquitectura y compara su SHA-256. Conserva la carpeta
completa para mantener las imágenes y las guías disponibles. Ejecuta los comandos anteriores
desde esa carpeta; en Windows usa `./qualyx.exe`.

No necesitas Rust para ejecutar el binario, pero sí las herramientas del proyecto que vas
a probar. `doctor` indica cuáles están disponibles. Los códigos 0, 1 y 2 significan aprobado,
fallos y resultado sin demostrar. Las instrucciones para compilar Qualyx, construir Compose
o ejecutar los ejemplos del repositorio requieren descargar el código fuente.
