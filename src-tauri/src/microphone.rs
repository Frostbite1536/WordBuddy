use base64::{engine::general_purpose::STANDARD, Engine};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

// LOCK ORDER (read this before adding any new code path that takes
// both mutexes). The two static mutexes in this module — `RECORDING`
// and `STREAM_HANDLE` — must NEVER be held at the same time. Today
// every call site takes one, releases it via `{ … }` scope, then
// takes the other. The order across functions differs (start_mic
// goes RECORDING→STREAM_HANDLE, stop goes STREAM_HANDLE→RECORDING),
// which is fine *because they don't nest*. A future refactor that
// nests them — e.g. trying to atomically swap state+stream from
// inside the cpal worker callback — will deadlock the rapid-hotkey
// path. If you ever need joint-atomic access, collapse them into a
// single Mutex<RecordingSession>; do not nest.

/// Voice Activity Detection thresholds — tuned for typical desktop microphones.
/// Lower thresholds = more sensitive (catches quieter speech).
/// Higher silence chunks = waits longer before ending (captures full sentences).
const RMS_THRESHOLD: f32 = 0.008;
const PEAK_THRESHOLD: f32 = 0.02;
const PRE_SPEECH_CHUNKS: usize = 20; // ~0.45s of pre-speech buffer
const SILENCE_CHUNKS: usize = 90; // ~2s of silence before stopping (natural speech pauses)
const MAX_SPEECH_SECONDS: usize = 60; // Auto-stop after 60s of continuous speech
const MIN_SPEECH_SECONDS_F32: f32 = 0.5; // Skip clips shorter than 0.5s (Whisper hallucinates on them)

struct RecordingState {
    sample_rate: u32,
    channels: u16,
    is_speaking: bool,
    silence_count: usize,
    pre_buffer: VecDeque<Vec<f32>>,
    speech_samples: Vec<f32>,
}

/// Wrapper to make cpal::Stream sendable across threads.
/// cpal::Stream is !Send due to platform audio backend internals.
/// Safety: The stream is only stored in a Mutex and the only operation
/// performed on it across threads is Drop (setting STREAM_HANDLE to None),
/// which stops the OS audio thread. It is never read from or used after
/// being placed in the mutex — only dropped.
struct SendableStream(#[allow(dead_code)] cpal::Stream);
unsafe impl Send for SendableStream {}

static RECORDING: Mutex<Option<Arc<Mutex<RecordingState>>>> = Mutex::new(None);
static STREAM_HANDLE: Mutex<Option<SendableStream>> = Mutex::new(None);

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

fn compute_peak(samples: &[f32]) -> f32 {
    samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
}

fn encode_wav(samples: &[f32], sample_rate: u32, channels: u16) -> Result<String, String> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut buffer = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut buffer, spec)
            .map_err(|e| format!("Failed to create WAV writer: {e}"))?;

        for &sample in samples {
            let scaled = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer
                .write_sample(scaled)
                .map_err(|e| format!("Failed to write sample: {e}"))?;
        }

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV: {e}"))?;
    }

    Ok(STANDARD.encode(buffer.into_inner()))
}

/// Start microphone capture with voice activity detection.
/// Emits "mic-speech-detected" event when speech is detected and stops.
#[tauri::command]
pub async fn start_mic_capture(app: AppHandle) -> Result<(), String> {
    // Stop any existing recording. If a prior session was still
    // holding a buffered utterance, emit it before the new stream
    // starts so a rapid hotkey re-press doesn't silently discard
    // the tail of what the user just said.
    if let Some(prior_wav) = stop_mic_capture_inner() {
        let _ = app.emit("mic-speech-detected", &prior_wav);
    }

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No input device available".to_string())?;

    let config = device
        .default_input_config()
        .map_err(|e| format!("Failed to get input config: {e}"))?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();

    let state = Arc::new(Mutex::new(RecordingState {
        sample_rate,
        channels,
        is_speaking: false,
        silence_count: 0,
        pre_buffer: VecDeque::new(),
        speech_samples: Vec::new(),
    }));

    let state_clone = Arc::clone(&state);
    let app_clone = app.clone();

    let stream = device
        .build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let chunk = data.to_vec();
                let rms = compute_rms(&chunk);
                let peak = compute_peak(&chunk);
                let is_voice = rms > RMS_THRESHOLD && peak > PEAK_THRESHOLD;

                let mut s = state_clone.lock().unwrap_or_else(|e| e.into_inner());

                if is_voice {
                    if !s.is_speaking {
                        // Speech onset — flush pre-buffer
                        s.is_speaking = true;
                        let drained: Vec<Vec<f32>> = s.pre_buffer.drain(..).collect();
                        for pre_chunk in drained {
                            s.speech_samples.extend(pre_chunk);
                        }
                    }
                    s.silence_count = 0;
                    s.speech_samples.extend(&chunk);

                    // Auto-stop after MAX_SPEECH_SECONDS to prevent unbounded growth
                    let max_samples = s.sample_rate as usize * s.channels as usize * MAX_SPEECH_SECONDS;
                    if s.speech_samples.len() >= max_samples {
                        let samples = std::mem::take(&mut s.speech_samples);
                        let sr = s.sample_rate;
                        let ch = s.channels;
                        s.is_speaking = false;
                        s.silence_count = 0;
                        s.pre_buffer.clear();
                        let min_samples = (sr as f32 * ch as f32 * MIN_SPEECH_SECONDS_F32) as usize;
                        if samples.len() >= min_samples {
                            if let Ok(wav_base64) = encode_wav(&samples, sr, ch) {
                                let _ = app_clone.emit("mic-speech-detected", &wav_base64);
                            }
                        }
                    }
                } else if s.is_speaking {
                    s.silence_count += 1;
                    s.speech_samples.extend(&chunk);

                    if s.silence_count >= SILENCE_CHUNKS {
                        // Speech ended — encode and emit (skip very short clips)
                        let samples = std::mem::take(&mut s.speech_samples);
                        let sr = s.sample_rate;
                        let ch = s.channels;
                        s.is_speaking = false;
                        s.silence_count = 0;
                        s.pre_buffer.clear();

                        let min_samples = (sr as f32 * ch as f32 * MIN_SPEECH_SECONDS_F32) as usize;
                        if samples.len() >= min_samples {
                            if let Ok(wav_base64) = encode_wav(&samples, sr, ch) {
                                let _ = app_clone.emit("mic-speech-detected", &wav_base64);
                            }
                        } else {
                            eprintln!("[mic] Skipping short clip ({} samples < {})", samples.len(), min_samples);
                        }
                    }
                } else {
                    // No speech — maintain rolling pre-buffer (O(1) via VecDeque)
                    s.pre_buffer.push_back(chunk);
                    if s.pre_buffer.len() > PRE_SPEECH_CHUNKS {
                        s.pre_buffer.pop_front();
                    }
                }
            },
            |err| {
                eprintln!("Audio stream error: {err}");
            },
            None,
        )
        .map_err(|e| format!("Failed to build input stream: {e}"))?;

    stream.play().map_err(|e| format!("Failed to start stream: {e}"))?;

    // Store state and stream handle
    {
        let mut rec = RECORDING.lock().unwrap_or_else(|e| e.into_inner());
        *rec = Some(state);
    }
    {
        let mut handle = STREAM_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
        *handle = Some(SendableStream(stream));
    }

    Ok(())
}

fn stop_mic_capture_inner() -> Option<String> {
    // Drop the stream to stop recording
    {
        let mut handle = STREAM_HANDLE.lock().unwrap_or_else(|e| e.into_inner());
        *handle = None;
    }

    // Get any remaining speech audio
    let state = {
        let mut rec = RECORDING.lock().unwrap_or_else(|e| e.into_inner());
        rec.take()
    };

    if let Some(state) = state {
        let s = state.lock().unwrap_or_else(|e| e.into_inner());
        if !s.speech_samples.is_empty() {
            // Loudly log encode failures rather than silently dropping
            // to None — the caller cannot distinguish "no clip" from
            // "clip lost during encode" without this signal.
            return match encode_wav(&s.speech_samples, s.sample_rate, s.channels) {
                Ok(wav) => Some(wav),
                Err(e) => {
                    eprintln!("[mic] encode_wav failed on stop: {e}");
                    None
                }
            };
        }
    }

    None
}

/// Stop microphone capture and return any remaining buffered audio as base64 WAV.
#[tauri::command]
pub async fn stop_mic_capture(app: AppHandle) -> Result<Option<String>, String> {
    let remaining = stop_mic_capture_inner();
    if let Some(ref wav) = remaining {
        let _ = app.emit("mic-speech-detected", wav);
    }
    Ok(remaining)
}
