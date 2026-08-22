/**
 * Pipelined streaming TTS playback.
 *
 * Two independent pumps coordinate through a small ready-queue:
 *   enqueue → [pending sentences] → synth pump → [ready audio] → play pump
 *
 * The synth pump runs ahead of the play pump (up to MAX_PREFETCH items
 * buffered) so that while sentence N is playing, sentence N+1 is already
 * being synthesized. After the first sentence, network + model latency is
 * hidden behind playback time — eliminating the long silences that
 * appeared with sequential synth-then-play.
 *
 * Both pumps respect cancellation + generation to drop in-flight work
 * when a new response supersedes the current one.
 */
import { invoke } from "@tauri-apps/api/core";

interface ReadyAudio {
  audio: string; // base64
  mimeType: string;
}

// How many synthesized sentences to buffer ahead of playback. Higher =
// more resilience to variable synth times (Gemini TTS retries on 500s
// can take up to 90s). 3 covers typical jitter without over-spending on
// cancelled streams.
const MAX_PREFETCH = 3;

export class TTSQueue {
  private pending: string[] = []; // sentences awaiting synth
  private ready: ReadyAudio[] = []; // synthesized audio awaiting playback
  private synthActive = false;
  private playActive = false;
  private cancelled = false;
  private paused = false;
  private generation = 0; // incremented on reset to invalidate in-flight work
  private currentAudio: HTMLAudioElement | null = null;
  private getVoiceId: (() => string | undefined) | null = null;
  private getProvider: (() => string | undefined) | null = null;
  private subscribers = new Set<() => void>();

  /** Set a function that returns the current TTS voice ID from settings */
  setVoiceIdGetter(getter: () => string | undefined): void {
    this.getVoiceId = getter;
  }

  /** Set a function that returns the current TTS provider from settings */
  setProviderGetter(getter: () => string | undefined): void {
    this.getProvider = getter;
  }

  /**
   * Subscribe to playback state changes. The callback fires whenever
   * `active` or `isPaused` could have changed (enqueue, cancel, play start,
   * play end, pause/resume). Returns an unsubscribe function.
   */
  subscribe(cb: () => void): () => void {
    this.subscribers.add(cb);
    return () => {
      this.subscribers.delete(cb);
    };
  }

  private notify(): void {
    this.subscribers.forEach((cb) => cb());
  }

  /** Add a sentence to be spoken */
  enqueue(sentence: string): void {
    if (this.cancelled) return;
    this.pending.push(sentence);
    this.notify();
    this.pumpSynth();
  }

  /** Cancel all pending and currently playing audio */
  cancel(): void {
    this.cancelled = true;
    this.pending = [];
    this.ready = [];
    if (this.currentAudio) {
      this.currentAudio.pause();
      this.currentAudio = null;
    }
    // Pump flags are just guards; in-flight synth/play promises will
    // observe the cancelled/generation mismatch and unwind on their own.
    this.playActive = false;
    this.paused = false;
    this.notify();
  }

  /** Reset for reuse after cancellation */
  reset(): void {
    this.cancel();
    this.generation++;
    this.cancelled = false;
  }

  /**
   * Pause the currently playing audio. Pending sentences and queued audio
   * are preserved so `resume()` continues from the same point.
   */
  pause(): void {
    if (this.currentAudio && !this.paused) {
      this.currentAudio.pause();
      this.paused = true;
      this.notify();
    }
  }

  /** Resume audio that was paused by `pause()`. */
  resume(): void {
    if (this.currentAudio && this.paused) {
      this.paused = false;
      // Promise rejection (e.g. autoplay policy) is non-fatal — the user
      // can click again. Notify before play() so UI flips immediately.
      this.notify();
      this.currentAudio.play().catch(() => {});
    }
  }

  /** Whether the queue is actively synthesizing, playing, or has pending items */
  get active(): boolean {
    return (
      this.playActive ||
      this.synthActive ||
      this.pending.length > 0 ||
      this.ready.length > 0
    );
  }

  /** Whether audio playback is currently paused via `pause()`. */
  get isPaused(): boolean {
    return this.paused;
  }

  /** Kick the synth pump. Idempotent; safe to call from enqueue + end of play. */
  private pumpSynth(): void {
    if (this.synthActive) return;
    if (this.cancelled) return;
    if (this.pending.length === 0) return;
    if (this.ready.length >= MAX_PREFETCH) return;

    this.synthActive = true;
    const gen = this.generation;
    const sentence = this.pending.shift()!;
    const voiceId = this.getVoiceId?.();
    const provider = this.getProvider?.();

    const params: Record<string, string> = { text: sentence };
    if (voiceId && voiceId !== "default") params.voiceId = voiceId;
    if (provider) params.provider = provider;

    const mimeType = provider === "gemini" ? "audio/wav" : "audio/mpeg";

    invoke<string>("synthesize_speech", params)
      .then((audio) => {
        if (this.cancelled || this.generation !== gen) return;
        this.ready.push({ audio, mimeType });
        // New audio available — try to start playback if idle.
        this.pumpPlay();
      })
      .catch(() => {
        // TTS failed for this sentence — silently drop and continue.
      })
      .finally(() => {
        this.synthActive = false;
        // Chain: if there's more to synthesize, go. Otherwise pumpPlay
        // may have already kicked off, which is fine.
        this.pumpSynth();
      });
  }

  /** Kick the play pump. Idempotent; safe to call from synth completion + end of play. */
  private pumpPlay(): void {
    if (this.playActive) return;
    if (this.cancelled) return;
    if (this.ready.length === 0) return;

    this.playActive = true;
    this.notify();
    const gen = this.generation;
    const item = this.ready.shift()!;

    // The synth pump has a free slot now that we drained one ready item.
    // Kick it so it starts the next sentence while this one plays.
    this.pumpSynth();

    this.playAudio(item.audio, item.mimeType)
      .catch(() => {
        // Playback failed — continue to the next item.
      })
      .finally(() => {
        this.playActive = false;
        this.notify();
        if (this.generation === gen && !this.cancelled) {
          // Play next ready item (or wait for synth to produce one).
          this.pumpPlay();
          // Also make sure synth is running — covers the case where
          // ready queue was empty when play ended.
          this.pumpSynth();
        }
      });
  }

  private playAudio(base64: string, mimeType: string): Promise<void> {
    return new Promise((resolve) => {
      if (this.cancelled) {
        resolve();
        return;
      }

      const audio = new Audio(`data:${mimeType};base64,${base64}`);
      this.currentAudio = audio;
      this.paused = false;
      this.notify();

      audio.onended = () => {
        this.currentAudio = null;
        this.paused = false;
        this.notify();
        resolve();
      };
      audio.onerror = () => {
        this.currentAudio = null;
        this.paused = false;
        this.notify();
        resolve();
      };

      audio.play().catch(() => {
        this.currentAudio = null;
        this.paused = false;
        this.notify();
        resolve();
      });
    });
  }
}
