//! Find installed browsers. Only real executables count; nothing is assumed from the OS.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Chromium,
    Firefox,
    Safari,
}

#[derive(Debug, Clone, Serialize)]
pub struct Browser {
    pub id: String,
    pub family: Family,
    pub path: PathBuf,
    pub version: Option<String>,
}

struct Candidate {
    id: &'static str,
    family: Family,
    windows: &'static [&'static str],
    macos: &'static [&'static str],
    linux: &'static [&'static str],
}

/// `%VAR%` placeholders are expanded on Windows.
const CANDIDATES: &[Candidate] = &[
    Candidate {
        id: "chrome",
        family: Family::Chromium,
        windows: &[
            r"%ProgramFiles%\Google\Chrome\Application\chrome.exe",
            r"%ProgramFiles(x86)%\Google\Chrome\Application\chrome.exe",
            r"%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe",
        ],
        macos: &["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"],
        linux: &["google-chrome", "google-chrome-stable"],
    },
    Candidate {
        id: "chromium",
        family: Family::Chromium,
        windows: &[r"%LOCALAPPDATA%\Chromium\Application\chrome.exe"],
        macos: &["/Applications/Chromium.app/Contents/MacOS/Chromium"],
        linux: &["chromium", "chromium-browser"],
    },
    Candidate {
        id: "edge",
        family: Family::Chromium,
        windows: &[
            r"%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe",
            r"%ProgramFiles%\Microsoft\Edge\Application\msedge.exe",
        ],
        macos: &["/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"],
        linux: &["microsoft-edge", "microsoft-edge-stable"],
    },
    Candidate {
        id: "brave",
        family: Family::Chromium,
        windows: &[
            r"%ProgramFiles%\BraveSoftware\Brave-Browser\Application\brave.exe",
            r"%LOCALAPPDATA%\BraveSoftware\Brave-Browser\Application\brave.exe",
        ],
        macos: &["/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"],
        linux: &["brave-browser", "brave"],
    },
    Candidate {
        id: "opera",
        family: Family::Chromium,
        windows: &[r"%LOCALAPPDATA%\Programs\Opera\opera.exe"],
        macos: &["/Applications/Opera.app/Contents/MacOS/Opera"],
        linux: &["opera"],
    },
    Candidate {
        id: "vivaldi",
        family: Family::Chromium,
        windows: &[
            r"%LOCALAPPDATA%\Vivaldi\Application\vivaldi.exe",
            r"%ProgramFiles%\Vivaldi\Application\vivaldi.exe",
        ],
        macos: &["/Applications/Vivaldi.app/Contents/MacOS/Vivaldi"],
        linux: &["vivaldi", "vivaldi-stable"],
    },
    Candidate {
        id: "firefox",
        family: Family::Firefox,
        windows: &[
            r"%ProgramFiles%\Mozilla Firefox\firefox.exe",
            r"%ProgramFiles(x86)%\Mozilla Firefox\firefox.exe",
        ],
        macos: &["/Applications/Firefox.app/Contents/MacOS/firefox"],
        linux: &["firefox"],
    },
    Candidate {
        id: "zen",
        family: Family::Firefox,
        windows: &[r"%ProgramFiles%\Zen Browser\zen.exe"],
        macos: &[
            "/Applications/Zen.app/Contents/MacOS/zen",
            "/Applications/Zen Browser.app/Contents/MacOS/zen",
        ],
        linux: &["zen", "zen-browser"],
    },
    Candidate {
        id: "safari",
        family: Family::Safari,
        windows: &[],
        macos: &["/usr/bin/safaridriver"],
        linux: &[],
    },
];

fn expand(p: &str) -> Option<PathBuf> {
    let mut out = String::new();
    let mut rest = p;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        out.push_str(&std::env::var(&after[..end]).ok()?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(PathBuf::from(out))
}

fn resolve(entry: &str) -> Option<PathBuf> {
    if entry.contains('/') || entry.contains('\\') {
        let p = expand(entry)?;
        p.is_file().then_some(p)
    } else {
        which::which(entry).ok()
    }
}

/// Version from the binary (macOS/Linux) or the file metadata (Windows GUI executables
/// don't print `--version`).
pub fn version(path: &Path, family: Family) -> Option<String> {
    if family == Family::Safari {
        let out = Command::new("defaults")
            .args([
                "read",
                "/Applications/Safari.app/Contents/Info",
                "CFBundleShortVersionString",
            ])
            .output()
            .ok()?;
        return Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
    }
    let out = if cfg!(windows) {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(Get-Item -LiteralPath '{}').VersionInfo.ProductVersion",
                    path.display().to_string().replace('\'', "''")
                ),
            ])
            .output()
            .ok()?
    } else {
        Command::new(path).arg("--version").output().ok()?
    };
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let v = text
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    Some(v.to_string())
}

/// Installed browsers on this machine, in a stable order.
pub fn detect() -> Vec<Browser> {
    CANDIDATES
        .iter()
        .filter_map(|c| {
            let list = if cfg!(windows) {
                c.windows
            } else if cfg!(target_os = "macos") {
                c.macos
            } else {
                c.linux
            };
            let path = list.iter().find_map(|e| resolve(e))?;
            Some(Browser {
                id: c.id.into(),
                family: c.family,
                version: version(&path, c.family),
                path,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_env_placeholders() {
        let path = std::env::var("PATH").unwrap();
        assert_eq!(
            expand("%PATH%/x").unwrap(),
            PathBuf::from(format!("{path}/x"))
        );
        assert!(expand("%QUALYX_NO_SUCH_VAR%/x").is_none());
    }

    #[test]
    fn detection_only_reports_existing_files() {
        for b in detect() {
            assert!(b.path.exists(), "{b:?}");
        }
    }
}
