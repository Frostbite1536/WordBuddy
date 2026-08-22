import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface UseMicrophoneResult {
  isRecording: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<void>;
  lastTranscript: string | null;
  // Latest mic-related error string suitable for display in the chat
  // shell. Cleared when a successful transcription lands. U6 audit:
  // STT failures used to be a silent console.error.
  micError: string | null;
  clearMicError: () => void;
}

export function useMicrophone(
  onTranscript: (text: string) => void,
): UseMicrophoneResult {
  const [isRecording, setIsRecording] = useState(false);
  const [lastTranscript, setLastTranscript] = useState<string | null>(null);
  const [micError, setMicError] = useState<string | null>(null);
  const onTranscriptRef = useRef(onTranscript);
  onTranscriptRef.current = onTranscript;
  const clearMicError = useCallback(() => setMicError(null), []);

  // Listen for mic-speech-detected events from the backend.
  // Wraps `listen()` in try/catch so a failed subscription cannot
  // surface as an unhandled rejection AND cannot leak by leaving
  // earlier listeners dangling on a future re-mount (mirror of the
  // M3 audit fix in ChatBar.tsx).
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    (async () => {
      try {
        const u = await listen<string>(
          "mic-speech-detected",
          async (event) => {
            const audioBase64 = event.payload;
            try {
              const text = await invoke<string>("transcribe_audio", {
                audio: audioBase64,
              });
              if (text && text.trim()) {
                setLastTranscript(text.trim());
                setMicError(null);
                onTranscriptRef.current(text.trim());
              }
            } catch (e) {
              console.error("Transcription failed:", e);
              const raw = e instanceof Error ? e.message : String(e);
              // Map common cases to actionable text. U6 audit.
              if (/permission|denied|access/i.test(raw)) {
                setMicError(
                  "Microphone permission was denied. Open your OS Privacy settings and allow WorkBuddy to use the mic.",
                );
              } else if (/api.?key|unauthorized|401/i.test(raw)) {
                setMicError(
                  "Speech-to-text rejected your API key. Open Settings → STT and check the key for your selected provider.",
                );
              } else if (/network|timeout|connect|dns/i.test(raw)) {
                setMicError(
                  "Network error reaching the speech-to-text provider. Check your connection.",
                );
              } else {
                setMicError(`Transcription failed: ${raw.slice(0, 200)}`);
              }
            }
          },
        );
        if (cancelled) {
          u();
          return;
        }
        unlisteners.push(u);
      } catch (err) {
        console.warn(
          '[useMicrophone] listen("mic-speech-detected") failed:',
          err,
        );
      }
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  // Ensure mic capture is stopped on unmount to prevent orphaned audio streams
  useEffect(() => {
    return () => {
      invoke("stop_mic_capture").catch(() => {});
    };
  }, []);

  const startRecording = useCallback(async () => {
    try {
      await invoke("start_mic_capture");
      setIsRecording(true);
      setMicError(null);
    } catch (e) {
      console.error("Failed to start mic capture:", e);
      const raw = e instanceof Error ? e.message : String(e);
      if (/no input device|no default|not found/i.test(raw)) {
        setMicError(
          "No microphone detected. Plug in or enable an input device, then try again.",
        );
      } else if (/permission|denied|access/i.test(raw)) {
        setMicError(
          "Microphone permission was denied. Open your OS Privacy settings and allow WorkBuddy to use the mic.",
        );
      } else {
        setMicError(`Couldn't start recording: ${raw.slice(0, 200)}`);
      }
    }
  }, []);

  const stopRecording = useCallback(async () => {
    try {
      await invoke("stop_mic_capture");
      setIsRecording(false);
    } catch (e) {
      console.error("Failed to stop mic capture:", e);
      const raw = e instanceof Error ? e.message : String(e);
      setMicError(`Couldn't stop recording cleanly: ${raw.slice(0, 200)}`);
    }
  }, []);

  return {
    isRecording,
    startRecording,
    stopRecording,
    lastTranscript,
    micError,
    clearMicError,
  };
}
