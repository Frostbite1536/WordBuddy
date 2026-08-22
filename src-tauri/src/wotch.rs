//! Launch integration with Wotch (the notch-style terminal for Claude Code).
//! See docs/WOTCH_INTEGRATION.md §5.1.
//!
//! This module provides:
//! - `launch_wotch` command — spawns Wotch, optionally pre-filling a Claude
//!   Code prompt in a new tab.
//! - `wotch_status` command — a health probe used by the UI to hide the
//!   launch button when Wotch isn't installed.
//!
//! Wotch's HTTP API (verified against api-server.js):
//! - Base port `19519`, with fallback attempts up to `19528`. Active port
//!   is written to `~/.wotch/api-port`.
//! - Bearer-token auth (`~/.wotch/api-token`).
//! - All responses use a `{ok: bool, data?: ..., error?: str, code?: str}`
//!   envelope. For `POST /v1/tabs`, tabId lives at `data.tabId`.
//! - `POST /v1/tabs/:id/input` expects `{data: "..."}` (NOT `{text}`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::HttpClient;

#[derive(Serialize)]
pub struct WotchStatus {
    /// Whether the `wotch` binary can be located on this system.
    pub installed: bool,
    /// Absolute path to the binary (only when `installed`).
    pub path: Option<String>,
    /// Whether Wotch's HTTP API is currently reachable (port file exists
    /// AND `GET /v1/health` responds within a short timeout).
    pub running: bool,
    /// Active port read from `~/.wotch/api-port` (only when `running`).
    pub port: Option<u16>,
}

#[derive(Serialize)]
pub struct WotchLaunchResult {
    /// True if Wotch was spawned OR was already running and received the prompt.
    pub spawned: bool,
    /// True if the initial prompt was pushed to Wotch via its HTTP API.
    pub prompt_pushed: bool,
    /// Human-readable note for logs / UI toast.
    pub message: String,
}

/// Probe current Wotch availability. Used by the frontend on mount to
/// decide whether to render the "Open in Wotch" button at all.
#[tauri::command]
pub async fn wotch_status(app: AppHandle) -> Result<WotchStatus, String> {
    let path = detect_wotch_path();
    let client = app.state::<HttpClient>().0.clone();
    let (running, port) = probe_wotch_api(&client).await;
    Ok(WotchStatus {
        installed: path.is_some(),
        path: path.as_ref().map(|p| p.to_string_lossy().to_string()),
        running,
        port,
    })
}

/// Spawn Wotch (or reuse a running instance) and, if `initial_prompt` is
/// non-empty, start a new Wotch tab with `claude "<prompt>"` typed.
#[tauri::command]
pub async fn launch_wotch(
    app: AppHandle,
    initial_prompt: Option<String>,
) -> Result<WotchLaunchResult, String> {
    // Per CLAUDE.md item 1: reuse the shared HTTP client from Tauri
    // state instead of building a per-request reqwest::Client. The
    // shared client also pools connections, which matters here
    // because await_wotch_api_up() loops the probe every 250 ms.
    let client = app.state::<HttpClient>().0.clone();

    let prompt = initial_prompt
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Fast path: if Wotch is already running, just push the prompt.
    let (running, port) = probe_wotch_api(&client).await;
    if running {
        if let (Some(p), Some(port)) = (prompt.as_ref(), port) {
            let pushed = push_prompt_to_wotch(&client, port, p).await.is_ok();
            return Ok(WotchLaunchResult {
                spawned: true,
                prompt_pushed: pushed,
                message: if pushed {
                    "Wotch already running — opened new tab with prompt.".into()
                } else {
                    "Wotch already running — failed to push prompt (see logs).".into()
                },
            });
        }
        // Running but no prompt requested.
        return Ok(WotchLaunchResult {
            spawned: true,
            prompt_pushed: false,
            message: "Wotch already running.".into(),
        });
    }

    // Cold-start: detect the binary and spawn it.
    let Some(path) = detect_wotch_path() else {
        return Err(
            "Wotch is not installed or not on PATH. Visit \
             https://github.com/Frostbite1536/Wotch/releases to install."
                .into(),
        );
    };

    let path_for_spawn = path.clone();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new(&path_for_spawn)
            .spawn()
            .map_err(|e| format!("Failed to spawn {}: {e}", path_for_spawn.display()))
    })
    .await
    .map_err(|e| format!("spawn task failed: {e}"))??;

    let mut prompt_pushed = false;
    if let Some(p) = prompt {
        // Wotch's API server takes a moment to come up. Poll for up to 5s
        // rather than sleep blind.
        if let Some(active_port) =
            await_wotch_api_up(&client, std::time::Duration::from_secs(5)).await
        {
            match push_prompt_to_wotch(&client, active_port, &p).await {
                Ok(()) => prompt_pushed = true,
                Err(e) => eprintln!("[wotch] push_prompt_to_wotch failed: {e}"),
            }
        } else {
            eprintln!("[wotch] API never came up after cold start");
        }
    }

    Ok(WotchLaunchResult {
        spawned: true,
        prompt_pushed,
        message: if prompt_pushed {
            "Launched Wotch with prompt.".into()
        } else {
            "Launched Wotch.".into()
        },
    })
}

// ── Internal helpers ────────────────────────────────────────────────────

/// Try to locate the `wotch` binary on this system.
fn detect_wotch_path() -> Option<PathBuf> {
    // 1. PATH lookup (works for Linux .deb installs).
    if let Some(found) = which_wotch() {
        return Some(found);
    }
    // 2. Platform-specific known install locations.
    for candidate in platform_install_candidates() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn which_wotch() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "Wotch.exe" } else { "wotch" };
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn platform_install_candidates() -> Vec<PathBuf> {
    #[allow(unused_mut)]
    let mut out: Vec<PathBuf> = Vec::new();
    #[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(unused_variables))]
    let home = dirs_next::home_dir();

    #[cfg(target_os = "windows")]
    {
        if let Some(local) = dirs_next::data_local_dir() {
            out.push(local.join("Programs").join("Wotch").join("Wotch.exe"));
        }
        if let Ok(pf) = std::env::var("PROGRAMFILES") {
            out.push(PathBuf::from(pf).join("Wotch").join("Wotch.exe"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from("/Applications/Wotch.app/Contents/MacOS/Wotch"));
        if let Some(h) = home.as_ref() {
            out.push(h.join("Applications/Wotch.app/Contents/MacOS/Wotch"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        out.push(PathBuf::from("/usr/bin/wotch"));
        out.push(PathBuf::from("/usr/local/bin/wotch"));
        if let Some(h) = home.as_ref() {
            out.push(h.join(".local/bin/wotch"));
            for dir in &["Applications", ".local/bin", "bin"] {
                let candidate_dir = h.join(dir);
                if let Ok(entries) = std::fs::read_dir(&candidate_dir) {
                    for entry in entries.flatten() {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        let lower = name_str.to_lowercase();
                        if lower.starts_with("wotch") && lower.ends_with(".appimage") {
                            out.push(entry.path());
                        }
                    }
                }
            }
        }
    }

    out
}

/// Read Wotch's port file + probe `/v1/health`. Returns `(running, port)`.
async fn probe_wotch_api(client: &reqwest::Client) -> (bool, Option<u16>) {
    let Some(home) = dirs_next::home_dir() else {
        return (false, None);
    };
    let port_path = home.join(".wotch").join("api-port");
    let port = match std::fs::read_to_string(&port_path) {
        Ok(s) => s.trim().parse::<u16>().ok(),
        Err(_) => None,
    };
    let Some(port) = port else { return (false, None) };

    // Cheap TCP-connect probe BEFORE the HTTPS handshake. The shared
    // HttpClient's connect_timeout is 10 s (suitable for real LLM
    // endpoints). Without this guard, every probe to a closed port
    // waits the full 10 s, which serialises the 250 ms poll loop and
    // makes the 5-second `await_wotch_api_up` deadline always fire
    // on cold start when Wotch is taking >2 s to come up.
    let tcp_probe = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await;
    if !matches!(tcp_probe, Ok(Ok(_))) {
        return (false, None);
    }

    // /v1/health requires auth per Wotch's middleware; send the token too.
    let token = std::fs::read_to_string(home.join(".wotch").join("api-token"))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Per-request timeout (2s overall) on the shared client — short
    // probes that should never hang the launch flow.
    let ok = client
        .get(format!("http://127.0.0.1:{port}/v1/health"))
        .timeout(std::time::Duration::from_secs(2))
        .bearer_auth(&token)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    // Keep (running, port) consistent — a stale `api-port` file from a
    // crashed Wotch session would otherwise return `(false, Some(port))`
    // and tempt future callers into using a port nothing is listening on.
    if ok {
        (true, Some(port))
    } else {
        (false, None)
    }
}

/// Poll Wotch's API for up to `timeout`, returning the active port once
/// the health endpoint responds.
async fn await_wotch_api_up(
    client: &reqwest::Client,
    timeout: std::time::Duration,
) -> Option<u16> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let (up, port) = probe_wotch_api(client).await;
        if up {
            return port;
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

#[derive(Deserialize)]
struct TabCreateResponse {
    ok: bool,
    #[serde(default)]
    data: Option<TabCreateData>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct TabCreateData {
    #[serde(rename = "tabId")]
    tab_id: String,
}

/// POST a new tab + push `claude "<prompt>"\r` into it via Wotch's HTTP API.
async fn push_prompt_to_wotch(
    client: &reqwest::Client,
    port: u16,
    prompt: &str,
) -> Result<(), String> {
    let home = dirs_next::home_dir().ok_or("no home dir")?;
    let token = std::fs::read_to_string(home.join(".wotch").join("api-token"))
        .map_err(|e| format!("read ~/.wotch/api-token: {e}"))?
        .trim()
        .to_string();

    // Per-request timeout (10s overall) on the shared client. Wotch's
    // tab-create + first input call should always finish well under that.
    let req_timeout = std::time::Duration::from_secs(10);

    // 1. Create a tab.
    let tab_resp: TabCreateResponse = client
        .post(format!("http://127.0.0.1:{port}/v1/tabs"))
        .timeout(req_timeout)
        .bearer_auth(&token)
        .json(&serde_json::json!({ "cwd": home.to_string_lossy() }))
        .send()
        .await
        .map_err(|e| format!("POST /v1/tabs: {e}"))?
        .json()
        .await
        .map_err(|e| format!("parse /v1/tabs response: {e}"))?;
    if !tab_resp.ok {
        return Err(format!(
            "Wotch /v1/tabs refused: {}",
            tab_resp.error.unwrap_or_else(|| "unknown".into())
        ));
    }
    let tab_id = tab_resp
        .data
        .ok_or("Wotch /v1/tabs response missing data")?
        .tab_id;

    // 2. Push the prompt. Wotch writes `body.data` verbatim to the PTY.
    let command = format!("claude {}\r", shell_quote(prompt));
    let input_resp = client
        .post(format!("http://127.0.0.1:{port}/v1/tabs/{tab_id}/input"))
        .timeout(req_timeout)
        .bearer_auth(&token)
        .json(&serde_json::json!({ "data": command }))
        .send()
        .await
        .map_err(|e| format!("POST /v1/tabs/:id/input: {e}"))?;

    // .send() resolves to Ok on any HTTP response, including 4xx / 5xx —
    // without the status check Wotch could silently reject the input
    // (auth failure, tab already closed, rate limit) and the caller would
    // still clear the input box thinking the prompt landed.
    if !input_resp.status().is_success() {
        let status = input_resp.status();
        let body = input_resp.text().await.unwrap_or_default();
        return Err(format!(
            "Wotch /v1/tabs/{tab_id}/input returned {status}: {body}"
        ));
    }

    Ok(())
}

/// Quote `s` for the shell that hosts the Wotch PTY.
///
/// POSIX (bash/zsh): wrap in single quotes, escape literal single
/// quotes via the `'\''` close-escape-reopen idiom.
///
/// Windows (PowerShell, the typical Claude Code host on Windows per
/// the Wotch docs): wrap in single quotes, escape literal single
/// quotes by doubling them ('). PowerShell does NOT understand the
/// POSIX `'\''` sequence — it parses the backslash literally.
///
/// Cmd.exe is not a supported host for `claude` invocation; users on
/// cmd would have hit other problems before this. The Windows branch
/// produces a string that PowerShell parses correctly; on cmd it
/// passes the inner single quotes through as literals (broken, but
/// out of scope for v1).
fn shell_quote(s: &str) -> String {
    if cfg!(windows) {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_empty() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn shell_quote_plain() {
        assert_eq!(shell_quote("hello world"), "'hello world'");
    }

    #[test]
    #[cfg(not(windows))]
    fn shell_quote_with_single_quote_posix() {
        // bash: close, escape literal, reopen
        assert_eq!(shell_quote("can't stop"), r"'can'\''t stop'");
    }

    #[test]
    #[cfg(windows)]
    fn shell_quote_with_single_quote_powershell() {
        // PowerShell: double the single quote inside ''
        assert_eq!(shell_quote("can't stop"), "'can''t stop'");
    }
}
