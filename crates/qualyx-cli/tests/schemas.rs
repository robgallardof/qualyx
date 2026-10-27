//! The JSON Schemas in `schemas/` must match the Rust types. Regenerate with
//! `cargo run -p qualyx-cli -- schema --out schemas`.

use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn committed_schemas_are_up_to_date() {
    let out = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_qualyx"))
        .args(["schema", "--out"])
        .arg(out.path())
        .output()
        .unwrap()
        .status;
    assert!(status.success());

    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    for entry in fs::read_dir(out.path()).unwrap() {
        let entry = entry.unwrap();
        let fresh = fs::read_to_string(entry.path()).unwrap();
        let name = entry.file_name();
        let old = fs::read_to_string(committed.join(&name)).unwrap_or_else(|_| {
            panic!(
                "schemas/{} is missing; regenerate it",
                name.to_string_lossy()
            )
        });
        assert_eq!(
            old.replace("\r\n", "\n"),
            fresh,
            "schemas/{} is stale; regenerate it",
            name.to_string_lossy()
        );
    }
}
