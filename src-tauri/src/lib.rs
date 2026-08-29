mod a11y;
mod analytics;
mod apply;
mod chat_store;
mod clipboard;
mod config;
mod context;
mod diagnostics;
pub mod engine;
pub mod extension;
mod input_inject;
mod llm;
mod secrets;
mod shortcuts;
mod snip_hook;
mod text_monitor;
mod widget;
mod window;

use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

/// Shared HTTP client for connection reuse across all API calls.
pub struct HttpClient(pub reqwest::Client);

/// Debug logging from frontend JS → stderr. Visible even when WebView2
/// freezes. Redacts known credential patterns + truncates so a future
/// log-to-file path (planned per docs/base-archive/PRODUCTION_READINESS.md §Logging)
/// can't accidentally promote PII or API keys into a persisted file.
#[tauri::command]
fn debug_log(message: String) {
    eprintln!("[js] {}", redact_for_log(&message));
}

/// Strip API-key shapes + truncate to 200 chars. Mirrors (loosely)
/// the redactor.ts patterns; we keep it conservative because anything
/// the JS layer sends here is suspect by construction.
fn redact_for_log(s: &str) -> String {
    // Drop ASCII control bytes that could confuse a downstream log
    // parser (newline injection in particular).
    let mut cleaned: String = s
        .chars()
        .filter(|c| !(c.is_control() && *c != ' '))
        .collect();
    // Cap length first so the regex passes are bounded.
    if cleaned.len() > 200 {
        cleaned.truncate(200);
        cleaned.push('…');
    }
    // Crude pattern coverage — just gut the obvious credential shapes.
    let patterns: &[(&str, &str)] = &[
        // sk-ant-..., sk-..., AIza... and 20+-char base64 runs
        // adjacent to credential context words.
        ("sk-ant-", "[key]"),
        ("sk-", "[key]"),
        ("AIza", "[key]"),
    ];
    for (needle, replacement) in patterns {
        if let Some(idx) = cleaned.find(needle) {
            cleaned.replace_range(idx..cleaned.len(), replacement);
            break;
        }
    }
    cleaned
}

pub fn run() {
    // No global request timeout — SSE streaming responses can run for minutes.
    // Non-streaming callers (TTS, STT, embeddings) set per-request timeouts.
    let shared_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(4)
        .build()
        .expect("failed to build HTTP client");

    // Browser extension: localhost HTTP server for instant element detection
    let ext_token = extension::load_or_create_token()
        .expect("CSPRNG unavailable — cannot generate extension auth token");
    let ext_state = Arc::new(tokio::sync::Mutex::new(extension::ExtensionState::new(
        ext_token,
    )));
    let ext_state_for_server = ext_state.clone();

    // Install the panic hook BEFORE building so any builder-time panic
    // (window setup, shortcut registration, extension server) reaches
    // the log file and not just stderr. Idempotent — safe to call
    // even if a future entry point already set a hook.
    diagnostics::install_panic_hook();

    // Note: tauri-plugin-updater intentionally NOT registered. Re-adding
    // it requires (a) generating a signing key with `tauri signer
    // generate`, (b) committing the public key to tauri.conf.json
    // updater.pubkey, (c) wiring TAURI_SIGNING_PRIVATE_KEY +
    // _PASSWORD into release.yml secrets. Until then, manual updates
    // via GitHub releases are the only safe path. See
    // docs/base-archive/PRODUCTION_READINESS.md §Auto-update.
    tauri::Builder::default()
        // Local-only diagnostic logging (O1 / PRINCIPLES.md §97).
        // Writes to OS-conventional app_log_dir; rotates when each
        // file hits 5 MB; KeepOne so disk usage stays bounded.
        // Settings → Diagnostics surfaces "Open log directory" +
        // "Copy last 5 MB" buttons sourced from this stream.
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("wordbuddy".to_string()),
                    },
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .manage(HttpClient(shared_client))
        .manage(ext_state)
        .setup(move |app| {
            if let Some(main_window) = app.get_webview_window("main") {
                window::setup_main_window(&main_window)?;
            }
            shortcuts::setup_shortcuts(app.handle())?;
            window::setup_tray(app)?;

            // Native field monitor: start with the app when enabled
            // (default ON, PLAN-03 task 5).
            if config::with_config_pub(|c| c.native_monitoring_enabled) {
                text_monitor::start(app.handle().clone());
            }

            // Analytics: capture TZ offset once, start nightly scheduler
            // (PLAN-05 task 2). Retention purge runs off-thread so a
            // large first-time prune can't stall setup (audit M10).
            crate::analytics::aggregate::capture_local_offset();
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let days = config::with_config_pub(|c| c.analytics_retention_days);
                    match crate::analytics::jobs::run_retention_purge(days) {
                        Ok(summary) if !summary.is_empty() => {
                            eprintln!("[analytics] startup retention: {summary}")
                        }
                        Err(e) => eprintln!("[analytics] startup retention failed: {e}"),
                        _ => {}
                    }
                    let _ = &handle;
                });
            }
            crate::analytics::jobs::start_scheduler(app.handle().clone());

            // Snippet hook starts ONLY if explicitly enabled (ledger W6:
            // default OFF). Settings toggles it via snippet_hook_start.
            if config::with_config_pub(|c| c.snippets_enabled) {
                let triggers = config::with_config_pub(|c| {
                    c.snippets.iter().map(|s| s.trigger.clone()).collect()
                });
                let excluded = config::with_config_pub(|c| c.excluded_processes.clone());
                let _ = snip_hook::start(
                    snip_hook::HookConfig {
                        triggers,
                        excluded_processes: excluded,
                    },
                    app.handle().clone(),
                );
            }

            // Start browser extension HTTP server on localhost.
            // Passes an AppHandle so the `/ask` endpoint can emit frontend
            // events (external questions pushed into the chat bar).
            let app_handle_for_ext = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                extension::start_extension_server(ext_state_for_server, app_handle_for_ext).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // LLM (multi-provider)
            llm::stream_response,
            llm::chat_with_vision,
            llm::list_providers,
            chat_store::save_turn_atomic,
            // Config / API keys
            config::get_api_key,
            config::set_api_key,
            config::validate_api_key,
            config::get_settings,
            config::set_settings,
            // Window management
            window::set_window_height,
            window::toggle_visibility,
            window::show_main_window,
            // Context detection
            context::detect_active_window,
            // Check engine (CONTRACTS §1)
            engine::check_text_command,
            // Native field monitor (PLAN-03)
            text_monitor::monitor_start,
            text_monitor::monitor_stop,
            text_monitor::monitor_status,
            // Fix application + widget (PLAN-04)
            apply::apply_fix_command,
            analytics::jobs::record_rewrite_command,
            widget::widget_show_for,
            widget::widget_hide,
            widget::selection_capture,
            widget::widget_set_size,
            // Persistent rule mutes (widget Ignore)
            config::ignore_rule,
            config::reset_ignored_rules,
            config::exclude_process,
            config::add_dictionary_word,
            text_monitor::snooze_monitor,
            // Analytics (PLAN-05)
            analytics::jobs::analytics_summary,
            analytics::jobs::analytics_aggregate_now,
            analytics::jobs::analytics_report_markdown,
            analytics::jobs::analytics_export_report,
            // Snippets (PLAN-06; ledger W6 — hook OFF unless enabled)
            snip_hook::snippet_test,
            snip_hook::snippet_hook_start,
            snip_hook::snippet_hook_stop,
            snip_hook::snippet_hook_status,
            snip_hook::snippet_set_paused,
            // Browser extension
            extension::get_extension_status,
            extension::extension_highlight,
            extension::regenerate_extension_token,
            // Accessibility-powered UI element detection
            a11y::detect_ui_elements,
            a11y::check_a11y_permission,
            a11y::open_a11y_settings,
            // Diagnostics — local-only crash + activity logging (O1).
            // No network upload (PRINCIPLES.md §97).
            diagnostics::open_log_dir,
            diagnostics::copy_last_log_tail,
            // Debug
            debug_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WordBuddy");
}
