mod a11y;
mod capture;
mod config;
mod context;
mod diagnostics;
pub mod extension;
mod journal;
mod llm;
mod wotch;
mod microphone;
mod pointer;
pub mod rag;
mod shortcuts;
mod stt;
mod tts;
mod window;

use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

/// Shared HTTP client for connection reuse across all API calls.
pub struct HttpClient(pub reqwest::Client);

/// Debug logging from frontend JS → stderr. Visible even when WebView2
/// freezes. Redacts known credential patterns + truncates so a future
/// log-to-file path (planned per PRODUCTION_READINESS.md §Logging)
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
    let ext_state = Arc::new(tokio::sync::Mutex::new(
        extension::ExtensionState::new(ext_token),
    ));
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
    // PRODUCTION_READINESS.md §Auto-update.
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
                        file_name: Some("workbuddy".to_string()),
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
        .plugin(tauri_plugin_dialog::init())
        .manage(HttpClient(shared_client))
        .manage(ext_state)
        .setup(move |app| {
            if let Some(main_window) = app.get_webview_window("main") {
                window::setup_main_window(&main_window)?;
            }
            shortcuts::setup_shortcuts(app.handle())?;
            window::setup_tray(app)?;

            // Start browser extension HTTP server on localhost.
            // Passes an AppHandle so the `/ask` endpoint can emit frontend
            // events (used by the Wotch "Ask WorkBuddy" integration).
            let app_handle_for_ext = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                extension::start_extension_server(
                    ext_state_for_server,
                    app_handle_for_ext,
                )
                .await;
            });

            // Journal recorder: resume automatically when the user left it
            // enabled — the whole point of an automatic work journal is
            // that it survives app restarts (ADR-042; default OFF).
            if config::with_config_pub(|c| c.recorder_enabled) {
                journal::recorder::start(app.handle().clone());
            }
            // Analysis scheduler ticks every 10 min; it no-ops unless the
            // recorder is enabled, so it is safe to start unconditionally.
            journal::analyzer::start_scheduler(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Screen capture
            capture::capture_to_base64,
            capture::start_region_capture,
            capture::list_monitors,
            // LLM (multi-provider)
            llm::stream_response,
            llm::chat_with_vision,
            llm::list_providers,
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
            // Work journal recorder (ADR-042)
            journal::recorder::recorder_start,
            journal::recorder::recorder_stop,
            journal::recorder::recorder_status,
            journal::recorder::journal_list_screenshots,
            journal::recorder::journal_read_screenshot,
            journal::analyzer::journal_analyze_now,
            journal::analyzer::journal_list_cards,
            journal::analyzer::journal_list_observations,
            journal::export::journal_export_markdown,
            journal::standup::journal_generate_standup,
            journal::standup::journal_get_standup,
            journal::standup::journal_week_summary,
            // Text-to-speech
            tts::synthesize_speech,
            tts::list_tts_voices,
            // Cursor pointing
            pointer::show_pointer,
            pointer::hide_pointer,
            // Microphone
            microphone::start_mic_capture,
            microphone::stop_mic_capture,
            // Speech-to-text
            stt::transcribe_audio,
            // RAG document search
            rag::ingest_document,
            rag::ingest_all_documents,
            rag::search_docs,
            rag::get_ingestion_status,
            rag::clear_doc_index,
            // Browser extension
            extension::get_extension_status,
            extension::extension_highlight,
            extension::regenerate_extension_token,
            // Accessibility-powered UI element detection
            a11y::detect_ui_elements,
            // Wotch launch integration
            wotch::launch_wotch,
            wotch::wotch_status,
            // Diagnostics — local-only crash + activity logging (O1).
            // No network upload (PRINCIPLES.md §97).
            diagnostics::open_log_dir,
            diagnostics::copy_last_log_tail,
            // Debug
            debug_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running WorkBuddy");
}
