//! Deterministic ecosystem discovery from files at the project root.
//!
//! M1 scope: only the root directory is inspected (no monorepo walking). Each detector is a
//! plain function `fn(&Path) -> Option<DetectedEcosystem>` so adding one is local.

use std::fs;
use std::path::Path;

use qualyx_core::{DetectedEcosystem, Ecosystem, ProjectProfile, TestTool};

type Detector = fn(&Path) -> Option<DetectedEcosystem>;

const DETECTORS: &[Detector] = &[detect_rust, detect_dotnet, detect_node, detect_python];

/// The script `npm init` writes when a package has no tests.
const NPM_PLACEHOLDER_TEST: &str = "no test specified";

pub fn discover(root: &Path) -> std::io::Result<ProjectProfile> {
    let root = root.canonicalize()?;
    let ecosystems = DETECTORS
        .iter()
        .filter_map(|detect| detect(&root))
        .collect();
    Ok(ProjectProfile {
        root: strip_verbatim(&root),
        ecosystems,
    })
}

/// Windows `canonicalize` returns `\\?\C:\...`, which many tools reject as a working dir.
fn strip_verbatim(p: &Path) -> std::path::PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => rest.into(),
        _ => p.to_path_buf(),
    }
}

fn detected(ecosystem: Ecosystem, tool: TestTool, manifest: &str) -> DetectedEcosystem {
    DetectedEcosystem {
        ecosystem,
        tool,
        manifest: manifest.to_string(),
        target: None,
        notes: Vec::new(),
        packages: Vec::new(),
    }
}

fn detect_rust(root: &Path) -> Option<DetectedEcosystem> {
    root.join("Cargo.toml")
        .is_file()
        .then(|| detected(Ecosystem::Rust, TestTool::CargoTest, "Cargo.toml"))
}

fn detect_dotnet(root: &Path) -> Option<DetectedEcosystem> {
    let mut solutions = files_with_ext(root, &["sln", "slnx"]);
    let mut projects = files_with_ext(root, &["csproj", "fsproj", "vbproj"]);
    // Prefer a solution: it covers every project. `dotnet test` refuses to guess between
    // several, so Qualyx passes the target explicitly and notes the ambiguity.
    let (pool, kind) = if !solutions.is_empty() {
        (&mut solutions, "solution")
    } else if !projects.is_empty() {
        (&mut projects, "project")
    } else {
        return None;
    };
    pool.sort();
    let chosen = pool[0].clone();
    let mut d = detected(Ecosystem::Dotnet, TestTool::DotnetTest, &chosen);
    d.target = Some(chosen.clone());
    if pool.len() > 1 {
        d.notes.push(format!(
            "{} {kind} files at the root; using {chosen}",
            pool.len()
        ));
    }
    Some(d)
}

fn detect_node(root: &Path) -> Option<DetectedEcosystem> {
    let text = fs::read_to_string(root.join("package.json")).ok()?;
    let mut d = detected(Ecosystem::Node, TestTool::NpmScript, "package.json");
    let json: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            d.notes.push(format!("package.json is not valid JSON: {e}"));
            return Some(d);
        }
    };
    let script = json.pointer("/scripts/test").and_then(|v| v.as_str());
    match script {
        None => return None,
        Some(s) if s.contains(NPM_PLACEHOLDER_TEST) => return None,
        Some(s) if is_node_test_runner(s) => d.tool = TestTool::NodeTest,
        Some(s) => d.notes.push(format!(
            "test script `{s}` uses a runner without a verified adapter; only its exit code is trusted"
        )),
    }
    Some(d)
}

/// True when the script is exactly a `node --test` invocation, so reporter flags can be appended.
fn is_node_test_runner(script: &str) -> bool {
    let mut words = script.split_whitespace();
    words.next() == Some("node")
        && words.next() == Some("--test")
        && !script.contains("&&")
        && !script.contains(';')
        && !script.contains('|')
}

fn detect_python(root: &Path) -> Option<DetectedEcosystem> {
    const CONFIGS: &[&str] = &[
        "pytest.ini",
        "pyproject.toml",
        "setup.cfg",
        "tox.ini",
        "conftest.py",
    ];
    if let Some(cfg) = CONFIGS.iter().find(|f| root.join(f).is_file()) {
        // pyproject/setup.cfg/tox.ini exist in non-test projects too; require some pytest signal.
        let has_tests_dir = root.join("tests").is_dir() || root.join("test").is_dir();
        let mentions_pytest = fs::read_to_string(root.join(cfg))
            .map(|t| t.contains("pytest"))
            .unwrap_or(false);
        if *cfg == "pytest.ini" || *cfg == "conftest.py" || mentions_pytest || has_tests_dir {
            return Some(detected(Ecosystem::Python, TestTool::Pytest, cfg));
        }
    }
    None
}

fn files_with_ext(root: &Path, exts: &[&str]) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let ext = Path::new(&name)
                .extension()?
                .to_string_lossy()
                .to_lowercase();
            exts.contains(&ext.as_str()).then_some(name)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, body) in files {
            let p = dir.path().join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
        dir
    }

    fn ecosystems(dir: &tempfile::TempDir) -> Vec<DetectedEcosystem> {
        discover(dir.path()).unwrap().ecosystems
    }

    #[test]
    fn empty_directory_has_no_ecosystems() {
        assert!(ecosystems(&project(&[])).is_empty());
    }

    #[test]
    fn detects_rust() {
        let e = ecosystems(&project(&[("Cargo.toml", "[package]")]));
        assert_eq!(e[0].ecosystem, Ecosystem::Rust);
        assert_eq!(e[0].tool, TestTool::CargoTest);
    }

    #[test]
    fn dotnet_prefers_solution_and_notes_ambiguity() {
        let e = ecosystems(&project(&[
            ("B.sln", ""),
            ("A.sln", ""),
            ("App.csproj", ""),
        ]));
        assert_eq!(e[0].target.as_deref(), Some("A.sln"));
        assert_eq!(e[0].notes.len(), 1);
    }

    #[test]
    fn dotnet_single_project() {
        let e = ecosystems(&project(&[("Tests.csproj", "")]));
        assert_eq!(e[0].target.as_deref(), Some("Tests.csproj"));
        assert!(e[0].notes.is_empty());
    }

    #[test]
    fn node_test_runner_gets_per_test_adapter() {
        let e = ecosystems(&project(&[(
            "package.json",
            r#"{"scripts":{"test":"node --test"}}"#,
        )]));
        assert_eq!(e[0].tool, TestTool::NodeTest);
    }

    #[test]
    fn unknown_node_runner_is_exit_code_only() {
        let e = ecosystems(&project(&[(
            "package.json",
            r#"{"scripts":{"test":"jest --ci"}}"#,
        )]));
        assert_eq!(e[0].tool, TestTool::NpmScript);
        assert!(e[0].notes[0].contains("exit code"));
    }

    #[test]
    fn chained_node_test_is_not_extended() {
        let e = ecosystems(&project(&[(
            "package.json",
            r#"{"scripts":{"test":"node --test && echo done"}}"#,
        )]));
        assert_eq!(e[0].tool, TestTool::NpmScript);
    }

    #[test]
    fn npm_placeholder_script_is_ignored() {
        let e = ecosystems(&project(&[(
            "package.json",
            r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#,
        )]));
        assert!(e.is_empty());
    }

    #[test]
    fn package_without_test_script_is_ignored() {
        assert!(ecosystems(&project(&[("package.json", r#"{"name":"x"}"#)])).is_empty());
    }

    #[test]
    fn python_needs_a_pytest_signal() {
        assert!(ecosystems(&project(&[("pyproject.toml", "[project]\nname='x'")])).is_empty());
        let e = ecosystems(&project(&[
            ("pyproject.toml", "[project]\nname='x'"),
            ("tests/test_a.py", ""),
        ]));
        assert_eq!(e[0].ecosystem, Ecosystem::Python);
        let e = ecosystems(&project(&[("pytest.ini", "")]));
        assert_eq!(e[0].manifest, "pytest.ini");
    }

    #[test]
    fn polyglot_project_reports_all() {
        let e = ecosystems(&project(&[
            ("Cargo.toml", ""),
            ("package.json", r#"{"scripts":{"test":"node --test"}}"#),
        ]));
        let kinds: Vec<_> = e.iter().map(|d| d.ecosystem).collect();
        assert_eq!(kinds, vec![Ecosystem::Rust, Ecosystem::Node]);
    }
}
