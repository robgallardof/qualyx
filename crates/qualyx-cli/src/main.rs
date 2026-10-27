//! `qualyx` command-line interface. Composition root only: the work lives in the library crates.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use qualyx_app::{TestRequest, render};
use qualyx_core::{Isolation, ProjectProfile, RunManifest};

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
        /// Where tools run (overrides `.qualyx/config.toml`).
        #[arg(long, value_enum)]
        isolation: Option<IsolationArg>,
        /// Only run what changed since this git revision, or `index` (last `qualyx index`).
        #[arg(long)]
        changed_since: Option<String>,
        /// Run up to N ecosystems in parallel.
        #[arg(long, default_value_t = 1)]
        jobs: usize,
        /// Print the manifest as JSON instead of the summary.
        #[arg(long)]
        json: bool,
    },
    /// Show which tests a change affects without running them.
    Impact {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Git revision to diff against, or `index`.
        #[arg(long)]
        base: String,
        #[arg(long)]
        json: bool,
    },
    /// Record content hashes of the project (baseline for `--changed-since index`).
    Index {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Group a run's failures, locate them and print how to reproduce each one.
    Triage {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        run: Option<String>,
        /// Also ask the configured `[llm]` for a root-cause hypothesis (labelled as such).
        #[arg(long)]
        llm: bool,
        #[arg(long)]
        json: bool,
    },
    /// Single-writer git worktrees for agents.
    Worktree {
        #[command(subcommand)]
        action: WorktreeAction,
    },
    /// Installed browsers; `--smoke` drives each one for real.
    Browsers {
        #[arg(long)]
        smoke: bool,
        #[arg(long)]
        json: bool,
    },
    /// Run a JSON browser flow and keep screenshots, console and failed requests as evidence.
    Browse {
        /// Flow file (see fixtures/web/flows).
        flow: PathBuf,
        /// Browser ids (chrome, edge, firefox, safari…). Default: every installed browser.
        #[arg(long = "browser")]
        browsers: Vec<String>,
        /// Serve this directory on loopback and use it as the base URL.
        #[arg(long)]
        serve: Option<PathBuf>,
        #[arg(long)]
        base_url: Option<String>,
        /// Show the browser window.
        #[arg(long)]
        headed: bool,
        /// Project whose `.qualyx/` receives the evidence.
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Host desktop (Computer Use, read-only): probe, accessibility snapshot, screenshot.
    Desktop {
        #[command(subcommand)]
        action: DesktopAction,
    },
    /// Check what the configured `[llm]` endpoint supports.
    LlmProbe {
        #[arg(default_value = ".")]
        path: PathBuf,
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
    /// Check that a run's evidence files still match the hashes in its manifest.
    Verify {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        run: Option<String>,
    },
    /// Serve the Qualyx tools over MCP (stdio). Only projects under a `--root` are reachable.
    Mcp {
        /// Directories the MCP client may act on. Defaults to the current directory.
        #[arg(long = "root")]
        roots: Vec<PathBuf>,
    },
    /// Print the MCP configuration snippet for a client.
    McpConfig {
        #[arg(value_enum)]
        client: qualyx_mcp::clients::Client,
        /// Directories the client may act on.
        #[arg(long = "root", required = true)]
        roots: Vec<PathBuf>,
    },
    /// Write the JSON Schemas of the public data contracts.
    Schema {
        #[arg(long, default_value = "schemas")]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum WorktreeAction {
    /// Create `.qualyx/worktrees/<name>` on branch `qualyx/<name>`.
    Create {
        name: String,
        #[arg(long, default_value = "HEAD")]
        base: String,
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    /// List worktrees.
    List {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    /// Remove a worktree and its branch (refused while a writer holds it).
    Remove {
        name: String,
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
}

#[derive(Subcommand)]
enum DesktopAction {
    /// What this host session allows.
    Probe,
    /// Top-level windows from the accessibility API.
    Snapshot,
    /// Full-screen PNG.
    Screenshot { out: PathBuf },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Markdown,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
enum IsolationArg {
    Process,
    Container,
}

impl From<IsolationArg> for Isolation {
    fn from(i: IsolationArg) -> Self {
        match i {
            IsolationArg::Process => Isolation::Process,
            IsolationArg::Container => Isolation::Container,
        }
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

fn dispatch(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Init { path } => {
            println!("Initialized {}", qualyx_app::init(&path)?.display());
            Ok(0)
        }
        Command::Doctor { path } => {
            let (profile, tools) = qualyx_app::doctor(&path)?;
            let mut missing_required = 0;
            for t in &tools {
                let state = match (&t.version, t.required) {
                    (Some(_), _) => "ok",
                    (None, true) => {
                        missing_required += 1;
                        "MISSING (required)"
                    }
                    (None, false) => "missing",
                };
                println!(
                    "{:<7} {:<19} {}",
                    t.name,
                    state,
                    t.version.as_deref().unwrap_or("")
                );
            }
            println!(
                "\n{} ecosystem(s) detected in {}",
                profile.ecosystems.len(),
                profile.root.display()
            );
            Ok(if missing_required > 0 { 1 } else { 0 })
        }
        Command::Discover { path, json } => {
            let profile = qualyx_app::discover(&path)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&profile)?);
            } else {
                print_profile(&profile);
            }
            Ok(0)
        }
        Command::Test {
            path,
            timeout,
            isolation,
            changed_since,
            jobs,
            json,
        } => {
            let req = TestRequest {
                timeout_secs: timeout,
                isolation: isolation.map(Into::into),
                changed_since,
                jobs,
            };
            let (manifest, run_dir) = qualyx_app::test(&path, &req, |d| {
                if !json {
                    eprintln!("running {} ({})…", d.ecosystem.as_str(), d.tool.as_str());
                }
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            } else {
                print!("{}", render::summary(&manifest));
                println!("evidence: {}", run_dir.display());
            }
            Ok(manifest.exit_code() as u8)
        }
        Command::Report {
            path,
            run,
            format,
            list,
        } => {
            if list {
                for r in qualyx_app::list_runs(&path, 20)? {
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
            let (manifest, _) = qualyx_app::load_run(&path, run.as_deref())?;
            match format {
                Format::Text => print!("{}", render::summary(&manifest)),
                Format::Markdown => print!("{}", render::markdown(&manifest)),
                Format::Json => println!("{}", serde_json::to_string_pretty(&manifest)?),
            }
            Ok(0)
        }
        Command::Impact { path, base, json } => {
            let profile = qualyx_app::discover(&path)?;
            let plan = qualyx_app::impact(&profile, &base)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
                return Ok(0);
            }
            println!(
                "{} changed file(s) since {} ({} ignored as non-code)",
                plan.changed_files.len(),
                plan.source,
                plan.ignored_files.len()
            );
            for e in &plan.ecosystems {
                let scope = match &e.scope {
                    qualyx_impact::Scope::Skip => "skip".to_string(),
                    qualyx_impact::Scope::All => "all".to_string(),
                    qualyx_impact::Scope::Packages(p) => p.join(", "),
                };
                println!("{:<7} {:<9} {scope}", e.ecosystem.as_str(), e.confidence);
                for r in &e.reasons {
                    println!("        {r}");
                }
            }
            Ok(0)
        }
        Command::Index { path } => {
            println!("indexed {} file(s)", qualyx_app::index(&path)?);
            Ok(0)
        }
        Command::Triage {
            path,
            run,
            llm,
            json,
        } => {
            let report = qualyx_app::triage(&path, run.as_deref(), llm)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
                return Ok(0);
            }
            if report.triage.groups.is_empty() {
                println!(
                    "No failures in run {} ({}).",
                    report.triage.run_id,
                    report.triage.status.as_str()
                );
            }
            for (i, g) in report.triage.groups.iter().enumerate() {
                println!(
                    "{}. [{}] {:?} — {}",
                    i + 1,
                    g.ecosystem.as_str(),
                    g.category,
                    g.signature
                );
                for t in &g.tests {
                    println!("     test: {t}");
                }
                for l in &g.locations {
                    println!("     at:   {l}");
                }
                for r in &g.reproduce {
                    println!("     run:  {}", r.command.join(" "));
                }
            }
            if let Some(h) = &report.llm_hypothesis {
                println!("\nModel hypothesis (unverified):\n{h}");
            }
            Ok(0)
        }
        Command::Worktree { action } => {
            match action {
                WorktreeAction::Create { name, base, repo } => {
                    let p = qualyx_scheduler::worktree::create(&repo, &name, &base)
                        .map_err(anyhow::Error::msg)?;
                    println!("{}", p.display());
                }
                WorktreeAction::List { repo } => {
                    for w in qualyx_scheduler::worktree::list(&repo) {
                        println!("{w}");
                    }
                }
                WorktreeAction::Remove { name, repo } => {
                    qualyx_scheduler::worktree::remove(&repo, &name).map_err(anyhow::Error::msg)?;
                    println!("removed {name}");
                }
            }
            Ok(0)
        }
        Command::Browsers { smoke, json } => {
            let list = qualyx_app::browser::browsers(smoke);
            if json {
                println!("{}", serde_json::to_string_pretty(&list)?);
                return Ok(0);
            }
            if list.is_empty() {
                println!("No supported browser found.");
            }
            for e in &list {
                let s = e
                    .smoke
                    .as_ref()
                    .map(|s| {
                        format!(
                            "{} via {} — {}",
                            s.status,
                            s.protocol.as_deref().unwrap_or("-"),
                            s.detail
                        )
                    })
                    .unwrap_or_default();
                println!(
                    "{:<9} {:<16} {}",
                    e.browser.id,
                    e.browser.version.as_deref().unwrap_or("?"),
                    s
                );
            }
            let failed = list
                .iter()
                .any(|e| e.smoke.as_ref().is_some_and(|s| s.status != "verified"));
            Ok(if failed { 1 } else { 0 })
        }
        Command::Browse {
            flow,
            browsers,
            serve,
            base_url,
            headed,
            project,
            json,
        } => {
            let req = qualyx_app::browser::BrowseRequest {
                flow_file: flow,
                browsers,
                serve_dir: serve,
                base_url,
                headed,
                restrict_origins: false,
            };
            let r = qualyx_app::browser::browse(&project, &req)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                for f in &r.flows {
                    println!(
                        "[{}] {} {} via {} — {} step(s)",
                        if f.passed { "PASS" } else { "FAIL" },
                        f.browser,
                        f.version.as_deref().unwrap_or("?"),
                        f.protocol,
                        f.steps.len()
                    );
                    for s in f.steps.iter().filter(|s| !s.passed) {
                        println!("    step {}: {}", s.index, s.error.as_deref().unwrap_or(""));
                    }
                    for c in &f.console {
                        println!("    console {}: {}", c.level, c.text);
                    }
                    for n in &f.network_failures {
                        println!("    network: {n}");
                    }
                }
                for (b, why) in &r.unavailable {
                    println!("[UNAVAILABLE] {b}: {why}");
                }
                println!("evidence: {}", r.run_dir.display());
            }
            Ok(if r.passed { 0 } else { 1 })
        }
        Command::Desktop { action } => {
            match action {
                DesktopAction::Probe => println!(
                    "{}",
                    serde_json::to_string_pretty(&qualyx_desktop::probe())?
                ),
                DesktopAction::Snapshot => {
                    let w = qualyx_desktop::snapshot().map_err(anyhow::Error::msg)?;
                    println!("{}", serde_json::to_string_pretty(&w)?);
                }
                DesktopAction::Screenshot { out } => {
                    qualyx_desktop::screenshot(&out).map_err(anyhow::Error::msg)?;
                    println!("wrote {}", out.display());
                }
            }
            Ok(0)
        }
        Command::LlmProbe { path } => {
            let (cfg, caps) = qualyx_app::llm_probe(&path)?;
            println!("{} {} ({:?})", cfg.base_url, cfg.model, cfg.kind);
            println!("{}", serde_json::to_string_pretty(&caps)?);
            Ok(if caps.completion { 0 } else { 1 })
        }
        Command::Verify { path, run } => {
            let (manifest, run_dir) = qualyx_app::load_run(&path, run.as_deref())?;
            let bad = qualyx_runners::verify_artifacts(&manifest, &run_dir);
            if bad.is_empty() {
                println!(
                    "{} artifact(s) of run {} match their recorded SHA-256",
                    manifest.artifact_sha256.len(),
                    manifest.run_id
                );
                Ok(0)
            } else {
                for b in &bad {
                    println!("MODIFIED or missing: {b}");
                }
                Ok(1)
            }
        }
        Command::Mcp { roots } => {
            let roots = if roots.is_empty() {
                vec![std::env::current_dir()?]
            } else {
                roots
            };
            qualyx_mcp::serve_stdio(roots)?;
            Ok(0)
        }
        Command::McpConfig { client, roots } => {
            let exe = std::env::current_exe()?;
            print!(
                "{}",
                qualyx_mcp::clients::config_snippet(client, &exe, &roots)?
            );
            Ok(0)
        }
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

fn print_profile(profile: &ProjectProfile) {
    if profile.ecosystems.is_empty() {
        println!(
            "No supported ecosystem detected in {}",
            profile.root.display()
        );
    }
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

/// Pretty JSON Schemas of the contracts other tools consume.
fn schemas() -> Vec<(&'static str, String)> {
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
