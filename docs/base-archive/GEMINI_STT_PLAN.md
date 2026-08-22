> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# Gemini Audio Input (STT) — Implementation Plan

**Goal:** Add Google Gemini as a third speech-to-text provider alongside
OpenAI Whisper and ElevenLabs STT. Reuses the user's existing Google API key
and gains Gemini's multimodal audio understanding capabilities.

**Status:** Planning
**Priority:** Medium — complements the Gemini TTS plan
**Scope:** Simple addition to existing `stt.rs` multi-provider dispatcher

**Google docs:**
https://ai.google.dev/gemini-api/docs/audio

---

## Why Add Gemini STT?

WorkBuddy already supports two STT providers. Adding a third Gemini-based
option means:

| Feature | Whisper | ElevenLabs Scribe | Gemini Flash |
|---------|---------|-------------------|----------------|
| Languages | 99+ | 99+ | 70+ |
| Cost | $0.006/min | Varies | ~$0.00192/min (32 tokens/sec × $1.00/1M tokens) |
| API key | OpenAI | ElevenLabs | Google (reused from LLM/TTS) |
| Context beyond transcription | No | No | **Yes** — understands emotion, non-speech sounds, can answer questions about audio |
| Real-time streaming | No | No | No (use Live API for that) |
| Timestamps | No (Whisper basic) | Yes | Yes (MM:SS via prompt) |
| Max audio length | 25 MB file | Varies | 9.5 hours (!) |
| Inline audio limit | ~25 MB | ~25 MB | 20 MB (request total) |
| Non-speech audio | No | No | **Yes** (birdsong, sirens, music) |

The "audio understanding" aspect is interesting for WorkBuddy: a student
could record a short clip of a trading stream or tutorial, and ask "what
concept is this explaining?" Gemini can both transcribe AND reason about
the content in a single call.

---

## API Essentials

### Endpoint
```
POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent
```

### Model
**Default:** `gemini-2.5-flash` (stable GA, supports audio input, cheap)
**Upgrade option:** `gemini-3-flash-preview` (newer, preview, may be less stable)

Why default to 2.5? The Gemini 3 Developer Guide notes: *"Gemini 3 Pro models
aren't designed around prioritizing supporting audio understanding... try using
models specifically built with those needs in mind."* While that note targets
3 Pro, pinning a preview-tier model as the only option creates fragility. A
stable default with an opt-in to the preview is safer.

Implementation: expose a `stt_model` config field (default `"gemini-2.5-flash"`)
or hardcode 2.5 and revisit when 3 Flash exits preview.

WorkBuddy push-to-talk recordings are short (typically 1-30 seconds), well
within the 20 MB inline limit, so no Files API needed.

### Headers
```
x-goog-api-key: {GEMINI_API_KEY}
Content-Type: application/json
```

### Request (inline audio, push-to-talk)
```json
{
  "contents": [{
    "parts": [
      { "text": "Transcribe this audio exactly. Return ONLY the transcribed text, no commentary or formatting." },
      {
        "inlineData": {
          "mimeType": "audio/wav",
          "data": "<base64-encoded WAV>"
        }
      }
    ]
  }]
}
```

### Response
```json
{
  "candidates": [{
    "content": {
      "parts": [{ "text": "Hello WorkBuddy, what is a prediction market?" }]
    }
  }]
}
```

### Supported Formats
- WAV (`audio/wav`) ← WorkBuddy records this
- MP3 (`audio/mp3`)
- AIFF (`audio/aiff`)
- AAC (`audio/aac`)
- OGG Vorbis (`audio/ogg`)
- FLAC (`audio/flac`)

WorkBuddy's `microphone.rs` records to WAV via the `hound` crate, so this
is already the right format.

### Technical Notes
- 32 tokens per second of audio (very cheap)
- Downsampled server-side to 16 Kbps
- Multi-channel collapsed to mono server-side
- Max inline request size: 20 MB (includes all parts — prompt + audio)
- Max total audio across all parts: 9.5 hours

---

## Implementation Plan

### Phase 1: Backend

#### 1a. Extend `src-tauri/src/stt.rs`

Add a third dispatch arm:

```rust
pub async fn transcribe_audio(
    app: tauri::AppHandle,
    audio: String,
) -> Result<String, String> {
    let client = &app.state::<HttpClient>().0;
    let stt_provider = config::with_config_pub(|c| c.stt_provider.clone());

    match stt_provider.as_str() {
        "elevenlabs" => transcribe_elevenlabs(client, &audio).await,
        "gemini"     => transcribe_gemini(client, &audio).await,
        _            => transcribe_whisper(client, &audio).await,
    }
}
```

#### 1b. New function: `transcribe_gemini`

```rust
/// Transcribe via Gemini audio understanding.
/// Uses the same Google API key as the Gemini LLM/TTS providers — no extra key.
async fn transcribe_gemini(
    client: &reqwest::Client,
    audio: &str,
) -> Result<String, String> {
    let api_key = config::read_api_key("google")
        .map_err(|_| "No Google API key configured for transcription".to_string())?;

    // Audio is already base64-encoded from the frontend — no need to decode/re-encode.
    // Gemini accepts base64-encoded WAV directly via inlineData.

    // Use stable 2.5 Flash by default; opt-in to 3 Flash preview later
    let model = "gemini-2.5-flash";
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
    );

    // Prompt engineered to suppress commentary — returns raw transcript only.
    let prompt = "Transcribe this audio exactly as spoken. \
                  Return ONLY the verbatim transcript with standard punctuation. \
                  Do NOT add commentary, headers, quotation marks, or explanations. \
                  If the audio is silent or unintelligible, return an empty string.";

    let body = serde_json::json!({
        "contents": [{
            "parts": [
                { "text": prompt },
                {
                    "inlineData": {
                        "mimeType": "audio/wav",
                        "data": audio
                    }
                }
            ]
        }],
        "generationConfig": {
            // Deterministic output — we want the literal transcript, not creative variation
            "temperature": 0.0,
            "maxOutputTokens": 2048
        }
    });

    let response = client
        .post(&url)
        .header("x-goog-api-key", &api_key)
        .header("content-type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Gemini STT request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!("Gemini STT error ({status}): {body_text}"));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Gemini STT response: {e}"))?;

    // Handle prompt-level safety block — no candidates array at all.
    if let Some(block_reason) = json["promptFeedback"]["blockReason"].as_str() {
        return Err(format!("Gemini blocked input: {block_reason}"));
    }

    // Handle empty candidates array (e.g., silent audio)
    let candidates = json["candidates"].as_array();
    if candidates.map_or(true, |c| c.is_empty()) {
        return Ok(String::new()); // silent audio — treat as no transcript
    }

    // Navigate to the text field with graceful fallbacks
    let text = json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("")   // silent/blocked → empty string, not error
        .trim()
        .to_string();

    // Strip common LLM artifacts that leak past the prompt
    let cleaned = strip_transcript_artifacts(&text);
    Ok(cleaned)
}

/// Strip leading prefixes like "Transcript:" and wrapping quotes that
/// Gemini sometimes adds despite strict prompting.
fn strip_transcript_artifacts(s: &str) -> String {
    let s = s.trim();
    // Remove "Transcript:" / "Transcription:" prefixes
    let s = s
        .strip_prefix("Transcript:").unwrap_or(s)
        .strip_prefix("Transcription:").unwrap_or(s)
        .trim();
    // Remove wrapping double quotes if present
    let s = if s.starts_with('"') && s.ends_with('"') && s.len() > 1 {
        &s[1..s.len()-1]
    } else {
        s
    };
    s.to_string()
}
```

#### 1c. Request size guard

The inline audio limit is 20 MB (total request). WorkBuddy recordings are
short, but add a guard just in case:

```rust
async fn transcribe_gemini(...) -> Result<String, String> {
    // ...
    // Base64 expands size by ~4/3. 20MB request limit → ~14MB audio → ~75s at 16kHz/16-bit mono
    // We cap input at 10MB base64 (~7.5MB audio, still ~30s which covers push-to-talk)
    const MAX_AUDIO_B64_SIZE: usize = 10 * 1024 * 1024;
    if audio.len() > MAX_AUDIO_B64_SIZE {
        return Err(format!(
            "Audio too large for inline Gemini STT ({} bytes, max {}). \
             Upload via Files API not yet supported.",
            audio.len(), MAX_AUDIO_B64_SIZE
        ));
    }
    // ...
}
```

For push-to-talk use cases this is plenty. If future use cases need longer
audio (e.g., lecture transcription), implement the Files API resumable upload.

### Phase 2: Frontend

#### 2a. Update Settings.tsx STT provider list

```tsx
{[
    { id: "whisper", name: "OpenAI Whisper" },
    { id: "elevenlabs", name: "ElevenLabs" },
    { id: "gemini", name: "Gemini Flash" },  // NEW
].map((p) => (
    // ...existing button rendering
))}
```

#### 2b. Update key-present logic

```tsx
{settings.stt_provider === "gemini" && !settings.api_keys?.google && (
    <p className="text-xs text-zinc-500">
        Add a Google API key in the AI Provider section above to enable Gemini STT.
    </p>
)}

{settings.stt_provider === "gemini" && settings.api_keys?.google && (
    <p className="text-xs text-accent">
        Gemini transcription is ready — hold the mic button to talk
    </p>
)}
```

No changes needed in `useMicrophone.ts` or the recording pipeline — the
provider selection is fully handled server-side.

### Phase 3: Testing

#### Test cases

1. **Happy path**: Short push-to-talk utterance → clean transcript returned
2. **Silent audio**: Record empty audio → returns empty string (per prompt instruction)
3. **Missing API key**: Select Gemini STT with no Google key → clear error
4. **Long audio**: Record 2+ minutes → succeeds if under 10MB base64 cap
5. **Too-long audio**: Oversized input → clear error before API call
6. **Mixed language**: Spanish utterance → returns Spanish transcript (no translation unless prompted)
7. **Non-speech**: Silent/noise recording → empty string
8. **Network failure**: Simulate timeout → clear error, no hang

#### Unit tests

Not much to unit test here (mostly just HTTP wiring). Consider:
```rust
#[test]
fn test_gemini_body_structure() {
    // Build a fake body and verify the JSON structure matches what
    // the Google docs show. Catches serialization regressions.
}
```

### Phase 4: Future Enhancements

Once the basic transcription works, Gemini's audio understanding opens up
interesting features that Whisper and ElevenLabs can't do:

#### 4a. Audio-aware tutor mode

Instead of simple transcription, ask Gemini to transcribe AND understand:

```
Transcribe this audio AND extract:
1. The question the student is asking
2. Any trading-related terms they mention
3. Their emotional tone (confused, frustrated, curious, excited)

Return as JSON with { "transcript", "question", "terms", "tone" }.
```

Feed the structured output back to Claude/the LLM so the tutor can adapt
its response to the student's emotional state.

#### 4b. Long-form content ingestion

Support uploading a lecture recording or YouTube URL (Gemini accepts both)
for:
- Auto-generated lesson notes
- Timestamp-indexed transcripts for RAG
- "Summarize the last 5 minutes" queries

Requires Files API resumable upload implementation.

#### 4c. Multi-audio conversations

Record a back-and-forth with the tutor via audio. Gemini handles up to
9.5 hours of audio across multiple parts in a single request. Could enable
a "voice-first" tutoring mode where the entire conversation is audio.

(Different from Live API — this is turn-based, not real-time.)

### Phase 5: Live API (Future)

For true real-time voice conversations (low-latency, bidirectional streaming),
Google provides the Live API — a separate WebSocket-based protocol. This is
outside the scope of this plan but worth a separate investigation if voice-first
tutoring becomes a priority.

Key difference:
- **Gemini STT (this plan)**: User records → upload → transcribe → send to LLM. ~2-3s round trip.
- **Gemini Live API**: Bidirectional streaming, sub-second latency, true conversation. Much more complex.

---

## Implementation Order

| Step | Files | Effort | Description |
|------|-------|--------|-------------|
| 1 | `src-tauri/src/stt.rs` | S | Add `transcribe_gemini` function |
| 2 | `src-tauri/src/stt.rs` | S | Add dispatch arm |
| 3 | `src-tauri/src/stt.rs` | S | Add size guard |
| 4 | `src/pages/Settings.tsx` | S | Add "Gemini Flash" button + key hint |
| 5 | Testing | S | Manual test with Google API key |

**Estimated total:** ~1 hour focused work. By far the simplest of the
three plans — piggybacks on the existing multi-provider STT architecture.

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Gemini returns commentary instead of pure transcript | Bad UX | Strict prompt + `temperature: 0.0` |
| 20 MB inline limit exceeded | Request fails | Pre-check size, error clearly |
| Google API key missing | 401 error | Check in Settings, show hint |
| Preview model changes behavior | API breaks | Pin model, monitor release notes |
| Prompt classifier rejection | Unlikely for plain transcription | Simple, factual prompt |

---

## Success Criteria

1. User can select "Gemini Flash" as STT provider in Settings
2. Push-to-talk recording is transcribed accurately via Gemini
3. Transcript contains no commentary, headers, or quote marks
4. Missing Google key shows clear, actionable error
5. Existing Whisper and ElevenLabs paths unchanged (backward compatible)
6. Implementation adds <100 lines of Rust

---

## Cost Comparison (approximate)

For a 10-second push-to-talk utterance:

| Provider | Cost per call |
|----------|---------------|
| Whisper | $0.001 |
| ElevenLabs Scribe | varies ($0.002-0.008) |
| Gemini Flash | ~$0.00032 (320 tokens × $1.00 / 1M tokens) |

Gemini is ~**3x cheaper** than Whisper for short utterances and reuses an
API key most power users already have configured. Calculation:
- 10s audio × 32 tokens/sec = 320 input tokens
- 320 tokens × $1.00 / 1,000,000 = $0.00032
- Whisper = $0.006/min = $0.001 for 10s
- Savings = ~3.1x

Still a meaningful cost reduction, plus the key reuse is a UX win.

---

## Relationship to Other Plans

This plan is the **simplest and highest-leverage** of the three Google-related
plans currently on the branch:

| Plan | Effort | Impact |
|------|--------|--------|
| [ACCESSIBILITY_POINTING_PLAN.md](./ACCESSIBILITY_POINTING_PLAN.md) | Large (2-3 sessions) | Pixel-precise cursor pointing |
| [GEMINI_TTS_PLAN.md](./GEMINI_TTS_PLAN.md) | Medium (1 session) | TTS alternative + audio tags |
| [GEMINI_STT_PLAN.md](./GEMINI_STT_PLAN.md) | Small (< 1 hour) | Cheaper STT + audio understanding |

Recommended order: **STT → TTS → Accessibility**. STT is the smallest win that
unlocks the largest cost reduction for voice-enabled WorkBuddy users.
