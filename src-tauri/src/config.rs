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
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub tutor_mode: bool,
    /// Whether accessibility-API-powered UI detection is enabled. When on,
    /// WordBuddy reads element names/bounding-rects from the foreground
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
    /// User-accepted vocabulary fed into the check engine's harper
    /// dictionary layer (PLAN-01; settings UI arrives in PLAN-06).
    #[serde(default)]
    pub personal_dictionary: Vec<String>,
    /// Master switch for browser-field checking (PLAN-02). Default on —
    /// the product's core promise. Excluded hosts are honored regardless.
    #[serde(default = "default_true")]
    pub browser_checking_enabled: bool,
    /// Hosts where the watcher never activates: no checks, no telemetry,
    /// no widget (INV-EXCL-001). Checked before any field-text read.
    #[serde(default)]
    pub excluded_hosts: Vec<String>,
    /// Native field monitoring (PLAN-03). First release defaults ON with
    /// a tray indicator while active.
    #[serde(default = "default_true")]
    pub native_monitoring_enabled: bool,
    /// Processes where the monitor never activates: no reads, no checks,
    /// no telemetry (INV-EXCL-001).
    #[serde(default)]
    pub excluded_processes: Vec<String>,
    /// Show the native suggestion card (PLAN-04).
    #[serde(default = "default_true")]
    pub widget_enabled: bool,
    /// Ctrl+Shift+W selection rewrite palette enabled.
    #[serde(default = "default_true")]
    pub selection_hotkey_enabled: bool,
    /// Writing goals shaping checks (CONTRACTS §1); edited in Settings.
    #[serde(default)]
    pub writing_goals: crate::engine::WritingGoals,
    /// Ordered personal style-guide replacement pairs (PLAN-06 task 3),
    /// applied as Delivery-kind issues after correctness.
    #[serde(default)]
    pub style_rules: Vec<crate::engine::StyleRule>,
    /// Retain short text samples for weekly tone analysis. Default OFF
    /// (INV-PRIV-002 exception requires explicit opt-in).
    #[serde(default)]
    pub retain_snippets: bool,
    /// Text-expansion snippets. The keyboard hook is OFF unless this is
    /// true AND the user starts it (ledger W6: default OFF).
    #[serde(default)]
    pub snippets_enabled: bool,
    /// Text-expansion definitions.
    #[serde(default)]
    pub snippets: Vec<crate::engine::Snippet>,
    /// Analytics retention window in days (audit M10): `check_events`,
    /// `rewrites`, `llm_calls`, `weekly_reports`, and exported report
    /// files older than this are purged at startup, nightly, and when
    /// the knob changes. 0 = keep forever (retention off).
    #[serde(default = "default_retention_days")]
    pub analytics_retention_days: u32,
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
            theme: default_theme(),
            tutor_mode: false,
            a11y_detection_enabled: true,
            mask_form_inputs: false,
            extension_highlight_enabled: true,
            personal_dictionary: Vec::new(),
            browser_checking_enabled: true,
            excluded_hosts: Vec::new(),
            native_monitoring_enabled: true,
            excluded_processes: Vec::new(),
            widget_enabled: true,
            selection_hotkey_enabled: true,
            writing_goals: Default::default(),
            style_rules: Vec::new(),
            retain_snippets: false,
            snippets_enabled: false,
            snippets: Vec::new(),
            analytics_retention_days: default_retention_days(),
        }
    }
}

fn default_retention_days() -> u32 {
    90
}



fn default_provider() -> String {
    "anthropic".to_string()
}

fn default_model() -> String {
    "claude-sonnet-4-20250514".to_string()
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
    let dir = base.join("wordbuddy");
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
            Ok(mut cfg) => {
                migrate_plaintext_keys(&mut cfg);
                cfg
            }
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

/// Audit M13 migration: any API keys still sitting in plaintext
/// config.json move into the OS vault, then the map is cleared and the
/// file rewritten without them. Idempotent — an already-migrated file
/// has no keys to move.
fn migrate_plaintext_keys(cfg: &mut AppConfig) {
    if cfg.api_keys.is_empty() {
        return;
    }
    let entries: Vec<(String, String)> = cfg
        .api_keys
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (service, key) in &entries {
        if let Err(e) = crate::secrets::set_secret(&crate::secrets::api_key_name(service), key) {
            // Keep this key in the file rather than lose it.
            eprintln!("[config] vault write for {service} failed ({e}); key kept in config.json");
            continue;
        }
        eprintln!("[config] migrated API key for '{service}' into OS vault");
    }
    cfg.api_keys.clear();
    save_config(cfg);
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

// API keys live in the OS vault (see secrets.rs) — audit M13. The
// vault-backed read_api_key/get_api_key/set_api_key are defined below;
// the plaintext-file versions they replaced were removed along with the
// migration in load_config.

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

/// Read an API key from the OS vault. Used internally by other modules.
pub fn read_api_key(service: &str) -> Result<String, String> {
    crate::secrets::get_secret(&crate::secrets::api_key_name(service))?
        .ok_or_else(|| format!("No API key configured for {service}"))
}

#[tauri::command]
pub fn get_api_key(service: String) -> Result<String, String> {
    read_api_key(&service)
}

/// Store an API key in the OS vault. Empty `key` deletes the credential.
#[tauri::command]
pub fn set_api_key(service: String, key: String) -> Result<(), String> {
    let name = crate::secrets::api_key_name(&service);
    if key.is_empty() {
        crate::secrets::delete_secret(&name);
    } else {
        crate::secrets::set_secret(&name, &key)?;
    }
    Ok(())
}
/// Update non-sensitive settings. API keys are managed separately via set_api_key.
#[tauri::command]
pub fn set_settings(settings: AppConfig) -> Result<(), String> {
    let retention_changed = with_config(|c| c.analytics_retention_days)
        != settings.analytics_retention_days;
    with_config_mut(|config| {
        config.provider = settings.provider;
        config.model = settings.model;
        config.theme = settings.theme;
        config.tutor_mode = settings.tutor_mode;
        config.a11y_detection_enabled = settings.a11y_detection_enabled;
        config.mask_form_inputs = settings.mask_form_inputs;
        config.extension_highlight_enabled = settings.extension_highlight_enabled;
        config.personal_dictionary = settings.personal_dictionary;
        config.browser_checking_enabled = settings.browser_checking_enabled;
        config.excluded_hosts = settings.excluded_hosts;
        config.native_monitoring_enabled = settings.native_monitoring_enabled;
        config.excluded_processes = settings.excluded_processes;
        config.widget_enabled = settings.widget_enabled;
        config.selection_hotkey_enabled = settings.selection_hotkey_enabled;
        config.writing_goals = settings.writing_goals;
        config.style_rules = settings.style_rules;
        config.retain_snippets = settings.retain_snippets;
        config.snippets_enabled = settings.snippets_enabled;
        config.snippets = settings.snippets;
        config.analytics_retention_days = settings.analytics_retention_days;
        // api_keys intentionally NOT copied — use set_api_key for key management
    });
    // Audit M10: apply the new retention window immediately so shrinking
    // it takes effect without waiting for the nightly pass.
    if retention_changed {
        let days = with_config(|c| c.analytics_retention_days);
        let summary = crate::analytics::jobs::run_retention_purge(days)
            .unwrap_or_else(|e| format!("retention purge failed: {e}"));
        eprintln!("[analytics] retention purge after settings change: {summary}");
    }
    Ok(())
}

/// Returns the full config including API keys.
/// Keys are sent over local IPC only (never over the network) and are needed
/// by the Settings page to pre-fill input fields. They are hydrated here
/// from the OS vault (audit M13) — config.json no longer holds them, and
/// INV-SEC-001/INV-SEC-003 ensure they never leave the machine except to
/// their target APIs.
#[tauri::command]
pub fn get_settings() -> Result<AppConfig, String> {
    let mut cfg = with_config(|config| config.clone());
    for provider in ["anthropic", "openai", "google", "groq", "ollama", "openrouter"] {
        if let Ok(Some(key)) =
            crate::secrets::get_secret(&crate::secrets::api_key_name(provider))
        {
            cfg.api_keys.insert(provider.to_string(), key);
        }
    }
    Ok(cfg)
}
