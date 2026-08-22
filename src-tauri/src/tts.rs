use crate::config;
use crate::HttpClient;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use tauri::{AppHandle, Manager};

/// Synthesize speech via the selected provider and return audio as base64.
/// ElevenLabs returns MP3; Gemini returns WAV (raw PCM wrapped in WAV header).
/// The frontend must set the correct MIME type on the returned data URI.
#[tauri::command]
pub async fn synthesize_speech(
    app: AppHandle,
    text: String,
    voice_id: Option<String>,
    provider: Option<String>,
) -> Result<String, String> {
    let provider = provider
        .or_else(|| Some(config::with_config_pub(|c| c.tts_provider.clone())))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "elevenlabs".to_string());

    match provider.as_str() {
        "gemini" => synthesize_gemini(app, text, voice_id).await,
        _ => synthesize_elevenlabs(app, text, voice_id).await,
    }
}

/// Synthesize via ElevenLabs (MP3 output).
async fn synthesize_elevenlabs(
    app: AppHandle,
    text: String,
    voice_id: Option<String>,
) -> Result<String, String> {
    let api_key = config::read_api_key("elevenlabs")
        .map_err(|_| "ElevenLabs API key not configured".to_string())?;

    let voice = voice_id.unwrap_or_else(|| "21m00Tcm4TlvDq8ikWAM".to_string());
    let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{voice}");
    let client = &app.state::<HttpClient>().0;

    let response = client
        .post(&url)
        .header("xi-api-key", &api_key)
        .header("content-type", "application/json")
        .json(&serde_json::json!({
            "text": text,
            "model_id": "eleven_flash_v2_5",
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75
            }
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("TTS request failed: {e}"))?;

    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("ElevenLabs error: {body}"));
    }

    // Check Content-Length before downloading to avoid OOM on unexpectedly large responses
    if let Some(len) = response.content_length() {
        if len > 50 * 1024 * 1024 {
            return Err("Audio response too large (Content-Length exceeds 50MB)".to_string());
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read audio: {e}"))?;

    // Post-download guard in case Content-Length was absent or inaccurate
    if bytes.len() > 50 * 1024 * 1024 {
        return Err("Audio response too large".to_string());
    }

    Ok(STANDARD.encode(&bytes))
}

/// Synthesize via Gemini 3.1 Flash TTS (returns raw PCM wrapped in WAV header).
/// Reuses the same Google API key as the Gemini LLM/STT providers.
async fn synthesize_gemini(
    app: AppHandle,
    text: String,
    voice_id: Option<String>,
) -> Result<String, String> {
    let api_key = config::read_api_key("google").map_err(|_| {
        "Google API key not configured (configure in Settings > AI Provider > Google)"
            .to_string()
    })?;

    let voice = voice_id
        .filter(|v| !v.is_empty() && v != "default")
        .unwrap_or_else(|| "Sulafat".to_string());

    // Default to preview model — can be swapped when stable GA is released.
    let model = "gemini-2.5-flash-preview-tts";
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
    );

    let body = json!({
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

    // Retry up to 3 times for transient server errors (documented Gemini
    // TTS limitation on 500s; 502/503/504 and 429 also appear under load).
    // Network errors (DNS, timeout, TLS) are NOT retried — retrying those
    // can block for up to 90s (3 × 30s timeout) before the user sees an
    // error, and transient network issues are rarely fixed by an immediate
    // retry.
    //
    // Between retries we honor a `Retry-After` header up to 5s (bounded by
    // the user's TTS latency budget — waiting longer than that to hear a
    // sentence spoken is a worse UX than falling back silently). Server
    // errors without a hint use exponential backoff (250ms, 500ms, capped
    // at 2s) to avoid slamming an already-struggling endpoint.
    let mut last_error = String::new();
    for attempt in 0..3 {
        let response = client
            .post(&url)
            .header("x-goog-api-key", &api_key)
            .header("content-type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("Gemini TTS request failed: {e}"))?;

        let status = response.status();
        let code = status.as_u16();
        // Retry transient server errors and rate-limit responses. Any other
        // non-success (400 Bad Request, 401 Unauthorized, etc.) is terminal.
        if code == 429 || status.is_server_error() {
            let retry_after_secs: Option<u64> = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u64>().ok());
            last_error = format!("Gemini {code} on attempt {}", attempt + 1);

            // Only sleep if there's another attempt to make.
            if attempt < 2 {
                let wait_ms: u64 = match retry_after_secs {
                    Some(secs) if secs <= 5 => secs * 1000,
                    Some(secs) => {
                        // Server asked for more than our 5s budget. Stop
                        // retrying and surface the reason so the caller's
                        // error message explains why we bailed early.
                        last_error = format!(
                            "Gemini {code}; Retry-After={}s exceeds 5s retry budget",
                            secs
                        );
                        break;
                    }
                    None => std::cmp::min(250u64 << attempt, 2_000),
                };
                if wait_ms > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                }
            }
            continue;
        }

        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(format!("Gemini TTS error ({status}): {body_text}"));
        }

        // Cap JSON response size before buffering. Gemini encodes PCM as
        // base64 inline, so long synthesized text can push the body into
        // tens of MB. 67 MB of base64 decodes to ~50 MB of PCM, matching
        // the ElevenLabs guard in synthesize_elevenlabs.
        const MAX_RESPONSE_BYTES: u64 = 67 * 1024 * 1024;
        if let Some(len) = response.content_length() {
            if len > MAX_RESPONSE_BYTES {
                return Err(format!(
                    "Gemini TTS response too large (Content-Length {} exceeds {} bytes)",
                    len, MAX_RESPONSE_BYTES
                ));
            }
        }

        let json_resp: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Invalid Gemini TTS response: {e}"))?;

        // Handle prompt classifier rejection
        if let Some(reason) = json_resp["promptFeedback"]["blockReason"].as_str() {
            return Err(format!("Gemini blocked TTS prompt: {reason}"));
        }

        let pcm_b64 = json_resp["candidates"][0]["content"]["parts"][0]["inlineData"]["data"]
            .as_str()
            .ok_or_else(|| {
                "No audio data in response (classifier may have rejected prompt)".to_string()
            })?;

        // Post-extraction guard for inaccurate/absent Content-Length.
        if (pcm_b64.len() as u64) > MAX_RESPONSE_BYTES {
            return Err(format!(
                "Gemini TTS PCM too large ({} bytes base64, max {})",
                pcm_b64.len(),
                MAX_RESPONSE_BYTES
            ));
        }

        let pcm_bytes = STANDARD
            .decode(pcm_b64)
            .map_err(|e| format!("Failed to decode Gemini PCM base64: {e}"))?;

        // Gemini returns signed 16-bit LE, 24 kHz, mono — wrap in WAV header
        let wav_bytes = pcm_to_wav(&pcm_bytes, 24000, 1, 16);

        return Ok(STANDARD.encode(&wav_bytes));
    }

    Err(format!("Gemini TTS failed after 3 attempts: {last_error}"))
}

/// Wrap raw PCM in a standard 44-byte WAV header.
/// PCM format: signed little-endian, configurable sample rate/channels/bit-depth.
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
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
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

/// List available voices for a given TTS provider.
#[tauri::command]
pub fn list_tts_voices(provider: String) -> Vec<serde_json::Value> {
    match provider.as_str() {
        "gemini" => vec![
            json!({ "id": "Sulafat", "name": "Sulafat (Warm)" }),
            json!({ "id": "Achird", "name": "Achird (Friendly)" }),
            json!({ "id": "Sadaltager", "name": "Sadaltager (Knowledgeable)" }),
            json!({ "id": "Vindemiatrix", "name": "Vindemiatrix (Gentle)" }),
            json!({ "id": "Kore", "name": "Kore (Firm)" }),
            json!({ "id": "Charon", "name": "Charon (Informative)" }),
            json!({ "id": "Rasalgethi", "name": "Rasalgethi (Informative)" }),
            json!({ "id": "Puck", "name": "Puck (Upbeat)" }),
            json!({ "id": "Laomedeia", "name": "Laomedeia (Upbeat)" }),
            json!({ "id": "Leda", "name": "Leda (Youthful)" }),
            json!({ "id": "Aoede", "name": "Aoede (Breezy)" }),
            json!({ "id": "Zephyr", "name": "Zephyr (Bright)" }),
            json!({ "id": "Autonoe", "name": "Autonoe (Bright)" }),
            json!({ "id": "Fenrir", "name": "Fenrir (Excitable)" }),
            json!({ "id": "Orus", "name": "Orus (Firm)" }),
            json!({ "id": "Alnilam", "name": "Alnilam (Firm)" }),
            json!({ "id": "Callirrhoe", "name": "Callirrhoe (Easy-going)" }),
            json!({ "id": "Umbriel", "name": "Umbriel (Easy-going)" }),
            json!({ "id": "Enceladus", "name": "Enceladus (Breathy)" }),
            json!({ "id": "Iapetus", "name": "Iapetus (Clear)" }),
            json!({ "id": "Erinome", "name": "Erinome (Clear)" }),
            json!({ "id": "Algieba", "name": "Algieba (Smooth)" }),
            json!({ "id": "Despina", "name": "Despina (Smooth)" }),
            json!({ "id": "Algenib", "name": "Algenib (Gravelly)" }),
            json!({ "id": "Achernar", "name": "Achernar (Soft)" }),
            json!({ "id": "Schedar", "name": "Schedar (Even)" }),
            json!({ "id": "Gacrux", "name": "Gacrux (Mature)" }),
            json!({ "id": "Pulcherrima", "name": "Pulcherrima (Forward)" }),
            json!({ "id": "Zubenelgenubi", "name": "Zubenelgenubi (Casual)" }),
            json!({ "id": "Sadachbia", "name": "Sadachbia (Lively)" }),
        ],
        _ => vec![
            json!({ "id": "default", "name": "Default (Rachel)" }),
            json!({ "id": "21m00Tcm4TlvDq8ikWAM", "name": "Rachel" }),
            json!({ "id": "AZnzlk1XvdvUeBnXmlld", "name": "Domi" }),
            json!({ "id": "EXAVITQu4vr4xnSDxMaL", "name": "Bella" }),
            json!({ "id": "ErXwobaYiN019PkySvjV", "name": "Antoni" }),
            json!({ "id": "MF3mGyEYCl7XYWbV9V6O", "name": "Elli" }),
            json!({ "id": "TxGEqnHWrfWFTfGW9XjX", "name": "Josh" }),
            json!({ "id": "VR6AewLTigWG4xSOukaG", "name": "Arnold" }),
            json!({ "id": "pNInz6obpgDQGcFmaJgB", "name": "Adam" }),
            json!({ "id": "yoZ06aMxZJJ28mfd3POQ", "name": "Sam" }),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // 1 channel
        assert_eq!(&wav[22..24], &1u16.to_le_bytes());
        // 24 kHz sample rate
        assert_eq!(&wav[24..28], &24000u32.to_le_bytes());
        // byte_rate = 24000 * 1 * 16 / 8 = 48000
        assert_eq!(&wav[28..32], &48000u32.to_le_bytes());
        // block_align = 1 * 16/8 = 2
        assert_eq!(&wav[32..34], &2u16.to_le_bytes());
        // bits_per_sample
        assert_eq!(&wav[34..36], &16u16.to_le_bytes());
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(&wav[40..44], &1000u32.to_le_bytes());
        assert_eq!(wav.len(), 1044);
    }

    #[test]
    fn test_pcm_to_wav_empty() {
        let wav = pcm_to_wav(&[], 24000, 1, 16);
        assert_eq!(wav.len(), 44);
        assert_eq!(&wav[0..4], b"RIFF");
    }
}
