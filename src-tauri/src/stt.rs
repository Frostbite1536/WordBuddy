use crate::config;
use crate::HttpClient;
use base64::{engine::general_purpose::STANDARD, Engine};
use tauri::Manager;

/// Transcribe base64-encoded WAV audio to text.
/// Supports three providers: OpenAI Whisper, ElevenLabs, and Gemini.
/// The provider is selected via the `stt_provider` config setting.
#[tauri::command]
pub async fn transcribe_audio(
    app: tauri::AppHandle,
    audio: String,
) -> Result<String, String> {
    let client = &app.state::<HttpClient>().0;

    let stt_provider = config::with_config_pub(|c| c.stt_provider.clone());

    match stt_provider.as_str() {
        "elevenlabs" => transcribe_elevenlabs(client, &audio).await,
        "gemini" => transcribe_gemini(client, &audio).await,
        _ => transcribe_whisper(client, &audio).await,
    }
}

/// Transcribe via OpenAI Whisper API.
async fn transcribe_whisper(
    client: &reqwest::Client,
    audio: &str,
) -> Result<String, String> {
    let api_key = config::read_api_key("openai")
        .or_else(|_| config::read_api_key("stt"))
        .map_err(|_| "No OpenAI or STT API key configured for transcription".to_string())?;

    let audio_bytes = STANDARD
        .decode(audio)
        .map_err(|e| format!("Failed to decode audio: {e}"))?;

    let audio_part = reqwest::multipart::Part::bytes(audio_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("Failed to create multipart part: {e}"))?;

    let form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .part("file", audio_part);

    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {api_key}"))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Transcription request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Whisper API error ({status}): {body}"));
    }

    #[derive(serde::Deserialize)]
    struct WhisperResponse {
        text: String,
    }

    let result: WhisperResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse transcription response: {e}"))?;

    Ok(result.text)
}

/// Transcribe via ElevenLabs Speech-to-Text API.
/// Uses the same ElevenLabs API key as TTS — no extra key needed.
async fn transcribe_elevenlabs(
    client: &reqwest::Client,
    audio: &str,
) -> Result<String, String> {
    let api_key = config::read_api_key("elevenlabs")
        .map_err(|_| "No ElevenLabs API key configured for transcription".to_string())?;

    let audio_bytes = STANDARD
        .decode(audio)
        .map_err(|e| format!("Failed to decode audio: {e}"))?;

    let audio_part = reqwest::multipart::Part::bytes(audio_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("Failed to create multipart part: {e}"))?;

    let form = reqwest::multipart::Form::new()
        .text("model_id", "scribe_v1")
        .part("file", audio_part);

    let response = client
        .post("https://api.elevenlabs.io/v1/speech-to-text")
        .header("xi-api-key", &api_key)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("ElevenLabs STT request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("ElevenLabs STT error ({status}): {body}"));
    }

    #[derive(serde::Deserialize)]
    struct ElevenLabsSTTResponse {
        text: String,
    }

    let result: ElevenLabsSTTResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse ElevenLabs STT response: {e}"))?;

    Ok(result.text)
}

/// Transcribe via Gemini audio understanding.
/// Uses the same Google API key as the Gemini LLM/TTS providers — no extra key.
/// Accepts base64-encoded WAV via inlineData (no re-encoding needed).
async fn transcribe_gemini(
    client: &reqwest::Client,
    audio: &str,
) -> Result<String, String> {
    let api_key = config::read_api_key("google")
        .map_err(|_| "No Google API key configured for transcription".to_string())?;

    // Base64 expands size by ~4/3. 20MB request limit → ~14MB audio → ~75s at 16kHz/16-bit mono.
    // Cap input at 10MB base64 (~7.5MB audio, still ~30s which covers push-to-talk).
    const MAX_AUDIO_B64_SIZE: usize = 10 * 1024 * 1024;
    if audio.len() > MAX_AUDIO_B64_SIZE {
        return Err(format!(
            "Audio too large for inline Gemini STT ({} bytes, max {}). \
             Files API upload not yet supported.",
            audio.len(),
            MAX_AUDIO_B64_SIZE
        ));
    }

    // Use stable 2.5 Flash by default; opt-in to 3 Flash preview later.
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
            // Deterministic — we want the literal transcript, not creative variation
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
        .unwrap_or("") // silent/blocked → empty string, not error
        .trim()
        .to_string();

    // Strip common LLM artifacts that leak past the prompt
    Ok(strip_transcript_artifacts(&text))
}

/// Strip leading prefixes like "Transcript:" and wrapping quotes that
/// Gemini sometimes adds despite strict prompting.
fn strip_transcript_artifacts(s: &str) -> String {
    let s = s.trim();
    // Strip known prefixes — each step rebinds so the next call sees the
    // previously-stripped value, not the original (avoids shadowing bug).
    let s = s.strip_prefix("Transcript:").unwrap_or(s);
    let s = s.strip_prefix("Transcription:").unwrap_or(s);
    let s = s.trim();
    // Remove wrapping double quotes if present
    let s = if s.starts_with('"') && s.ends_with('"') && s.len() > 1 {
        &s[1..s.len() - 1]
    } else {
        s
    };
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_transcript_prefix() {
        assert_eq!(strip_transcript_artifacts("Transcript: hello world"), "hello world");
        assert_eq!(strip_transcript_artifacts("Transcription: hi"), "hi");
    }

    #[test]
    fn strips_wrapping_quotes() {
        assert_eq!(strip_transcript_artifacts("\"hello world\""), "hello world");
    }

    #[test]
    fn preserves_internal_quotes() {
        assert_eq!(
            strip_transcript_artifacts("she said \"hi\" loudly"),
            "she said \"hi\" loudly"
        );
    }

    #[test]
    fn handles_empty_and_whitespace() {
        assert_eq!(strip_transcript_artifacts(""), "");
        assert_eq!(strip_transcript_artifacts("  \n  "), "");
    }
}
