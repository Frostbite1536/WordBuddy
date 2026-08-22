> STALE — describes the WorkBuddy base; authoritative specs live in docs/plans/

# Gemini 3.1 Flash TTS — Implementation Plan

**Goal:** Add Google Gemini 3.1 Flash TTS as a second TTS provider alongside
ElevenLabs, giving users a cheaper alternative that reuses their existing
Google API key (already configured if using Gemini as the LLM provider) and
unlocks expressive speech via audio tags.

**Status:** Planning
**Priority:** Medium — nice-to-have, reduces dependency on ElevenLabs
**Scope:** Rust backend + Settings UI

**Reference implementation:**
[MatiousCorp/claude-tts](https://github.com/MatiousCorp/claude-tts/blob/main/plugins/claude-tts/hooks/scripts/tts-worker.sh)
(bash-based but correct API usage)

**Google docs:**
https://ai.google.dev/gemini-api/docs/speech-generation

---

## Why Add Gemini TTS?

| Feature | ElevenLabs (current) | Gemini 3.1 Flash TTS |
|---------|----------------------|----------------------|
| Cost | Premium pricing | Lower per-char cost |
| Voices | 1000+ cloned voices | 30 curated voices |
| Quality | Industry-leading | "Most attractive quadrant" per Artificial Analysis TTS leaderboard (Elo 1,211) |
| Languages | ~29 | 70+ |
| API key | Separate subscription | Reuses Gemini API key |
| Expressive control | Voice settings only | Audio tags + natural language direction |
| Multi-speaker | No | Yes (up to 2) |
| Streaming | Yes (chunks) | **No** (full response only) |
| Preview status | GA | **Preview** |

For WorkBuddy's use case — reading AI tutor responses aloud — Gemini's lower
cost and reuse of the Google API key are significant wins. The lack of
streaming is acceptable because we already chunk by sentence via `TTSQueue`.

---

## API Essentials

### Endpoint
```
POST https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent
```

### Headers
```
x-goog-api-key: {GEMINI_API_KEY}
Content-Type: application/json
```

### Request (single-speaker)
```json
{
  "contents": [{ "parts": [{ "text": "Hello there!" }] }],
  "generationConfig": {
    "responseModalities": ["AUDIO"],
    "speechConfig": {
      "voiceConfig": {
        "prebuiltVoiceConfig": { "voiceName": "Kore" }
      }
    }
  }
}
```

### Response
```json
{
  "candidates": [{
    "content": {
      "parts": [{
        "inlineData": {
          "mimeType": "audio/L16;rate=24000",
          "data": "<base64-encoded raw PCM>"
        }
      }]
    }
  }]
}
```

**Critical:** The audio data is **raw PCM** (signed 16-bit LE, 24 kHz, mono),
NOT a playable audio format. It must be wrapped in a WAV header before the
browser `<audio>` element can play it.

### Models
- `gemini-3.1-flash-tts-preview` (newest, best, default)
- `gemini-2.5-flash-preview-tts` (older, similar quality, slightly cheaper)
- `gemini-2.5-pro-preview-tts` (most capable)

### Voice Options (30 total)
| Voice | Style | Voice | Style |
|-------|-------|-------|-------|
| Kore | Firm (default) | Puck | Upbeat |
| Charon | Informative | Zephyr | Bright |
| Fenrir | Excitable | Leda | Youthful |
| Orus | Firm | Aoede | Breezy |
| Callirrhoe | Easy-going | Autonoe | Bright |
| Enceladus | Breathy | Iapetus | Clear |
| Umbriel | Easy-going | Algieba | Smooth |
| Despina | Smooth | Erinome | Clear |
| Algenib | Gravelly | Rasalgethi | Informative |
| Laomedeia | Upbeat | Achernar | Soft |
| Alnilam | Firm | Schedar | Even |
| Gacrux | Mature | Pulcherrima | Forward |
| Achird | Friendly | Zubenelgenubi | Casual |
| Vindemiatrix | Gentle | Sadachbia | Lively |
| Sadaltager | Knowledgeable | Sulafat | Warm |

For an AI tutor, **Sulafat (Warm)**, **Achird (Friendly)**, or
**Sadaltager (Knowledgeable)** are likely the best defaults.

### Model Specs (gemini-3.1-flash-tts-preview)
- **Session context window:** 32k tokens (per official docs)
- **Batch API:** Supported (not used here — we need low latency)
- **Caching:** Not supported
- **Latest update:** April 2026
- **Knowledge cutoff:** January 2025

### Known Limitations (from Google docs)
1. **Occasional 500 errors** — random failures return text tokens instead of audio. Must implement retry logic.
2. **Prompt classifier false rejections** — vague prompts may be rejected as `PROHIBITED_CONTENT`. Prefix clearly with synthesis instruction.
3. **Voice inconsistency** — voice may not strictly match selection if transcript tone conflicts.
4. **32k session context limit** — not a concern for sentence-level TTS (sentences rarely exceed 500 tokens).
5. **No streaming** — must wait for full response before playback starts. Acceptable because we chunk by sentence via `TTSQueue`.

---

## Implementation Plan

### Phase 1: Rust Backend Refactor

**Goal:** Support multiple TTS providers behind a unified API.

#### 1a. Update `src-tauri/src/config.rs`

Add `tts_provider` field:
```rust
pub struct AppConfig {
    // ...existing fields
    #[serde(default = "default_tts_provider")]
    pub tts_provider: String,  // "elevenlabs" | "gemini"
    #[serde(default)]
    pub tts_voice: String,     // already exists — reuse for both providers
}

fn default_tts_provider() -> String {
    "elevenlabs".to_string()  // backward compatible
}
```

Update `set_settings` to propagate the new field.

#### 1b. Rewrite `src-tauri/src/tts.rs`

```rust
// Note: dispatch uses string matching on the config value to stay consistent
// with the existing stt.rs provider pattern. No enum is declared here —
// the valid values are simply "elevenlabs" and "gemini".

#[tauri::command]
pub async fn synthesize_speech(
    app: AppHandle,
    text: String,
    voice_id: Option<String>,
    provider: Option<String>,  // NEW — "elevenlabs" or "gemini"
) -> Result<String, String> {
    let provider = provider
        .or_else(|| Some(config::with_config_pub(|c| c.tts_provider.clone())))
        .unwrap_or_else(|| "elevenlabs".to_string());

    match provider.as_str() {
        "gemini" => synthesize_gemini(app, text, voice_id).await,
        _ => synthesize_elevenlabs(app, text, voice_id).await,
    }
}

async fn synthesize_elevenlabs(...) -> Result<String, String> { /* existing code */ }

async fn synthesize_gemini(
    app: AppHandle,
    text: String,
    voice_id: Option<String>,
) -> Result<String, String> {
    let api_key = config::read_api_key("google")
        .map_err(|_| "Google API key not configured (configure in Settings > AI Provider > Google)".to_string())?;

    let voice = voice_id.unwrap_or_else(|| "Sulafat".to_string());
    let model = "gemini-3.1-flash-tts-preview";
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
    );

    let body = serde_json::json!({
        "contents": [{ "parts": [{ "text": text }] }],
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "speechConfig": {
                "voiceConfig": {
                    "prebuiltVoiceConfig": { "voiceName": voice }
                }
            }
        }
    });

    let client = &app.state::<HttpClient>().0;

    // Retry up to 2 times for random 500 errors (per Google docs limitation)
    let mut last_error = String::new();
    for attempt in 0..3 {
        let response = client
            .post(&url)
            .header("x-goog-api-key", &api_key)
            .header("content-type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                last_error = format!("Request failed: {e}");
                continue;
            }
        };

        if response.status() == 500 {
            last_error = format!("Gemini 500 on attempt {}", attempt + 1);
            continue;
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(format!("Gemini TTS error ({status}): {text}"));
        }

        let json: serde_json::Value = response.json().await
            .map_err(|e| format!("Invalid response: {e}"))?;

        let pcm_b64 = json["candidates"][0]["content"]["parts"][0]["inlineData"]["data"]
            .as_str()
            .ok_or("No audio data in response (classifier may have rejected prompt)")?;

        use base64::{engine::general_purpose::STANDARD, Engine};
        let pcm_bytes = STANDARD.decode(pcm_b64)
            .map_err(|e| format!("Failed to decode base64: {e}"))?;

        // Wrap raw PCM in WAV header for browser playback
        let wav_bytes = pcm_to_wav(&pcm_bytes, 24000, 1, 16);

        return Ok(STANDARD.encode(&wav_bytes));
    }

    Err(format!("Gemini TTS failed after 3 attempts: {last_error}"))
}

/// Wrap raw PCM in a standard WAV header.
/// Format: signed 16-bit little-endian, mono, configurable sample rate.
fn pcm_to_wav(pcm: &[u8], sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_size = pcm.len() as u32;
    let total_size = 36 + data_size;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&total_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());                // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes());                 // PCM format
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}
```

Note: We write the WAV header manually (44 bytes) rather than pull in the
`hound` crate just for this. Both approaches are valid; the manual approach
avoids a dependency on `hound` for TTS (it's already in deps for microphone
recording, but the microphone code writes files, not in-memory bytes).

#### 1c. Add Tauri command: `list_tts_voices`

```rust
#[tauri::command]
pub fn list_tts_voices(provider: String) -> Vec<serde_json::Value> {
    match provider.as_str() {
        "gemini" => vec![
            json!({ "id": "Sulafat", "name": "Sulafat (Warm)" }),
            json!({ "id": "Achird", "name": "Achird (Friendly)" }),
            json!({ "id": "Sadaltager", "name": "Sadaltager (Knowledgeable)" }),
            json!({ "id": "Kore", "name": "Kore (Firm)" }),
            json!({ "id": "Vindemiatrix", "name": "Vindemiatrix (Gentle)" }),
            json!({ "id": "Puck", "name": "Puck (Upbeat)" }),
            json!({ "id": "Charon", "name": "Charon (Informative)" }),
            json!({ "id": "Leda", "name": "Leda (Youthful)" }),
            json!({ "id": "Rasalgethi", "name": "Rasalgethi (Informative)" }),
            json!({ "id": "Aoede", "name": "Aoede (Breezy)" }),
            // ... all 30
        ],
        _ => vec![
            // ElevenLabs voices (keep existing Rachel default + a few others)
            json!({ "id": "default", "name": "Default (Rachel)" }),
            json!({ "id": "21m00Tcm4TlvDq8ikWAM", "name": "Rachel" }),
            // ... more ElevenLabs options if desired
        ],
    }
}
```

### Phase 2: Frontend — Settings UI

#### 2a. Update `src/contexts/app.context.tsx`

```typescript
interface Settings {
    // ...existing
    tts_provider: string;     // NEW: "elevenlabs" | "gemini"
    tts_voice: string;        // already exists
}

const defaultSettings: Settings = {
    // ...existing
    tts_provider: "elevenlabs",  // backward compat default
};
```

#### 2b. Update `src/pages/Settings.tsx`

Replace the single "Voice Responses" section with:

```tsx
<section className="space-y-3">
  <div className="flex items-center justify-between">
    <div>
      <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
        <Volume2 size={14} /> Voice Responses
      </h2>
      <p className="text-xs text-zinc-600">
        Read responses aloud. Choose a provider and voice.
      </p>
    </div>
    <Toggle
      checked={settings.tts_enabled}
      onChange={() => updateSettings({ tts_enabled: !settings.tts_enabled })}
      disabled={!hasTtsKey}  // computed below
      label="Voice responses"
    />
  </div>

  {/* Provider selector */}
  <div className="space-y-1">
    <label className="text-xs text-zinc-500">TTS Provider</label>
    <div className="flex gap-2">
      {[
        { id: "elevenlabs", name: "ElevenLabs", hint: "Premium quality, separate key" },
        { id: "gemini", name: "Gemini 3.1 Flash", hint: "Uses Google API key, lower cost" },
      ].map((p) => (
        <button
          key={p.id}
          onClick={() => updateSettings({ tts_provider: p.id, tts_voice: "default" })}
          title={p.hint}
          className={/* styled like other provider pills */}
        >
          {p.name}
        </button>
      ))}
    </div>
  </div>

  {/* Voice selector (changes based on provider) */}
  <VoiceSelector
    provider={settings.tts_provider}
    value={settings.tts_voice}
    onChange={(v) => updateSettings({ tts_voice: v })}
  />
</section>
```

#### 2c. New component: `VoiceSelector`

```tsx
function VoiceSelector({ provider, value, onChange }) {
  const [voices, setVoices] = useState<{id: string, name: string}[]>([]);

  useEffect(() => {
    invoke<{id: string, name: string}[]>("list_tts_voices", { provider })
      .then(setVoices).catch(() => setVoices([]));
  }, [provider]);

  return (
    <div className="space-y-1">
      <label className="text-xs text-zinc-500">Voice</label>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm"
      >
        {voices.map(v => (
          <option key={v.id} value={v.id}>{v.name}</option>
        ))}
      </select>
    </div>
  );
}
```

### Phase 3: Wire Up Streaming TTS

No changes needed in `TTSQueue` or `SentenceBuffer` — they already call
`synthesize_speech` with `{ text, voiceId }`. We add one parameter:

#### 3a. Update `src/lib/ttsQueue.ts`

```typescript
private getProvider: (() => string) | null = null;

setProviderGetter(getter: () => string): void {
    this.getProvider = getter;
}

// In processNext():
const params: Record<string, string> = { text: sentence };
const voiceId = this.getVoiceId?.();
if (voiceId && voiceId !== "default") {
    params.voiceId = voiceId;
}
const provider = this.getProvider?.();
if (provider) {
    params.provider = provider;
}
const base64Audio = await invoke<string>("synthesize_speech", params);
```

#### 3b. Update `ChatBar.tsx` and `ResponsePanel.tsx`

Set provider getter when settings change (similar pattern to existing voiceIdGetter):

```typescript
ttsQueueRef.current.setProviderGetter(() => settingsRef.current.tts_provider);
```

And in `ResponsePanel.handleListen`, pass `provider: settings.tts_provider` in the invoke params.

#### 3c. Handle MIME type in audio playback

Gemini returns WAV, ElevenLabs returns MP3. Update `new Audio()` calls in
**both** locations:
- `src/lib/ttsQueue.ts` `playAudio()` (around line 109)
- `src/components/ResponsePanel.tsx` `handleListen()` (around line 160)

```typescript
const mimeType = settingsRef.current.tts_provider === "gemini" ? "audio/wav" : "audio/mpeg";
const audio = new Audio(`data:${mimeType};base64,${base64Audio}`);
```

#### 3d. Refactor hardcoded ElevenLabs gates (CRITICAL)

Two places hardcode `settings.api_keys?.elevenlabs` as the gate for whether
TTS is usable. Under this plan, a user with only a Google key selects Gemini
TTS, but these gates would silently fail. Must refactor:

**In `src/components/ChatBar.tsx` (~line 109):**

```typescript
// BEFORE:
if (settingsRef.current.tts_enabled && settingsRef.current.api_keys?.elevenlabs) {
    sentenceBufferRef.current.push(event.payload);
}

// AFTER:
const provider = settingsRef.current.tts_provider;
const hasKey = provider === "gemini"
    ? !!settingsRef.current.api_keys?.google
    : !!settingsRef.current.api_keys?.elevenlabs;
if (settingsRef.current.tts_enabled && hasKey) {
    sentenceBufferRef.current.push(event.payload);
}
```

**In `src/components/ResponsePanel.tsx` (~line 245):**

```typescript
// BEFORE:
const ttsAvailable = !!settings.api_keys?.elevenlabs && settings.tts_enabled;

// AFTER:
const hasKey = settings.tts_provider === "gemini"
    ? !!settings.api_keys?.google
    : !!settings.api_keys?.elevenlabs;
const ttsAvailable = hasKey && settings.tts_enabled;
```

Without this refactor, streaming TTS and the "Listen" button will both be
silently disabled for any user on the Gemini provider.

#### 3e. Update `set_settings` in config.rs

Per existing pattern, `set_settings` at `config.rs:216-234` manually copies
every field. Must add:

```rust
config.tts_provider = settings.tts_provider;
```

Otherwise the toggle in Settings.tsx will update the frontend state but
never persist to disk.

### Phase 4: Edge Cases & Polish

#### 4a. API key reuse

If user has Google API key configured for Gemini LLM provider, Gemini TTS
should "just work". Check in Settings if Google key is present:

```tsx
const hasGoogleKey = !!settings.api_keys?.google;
const hasElevenLabsKey = !!settings.api_keys?.elevenlabs;
const hasTtsKey = settings.tts_provider === "gemini" ? hasGoogleKey : hasElevenLabsKey;
```

Show hint if user selects Gemini but no Google key:
> "Gemini TTS uses your Google API key. Add one in AI Provider section above."

#### 4b. Retry logic & error handling

The 3-attempt retry in `synthesize_gemini` handles the documented 500 error
randomness. If all 3 fail, the existing `.catch(() => {})` in `TTSQueue`
silently drops the sentence and the queue continues. Good UX.

For the "Listen" button in ResponsePanel, show a brief error toast on failure
so the user knows TTS didn't work.

#### 4c. Minimum text length

Gemini has a classifier that rejects vague prompts. For very short sentences
(< 10 chars), skip TTS entirely or pad with a tutor-like prefix. Already
handled in `SentenceBuffer.minLength = 10` for streaming.

#### 4d. Cost-conscious defaults

Default `tts_enabled: false` (already the case). Default `tts_provider: elevenlabs`
for users who already have it working. New users with only a Google API key
can easily switch to Gemini.

---

## Future Enhancements (Phase 5+)

### 5a. Expressive Speech via Audio Tags

Gemini supports inline audio tags like `[whispers]`, `[excited]`, `[encouraging]`.
For tutor mode, we could automatically inject tags based on context:

```typescript
// In prompts.ts, when tutor_mode + gemini provider:
prompt += `
You can use audio tags to add expression to your responses when pointing out
key ideas. Use sparingly — at most 1-2 per response. Supported tags:
[encouraging], [excited], [curious], [whispers], [serious].

Example: "Great question! [excited] Let me show you how this works..."
`;
```

### 5b. "Voice Persona" Presets

Package full-prompt "director's notes" as selectable personas:
- **Encouraging tutor** (warm, supportive)
- **Patient professor** (measured, thoughtful)
- **Enthusiastic coach** (energetic, upbeat)
- **Calm mentor** (reassuring, relaxed)

Each preset maps to a voice + prompt prefix + suggested audio tags.

### 5c. Multi-Speaker Dialog Mode

Gemini supports 2 speakers. Could power a "dialog learning" feature where
a concept is taught via back-and-forth conversation between two voices.
Out of scope for current roadmap but a cool possibility.

### 5d. STT Via Gemini

Gemini supports speech input too — could become a third STT provider
alongside Whisper and ElevenLabs. Would be a separate plan.

---

## Implementation Order

| Step | Files | Effort | Description |
|------|-------|--------|-------------|
| 1 | `src-tauri/src/config.rs` | S | Add `tts_provider` field + default |
| 2 | `src-tauri/src/tts.rs` | M | Refactor to provider dispatch + add Gemini |
| 3 | `src-tauri/src/tts.rs` | S | Add `pcm_to_wav` helper |
| 4 | `src-tauri/src/tts.rs` | S | Add `list_tts_voices` command |
| 5 | `src-tauri/src/lib.rs` | S | Register new command |
| 6 | `src/contexts/app.context.tsx` | S | Add `tts_provider` to Settings |
| 7 | `src/pages/Settings.tsx` | M | Provider selector + VoiceSelector |
| 8 | `src/lib/ttsQueue.ts` | S | Pass `provider` to invoke |
| 9 | `src/components/ChatBar.tsx` | S | Wire provider getter |
| 10 | `src/components/ResponsePanel.tsx` | S | Pass provider + detect MIME type |
| 11 | Testing | M | Verify both providers, retry logic, voice switching |

**Estimated total:** 1 focused session.

---

## Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Preview model behavior changes | API breaks | Pin to `gemini-3.1-flash-tts-preview`, monitor release notes |
| Random 500 errors | Occasional dropped sentences | 3-attempt retry with backoff |
| Classifier false rejection | Silent failure for certain text | Detect `PROHIBITED_CONTENT`, fall back to ElevenLabs if configured |
| WAV header incorrect | Audio doesn't play | Unit test `pcm_to_wav` against known-good WAV bytes |
| Voice name typo | 400 error | Validate voice ID against list, default to "Sulafat" |
| CSP blocks generativelanguage.googleapis.com | Request blocked | Already in `tauri.conf.json` CSP connect-src |
| Large response OOM | Memory blowup | Check Content-Length, cap at 50MB (existing ElevenLabs guard) |

---

## Success Criteria

1. User can switch TTS provider in Settings and immediately hear the next response spoken by the new provider
2. Voice selector shows correct voices for each provider
3. Gemini TTS reuses Google API key (no extra key needed if user has Gemini LLM configured)
4. Streaming TTS (sentence-by-sentence during response) works identically for both providers
5. Retry logic handles Gemini's random 500 errors without user-visible failures
6. All existing ElevenLabs functionality preserved (backward compatible)

---

## Privacy Considerations

- Same as ElevenLabs: text is sent to Google's servers for synthesis
- No new data collection beyond what Google's Gemini API already processes
- Update `PRIVACY_POLICY.md` to mention Gemini TTS as an alternative
- User opts in by choosing provider in Settings; no silent provider switches

---

## CSP Notes

The Tauri CSP already allows `https://generativelanguage.googleapis.com` (used
for Gemini LLM). No CSP changes needed for Gemini TTS.

---

## Appendix: Test Plan

### Manual Test Cases

1. **Provider switch mid-session:** Start with ElevenLabs, switch to Gemini, ask question → response spoken by Gemini voice
2. **Missing key:** Switch to Gemini without Google API key → clear error message, no crash
3. **Retry:** Mock 500 response for first 2 attempts → 3rd succeeds → user hears audio
4. **Voice switch:** Change voice in Settings → next response uses new voice
5. **Streaming:** With TTS enabled, response streams sentence-by-sentence via chosen provider
6. **Listen button:** Click 🔊 on finalized message → full message read aloud by chosen provider
7. **Short sentence:** "Sure." (< 10 chars) → skipped per SentenceBuffer min length (no wasted API call)
8. **Long response:** 500-word response → all sentences queued and played in order
9. **Rapid cancel:** Start response, immediately ask new question → old TTS cancelled cleanly
10. **WAV playback:** Gemini WAV plays in `<audio>` element without errors across Chrome/Edge/Safari WebView

### Automated Tests

Add Rust unit test for `pcm_to_wav`:
```rust
#[test]
fn test_pcm_to_wav_header() {
    let pcm = vec![0u8; 1000];
    let wav = pcm_to_wav(&pcm, 24000, 1, 16);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    // fmt chunk size
    assert_eq!(&wav[16..20], &16u32.to_le_bytes());
    // format = PCM
    assert_eq!(&wav[20..22], &1u16.to_le_bytes());
    // 24 kHz
    assert_eq!(&wav[24..28], &24000u32.to_le_bytes());
    assert_eq!(wav.len(), 1044); // 44 header + 1000 data
}
```
