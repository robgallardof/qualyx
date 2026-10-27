//! Change-impact analysis: which tests must run for a set of changed files.
//!
//! Deterministic and conservative. A change maps to the narrowest scope Qualyx can *prove*
//! (Rust: owning packages plus every workspace package that depends on them). Anything it
//! can't map falls back to the whole ecosystem, with the reason recorded.
//!
//! Changes come from git (`base...HEAD` plus the working tree) or, outside git, from a
//! content-hash index stored in `.qualyx/index.json`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use qualyx_core::{DetectedEcosystem, Ecosystem, ProjectProfile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Files that never change test outcomes.
fn is_inert(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    lower.ends_with(".md")
        || lower.ends_with(".txt") && !name.starts_with("requirements")
        || lower.starts_with("docs/")
        || lower.starts_with(".github/")
        || lower.starts_with(".qualyx/")
        || name.starts_with("license")
        || name == ".gitignore"
        || name == ".gitattributes"
}

fn ecosystem_extensions(e: Ecosystem) -> &'static [&'static str] {
    match e {
        Ecosystem::Rust => &["rs", "toml", "lock"],
        Ecosystem::Dotnet => &[
            "cs", "fs", "vb", "csproj", "fsproj", "vbproj", "sln", "slnx", "props", "targets",
            "json", "resx", "config",
        ],
        Ecosystem::Node => &[
            "js", "mjs", "cjs", "ts", "mts", "cts", "jsx", "tsx", "json", "vue", "svelte", "snap",
        ],
        Ecosystem::Python => &[
            "py", "pyi", "toml", "cfg", "ini", "txt", "json", "yaml", "yml",
        ],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    /// Nothing changed that this ecosystem's tests depend on.
    Skip,
    /// The whole ecosystem must run.
    All,
    /// Only these packages (and they already include their dependents).
    Packages(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcosystemImpact {
    pub ecosystem: Ecosystem,
    pub scope: Scope,
    /// `exact` when every changed file was mapped, `fallback` when Qualyx widened the scope.
    pub confidence: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactPlan {
    pub source: String,
    pub changed_files: Vec<String>,
    pub ignored_files: Vec<String>,
    pub ecosystems: Vec<EcosystemImpact>,
}

impl ImpactPlan {
    /// The profile restricted to what must run.
    pub fn apply(&self, profile: &ProjectProfile) -> ProjectProfile {
        let ecosystems = profile
            .ecosystems
            .iter()
            .filter_map(|d| {
                let imp = self
                    .ecosystems
                    .iter()
                    .find(|i| i.ecosystem == d.ecosystem)?;
                match &imp.scope {
                    Scope::Skip => None,
                    Scope::All => Some(d.clone()),
                    Scope::Packages(p) => Some(DetectedEcosystem {
                        packages: p.clone(),
                        ..d.clone()
                    }),
                }
            })
            .collect();
        ProjectProfile {
            root: profile.root.clone(),
            ecosystems,
        }
    }
}

// ---------------------------------------------------------------- change sources

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let git = qualyx_runners::process::resolve(&["git"])?;
    let out = std::process::Command::new(git)
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Path of `root` relative to the git toplevel, so git paths can be made project-relative.
fn git_prefix(root: &Path) -> Option<String> {
    Some(
        git(root, &["rev-parse", "--show-prefix"])?
            .trim()
            .to_string(),
    )
}

/// Files changed between `base` and the working tree, relative to `root`.
pub fn changed_since_git(root: &Path, base: &str) -> Result<Vec<String>, String> {
    if base.starts_with('-') {
        return Err(format!("invalid base `{base}`"));
    }
    let prefix = git_prefix(root).ok_or("not a git repository")?;
    git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ],
    )
    .ok_or_else(|| format!("unknown git revision `{base}`"))?;
    let mut files = BTreeSet::new();
    let committed = git(
        root,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            &format!("{base}...HEAD"),
        ],
    )
    .unwrap_or_default();
    let worktree = git(root, &["diff", "--name-only", "--no-renames", "HEAD"]).unwrap_or_default();
    let untracked = git(
        root,
        &["ls-files", "--others", "--exclude-standard", "--full-name"],
    )
    .unwrap_or_default();
    for line in committed
        .lines()
        .chain(worktree.lines())
        .chain(untracked.lines())
    {
        if let Some(rel) = line.trim().strip_prefix(prefix.as_str())
            && !rel.is_empty()
        {
            files.insert(rel.to_string());
        }
    }
    Ok(files.into_iter().collect())
}

const INDEX_SKIP: &[&str] = &[
    ".git",
    ".qualyx",
    "target",
    "node_modules",
    "bin",
    "obj",
    "__pycache__",
    ".pytest_cache",
    ".venv",
    "venv",
];

/// Content hashes of every source file under `root` (relative, forward slashes).
pub fn snapshot(root: &Path) -> std::io::Result<BTreeMap<String, String>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> std::io::Result<()> {
        for e in fs::read_dir(dir)? {
            let e = e?;
            let ty = e.file_type()?;
            let name = e.file_name().to_string_lossy().into_owned();
            if ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                if !INDEX_SKIP.contains(&name.as_str()) {
                    walk(root, &e.path(), out)?;
                }
            } else {
                let rel = e
                    .path()
                    .strip_prefix(root)
                    .unwrap_or(&e.path())
                    .to_string_lossy()
                    .replace('\\', "/");
                let hash: String = Sha256::digest(fs::read(e.path())?)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                out.insert(rel, hash);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

fn index_path(root: &Path) -> PathBuf {
    root.join(".qualyx").join("index.json")
}

/// Store the current snapshot as the baseline for `changed_since_index`.
pub fn write_index(root: &Path) -> std::io::Result<usize> {
    let snap = snapshot(root)?;
    fs::create_dir_all(root.join(".qualyx"))?;
    fs::write(index_path(root), serde_json::to_vec_pretty(&snap)?)?;
    Ok(snap.len())
}

/// Files added, removed or modified since the last `write_index`.
pub fn changed_since_index(root: &Path) -> Result<Vec<String>, String> {
    let old: BTreeMap<String, String> = fs::read(index_path(root))
        .map_err(|_| "no index yet (run `qualyx index` first)".to_string())
        .and_then(|b| serde_json::from_slice(&b).map_err(|e| format!("corrupt index: {e}")))?;
    let new = snapshot(root).map_err(|e| e.to_string())?;
    let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    Ok(keys
        .into_iter()
        .filter(|k| old.get(*k) != new.get(*k))
        .cloned()
        .collect())
}

// ---------------------------------------------------------------- Rust workspace graph

#[derive(Debug, Clone)]
struct Package {
    name: String,
    /// Directory relative to the project root, forward slashes, no trailing slash.
    dir: String,
    /// Names of workspace packages this one depends on (any kind: normal, dev, build).
    deps: Vec<String>,
}

fn cargo_packages(root: &Path) -> Result<Vec<Package>, String> {
    let cargo = qualyx_runners::process::resolve(&["cargo"]).ok_or("cargo not found")?;
    let out = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("")
        ));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let root_canon = root.canonicalize().map_err(|e| e.to_string())?;
    let rel = |p: &str| -> String {
        let p = Path::new(p);
        let dir = p.parent().unwrap_or(p);
        let dir = dir.canonicalize().unwrap_or(dir.to_path_buf());
        dir.strip_prefix(&root_canon)
            .map(|d| d.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default()
    };
    let packages = meta["packages"].as_array().cloned().unwrap_or_default();
    let names: BTreeSet<String> = packages
        .iter()
        .filter_map(|p| p["name"].as_str().map(String::from))
        .collect();
    Ok(packages
        .iter()
        .map(|p| Package {
            name: p["name"].as_str().unwrap_or_default().to_string(),
            dir: rel(p["manifest_path"].as_str().unwrap_or_default()),
            deps: p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| d["name"].as_str())
                .filter(|n| names.contains(*n))
                .map(String::from)
                .collect(),
        })
        .collect())
}

/// Owning package of a file: the package whose directory is the longest prefix.
fn owner<'a>(packages: &'a [Package], file: &str) -> Option<&'a Package> {
    packages
        .iter()
        .filter(|p| p.dir.is_empty() || file == p.dir || file.starts_with(&format!("{}/", p.dir)))
        .max_by_key(|p| p.dir.len())
}

/// `seeds` plus every package that (transitively) depends on one of them.
fn with_dependents(packages: &[Package], seeds: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = seeds.clone();
    let mut queue: VecDeque<String> = seeds.iter().cloned().collect();
    while let Some(n) = queue.pop_front() {
        for p in packages.iter().filter(|p| p.deps.contains(&n)) {
            if out.insert(p.name.clone()) {
                queue.push_back(p.name.clone());
            }
        }
    }
    out
}

/// Files whose change invalidates the whole Rust build.
fn rust_global(file: &str) -> bool {
    matches!(
        file,
        "Cargo.toml" | "Cargo.lock" | "rust-toolchain" | "rust-toolchain.toml" | "build.rs"
    ) || file.starts_with(".cargo/")
}

fn rust_impact(root: &Path, files: &[&str]) -> EcosystemImpact {
    let mut reasons = Vec::new();
    let relevant: Vec<&str> = files
        .iter()
        .copied()
        .filter(|f| has_ext(f, Ecosystem::Rust) || !f.contains('.'))
        .collect();
    if relevant.is_empty() {
        return impact(
            Ecosystem::Rust,
            Scope::Skip,
            "exact",
            vec!["no Rust sources or manifests changed".into()],
        );
    }
    if let Some(g) = relevant.iter().find(|f| rust_global(f)) {
        return impact(
            Ecosystem::Rust,
            Scope::All,
            "fallback",
            vec![format!("{g} affects every package")],
        );
    }
    let packages = match cargo_packages(root) {
        Ok(p) => p,
        Err(e) => {
            return impact(
                Ecosystem::Rust,
                Scope::All,
                "fallback",
                vec![format!("package graph unavailable ({e})")],
            );
        }
    };
    let mut seeds = BTreeSet::new();
    for f in &relevant {
        match owner(&packages, f) {
            Some(p) => {
                reasons.push(format!("{f} -> {}", p.name));
                seeds.insert(p.name.clone());
            }
            None => {
                return impact(
                    Ecosystem::Rust,
                    Scope::All,
                    "fallback",
                    vec![format!("{f} belongs to no workspace package")],
                );
            }
        }
    }
    let all = with_dependents(&packages, &seeds);
    let added: Vec<&String> = all.difference(&seeds).collect();
    if !added.is_empty() {
        reasons.push(format!(
            "dependents: {}",
            added
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if all.len() == packages.len() {
        return impact(Ecosystem::Rust, Scope::All, "exact", reasons);
    }
    impact(
        Ecosystem::Rust,
        Scope::Packages(all.into_iter().collect()),
        "exact",
        reasons,
    )
}

fn has_ext(file: &str, e: Ecosystem) -> bool {
    let ext = file
        .rsplit('/')
        .next()
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, x)| x.to_ascii_lowercase());
    ext.is_some_and(|x| ecosystem_extensions(e).contains(&x.as_str()))
}

fn impact(
    ecosystem: Ecosystem,
    scope: Scope,
    confidence: &str,
    reasons: Vec<String>,
) -> EcosystemImpact {
    EcosystemImpact {
        ecosystem,
        scope,
        confidence: confidence.into(),
        reasons,
    }
}

/// Map changed files (relative to the project root) to what each ecosystem must run.
pub fn analyze(profile: &ProjectProfile, changed: Vec<String>, source: &str) -> ImpactPlan {
    let (ignored, files): (Vec<String>, Vec<String>) =
        changed.into_iter().partition(|f| is_inert(f));
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let ecosystems = profile
        .ecosystems
        .iter()
        .map(|d| match d.ecosystem {
            Ecosystem::Rust => rust_impact(&profile.root, &refs),
            e => {
                // No package graph yet for these ecosystems: any relevant change runs them all.
                let hits: Vec<&&str> = refs.iter().filter(|f| has_ext(f, e)).collect();
                let unknown: Vec<&&str> = refs.iter().filter(|f| !f.contains('.')).collect();
                if hits.is_empty() && unknown.is_empty() {
                    impact(
                        e,
                        Scope::Skip,
                        "exact",
                        vec![format!("no {} files changed", e.as_str())],
                    )
                } else {
                    let sample: Vec<&str> = hits
                        .iter()
                        .chain(unknown.iter())
                        .take(5)
                        .map(|s| **s)
                        .collect();
                    impact(
                        e,
                        Scope::All,
                        "fallback",
                        vec![format!("changed: {}", sample.join(", "))],
                    )
                }
            }
        })
        .collect();
    ImpactPlan {
        source: source.into(),
        changed_files: files,
        ignored_files: ignored,
        ecosystems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, dir: &str, deps: &[&str]) -> Package {
        Package {
            name: name.into(),
            dir: dir.into(),
            deps: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn owner_is_longest_prefix() {
        let ps = vec![
            pkg("root", "", &[]),
            pkg("a", "crates/a", &[]),
            pkg("ab", "crates/ab", &[]),
        ];
        assert_eq!(owner(&ps, "crates/a/src/lib.rs").unwrap().name, "a");
        assert_eq!(owner(&ps, "crates/ab/src/lib.rs").unwrap().name, "ab");
        assert_eq!(owner(&ps, "src/main.rs").unwrap().name, "root");
    }

    #[test]
    fn dependents_are_transitive() {
        let ps = vec![
            pkg("core", "c", &[]),
            pkg("store", "s", &["core"]),
            pkg("app", "a", &["store"]),
            pkg("other", "o", &[]),
        ];
        let seeds = BTreeSet::from(["core".to_string()]);
        let all: Vec<String> = with_dependents(&ps, &seeds).into_iter().collect();
        assert_eq!(all, ["app", "core", "store"]);
    }

    #[test]
    fn inert_files_are_ignored() {
        for f in [
            "README.md",
            "docs/x.png",
            ".github/workflows/ci.yml",
            "LICENSE-MIT",
        ] {
            assert!(is_inert(f), "{f}");
        }
        for f in ["src/lib.rs", "requirements.txt", "package.json"] {
            assert!(!is_inert(f), "{f}");
        }
    }

    fn profile(root: &Path, es: &[Ecosystem]) -> ProjectProfile {
        ProjectProfile {
            root: root.into(),
            ecosystems: es
                .iter()
                .map(|e| DetectedEcosystem {
                    ecosystem: *e,
                    tool: qualyx_core::TestTool::Pytest,
                    manifest: String::new(),
                    target: None,
                    notes: vec![],
                    packages: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn non_rust_ecosystems_skip_or_run_all() {
        let p = profile(Path::new("."), &[Ecosystem::Python, Ecosystem::Node]);
        let plan = analyze(&p, vec!["app/models.py".into(), "README.md".into()], "test");
        assert_eq!(plan.ecosystems[0].scope, Scope::All);
        assert_eq!(plan.ecosystems[1].scope, Scope::Skip);
        assert_eq!(plan.ignored_files, ["README.md"]);
        let applied = plan.apply(&p);
        assert_eq!(applied.ecosystems.len(), 1);
    }

    #[test]
    fn index_detects_add_modify_remove() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a.py"), "1").unwrap();
        fs::write(d.path().join("b.py"), "1").unwrap();
        fs::create_dir_all(d.path().join("node_modules")).unwrap();
        fs::write(d.path().join("node_modules/x.js"), "1").unwrap();
        assert_eq!(write_index(d.path()).unwrap(), 2);
        assert!(changed_since_index(d.path()).unwrap().is_empty());
        fs::write(d.path().join("a.py"), "2").unwrap();
        fs::remove_file(d.path().join("b.py")).unwrap();
        fs::write(d.path().join("c.py"), "1").unwrap();
        assert_eq!(
            changed_since_index(d.path()).unwrap(),
            ["a.py", "b.py", "c.py"]
        );
    }

    fn git_in(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A real three-crate workspace: core <- store <- app, plus an unrelated crate.
    fn workspace() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let w = d.path();
        fs::write(
            w.join("Cargo.toml"),
            "[workspace]\nresolver = \"2\"\nmembers = [\"core\", \"store\", \"app\", \"other\"]\n",
        )
        .unwrap();
        for (name, deps) in [
            ("core", ""),
            ("store", "core = { path = \"../core\" }"),
            ("app", "store = { path = \"../store\" }"),
            ("other", ""),
        ] {
            fs::create_dir_all(w.join(name).join("src")).unwrap();
            fs::write(w.join(name).join("Cargo.toml"), format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}\n")).unwrap();
            fs::write(w.join(name).join("src/lib.rs"), "").unwrap();
        }
        git_in(w, &["init", "-q"]);
        git_in(w, &["add", "-A"]);
        git_in(w, &["commit", "-q", "-m", "init"]);
        d
    }

    fn rust_profile(root: &Path) -> ProjectProfile {
        let mut p = profile(root, &[Ecosystem::Rust]);
        p.ecosystems[0].tool = qualyx_core::TestTool::CargoTest;
        p
    }

    #[test]
    fn rust_change_selects_package_and_dependents() {
        let d = workspace();
        fs::write(d.path().join("core/src/lib.rs"), "pub fn x() {}").unwrap();
        let changed = changed_since_git(d.path(), "HEAD").unwrap();
        assert_eq!(changed, ["core/src/lib.rs"]);
        let plan = analyze(&rust_profile(d.path()), changed, "git");
        assert_eq!(
            plan.ecosystems[0].scope,
            Scope::Packages(vec!["app".into(), "core".into(), "store".into()])
        );
        assert_eq!(plan.ecosystems[0].confidence, "exact");
    }

    #[test]
    fn leaf_change_selects_only_the_leaf() {
        let d = workspace();
        fs::write(d.path().join("other/src/new.rs"), "").unwrap();
        let plan = analyze(
            &rust_profile(d.path()),
            changed_since_git(d.path(), "HEAD").unwrap(),
            "git",
        );
        assert_eq!(
            plan.ecosystems[0].scope,
            Scope::Packages(vec!["other".into()])
        );
    }

    #[test]
    fn root_manifest_change_falls_back_to_all() {
        let d = workspace();
        fs::write(d.path().join("Cargo.toml"), "[workspace]\nresolver = \"2\"\nmembers = [\"core\", \"store\", \"app\", \"other\"]\n# edit\n").unwrap();
        let plan = analyze(
            &rust_profile(d.path()),
            changed_since_git(d.path(), "HEAD").unwrap(),
            "git",
        );
        assert_eq!(plan.ecosystems[0].scope, Scope::All);
        assert_eq!(plan.ecosystems[0].confidence, "fallback");
    }

    #[test]
    fn docs_only_change_runs_nothing() {
        let d = workspace();
        fs::write(d.path().join("README.md"), "hi").unwrap();
        let plan = analyze(
            &rust_profile(d.path()),
            changed_since_git(d.path(), "HEAD").unwrap(),
            "git",
        );
        assert_eq!(plan.ecosystems[0].scope, Scope::Skip);
        assert!(plan.apply(&rust_profile(d.path())).ecosystems.is_empty());
    }

    #[test]
    fn rejects_option_like_and_unknown_revisions() {
        let d = workspace();
        assert!(changed_since_git(d.path(), "--output=/tmp/x").is_err());
        assert!(changed_since_git(d.path(), "no-such-branch").is_err());
    }
}
