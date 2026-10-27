//! Host desktop companion (Computer Use, M4): capability probe, accessibility snapshot and
//! screenshot, accessibility-first.
//!
//! Scope is deliberately read-only. Input injection is not implemented: it needs per-OS
//! consent flows (macOS Accessibility, Wayland portals) and a review gate that M4 doesn't
//! have yet. Every unsupported case says so instead of guessing.
//!
//! Containers can't reach the host desktop; this must run on the host itself.

use std::path::Path;
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub os: String,
    /// "windows-desktop", "macos-aqua", "x11", "wayland" or "none".
    pub session: String,
    /// "available", "permission-required", "unavailable".
    pub accessibility_tree: String,
    pub screenshot: String,
    pub input: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Window {
    pub name: String,
    pub class: String,
    pub process_id: Option<u32>,
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// UI Automation over PowerShell: top-level windows of the desktop root.
const WIN_SNAPSHOT: &str = r#"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::RootElement
$cond = [System.Windows.Automation.Condition]::TrueCondition
$items = $root.FindAll([System.Windows.Automation.TreeScope]::Children, $cond)
$out = foreach ($e in $items) {
  $c = $e.Current
  [pscustomobject]@{ name = $c.Name; class = $c.ClassName; process_id = $c.ProcessId }
}
@($out) | ConvertTo-Json -Compress
"#;

const WIN_SCREENSHOT: &str = r#"
param([string]$Path)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$b = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Left, $b.Top, 0, 0, $bmp.Size)
$bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
"#;

pub fn probe() -> Capabilities {
    let os = std::env::consts::OS.to_string();
    let mut notes = Vec::new();
    let (session, a11y, shot) = match os.as_str() {
        "windows" => {
            let ok = snapshot().is_ok();
            if !ok {
                notes.push(
                    "UI Automation query failed (non-interactive session or service account)"
                        .into(),
                );
            }
            (
                "windows-desktop",
                if ok { "available" } else { "unavailable" },
                if ok { "available" } else { "unavailable" },
            )
        }
        "macos" => {
            let ok = run(
                "osascript",
                &[
                    "-e",
                    "tell application \"System Events\" to count processes",
                ],
            )
            .is_some();
            if !ok {
                notes.push("grant Accessibility/Automation to the terminal in System Settings > Privacy & Security".into());
            }
            (
                "macos-aqua",
                if ok {
                    "available"
                } else {
                    "permission-required"
                },
                "available (screencapture; Screen Recording permission may be required)",
            )
        }
        _ => {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                notes.push("Wayland: global capture and input go through xdg-desktop-portal with user consent; Qualyx does not bypass it".into());
                ("wayland", "unavailable", "unavailable")
            } else if std::env::var_os("DISPLAY").is_some() {
                let wmctrl = which::which("wmctrl").is_ok();
                if !wmctrl {
                    notes.push("install wmctrl to list X11 windows".into());
                }
                (
                    "x11",
                    if wmctrl { "available" } else { "unavailable" },
                    if which::which("import").is_ok() {
                        "available"
                    } else {
                        "unavailable"
                    },
                )
            } else {
                notes.push("no graphical session (DISPLAY/WAYLAND_DISPLAY unset)".into());
                ("none", "unavailable", "unavailable")
            }
        }
    };
    Capabilities {
        os,
        session: session.into(),
        accessibility_tree: a11y.into(),
        screenshot: shot.into(),
        input: "not-implemented".into(),
        notes,
    }
}

/// Top-level windows from the platform accessibility API.
pub fn snapshot() -> Result<Vec<Window>, String> {
    match std::env::consts::OS {
        "windows" => {
            let out = run(
                "powershell",
                &["-NoProfile", "-NonInteractive", "-Command", WIN_SNAPSHOT],
            )
            .ok_or("UI Automation query failed")?;
            let v: serde_json::Value =
                serde_json::from_str(out.trim()).map_err(|e| e.to_string())?;
            Ok(v.as_array()
                .into_iter()
                .flatten()
                .map(|w| Window {
                    name: w["name"].as_str().unwrap_or_default().into(),
                    class: w["class"].as_str().unwrap_or_default().into(),
                    process_id: w["process_id"].as_u64().map(|p| p as u32),
                })
                .collect())
        }
        "macos" => {
            let out = run("osascript", &["-e", "tell application \"System Events\" to get name of every process whose visible is true"])
                .ok_or("System Events denied (Accessibility permission required)")?;
            Ok(out
                .trim()
                .split(", ")
                .map(|n| Window {
                    name: n.into(),
                    class: "process".into(),
                    process_id: None,
                })
                .collect())
        }
        _ if std::env::var_os("WAYLAND_DISPLAY").is_some() => {
            Err("not supported on Wayland without a portal".into())
        }
        _ => {
            let out = run("wmctrl", &["-lp"]).ok_or("wmctrl unavailable or no X11 display")?;
            Ok(out
                .lines()
                .filter_map(|l| {
                    let mut f = l.split_whitespace();
                    let _id = f.next()?;
                    let _desk = f.next()?;
                    let pid = f.next()?.parse().ok();
                    let _host = f.next()?;
                    Some(Window {
                        name: f.collect::<Vec<_>>().join(" "),
                        class: "x11".into(),
                        process_id: pid,
                    })
                })
                .collect())
        }
    }
}

/// Full-screen PNG at `path`.
pub fn screenshot(path: &Path) -> Result<(), String> {
    let p = path.to_string_lossy().into_owned();
    let ok = match std::env::consts::OS {
        "windows" => {
            let script = format!("& {{ {WIN_SCREENSHOT} }} -Path '{}'", p.replace('\'', "''"));
            run(
                "powershell",
                &["-NoProfile", "-NonInteractive", "-Command", &script],
            )
            .is_some()
        }
        "macos" => run("screencapture", &["-x", &p]).is_some(),
        _ if std::env::var_os("WAYLAND_DISPLAY").is_some() => {
            return Err("not supported on Wayland without a portal".into());
        }
        _ => run("import", &["-window", "root", &p]).is_some(),
    };
    if ok && path.is_file() {
        Ok(())
    } else {
        Err("screenshot failed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_never_claims_input() {
        let c = probe();
        assert_eq!(c.input, "not-implemented");
        assert!(
            ["available", "permission-required", "unavailable"]
                .contains(&c.accessibility_tree.as_str())
        );
    }

    /// Needs an interactive desktop: QUALYX_TEST_DESKTOP=1.
    #[test]
    fn interactive_snapshot_and_screenshot() {
        if std::env::var("QUALYX_TEST_DESKTOP").as_deref() != Ok("1") {
            eprintln!("NOT RUN: set QUALYX_TEST_DESKTOP=1 on an interactive desktop");
            return;
        }
        let w = snapshot().unwrap();
        assert!(!w.is_empty());
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("screen.png");
        screenshot(&p).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }
}
