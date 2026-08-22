use crate::config;
use crate::HttpClient;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager};

const MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024; // 10MB

/// Generation counter for stream cancellation. Each new stream_response
/// increments this. If the value changes during streaming, the old stream
/// knows it's been superseded and should abort.
static STREAM_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Supported LLM providers.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Anthropic,
    Openai,
    Google,
    Groq,
    Ollama,
    // The frontend stores and sends "openrouter" (see list_providers ids),
    // but rename_all = "snake_case" would map this variant to
    // "open_router" — so selecting OpenRouter made stream_response fail
    // to deserialize its `provider` arg. Rename to match the frontend;
    // keep the old snake_case spelling as an alias for any persisted
    // value that used it.
    #[serde(rename = "openrouter", alias = "open_router")]
    OpenRouter,
}

impl Default for Provider {
    fn default() -> Self {
        Provider::Anthropic
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: serde_json::Value,
}

/// Provider-specific configuration.
struct ProviderConfig {
    api_url: String,
    api_key: String,
    auth_header: String,
    extra_headers: Vec<(String, String)>,
    uses_anthropic_format: bool,
}

/// Accept only loopback Ollama URLs. Ollama runs unauthenticated, so any
/// non-loopback host would leak prompts and screenshots to an arbitrary
/// endpoint on a misconfigured or tampered config.
fn validate_ollama_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .ok_or_else(|| {
            format!("Ollama URL must start with http:// or https:// (got {trimmed})")
        })?;
    // Strip optional path and port to isolate the host.
    let host_port = rest.split('/').next().unwrap_or(rest);
    // Bracketed IPv6 (e.g. `[::1]:11434`) carries colons inside the brackets,
    // so `split(':').next()` would chop at the first colon and produce `"["`.
    // Slice through the closing bracket instead.
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        match rest.find(']') {
            Some(end) => &host_port[..=end + 1],
            None => host_port,
        }
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    let is_loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    if !is_loopback {
        return Err(format!(
            "Ollama URL must point at localhost or 127.0.0.1 (got host {host})"
        ));
    }
    Ok(trimmed.to_string())
}

fn get_provider_config(provider: &Provider) -> Result<ProviderConfig, String> {
    match provider {
        Provider::Anthropic => Ok(ProviderConfig {
            api_url: "https://api.anthropic.com/v1/messages".into(),
            api_key: config::read_api_key("anthropic")?,
            auth_header: "x-api-key".into(),
            extra_headers: vec![
                ("anthropic-version".into(), "2023-06-01".into()),
            ],
            uses_anthropic_format: true,
        }),
        Provider::Openai => Ok(ProviderConfig {
            api_url: "https://api.openai.com/v1/chat/completions".into(),
            api_key: config::read_api_key("openai")?,
            auth_header: "Authorization".into(),
            extra_headers: vec![],
            uses_anthropic_format: false,
        }),
        Provider::Google => Ok(ProviderConfig {
            api_url: "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".into(),
            api_key: config::read_api_key("google")?,
            auth_header: "Authorization".into(),
            extra_headers: vec![],
            uses_anthropic_format: false,
        }),
        Provider::Groq => Ok(ProviderConfig {
            api_url: "https://api.groq.com/openai/v1/chat/completions".into(),
            api_key: config::read_api_key("groq")?,
            auth_header: "Authorization".into(),
            extra_headers: vec![],
            uses_anthropic_format: false,
        }),
        Provider::Ollama => {
            let raw = config::read_api_key("ollama_url")
                .unwrap_or_else(|_| "http://localhost:11434".into());
            // Ollama has no auth header, so a non-loopback URL would silently
            // send prompts (and screenshots) to an arbitrary host. Enforce
            // loopback-only at call time — defense against a corrupted config.
            let base_url = validate_ollama_url(raw.trim())?;
            Ok(ProviderConfig {
                api_url: format!("{base_url}/v1/chat/completions"),
                api_key: String::new(), // Ollama doesn't need a key
                auth_header: String::new(),
                extra_headers: vec![],
                uses_anthropic_format: false,
            })
        }
        Provider::OpenRouter => Ok(ProviderConfig {
            api_url: "https://openrouter.ai/api/v1/chat/completions".into(),
            api_key: config::read_api_key("openrouter")?,
            auth_header: "Authorization".into(),
            extra_headers: vec![],
            uses_anthropic_format: false,
        }),
    }
}

fn format_auth(config: &ProviderConfig) -> String {
    if config.auth_header == "Authorization" && !config.api_key.is_empty() {
        format!("Bearer {}", config.api_key)
    } else {
        config.api_key.clone()
    }
}

fn build_image_block_anthropic(base64: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": "image/jpeg",
            "data": base64,
        }
    })
}

fn build_image_block_openai(base64: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "image_url",
        "image_url": {
            "url": format!("data:image/jpeg;base64,{base64}"),
            "detail": "auto"
        }
    })
}

fn build_content_blocks(
    user_message: &str,
    screenshot_base64: &Option<String>,
    anthropic_format: bool,
) -> serde_json::Value {
    match screenshot_base64 {
        Some(ref img) if !img.is_empty() => {
            let image_block = if anthropic_format {
                build_image_block_anthropic(img)
            } else {
                build_image_block_openai(img)
            };
            serde_json::json!([
                image_block,
                { "type": "text", "text": user_message }
            ])
        }
        _ => serde_json::Value::String(user_message.to_string()),
    }
}

fn build_request_body(
    provider_config: &ProviderConfig,
    model: &str,
    system_prompt: &str,
    messages: &[ChatMessage],
    has_screenshot: bool,
    use_pointing_tools: bool,
) -> serde_json::Value {
    if provider_config.uses_anthropic_format {
        // Anthropic Messages API. Only attach the cursor/highlight tool
        // definitions when a screenshot is present AND the user has enabled
        // the cursor overlay — without an image the model has no real pixel
        // coordinates and would hallucinate positions.
        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": 4096,
            "system": system_prompt,
            "messages": messages,
            "stream": true,
        });
        if has_screenshot && use_pointing_tools {
            body["tools"] = serde_json::json!([
                {
                    "name": "point_at",
                    "description": "Point an animated cursor at a specific location on the student's screen. Use SPARINGLY — only when the student needs to find a specific element. Write your full explanation first, then call point_at ONCE at the end if needed. If a DETECTED UI ELEMENTS list is present in the system prompt, use its coordinates verbatim.",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "x": { "type": "number", "description": "X pixel coordinate in the screenshot" },
                            "y": { "type": "number", "description": "Y pixel coordinate in the screenshot" },
                            "label": { "type": "string", "description": "1-3 word label for the element (e.g., 'Place Order button')" }
                        },
                        "required": ["x", "y", "label"]
                    }
                },
                {
                    "name": "highlight",
                    "description": "Highlight a rectangular region on the student's screen with a spotlight effect. Use for larger areas like panels, sections, or code blocks. When the region size is known (e.g., from a rect in DETECTED UI ELEMENTS), include `width` and `height` so the highlight matches the element; otherwise they default to a 120x40 box centered on (x, y).",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "x": { "type": "number", "description": "X coordinate of the region center" },
                            "y": { "type": "number", "description": "Y coordinate of the region center" },
                            "width": { "type": "number", "description": "Region width in pixels (optional; default 120)" },
                            "height": { "type": "number", "description": "Region height in pixels (optional; default 40)" },
                            "label": { "type": "string", "description": "1-3 word label for the region" }
                        },
                        "required": ["x", "y", "label"]
                    }
                }
            ]);
        }
        body
    } else {
        // OpenAI-compatible Chat Completions API
        let mut all_messages = vec![serde_json::json!({
            "role": "system",
            "content": system_prompt,
        })];
        for msg in messages {
            all_messages.push(serde_json::json!({
                "role": msg.role,
                "content": msg.content,
            }));
        }
        serde_json::json!({
            "model": model,
            "max_tokens": 4096,
            "messages": all_messages,
            "stream": true,
        })
    }
}

/// Stream a response from any supported LLM provider.
#[tauri::command]
pub async fn stream_response(
    app: AppHandle,
    system_prompt: String,
    user_message: String,
    screenshot_base64: Option<String>,
    conversation_history: Vec<ChatMessage>,
    model: Option<String>,
    provider: Option<Provider>,
    use_pointing_tools: Option<bool>,
) -> Result<(), String> {
    let provider = provider.unwrap_or_default();
    let provider_config = get_provider_config(&provider)?;

    let default_model = match provider {
        Provider::Anthropic => "claude-sonnet-4-20250514",
        Provider::Openai => "gpt-4o",
        Provider::Google => "gemini-2.5-flash",
        Provider::Groq => "llama-3.3-70b-versatile",
        Provider::Ollama => "llama3.2-vision",
        Provider::OpenRouter => "anthropic/claude-sonnet-4-20250514",
    };
    let model = model.unwrap_or_else(|| default_model.to_string());
    let client = &app.state::<HttpClient>().0;

    let content = build_content_blocks(
        &user_message,
        &screenshot_base64,
        provider_config.uses_anthropic_format,
    );

    let mut messages = conversation_history;
    messages.push(ChatMessage {
        role: "user".to_string(),
        content,
    });

    let has_image = screenshot_base64.as_ref().map_or(false, |s| !s.is_empty());
    let tools_enabled = use_pointing_tools.unwrap_or(false);
    let body = build_request_body(&provider_config, &model, &system_prompt, &messages, has_image, tools_enabled);

    // Increment generation — any previously running stream will see this and abort
    let my_generation = STREAM_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    eprintln!(
        "[llm] stream_response: gen={} provider={:?} model={} prompt_len={} has_image={} history={}",
        my_generation, provider, model, system_prompt.len(), has_image, messages.len()
    );

    let mut request = client
        .post(&provider_config.api_url)
        .header("content-type", "application/json");

    // Add auth header (skip for Ollama which has no key)
    if !provider_config.auth_header.is_empty() {
        request = request.header(
            &provider_config.auth_header,
            format_auth(&provider_config),
        );
    }

    // Add provider-specific headers
    for (key, value) in &provider_config.extra_headers {
        request = request.header(key.as_str(), value.as_str());
    }

    let response = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("LLM API error ({status}): {body}"));
    }

    if !provider_config.uses_anthropic_format {
        parse_openai_stream(&app, response, my_generation).await?;
        return Ok(());
    }

    // CONTRACT: every Ok(()) return path from this function MUST have
    // emitted `chat_stream_complete` exactly once. Err(...) returns
    // intentionally do NOT — the frontend's invoke().catch handler
    // is responsible for clearing isStreaming on rejection. Mixing
    // both signals on the same flow would race (catch fires AND the
    // listener fires) and double-fire the post-stream persistence
    // path (saveTurn would write twice).
    //
    // Anthropic: parse stream and handle tool_use continuation loop.
    // When the model calls tools (point_at, highlight), Anthropic stops the
    // response with stop_reason=tool_use. We send back tool results so the
    // model can continue its explanation.
    let mut output = parse_anthropic_stream(&app, response, my_generation).await?;
    let mut continuation_count = 0;
    const MAX_CONTINUATIONS: usize = 2; // keep response snappy

    while !output.tool_calls.is_empty() && continuation_count < MAX_CONTINUATIONS {
        continuation_count += 1;
        eprintln!("[llm] Tool continuation #{}: {} tool calls, sending results back",
            continuation_count, output.tool_calls.len());

        // Check if superseded
        if STREAM_GENERATION.load(Ordering::SeqCst) != my_generation {
            eprintln!("[llm] gen={} superseded during tool continuation", my_generation);
            let _ = app.emit("chat_stream_complete", ());
            return Ok(());
        }

        // Build the assistant message with text + tool_use blocks
        let mut assistant_content: Vec<serde_json::Value> = Vec::new();
        if !output.text.is_empty() {
            assistant_content.push(serde_json::json!({
                "type": "text",
                "text": output.text,
            }));
        }
        for tc in &output.tool_calls {
            assistant_content.push(serde_json::json!({
                "type": "tool_use",
                "id": tc.id,
                "name": tc.name,
                "input": tc.input,
            }));
        }

        // Build tool_result messages
        let mut tool_results: Vec<serde_json::Value> = Vec::new();
        for tc in &output.tool_calls {
            tool_results.push(serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tc.id,
                "content": format!("Done — {} shown to student.", tc.name),
            }));
        }

        // Add assistant + tool_result to messages
        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: serde_json::Value::Array(assistant_content),
        });
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: serde_json::Value::Array(tool_results),
        });

        // Make a new API request to continue the response
        let body = build_request_body(&provider_config, &model, &system_prompt, &messages, has_image, tools_enabled);
        let mut request = client
            .post(&provider_config.api_url)
            .header("content-type", "application/json");
        if !provider_config.auth_header.is_empty() {
            request = request.header(
                &provider_config.auth_header,
                format_auth(&provider_config),
            );
        }
        for (key, value) in &provider_config.extra_headers {
            request = request.header(key.as_str(), value.as_str());
        }

        let response = request
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Tool continuation request failed: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            eprintln!("[llm] Tool continuation API error ({status}): {body_text}");
            let _ = app.emit("chat_stream_complete", ());
            return Ok(());
        }

        output = parse_anthropic_stream(&app, response, my_generation).await?;
    }

    // All done — emit completion
    let _ = app.emit("chat_stream_complete", ());
    Ok(())
}

/// Payload emitted when a tool_use content block completes.
#[derive(Serialize, Clone)]
struct ToolUsePayload {
    name: String,
    input: serde_json::Value,
}

/// Completed tool call — includes the id needed for tool_result messages.
#[derive(Clone)]
struct CompletedToolCall {
    id: String,
    name: String,
    input: serde_json::Value,
}

/// What the stream produced — text content and/or tool calls.
struct StreamOutput {
    text: String,
    tool_calls: Vec<CompletedToolCall>,
}

/// Parse Anthropic SSE: `event: <type>\ndata: <json>\n\n`
/// Handles both text content blocks and tool_use content blocks.
/// Returns the accumulated text and any tool calls so the caller can
/// continue the conversation with tool results if needed.
async fn parse_anthropic_stream(
    app: &AppHandle,
    response: reqwest::Response,
    generation: u64,
) -> Result<StreamOutput, String> {
    #[derive(Deserialize)]
    struct StreamEvent {
        #[serde(rename = "type")]
        event_type: String,
        delta: Option<Delta>,
        content_block: Option<ContentBlock>,
    }
    #[derive(Deserialize)]
    struct Delta {
        text: Option<String>,
        partial_json: Option<String>,
    }
    #[derive(Deserialize)]
    struct ContentBlock {
        #[serde(rename = "type")]
        block_type: Option<String>,
        id: Option<String>,
        name: Option<String>,
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut current_event_type = String::new();
    // Tool-use state tracking
    let mut current_tool_name: Option<String> = None;
    let mut current_tool_id: Option<String> = None;
    let mut tool_json_buffer = String::new();
    let mut completed_tool_calls: Vec<CompletedToolCall> = Vec::new();
    // Helper: on an early return, move out the accumulated completed tool
    // calls so the tool-continuation loop in `stream_response` can still
    // fire any pointers the model finished before the stream went quiet.
    // Partial (in-progress) tool blocks are intentionally dropped — their
    // JSON is incomplete.
    let mut full_text = String::new();
    // Track last time we received actual content (not just keep-alive pings).
    let mut last_content_time: Option<std::time::Instant> = None;
    let content_timeout = std::time::Duration::from_secs(30);
    let stream_start = std::time::Instant::now();
    let mut chunk_count: u64 = 0;
    let mut last_heartbeat = std::time::Instant::now();

    let mut text_bytes: usize = 0;
    // Batch text emissions to reduce WebView2 IPC pressure on Windows.
    // Instead of emitting per-delta (100-300 events), accumulate and emit
    // every ~80ms (reducing to ~100 events). This prevents the IPC bridge
    // from overwhelming the Win32 message pump on transparent windows.
    let mut pending_text = String::new();
    let mut last_emit_time = std::time::Instant::now();
    const EMIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(80);

    loop {
        // 2min timeout per raw chunk (catches complete connection drops)
        let chunk = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            stream.next(),
        ).await {
            Ok(Some(chunk)) => chunk.map_err(|e| format!("Stream error: {e}"))?,
            Ok(None) => {
                eprintln!("[llm] Stream ended normally after {} chunks, {:.1}s, {} text chars",
                    chunk_count, stream_start.elapsed().as_secs_f32(), text_bytes);
                break;
            }
            Err(_) => {
                // On timeout, flushing pending_text IS correct — the stream
                // is still active for this request and the text is wanted.
                if !pending_text.is_empty() {
                    let _ = app.emit("chat_stream_chunk", &pending_text);
                }
                eprintln!("[llm] Raw chunk timeout after 120s, {} chunks, {} text bytes", chunk_count, text_bytes);
                // Hand back completed tool calls so the continuation loop
                // can still fire pointers the model finished before the
                // stream stalled. Partial tool JSON is dropped.
                return Ok(StreamOutput { text: full_text, tool_calls: completed_tool_calls });
            }
        };

        chunk_count += 1;

        // Check if a newer stream has started — if so, abort this one.
        // Do NOT flush pending_text: the frontend has already cleared its
        // buffer for the new stream, so any flushed text would be prepended
        // to the new response and produce garbled output.
        if STREAM_GENERATION.load(Ordering::SeqCst) != generation {
            eprintln!("[llm] gen={} superseded, dropping {}b of pending text",
                generation, pending_text.len());
            // Superseded: the frontend has started a new stream and does
            // not want any more events for this one, including tool calls
            // that would fire a pointer after the user moved on.
            return Ok(StreamOutput { text: full_text, tool_calls: vec![] });
        }

        // Heartbeat every 10s so we can see if the loop is still running
        if last_heartbeat.elapsed().as_secs() >= 10 {
            let content_age = last_content_time.map(|t| t.elapsed().as_secs()).unwrap_or(0);
            eprintln!("[llm] heartbeat: {}s elapsed, {} chunks, {} text chars, last_content={}s ago, buf={}b",
                stream_start.elapsed().as_secs(), chunk_count, text_bytes, content_age, buffer.len());
            last_heartbeat = std::time::Instant::now();
        }

        // Check if we've received actual content recently (pings don't count).
        if let Some(t) = last_content_time {
            if t.elapsed() > content_timeout {
                if !pending_text.is_empty() {
                    let _ = app.emit("chat_stream_chunk", &pending_text);
                }
                eprintln!("[llm] Content timeout after {:.1}s — no text for 30s, {} chunks, {} text bytes",
                    stream_start.elapsed().as_secs_f32(), chunk_count, text_bytes);
                // Preserve completed tool calls — see note above.
                return Ok(StreamOutput { text: full_text, tool_calls: completed_tool_calls });
            }
        } else if stream_start.elapsed().as_secs() > 60 {
            // No content received at all after 60s — model never started generating
            eprintln!("[llm] First-content timeout — no text received after 60s");
            return Ok(StreamOutput { text: full_text, tool_calls: completed_tool_calls });
        }

        if buffer.len() + chunk.len() > MAX_BUFFER_SIZE {
            return Err("Stream buffer exceeded maximum size".to_string());
        }
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                if current_event_type == "message_stop" {
                    // Flush any remaining batched text
                    if !pending_text.is_empty() {
                        let _ = app.emit("chat_stream_chunk", &pending_text);
                        pending_text.clear();
                    }
                    eprintln!("[llm] message_stop received — {} text bytes, {} tool calls, {:.1}s",
                        text_bytes, completed_tool_calls.len(), stream_start.elapsed().as_secs_f32());
                    // Caller (stream_response) handles chat_stream_complete emission
                    return Ok(StreamOutput { text: full_text, tool_calls: completed_tool_calls });
                }
                current_event_type.clear();
                continue;
            }

            if let Some(event_type) = line.strip_prefix("event: ") {
                current_event_type = event_type.to_string();
                // Log non-routine events
                if event_type != "content_block_delta" && event_type != "ping" {
                    eprintln!("[llm] event: {} at {:.1}s", event_type, stream_start.elapsed().as_secs_f32());
                }
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(event) = serde_json::from_str::<StreamEvent>(data) {
                    match event.event_type.as_str() {
                        "content_block_start" => {
                            // Check if this is a tool_use block
                            if let Some(ref block) = event.content_block {
                                if block.block_type.as_deref() == Some("tool_use") {
                                    eprintln!("[llm] tool_use block started: {:?} id={:?}", block.name, block.id);
                                    current_tool_name = block.name.clone();
                                    current_tool_id = block.id.clone();
                                    tool_json_buffer.clear();
                                } else {
                                    eprintln!("[llm] content block started: {:?}", block.block_type);
                                }
                            }
                        }
                        "content_block_delta" => {
                            if let Some(ref delta) = event.delta {
                                // Text content → accumulate for batched emission
                                if let Some(ref text) = delta.text {
                                    text_bytes += text.len();
                                    full_text.push_str(text);
                                    pending_text.push_str(text);
                                    last_content_time = Some(std::time::Instant::now());
                                    // Emit batch when interval elapsed or buffer large enough
                                    if last_emit_time.elapsed() >= EMIT_INTERVAL || pending_text.len() > 200 {
                                        let _ = app.emit("chat_stream_chunk", &pending_text);
                                        pending_text.clear();
                                        last_emit_time = std::time::Instant::now();
                                    }
                                }
                                // Tool input JSON → only accumulate when inside a tool_use block.
                                // These deltas also count as real content so the stream
                                // isn't killed during a long tool-only response.
                                if current_tool_name.is_some() {
                                    if let Some(ref partial) = delta.partial_json {
                                        tool_json_buffer.push_str(partial);
                                        last_content_time = Some(std::time::Instant::now());
                                    }
                                }
                            }
                        }
                        "content_block_stop" => {
                            // Flush any pending text before handling block stop
                            if !pending_text.is_empty() {
                                let _ = app.emit("chat_stream_chunk", &pending_text);
                                pending_text.clear();
                                last_emit_time = std::time::Instant::now();
                            }
                            // If we were accumulating tool input, emit the completed tool call
                            if let Some(tool_name) = current_tool_name.take() {
                                let tool_id = current_tool_id.take().unwrap_or_default();
                                eprintln!("[llm] tool_use complete: {} id={} ({}b JSON)", tool_name, tool_id, tool_json_buffer.len());
                                match serde_json::from_str::<serde_json::Value>(&tool_json_buffer) {
                                    Ok(input) => {
                                        let _ = app.emit("tool_use_complete", ToolUsePayload {
                                            name: tool_name.clone(),
                                            input: input.clone(),
                                        });
                                        completed_tool_calls.push(CompletedToolCall {
                                            id: tool_id,
                                            name: tool_name,
                                            input,
                                        });
                                    }
                                    Err(e) => {
                                        eprintln!("[llm] Failed to parse tool_use JSON for '{}': {} — buffer ({}b): {}",
                                            tool_name, e, tool_json_buffer.len(),
                                            &tool_json_buffer[..tool_json_buffer.len().min(200)]);
                                    }
                                }
                                tool_json_buffer.clear();
                            }
                        }
                        _ => {}
                    }
                } else if data.len() > 2 {
                    // Log unparseable data (but not empty objects)
                    eprintln!("[llm] unparseable SSE data ({}b): {}",
                        data.len(), &data[..data.len().min(200)]);
                }
            }
        }
    }

    eprintln!("[llm] Anthropic stream ended (loop exited), {} text bytes", text_bytes);
    // Caller (stream_response) handles chat_stream_complete emission
    Ok(StreamOutput { text: full_text, tool_calls: completed_tool_calls })
}

/// Parse OpenAI-compatible SSE: `data: <json>\n\n` with `data: [DONE]` sentinel
async fn parse_openai_stream(
    app: &AppHandle,
    response: reqwest::Response,
    generation: u64,
) -> Result<(), String> {
    #[derive(Deserialize)]
    struct StreamChunk {
        choices: Option<Vec<Choice>>,
    }
    #[derive(Deserialize)]
    struct Choice {
        delta: Option<ChoiceDelta>,
    }
    #[derive(Deserialize)]
    struct ChoiceDelta {
        content: Option<String>,
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    // Batch text emissions (same rationale as Anthropic parser)
    let mut pending_text = String::new();
    let mut last_emit_time = std::time::Instant::now();
    const EMIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(80);

    loop {
        // Check if superseded by a newer stream
        if STREAM_GENERATION.load(Ordering::SeqCst) != generation {
            eprintln!("[llm] OpenAI gen={} superseded, aborting", generation);
            return Ok(());
        }

        let chunk = match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            stream.next(),
        ).await {
            Ok(Some(chunk)) => chunk.map_err(|e| format!("Stream error: {e}"))?,
            Ok(None) => break,
            Err(_) => {
                if !pending_text.is_empty() {
                    let _ = app.emit("chat_stream_chunk", &pending_text);
                }
                eprintln!("[llm] Stream timeout — no data for 30s, ending response");
                let _ = app.emit("chat_stream_complete", ());
                return Ok(());
            }
        };

        if buffer.len() + chunk.len() > MAX_BUFFER_SIZE {
            let _ = app.emit("chat_stream_complete", ());
            return Err("Stream buffer exceeded maximum size".to_string());
        }
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    if !pending_text.is_empty() {
                        let _ = app.emit("chat_stream_chunk", &pending_text);
                    }
                    let _ = app.emit("chat_stream_complete", ());
                    return Ok(());
                }
                if let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) {
                    if let Some(choices) = chunk.choices {
                        if let Some(choice) = choices.first() {
                            if let Some(delta) = &choice.delta {
                                if let Some(text) = &delta.content {
                                    pending_text.push_str(text);
                                    if last_emit_time.elapsed() >= EMIT_INTERVAL || pending_text.len() > 200 {
                                        let _ = app.emit("chat_stream_chunk", &pending_text);
                                        pending_text.clear();
                                        last_emit_time = std::time::Instant::now();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !pending_text.is_empty() {
        let _ = app.emit("chat_stream_chunk", &pending_text);
    }
    let _ = app.emit("chat_stream_complete", ());
    Ok(())
}

/// Non-streaming single-shot request. Useful for quick queries or
/// providers that don't support streaming.
#[tauri::command]
pub async fn chat_with_vision(
    app: AppHandle,
    system_prompt: String,
    user_message: String,
    screenshot_base64: Option<String>,
    conversation_history: Option<Vec<ChatMessage>>,
    model: Option<String>,
    provider: Option<Provider>,
) -> Result<String, String> {
    let provider = provider.unwrap_or_default();
    let provider_config = get_provider_config(&provider)?;

    let default_model = match provider {
        Provider::Anthropic => "claude-sonnet-4-20250514",
        Provider::Openai => "gpt-4o",
        Provider::Google => "gemini-2.5-flash",
        Provider::Groq => "llama-3.3-70b-versatile",
        Provider::Ollama => "llama3.2-vision",
        Provider::OpenRouter => "anthropic/claude-sonnet-4-20250514",
    };
    let model = model.unwrap_or_else(|| default_model.to_string());
    let client = &app.state::<HttpClient>().0;

    let content = build_content_blocks(
        &user_message,
        &screenshot_base64,
        provider_config.uses_anthropic_format,
    );

    let mut messages = conversation_history.unwrap_or_default();
    messages.push(ChatMessage {
        role: "user".to_string(),
        content,
    });

    // Build non-streaming request — pass has_screenshot=false so tool
    // definitions are never attached (tool_use responses aren't
    // event-emitted in non-streaming mode anyway).
    let mut body = build_request_body(&provider_config, &model, &system_prompt, &messages, false, false);
    if let Some(obj) = body.as_object_mut() {
        obj.insert("stream".into(), serde_json::Value::Bool(false));
    }

    let mut request = client
        .post(&provider_config.api_url)
        .header("content-type", "application/json");

    if !provider_config.auth_header.is_empty() {
        request = request.header(
            &provider_config.auth_header,
            format_auth(&provider_config),
        );
    }
    for (key, value) in &provider_config.extra_headers {
        request = request.header(key.as_str(), value.as_str());
    }

    let response = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("LLM API error ({status}): {body}"));
    }

    let resp: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    if provider_config.uses_anthropic_format {
        // Anthropic: { content: [{ type: "text", text: "..." }, ...] }
        // Iterate all content blocks — some may be tool_use, not text
        let text = resp["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b["type"].as_str() == Some("text") {
                            b["text"].as_str()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.is_empty() {
            Err("No text in Anthropic response".to_string())
        } else {
            Ok(text)
        }
    } else {
        // OpenAI: { choices: [{ message: { content: "..." } }] }
        resp["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "No text in response".to_string())
    }
}

/// Non-streaming text completion for engine consumers (PLAN-01 style
/// pass). Shares the app's `HttpClient` and the same request shape as
/// `chat_with_vision` (which it delegates to) — never a fresh client.
/// Adds an explicit 30 s per-request timeout: unlike the streaming chat
/// path there is no heartbeat to detect a stalled connection.
pub async fn complete_text(
    app: AppHandle,
    system_prompt: String,
    user_prompt: String,
    model: Option<String>,
    provider: Option<Provider>,
) -> Result<String, String> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        chat_with_vision(app, system_prompt, user_prompt, None, None, model, provider),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err("style pass timed out after 30s".to_string()),
    }
}

/// Currently configured provider + model for engine-internal LLM calls.
/// `None` when the configured provider string is unknown.
pub fn configured_provider_and_model() -> Option<(Provider, String)> {
    let (provider_str, model) =
        config::with_config_pub(|c| (c.provider.clone(), c.model.clone()));
    let provider = provider_from_str(&provider_str)?;
    Some((provider, model))
}

/// Parse a provider id string as the frontend stores it ("openrouter",
/// "anthropic", ...). Serde's snake_case would want "open_router", so a
/// manual map is the honest way to accept the settings values.
pub fn provider_from_str(s: &str) -> Option<Provider> {
    match s.trim().to_ascii_lowercase().as_str() {
        "anthropic" => Some(Provider::Anthropic),
        "openai" => Some(Provider::Openai),
        "google" => Some(Provider::Google),
        "groq" => Some(Provider::Groq),
        "ollama" => Some(Provider::Ollama),
        "openrouter" | "open_router" => Some(Provider::OpenRouter),
        _ => None,
    }
}

/// Default model per provider — single source shared by the chat commands
/// above. (The base repo's journal analyzer was the other consumer; it is
/// removed with the journal feature.)
pub fn default_model_for(provider: &Provider) -> &'static str {
    match provider {
        Provider::Anthropic => "claude-sonnet-4-20250514",
        Provider::Openai => "gpt-4o",
        Provider::Google => "gemini-2.5-flash",
        Provider::Groq => "llama-3.3-70b-versatile",
        Provider::Ollama => "llama3.2-vision",
        Provider::OpenRouter => "anthropic/claude-sonnet-4-20250514",
    }
}

/// Non-streaming completion with MULTIPLE images. Provenance: built for the
/// base repo's journal batch analysis; kept API-intact per ledger W7 until
/// the PLAN-07 prune. Current consumers: none.
/// No SSE, no events, no tools — returns the raw text so the caller can
/// parse/validate JSON and drive its own retry loop. Uses the shared
/// HttpClient with an explicit per-request timeout (this path has no
/// streaming heartbeat to detect stalls).
pub async fn complete_with_images(
    app: &AppHandle,
    provider: &Provider,
    model: &str,
    system_prompt: &str,
    user_text: &str,
    images_base64: &[String],
) -> Result<String, String> {
    let provider_config = get_provider_config(provider)?;
    let client = &app.state::<HttpClient>().0;

    let content: serde_json::Value = if images_base64.is_empty() {
        serde_json::Value::String(user_text.to_string())
    } else {
        let mut blocks: Vec<serde_json::Value> = images_base64
            .iter()
            .map(|img| {
                if provider_config.uses_anthropic_format {
                    build_image_block_anthropic(img)
                } else {
                    build_image_block_openai(img)
                }
            })
            .collect();
        blocks.push(serde_json::json!({ "type": "text", "text": user_text }));
        serde_json::Value::Array(blocks)
    };

    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content,
    }];
    let mut body =
        build_request_body(&provider_config, model, system_prompt, &messages, false, false);
    if let Some(obj) = body.as_object_mut() {
        obj.insert("stream".into(), serde_json::Value::Bool(false));
        obj.insert("max_tokens".into(), serde_json::json!(8192));
    }

    let mut request = client
        .post(&provider_config.api_url)
        .header("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(180));
    if !provider_config.auth_header.is_empty() {
        request = request.header(&provider_config.auth_header, format_auth(&provider_config));
    }
    for (key, value) in &provider_config.extra_headers {
        request = request.header(key.as_str(), value.as_str());
    }

    let response = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let preview: String = body.chars().take(500).collect();
        return Err(format!("LLM API error ({status}): {preview}"));
    }

    let resp: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    if provider_config.uses_anthropic_format {
        let text = resp["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b["type"].as_str() == Some("text") {
                            b["text"].as_str()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.is_empty() {
            Err("No text in Anthropic response".to_string())
        } else {
            Ok(text)
        }
    } else {
        resp["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "No text in response".to_string())
    }
}

/// Return the list of available providers and their default models.
#[tauri::command]
pub fn list_providers() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "id": "anthropic",
            "name": "Anthropic (Claude)",
            "key_required": true,
            "models": [
                { "id": "claude-sonnet-4-20250514", "name": "Claude Sonnet 4 (recommended)" },
                { "id": "claude-opus-4-1-20250805", "name": "Claude Opus 4.1 (most capable)" },
                { "id": "claude-haiku-4-5-20251001", "name": "Claude Haiku 4.5 (fastest)" },
            ]
        }),
        serde_json::json!({
            "id": "openai",
            "name": "OpenAI (GPT)",
            "key_required": true,
            "models": [
                { "id": "gpt-4o", "name": "GPT-4o (recommended)" },
                { "id": "gpt-4o-mini", "name": "GPT-4o Mini (faster/cheaper)" },
                { "id": "gpt-4.1", "name": "GPT-4.1 (latest)" },
            ]
        }),
        serde_json::json!({
            "id": "google",
            "name": "Google (Gemini)",
            "key_required": true,
            "models": [
                { "id": "gemini-2.5-flash", "name": "Gemini 2.5 Flash (recommended)" },
                { "id": "gemini-2.5-pro", "name": "Gemini 2.5 Pro (most capable)" },
            ]
        }),
        serde_json::json!({
            "id": "groq",
            "name": "Groq (fast, free tier)",
            "key_required": true,
            "models": [
                { "id": "llama-3.3-70b-versatile", "name": "Llama 3.3 70B (recommended)" },
                { "id": "llama-3.1-8b-instant", "name": "Llama 3.1 8B (fastest)" },
                { "id": "mixtral-8x7b-32768", "name": "Mixtral 8x7B" },
            ]
        }),
        serde_json::json!({
            "id": "ollama",
            "name": "Ollama (local, free)",
            "key_required": false,
            "models": [
                { "id": "llama3.2-vision", "name": "Llama 3.2 Vision (recommended)" },
                { "id": "llama3.2", "name": "Llama 3.2" },
                { "id": "mistral", "name": "Mistral 7B" },
                { "id": "gemma2", "name": "Gemma 2" },
            ]
        }),
        serde_json::json!({
            "id": "openrouter",
            "name": "OpenRouter (100+ models)",
            "key_required": true,
            "models": [
                { "id": "anthropic/claude-sonnet-4-20250514", "name": "Claude Sonnet 4 via OpenRouter" },
                { "id": "openai/gpt-4o", "name": "GPT-4o via OpenRouter" },
                { "id": "google/gemini-2.5-flash", "name": "Gemini Flash via OpenRouter" },
                { "id": "meta-llama/llama-3.3-70b-instruct", "name": "Llama 3.3 70B via OpenRouter" },
            ]
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_ollama_url — INV-LLM (loopback-only Ollama) ─────
    //
    // Ollama has no auth header. A non-loopback URL would silently
    // ship every prompt + screenshot to an arbitrary host. The
    // validator is the single defense; tests cover the obvious
    // bypass attempts.

    #[test]
    fn ollama_url_accepts_localhost() {
        assert!(validate_ollama_url("http://localhost:11434").is_ok());
        assert!(validate_ollama_url("http://localhost").is_ok());
        assert!(validate_ollama_url("http://localhost:11434/").is_ok());
    }

    #[test]
    fn ollama_url_accepts_loopback_ipv4_and_ipv6() {
        assert!(validate_ollama_url("http://127.0.0.1:11434").is_ok());
        assert!(validate_ollama_url("http://[::1]:11434").is_ok());
    }

    #[test]
    fn ollama_url_strips_trailing_slash() {
        let v = validate_ollama_url("http://localhost:11434/").unwrap();
        assert_eq!(v, "http://localhost:11434");
    }

    #[test]
    fn ollama_url_rejects_arbitrary_host() {
        assert!(validate_ollama_url("http://attacker.example.com:11434").is_err());
        assert!(validate_ollama_url("http://192.168.1.10:11434").is_err());
        assert!(validate_ollama_url("https://api.example.com").is_err());
    }

    #[test]
    fn ollama_url_rejects_missing_scheme() {
        assert!(validate_ollama_url("localhost:11434").is_err());
        assert!(validate_ollama_url("//localhost:11434").is_err());
        assert!(validate_ollama_url("").is_err());
    }

    #[test]
    fn ollama_url_does_not_match_localhost_substring_in_path() {
        // host parsing splits on '/' before the host portion, so a path
        // segment containing `localhost` cannot smuggle through.
        assert!(validate_ollama_url("http://attacker.com/localhost").is_err());
        assert!(validate_ollama_url("http://localhost.attacker.com").is_err());
    }

    // ── format_auth — provider-specific Authorization header shape ──

    #[test]
    fn format_auth_bearer_for_authorization_header() {
        let cfg = ProviderConfig {
            api_url: String::new(),
            api_key: "sk-abc".into(),
            auth_header: "Authorization".into(),
            extra_headers: vec![],
            uses_anthropic_format: false,
        };
        assert_eq!(format_auth(&cfg), "Bearer sk-abc");
    }

    #[test]
    fn format_auth_bare_for_x_api_key() {
        // Anthropic uses x-api-key with the raw key (no Bearer prefix).
        let cfg = ProviderConfig {
            api_url: String::new(),
            api_key: "sk-ant-abc".into(),
            auth_header: "x-api-key".into(),
            extra_headers: vec![],
            uses_anthropic_format: true,
        };
        assert_eq!(format_auth(&cfg), "sk-ant-abc");
    }

    #[test]
    fn format_auth_empty_when_no_key() {
        // Ollama path — empty key shouldn't get a "Bearer " prefix.
        let cfg = ProviderConfig {
            api_url: String::new(),
            api_key: String::new(),
            auth_header: String::new(),
            extra_headers: vec![],
            uses_anthropic_format: false,
        };
        assert_eq!(format_auth(&cfg), "");
    }

    // ── Image block builders — payload shape per provider ────────

    #[test]
    fn anthropic_image_block_uses_base64_source() {
        let v = build_image_block_anthropic("ABCDEF==");
        assert_eq!(v["type"], "image");
        assert_eq!(v["source"]["type"], "base64");
        assert_eq!(v["source"]["media_type"], "image/jpeg");
        assert_eq!(v["source"]["data"], "ABCDEF==");
    }

    #[test]
    fn openai_image_block_uses_data_uri() {
        let v = build_image_block_openai("ABCDEF==");
        assert_eq!(v["type"], "image_url");
        assert_eq!(v["image_url"]["url"], "data:image/jpeg;base64,ABCDEF==");
        assert_eq!(v["image_url"]["detail"], "auto");
    }

    // ── Provider wire format — must match the frontend's ids ─────
    //
    // The Settings page and list_providers use "openrouter" (one word).
    // A rename_all=snake_case enum would deserialize only "open_router",
    // silently breaking OpenRouter chat. Pin every provider id.

    #[test]
    fn provider_deserializes_frontend_ids() {
        for (id, expected) in [
            ("anthropic", Provider::Anthropic),
            ("openai", Provider::Openai),
            ("google", Provider::Google),
            ("groq", Provider::Groq),
            ("ollama", Provider::Ollama),
            ("openrouter", Provider::OpenRouter),
        ] {
            let parsed: Provider = serde_json::from_str(&format!("\"{id}\""))
                .unwrap_or_else(|e| panic!("'{id}' failed to deserialize: {e}"));
            assert_eq!(parsed, expected, "id '{id}'");
        }
        // Legacy alias from the pre-fix serialization stays accepted.
        let legacy: Provider = serde_json::from_str("\"open_router\"").unwrap();
        assert_eq!(legacy, Provider::OpenRouter);
    }

    #[test]
    fn provider_serializes_to_frontend_ids() {
        assert_eq!(serde_json::to_string(&Provider::OpenRouter).unwrap(), "\"openrouter\"");
        assert_eq!(serde_json::to_string(&Provider::Anthropic).unwrap(), "\"anthropic\"");
    }

    #[test]
    fn provider_from_str_agrees_with_serde() {
        for id in ["anthropic", "openai", "google", "groq", "ollama", "openrouter"] {
            let via_serde: Provider = serde_json::from_str(&format!("\"{id}\"")).unwrap();
            assert_eq!(provider_from_str(id), Some(via_serde), "id '{id}'");
        }
        assert_eq!(provider_from_str("open_router"), Some(Provider::OpenRouter));
        assert_eq!(provider_from_str("bogus"), None);
    }

    // ── STREAM_GENERATION — supersede counter ────────────────────

    #[test]
    fn atomic_u64_supersede_contract() {
        // Captures the contract relied on by the parsers' supersede
        // checks: a fresh fetch_add gives a strictly-higher value
        // than the last observed generation, and a load after that
        // returns the new value. We exercise this against a LOCAL
        // AtomicU64 rather than the production STREAM_GENERATION
        // counter — under cargo test's parallel-by-default runner,
        // mutating the real counter from a unit test would race any
        // future integration test that asserts a specific generation
        // value (PR #33 P2 audit).
        use std::sync::atomic::AtomicU64;
        let counter = AtomicU64::new(0);
        let g1 = counter.fetch_add(1, Ordering::SeqCst);
        let g2 = counter.fetch_add(1, Ordering::SeqCst);
        assert!(g2 > g1, "fetch_add must return strictly-greater values");
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
