//! `qualyx` command-line interface. Composition root only: the work lives in the library crates.

mod render;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use qualyx_core::{Ecosystem, ProjectProfile, RunManifest};
use qualyx_runners::{RunOptions, process};
use qualyx_store::{Layout, Store};
use serde::Deserialize;

#[derive(Parser)]
#[command(
    name = "qualyx",
    version,
    about = "Discover, run and report a project's tests with evidence."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create `.qualyx/` (local state, never committed) with a default config.
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Check which native tools are installed and which ones this project needs.
    Doctor {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Detect ecosystems and the test tool Qualyx would use for each.
    Discover {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Run the project's tests with their native tools and record the evidence.
    Test {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Per-runner timeout in seconds (overrides `.qualyx/config.toml`).
        #[arg(long)]
        timeout: Option<u64>,
        /// Print the manifest as JSON instead of the summary.
        #[arg(long)]
        json: bool,
    },
    /// Show a stored run (the latest by default) or list runs.
    Report {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Run id to show.
        #[arg(long)]
        run: Option<String>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// List recent runs instead of showing one.
        #[arg(long)]
        list: bool,
    },
    /// Write the JSON Schemas of the public data contracts.
    Schema {
        #[arg(long, default_value = "schemas")]
        out: PathBuf,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Markdown,
    Json,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Config {
    run: RunConfig,
}

#[derive(Deserialize)]
#[serde(default)]
struct RunConfig {
    timeout_secs: u64,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig {
            timeout_secs: RunOptions::default().timeout.as_secs(),
        }
    }
}

const DEFAULT_CONFIG: &str = "# Qualyx project settings.\n\n[run]\n# Per-runner wall-clock limit in seconds.\ntimeout_secs = 1800\n";

fn load_config(layout: &Layout) -> Result<Config> {
    match fs::read_to_string(layout.config()) {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("invalid {}", layout.config().display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}

fn main() -> ExitCode {
    match dispatch(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("qualyx: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn discover(path: &Path) -> Result<ProjectProfile> {
    qualyx_discovery::discover(path)
        .with_context(|| format!("cannot read project at {}", path.display()))
}

fn dispatch(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Init { path } => init(&path),
        Command::Doctor { path } => doctor(&path),
        Command::Discover { path, json } => {
            let profile = discover(&path)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&profile)?);
            } else if profile.ecosystems.is_empty() {
                println!(
                    "No supported ecosystem detected in {}",
                    profile.root.display()
                );
            } else {
                for d in &profile.ecosystems {
                    println!(
                        "{:<7} {:<12} {}",
                        d.ecosystem.as_str(),
                        d.tool.as_str(),
                        d.target.as_deref().unwrap_or(&d.manifest)
                    );
                    for n in &d.notes {
                        println!("        note: {n}");
                    }
                }
            }
            Ok(0)
        }
        Command::Test {
            path,
            timeout,
            json,
        } => test(&path, timeout, json),
        Command::Report {
            path,
            run,
            format,
            list,
        } => report(&path, run, format, list),
        Command::Schema { out } => {
            fs::create_dir_all(&out)?;
            for (name, schema) in schemas() {
                let file = out.join(format!("{name}.schema.json"));
                fs::write(&file, schema)?;
                println!("wrote {}", file.display());
            }
            Ok(0)
        }
    }
}

/// Pretty JSON Schemas of the contracts other tools consume.
pub fn schemas() -> Vec<(&'static str, String)> {
    let pretty =
        |s: schemars::Schema| serde_json::to_string_pretty(&s).expect("schema serializes") + "\n";
    vec![
        (
            "project-profile",
            pretty(schemars::schema_for!(ProjectProfile)),
        ),
        ("run-manifest", pretty(schemars::schema_for!(RunManifest))),
    ]
}

fn init(path: &Path) -> Result<u8> {
    let layout = Layout::new(path);
    layout.ensure()?;
    if !layout.config().exists() {
        fs::write(layout.config(), DEFAULT_CONFIG)?;
    }
    println!("Initialized {}", layout.dir.display());
    Ok(0)
}

struct ToolCheck {
    name: &'static str,
    program: &'static [&'static str],
    version_args: &'static [&'static str],
    needed_by: Option<Ecosystem>,
}

const TOOLS: &[ToolCheck] = &[
    ToolCheck {
        name: "git",
        program: &["git"],
        version_args: &["--version"],
        needed_by: None,
    },
    ToolCheck {
        name: "cargo",
        program: &["cargo"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Rust),
    },
    ToolCheck {
        name: "dotnet",
        program: &["dotnet"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Dotnet),
    },
    ToolCheck {
        name: "node",
        program: &["node"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Node),
    },
    ToolCheck {
        name: "npm",
        program: &["npm"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Node),
    },
    ToolCheck {
        name: "python",
        program: &["python", "python3"],
        version_args: &["--version"],
        needed_by: Some(Ecosystem::Python),
    },
    ToolCheck {
        name: "pytest",
        program: &["python", "python3"],
        version_args: &["-m", "pytest", "--version"],
        needed_by: Some(Ecosystem::Python),
    },
];

fn doctor(path: &Path) -> Result<u8> {
    let profile = discover(path)?;
    let needed: Vec<Ecosystem> = profile.ecosystems.iter().map(|d| d.ecosystem).collect();
    let mut missing_required = 0;
    for t in TOOLS {
        let required = t.needed_by.is_some_and(|e| needed.contains(&e));
        let found = process::resolve(t.program)
            .and_then(|p| process::version(&p, t.version_args, &profile.root));
        let state = match (&found, required) {
            (Some(_), _) => "ok",
            (None, true) => {
                missing_required += 1;
                "MISSING (required)"
            }
            (None, false) => "missing",
        };
        println!("{:<7} {:<19} {}", t.name, state, found.unwrap_or_default());
    }
    println!(
        "\n{} ecosystem(s) detected in {}",
        profile.ecosystems.len(),
        profile.root.display()
    );
    Ok(if missing_required > 0 { 1 } else { 0 })
}

fn test(path: &Path, timeout: Option<u64>, json: bool) -> Result<u8> {
    let profile = discover(path)?;
    let layout = Layout::new(&profile.root);
    layout.ensure()?;
    let config = load_config(&layout)?;
    let opts = RunOptions {
        timeout: Duration::from_secs(timeout.unwrap_or(config.run.timeout_secs)),
    };

    let run_id = qualyx_store::new_run_id(chrono::Utc::now());
    let run_dir = layout.run_dir(&run_id);
    let manifest = qualyx_runners::run(&profile, &run_id, &run_dir, &opts, |d| {
        if !json {
            eprintln!("running {} ({})…", d.ecosystem.as_str(), d.tool.as_str());
        }
    })?;

    let manifest_path = qualyx_store::write_manifest(&run_dir, &manifest)?;
    fs::write(run_dir.join("report.md"), render::markdown(&manifest))?;
    Store::open(&layout.db())?.insert_run(&manifest, &manifest_path)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
    } else {
        print!("{}", render::summary(&manifest));
        println!("evidence: {}", run_dir.display());
    }
    Ok(manifest.exit_code() as u8)
}

fn report(path: &Path, run: Option<String>, format: Format, list: bool) -> Result<u8> {
    let root = discover(path)?.root;
    let layout = Layout::new(&root);
    if !layout.db().exists() {
        bail!(
            "no runs recorded in {} (run `qualyx test` first)",
            root.display()
        );
    }
    let store = Store::open(&layout.db())?;

    if list {
        for r in store.list_runs(20)? {
            println!(
                "{}  {:<12} {}/{} passed, {} failed  {}",
                r.id,
                r.status.as_str(),
                r.passed,
                r.total,
                r.failed,
                r.started_at
            );
        }
        return Ok(0);
    }

    let id = match run {
        Some(id) if qualyx_store::is_valid_run_id(&id) => id,
        Some(id) => bail!("invalid run id `{id}`"),
        None => store.latest_run_id()?.context("no runs recorded")?,
    };
    let manifest_path = store
        .manifest_path(&id)?
        .with_context(|| format!("unknown run `{id}`"))?;
    let manifest = qualyx_store::read_manifest(&manifest_path)?;
    match format {
        Format::Text => print!("{}", render::summary(&manifest)),
        Format::Markdown => print!("{}", render::markdown(&manifest)),
        Format::Json => println!("{}", serde_json::to_string_pretty(&manifest)?),
    }
    Ok(0)
}
