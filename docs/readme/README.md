# README authoring

English and Spanish sources live in `content/`; links are relative to the generated repository-root README. Edit sources, not generated READMEs or assets.

Use Python 3.11+ and [Readme Builder](https://github.com/robgallardof/readme-builder) revision `77d458bf46977e7bd7ac1da430c483838cc5e4e5`. This retains the showcase presentation adopted by the README. From that clone, with Qualyx checked out beside it:

```sh
python -m readme_builder build ../qualyx/readme.json --output ../qualyx
python -m readme_builder build ../qualyx/readme.json --output ../qualyx --check
python -m readme_builder check ../qualyx
python -m readme_builder preview ../qualyx --output ../qualyx/target/readme-preview.html
```

The ownership manifest detects manual drift. The initial migration used `--force` to adopt the existing READMEs; subsequent builds should not require it. Assets are static, with light/dark variants. Essential instructions remain normal Markdown. No external image rendering service is required.

## Archify workflows

Editable workflow specifications live under `assets/readme/diagrams/`:
`first-check-{en,es}.archify.json` and `code-mode-{en,es}.archify.json`.
The first check covers native discovery/execution/evidence. The Code Mode flow adds
progressive search, isolated composition, host authorization and asynchronous jobs.
Every node cites the source at the specification's pinned repository revision.

Use Archify 3.2.0 to regenerate each diagram from the repository root, replacing
`<archify>` with its installed `bin/archify.mjs` path and `<name>` with a stem above:

```sh
node <archify> finalize workflow assets/readme/diagrams/<name>.archify.json .archify/readme-20261102/<name>.html --repo-root . --quality showcase --json
```

Keep `meta.output` and the output argument aligned when choosing another local artifact
folder. `finalize` validates repository provenance, renders HTML, checks the artifact and
runs its real-browser layout gate. In the resulting viewer, use canonical `svg-light`
and `svg-dark` exports, saving them as `<name>-light.svg` and `<name>-dark.svg` beside
the source. Do not hand-edit exported SVGs. Regenerate both READMEs after updating their
sources or kit. Preserve the Archify and font license notices in that directory.
