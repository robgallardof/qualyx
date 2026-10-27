# README authoring

English and Spanish sources live in `content/`; links are relative to the generated repository-root README. Edit sources, not generated READMEs or assets.

Use Python 3.11+ and [Readme Builder](https://github.com/robgallardof/readme-builder) revision `ac565123d25081b2bb8b1158f387b5fb43323cb6`. This retains the showcase presentation adopted by the README. From that clone, with Qualyx checked out beside it:

```sh
python -m readme_builder build ../qualyx/readme.json --output ../qualyx
python -m readme_builder build ../qualyx/readme.json --output ../qualyx --check
python -m readme_builder check ../qualyx
python -m readme_builder preview ../qualyx --output ../qualyx/target/readme-preview.html
```

The ownership manifest detects manual drift. The initial migration used `--force` to adopt the existing READMEs; subsequent builds should not require it. Assets are static, with light/dark variants. Essential instructions remain normal Markdown. No external image rendering service is required.
