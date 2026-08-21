//! Cross-platform discovery of an installed Chromium-family browser, plus the
//! one-shot headless `--dump-dom` render used as the JavaScript fallback for
//! `web_fetch`. Deliberately no DevTools protocol / no browser crates: spawning
//! the user's own browser as a child process keeps the build zero-new-deps.
//!
//! Rendering runs with a throwaway profile directory, so the headless instance
//! carries none of the user's cookies or sessions — pages render anonymously
//! inside Chrome's own sandbox.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

/// The platform flavours [`candidates`] knows about, decoupled from `cfg!` so
/// every list is testable from any host OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

/// Locate an installed Chromium-family browser (Chrome, Edge, Chromium, Brave)
/// on this machine, checking well-known install locations then `PATH`.
pub fn discover() -> Option<PathBuf> {
    let os = if cfg!(target_os = "windows") {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    };
    let env = |name: &str| std::env::var(name).ok();
    candidates(os, &env)
        .into_iter()
        .find(|path| path.is_file())
        .or_else(|| {
            path_names(os)
                .iter()
                .find_map(|name| search_path(name, &env, os == Os::Windows))
        })
}

/// Well-known absolute install locations per platform. Pure given the env
/// lookup, so unit tests can exercise every OS list from one host.
pub fn candidates(os: Os, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    match os {
        Os::Windows => {
            let suffixes = [
                r"Google\Chrome\Application\chrome.exe",
                r"Chromium\Application\chrome.exe",
                r"Microsoft\Edge\Application\msedge.exe",
                r"BraveSoftware\Brave-Browser\Application\brave.exe",
            ];
            let bases = ["LOCALAPPDATA", "PROGRAMFILES", "ProgramFiles(x86)", "ProgramW6432"];
            for base in bases {
                if let Some(base) = env(base) {
                    for suffix in suffixes {
                        found.push(Path::new(&base).join(suffix));
                    }
                }
            }
        }
        Os::Mac => {
            let bundles = [
                "Google Chrome.app/Contents/MacOS/Google Chrome",
                "Chromium.app/Contents/MacOS/Chromium",
                "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                "Brave Browser.app/Contents/MacOS/Brave Browser",
            ];
            // Built as strings (not `Path::join`) so the separators stay `/`
            // even when the candidate list is rendered on a Windows host.
            for bundle in bundles {
                found.push(PathBuf::from(format!("/Applications/{bundle}")));
            }
            if let Some(home) = env("HOME") {
                for bundle in bundles {
                    found.push(PathBuf::from(format!("{home}/Applications/{bundle}")));
                }
            }
        }
        Os::Linux => {
            for name in path_names(Os::Linux) {
                found.push(PathBuf::from(format!("/usr/bin/{name}")));
            }
            found.push(PathBuf::from("/opt/google/chrome/chrome"));
            found.push(PathBuf::from("/snap/bin/chromium"));
            found.push(PathBuf::from("/usr/lib/chromium/chromium"));
        }
    }
    found
}

/// Executable names worth resolving via `PATH`, most specific first.
fn path_names(os: Os) -> &'static [&'static str] {
    match os {
        Os::Windows => &["chrome", "msedge", "brave"],
        Os::Mac => &[],
        Os::Linux => &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
            "microsoft-edge-stable",
            "brave-browser",
        ],
    }
}

fn search_path(
    name: &str,
    env: &dyn Fn(&str) -> Option<String>,
    windows: bool,
) -> Option<PathBuf> {
    let path = env("PATH")?;
    for dir in std::env::split_paths(&path) {
        let file = if windows {
            dir.join(format!("{name}.exe"))
        } else {
            dir.join(name)
        };
        if file.is_file() {
            return Some(file);
        }
    }
    None
}

/// Render `url` in the discovered browser headlessly and return the post-
/// JavaScript DOM as an HTML string. `--virtual-time-budget` fast-forwards the
/// page's timers/async work so SPA content settles before the dump; the whole
/// child is killed at `timeout`. Uses (and afterwards removes) a throwaway
/// profile directory so no user state is exposed to the page.
pub async fn render_dom(browser: &Path, url: &str, timeout: Duration) -> Result<String> {
    let profile = std::env::temp_dir().join(format!(
        "ikode-headless-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let output = tokio::time::timeout(
        timeout,
        tokio::process::Command::new(browser)
            .arg("--headless")
            .arg("--disable-gpu")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-extensions")
            .arg("--disable-sync")
            .arg("--mute-audio")
            .arg("--hide-scrollbars")
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("--virtual-time-budget=5000")
            .arg("--timeout=15000")
            .arg("--dump-dom")
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await;
    let removed = std::fs::remove_dir_all(&profile);
    let _ = removed; // best-effort cleanup; Chrome may already have removed it
    let output = output
        .map_err(|_| anyhow!("headless render timed out after {}s", timeout.as_secs()))?
        .context("launch headless browser")?;
    if !output.status.success() {
        return Err(anyhow!("headless browser exited with {}", output.status));
    }
    let dom = String::from_utf8_lossy(&output.stdout).into_owned();
    if dom.trim().is_empty() {
        return Err(anyhow!("headless browser produced no DOM"));
    }
    Ok(dom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn windows_candidates_cover_chrome_and_edge_across_install_roots() {
        let env = env_of(&[
            ("LOCALAPPDATA", r"C:\Users\x\AppData\Local"),
            ("PROGRAMFILES", r"C:\Program Files"),
        ]);
        let found = candidates(Os::Windows, &env);
        let rendered: Vec<String> = found.iter().map(|p| p.display().to_string()).collect();
        assert!(rendered
            .iter()
            .any(|p| p == r"C:\Users\x\AppData\Local\Google\Chrome\Application\chrome.exe"));
        assert!(rendered
            .iter()
            .any(|p| p == r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"));
        // Roots without an env var simply contribute nothing.
        assert!(!rendered.iter().any(|p| p.contains("ProgramW6432")));
    }

    #[test]
    fn mac_candidates_include_system_and_user_application_bundles() {
        let env = env_of(&[("HOME", "/Users/dev")]);
        let rendered: Vec<String> = candidates(Os::Mac, &env)
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert!(rendered
            .iter()
            .any(|p| p == "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
        assert!(rendered
            .iter()
            .any(|p| p == "/Users/dev/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"));
    }

    #[test]
    fn linux_candidates_cover_usr_bin_opt_and_snap() {
        let rendered: Vec<String> = candidates(Os::Linux, &env_of(&[]))
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert!(rendered.iter().any(|p| p == "/usr/bin/google-chrome"));
        assert!(rendered.iter().any(|p| p == "/usr/bin/chromium-browser"));
        assert!(rendered.iter().any(|p| p == "/opt/google/chrome/chrome"));
        assert!(rendered.iter().any(|p| p == "/snap/bin/chromium"));
    }
}
