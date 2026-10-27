//! One source of truth for the server launch command, rendered into each client's format
//! (portable agent tooling: the same server works in every agent).

use std::path::{Path, PathBuf};

use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Client {
    /// Claude Code: `.mcp.json` (project) or `claude mcp add`.
    Claude,
    /// OpenAI Codex CLI: `~/.codex/config.toml`.
    Codex,
    /// OpenCode: `opencode.json`.
    Opencode,
    /// Cursor: `.cursor/mcp.json`.
    Cursor,
    /// VS Code: `.vscode/mcp.json`.
    Vscode,
    /// Any client that reads the common `mcpServers` JSON shape.
    Generic,
}

/// `[program, args...]` that starts the server for the given roots.
pub fn launch(exe: &Path, roots: &[PathBuf]) -> std::io::Result<Vec<String>> {
    let mut v = vec![exe.display().to_string(), "mcp".to_string()];
    for r in roots {
        v.push("--root".into());
        v.push(
            r.canonicalize()
                .map(|p| strip_verbatim(&p))?
                .display()
                .to_string(),
        );
    }
    Ok(v)
}

fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\")
        .map(PathBuf::from)
        .unwrap_or_else(|| p.to_path_buf())
}

fn toml_str(s: &str) -> String {
    // TOML basic string: escape backslashes and quotes.
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn config_snippet(client: Client, exe: &Path, roots: &[PathBuf]) -> std::io::Result<String> {
    let cmd = launch(exe, roots)?;
    let (program, args) = (cmd[0].clone(), cmd[1..].to_vec());
    let pretty = |v: serde_json::Value| serde_json::to_string_pretty(&v).unwrap_or_default() + "\n";
    let mcp_servers = pretty(json!({"mcpServers": {"qualyx": {"command": program, "args": args}}}));
    Ok(match client {
        Client::Claude => format!(
            "# .mcp.json in the project root, or run:\n#   claude mcp add qualyx -- {}\n{mcp_servers}",
            cmd.join(" ")
        ),
        Client::Cursor | Client::Generic => mcp_servers,
        Client::Codex => format!(
            "# ~/.codex/config.toml\n[mcp_servers.qualyx]\ncommand = {}\nargs = [{}]\n",
            toml_str(&program),
            args.iter()
                .map(|a| toml_str(a))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Client::Opencode => pretty(json!({
            "$schema": "https://opencode.ai/config.json",
            "mcp": {"qualyx": {"type": "local", "command": cmd, "enabled": true}}
        })),
        Client::Vscode => pretty(
            json!({"servers": {"qualyx": {"type": "stdio", "command": program, "args": args}}}),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_client_gets_the_same_launch_command() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/opt/qualyx");
        let roots = vec![dir.path().to_path_buf()];
        let root = strip_verbatim(&dir.path().canonicalize().unwrap())
            .display()
            .to_string();
        for c in [
            Client::Claude,
            Client::Cursor,
            Client::Generic,
            Client::Vscode,
            Client::Opencode,
        ] {
            let s = config_snippet(c, exe, &roots).unwrap();
            let json_part = &s[s.find('{').unwrap()..];
            let v: serde_json::Value = serde_json::from_str(json_part).unwrap();
            assert!(
                v.to_string().contains(&root.replace('\\', "\\\\")),
                "{c:?}: {s}"
            );
        }
        let codex = config_snippet(Client::Codex, exe, &roots).unwrap();
        let parsed: toml::Value = toml::from_str(&codex).unwrap();
        let args = parsed["mcp_servers"]["qualyx"]["args"].as_array().unwrap();
        assert_eq!(args[0].as_str(), Some("mcp"));
        assert_eq!(args[2].as_str(), Some(root.as_str()));
    }
}
