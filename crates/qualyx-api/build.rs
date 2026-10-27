//! Embed the prebuilt dashboard (`dashboard/`, produced by `web/dashboard`) as a static table.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        if path.is_dir() {
            walk(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("dashboard");
    println!("cargo:rerun-if-changed={}", root.display());
    assert!(
        root.join("index.html").is_file(),
        "dashboard/index.html is missing; run `npm run build` in web/dashboard"
    );
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut code = String::from("pub(crate) static ASSETS: &[(&str, &[u8])] = &[\n");
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        writeln!(
            code,
            "    ({rel:?}, include_bytes!({:?})),",
            file.display().to_string()
        )
        .unwrap();
    }
    code.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("dashboard_assets.rs");
    fs::write(out, code).unwrap();
}
