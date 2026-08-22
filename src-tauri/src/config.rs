use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static CONFIG: Mutex<Option<AppConfig>> = Mutex::new(None);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    #[serde(default)]
    pub api_keys: HashMap<String, String>,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_true")]
    pub auto_screenshot: bool,
    #[serde(default)]
    pub tts_enabled: bool,
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    /// TTS provider: "elevenlabs" (default, backward compatible) or "gemini".
    #[serde(default = "default_tts_provider")]
    pub tts_provider: String,
    #[serde(default = "default_stt_provider")]
    pub stt_provider: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub tutor_mode: bool,
    /// Monitor index to capture ("auto" or numeric index as string).
    /// Stored as a proper field rather than in `api_keys` — it's a display
    /// preference, not a credential.
    #[serde(default = "default_capture_monitor")]
    pub capture_monitor: String,
    /// Whether the full-screen cursor overlay is enabled. When on, the model
    /// can visually point at elements on screen via an animated cursor.
    /// Default false — opt-in because the overlay adds a second WebView.
    #[serde(default)]
    pub cursor_overlay_enabled: bool,
    /// Whether accessibility-API-powered UI detection is enabled. When on,
    /// WorkBuddy reads element names/bounding-rects from the foreground
    /// window's a11y tree for pixel-precise pointing in IDEs and terminals.
    /// Default true — data stays local.
    #[serde(default = "default_true")]
    pub a11y_detection_enabled: bool,
    /// Privacy toggle: when true, the browser extension's scan data has
    /// user-entered form values (input/textarea) replaced with a type-aware
    /// placeholder like `[input: email]` before being sent to the LLM. Field
    /// position and type are preserved for context; user-entered text is
    /// scrubbed. Default false — current behavior is preserved. Password
    /// fields are already skipped by the extension regardless of this flag.
    #[serde(default)]
    pub mask_form_inputs: bool,
    /// When the Anthropic `highlight` tool fires AND the browser extension
    /// is connected with fresh data, route the highlight to the extension's
    /// in-page CSS overlay (native to the page, scrolls with content) instead
    /// of the full-screen Tauri cursor overlay. Default true (ADR-033).
    /// Turning this off forces the cursor overlay for both tools regardless
    /// of extension state.
    #[serde(default = "default_true")]
    pub extension_highlight_enabled: bool,
    /// Whether the "Open in Wotch" button appears in the chat toolbar and
    /// whether `launch_wotch` is wired. Default true. Turning off hides
    /// the button without affecting anything else.
    #[serde(default = "default_true")]
    pub wotch_integration_enabled: bool,
    /// Work-journal recorder (ADR-042). Default OFF — writing screenshots
    /// to disk consciously revokes the old INV-SEC-004 "screenshots never
    /// touch disk" invariant, so it must be an explicit opt-in.
    #[serde(default)]
    pub recorder_enabled: bool,
    /// Seconds between journal captures (Dayflow parity default: 10).
    #[serde(default = "default_recorder_interval")]
    pub recorder_interval_secs: u32,
    /// Days raw journal frames are kept before the hourly purge deletes
    /// them. Analysis products (observations, timeline cards) are kept.
    #[serde(default = "default_recorder_retention")]
    pub recorder_retention_days: u32,
    /// Monitor(s) the journal recorder captures. Empty = follow
    /// `capture_monitor` (legacy behavior). "all" = composite every
    /// monitor side-by-side per frame. Numeric string = explicit index.
    /// Separate from `capture_monitor` because assistant screenshots
    /// need a single monitor (pointing coordinates are per-monitor),
    /// while the journal wants the whole desk.
    #[serde(default)]
    pub journal_capture_monitor: String,
    /// Provider used for journal batch analysis. Empty = use the chat
    /// provider. Kept separate so a cheap/local model can do analysis.
    #[serde(default)]
    pub analysis_provider: String,
    /// Model used for journal batch analysis. Empty = use the chat model.
    #[serde(default)]
    pub analysis_model: String,
}

impl Default for AppConfig {
    /// Single source of truth for factory defaults. Used for first-launch
    /// initialization AND as the fallback when an existing config file is
    /// unreadable or corrupt — both paths now produce the same values.
    fn default() -> Self {
        Self {
            api_keys: HashMap::new(),
            provider: default_provider(),
            model: default_model(),
            auto_screenshot: true,
            tts_enabled: false,
            tts_voice: default_tts_voice(),
            tts_provider: default_tts_provider(),
            stt_provider: default_stt_provider(),
            theme: default_theme(),
            tutor_mode: false,
            capture_monitor: default_capture_monitor(),
            cursor_overlay_enabled: false,
            a11y_detection_enabled: true,
            mask_form_inputs: false,
            extension_highlight_enabled: true,
            wotch_integration_enabled: true,
            recorder_enabled: false,
            recorder_interval_secs: default_recorder_interval(),
            recorder_retention_days: default_recorder_retention(),
            journal_capture_monitor: String::new(),
            analysis_provider: String::new(),
            analysis_model: String::new(),
        }
    }
}

fn default_recorder_interval() -> u32 {
    10
}

fn default_recorder_retention() -> u32 {
    14
}

fn default_tts_provider() -> String {
    "elevenlabs".to_string()
}

fn default_capture_monitor() -> String {
    "auto".to_string()
}

fn default_provider() -> String {
    "anthropic".to_string()
}

fn default_model() -> String {
    "claude-sonnet-4-20250514".to_string()
}

fn default_tts_voice() -> String {
    "default".to_string()
}

fn default_stt_provider() -> String {
    "whisper".to_string()
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_true() -> bool {
    true
}

fn config_path() -> Result<PathBuf, String> {
    let base = dirs_next::config_dir()
        .ok_or_else(|| "Could not determine config directory".to_string())?;
    let dir = base.join("workbuddy");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
    Ok(dir.join("config.json"))
}

fn load_config() -> AppConfig {
    let path = match config_path() {
        Ok(p) => p,
        Err(_) => return AppConfig::default(),
    };
    if path.exists() {
        // Missing fields are filled via per-field `#[serde(default = ...)]`,
        // and a wholly unreadable/corrupt file falls back to the same
        // `AppConfig::default()` used for first launch. Both paths now go
        // through one source of truth (the `Default` impl).
        let data = fs::read_to_string(&path).unwrap_or_default();
        match serde_json::from_str::<AppConfig>(&data) {
            Ok(cfg) => cfg,
            Err(e) => {
                // O6 audit: a corrupt config.json silently resets every
                // setting (including all API keys + cohort enrollment).
                // Preserve the bad file under a timestamped suffix so a
                // postmortem is possible, and log loudly so the operator
                // can pull it from the OS log file (added in M-tier
                // ops work).
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let backup = path.with_file_name(format!(
                    "config.json.corrupt-{}",
                    ts
                ));
                match fs::rename(&path, &backup) {
                    Ok(_) => eprintln!(
                        "[config] corrupt JSON ({}). Quarantined to {} \
                         and reset to defaults.",
                        e,
                        backup.display()
                    ),
                    Err(rename_err) => eprintln!(
                        "[config] corrupt JSON ({}); quarantine failed ({}). \
                         Resetting to defaults — corrupt file STAYS at {}.",
                        e,
                        rename_err,
                        path.display()
                    ),
                }
                AppConfig::default()
            }
        }
    } else {
        let config = AppConfig::default();
        save_config(&config);
        config
    }
}

fn save_config(config: &AppConfig) {
    let path = match config_path() {
        Ok(p) => p,
        Err(_) => return,
    };
    if let Ok(data) = serde_json::to_string_pretty(config) {
        // Write with restrictive permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let _ = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(data.as_bytes())
                });
        }
        #[cfg(not(unix))]
        {
            let _ = fs::write(&path, &data);
        }
    }
}

/// Read config without writing to disk (public). Recovers from poisoned mutex.
pub fn with_config_pub<F, R>(f: F) -> R
where
    F: FnOnce(&AppConfig) -> R,
{
    with_config(f)
}

/// Read config without writing to disk. Recovers from poisoned mutex.
fn with_config<F, R>(f: F) -> R
where
    F: FnOnce(&AppConfig) -> R,
{
    let mut lock = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    if lock.is_none() {
        *lock = Some(load_config());
    }
    f(lock.as_ref().unwrap())
}

/// Read and mutate config, then persist to disk. Recovers from poisoned mutex.
fn with_config_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut AppConfig) -> R,
{
    let mut lock = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
    if lock.is_none() {
        *lock = Some(load_config());
    }
    let config = lock.as_mut().unwrap();
    let result = f(config);
    save_config(config);
    result
}

/// Read an API key from local config. Used internally by other modules.
pub fn read_api_key(service: &str) -> Result<String, String> {
    with_config(|config| {
        config
            .api_keys
            .get(service)
            .cloned()
            .ok_or_else(|| format!("No API key configured for {service}"))
    })
}

#[tauri::command]
pub fn get_api_key(service: String) -> Result<String, String> {
    read_api_key(&service)
}

#[tauri::command]
pub fn set_api_key(service: String, key: String) -> Result<(), String> {
    with_config_mut(|config| {
        config.api_keys.insert(service, key);
    });
    Ok(())
}

/// Validate an Anthropic API key by making a lightweight request.
#[tauri::command]
pub async fn validate_api_key(
    app: tauri::AppHandle,
    key: String,
) -> Result<bool, String> {
    use crate::HttpClient;
    use tauri::Manager;

    let client = &app.state::<HttpClient>().0;

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    Ok(response.status().is_success())
}

/// Returns the full config including API keys.
/// Keys are sent over local IPC only (never over the network) and are needed
/// by the Settings page to pre-fill input fields. INV-SEC-001 and INV-SEC-003
/// ensure keys never leave the machine except to their target APIs.
#[tauri::command]
pub fn get_settings() -> Result<AppConfig, String> {
    Ok(with_config(|config| config.clone()))
}

/// Update non-sensitive settings. API keys are managed separately via set_api_key.
#[tauri::command]
pub fn set_settings(settings: AppConfig) -> Result<(), String> {
    with_config_mut(|config| {
        config.provider = settings.provider;
        config.model = settings.model;
        config.auto_screenshot = settings.auto_screenshot;
        config.tts_enabled = settings.tts_enabled;
        config.tts_voice = settings.tts_voice;
        config.tts_provider = settings.tts_provider;
        config.stt_provider = settings.stt_provider;
        config.theme = settings.theme;
        config.tutor_mode = settings.tutor_mode;
        config.capture_monitor = settings.capture_monitor;
        config.cursor_overlay_enabled = settings.cursor_overlay_enabled;
        config.a11y_detection_enabled = settings.a11y_detection_enabled;
        config.mask_form_inputs = settings.mask_form_inputs;
        config.extension_highlight_enabled = settings.extension_highlight_enabled;
        config.wotch_integration_enabled = settings.wotch_integration_enabled;
        config.recorder_enabled = settings.recorder_enabled;
        config.recorder_interval_secs = settings.recorder_interval_secs;
        config.recorder_retention_days = settings.recorder_retention_days;
        config.journal_capture_monitor = settings.journal_capture_monitor;
        config.analysis_provider = settings.analysis_provider;
        config.analysis_model = settings.analysis_model;
        // api_keys intentionally NOT copied — use set_api_key for key management
    });
    Ok(())
}
