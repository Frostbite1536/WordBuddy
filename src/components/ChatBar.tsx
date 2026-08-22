import { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, emit } from "@tauri-apps/api/event";
import {
  BookOpen,
  Camera,
  Send,
  Settings,
  Mic,
  Loader2,
  Briefcase,
  CalendarDays,
  Terminal,
  X,
  Minus,
  Trash2,
  ChevronDown,
  ChevronUp,
  Volume2,
  VolumeX,
  Pause,
  Play,
} from "lucide-react";
import { exit } from "@tauri-apps/plugin-process";
import { useApp } from "../contexts/app.context";
import { buildSystemPrompt } from "../lib/prompts";
import { useMicrophone } from "../hooks/useMicrophone";
import { saveTurn } from "../lib/db";
import { friendlyStreamError } from "../lib/friendlyError";
import { SentenceBuffer } from "../lib/sentenceBuffer";
import { TTSQueue } from "../lib/ttsQueue";

interface CaptureResult {
  base64: string;
  width: number;
  height: number;
  detected_elements: string | null;
}

export default function ChatBar() {
  const {
    settings,
    updateSettings,
    isExpanded,
    setIsExpanded,
    currentContext,
    messages,
    setMessages,
    clearMessages,
    isStreaming,
    setIsStreaming,
    setCurrentPage,
    setScreenshotDims,
    conversationIdRef,
    externalQuestion,
    setExternalQuestion,
    journalChatContext,
    setJournalChatContext,
    submitExternalRef,
  } = useApp();

  const [input, setInput] = useState("");
  const [hasScreenshot, setHasScreenshot] = useState(false);
  // Whether the `wotch` binary can be located on this machine. Controls
  // the visibility of the "Open in Wotch" toolbar button. Probed once on
  // mount; users who install Wotch while WorkBuddy is running will need
  // to reopen WorkBuddy (or we poll — deferred to v1.1).
  const [wotchInstalled, setWotchInstalled] = useState(false);
  // Journal recorder indicator: polled so the bar shows a live "recording"
  // dot even when the recorder was auto-started on launch (ADR-042).
  const [recorderRunning, setRecorderRunning] = useState(false);
  useEffect(() => {
    let cancelled = false;
    const poll = () => {
      invoke<{ running: boolean }>("recorder_status")
        .then((s) => { if (!cancelled) setRecorderRunning(s.running); })
        .catch(() => { if (!cancelled) setRecorderRunning(false); });
    };
    poll();
    const interval = setInterval(poll, 15000);
    return () => { cancelled = true; clearInterval(interval); };
  }, []);
  // Persists the last screenshot dimensions we broadcast to the cursor
  // overlay window. Re-emitted when the overlay signals `overlay_ready`
  // so a late-mounting overlay still gets the coord-mapping context.
  const lastEmittedDimsRef = useRef<{ width: number; height: number } | null>(null);
  // Mirrors TTSQueue playback state so the pause/play button can show the
  // right icon and only render while audio is actually active.
  const [ttsPlaybackState, setTtsPlaybackState] = useState({
    active: false,
    paused: false,
  });
  const lastScreenshotRef = useRef<string | null>(null);
  const lastScreenshotDimsRef = useRef<{ width: number; height: number } | null>(null);
  const streamBufferRef = useRef("");
  const renderTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const submittingRef = useRef(false);
  const lastUserMsgRef = useRef<{ id: string; role: string; content: string; timestamp: number } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const handleSubmitRef = useRef<(overrideText?: string) => void>(() => {});
  // Streaming TTS: sentence buffer + audio queue
  const ttsQueueRef = useRef(new TTSQueue());
  const sentenceBufferRef = useRef(new SentenceBuffer((sentence) => {
    ttsQueueRef.current.enqueue(sentence);
  }));
  // Refs to avoid stale closures in event listeners
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const journalChatContextRef = useRef(journalChatContext);
  journalChatContextRef.current = journalChatContext;
  // Keep TTS voice + provider getters in sync with current settings
  ttsQueueRef.current.setVoiceIdGetter(() => settingsRef.current.tts_voice);
  ttsQueueRef.current.setProviderGetter(() => settingsRef.current.tts_provider);
  // Subscribe to queue state so the pause/play button reflects reality
  // (visible only while audio is active; icon flips on pause/resume).
  useEffect(() => {
    const queue = ttsQueueRef.current;
    const sync = () => {
      setTtsPlaybackState({ active: queue.active, paused: queue.isPaused });
    };
    const unsubscribe = queue.subscribe(sync);
    sync();
    return unsubscribe;
  }, []);

  const currentContextRef = useRef(currentContext);
  currentContextRef.current = currentContext;
  const messagesRef = useRef(messages);
  messagesRef.current = messages;

  // Push-to-talk microphone with auto-submit on transcript
  const { isRecording, startRecording, stopRecording, micError, clearMicError } = useMicrophone(
    useCallback((text: string) => {
      setInput(text);
      // Pass transcript directly to handleSubmit to avoid stale closure on input state
      handleSubmitRef.current(text);
    }, []),
  );

  // U6 audit: surface mic / transcription failures into the chat
  // thread so a denied mic permission or rejected STT key isn't a
  // silent dead-end. Pop the chat shell open so the message is
  // visible even if the user was minimised when they hit the
  // shortcut.
  useEffect(() => {
    if (!micError) return;
    setIsExpanded(true);
    setMessages((prev) => [
      ...prev,
      {
        id: crypto.randomUUID(),
        role: "assistant" as const,
        content: micError,
        timestamp: Date.now(),
      },
    ]);
    clearMicError();
  }, [micError, clearMicError, setIsExpanded, setMessages]);

  // Mic refs — declared after useMicrophone so the variables exist
  const isRecordingRef = useRef(isRecording);
  isRecordingRef.current = isRecording;
  const startRecordingRef = useRef(startRecording);
  startRecordingRef.current = startRecording;
  const stopRecordingRef = useRef(stopRecording);
  stopRecordingRef.current = stopRecording;

  // Listen for streaming chunks — stable refs avoid stale closures.
  // Uses cancelled flag to handle race between unmount and async listen() resolution.
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    // Wrapper around `listen()` that survives a single failed
    // subscription. Without it, an unhandled rejection would skip
    // every later registration AND leave the earlier ones live, so
    // a second mount would double-stack listeners (M3 audit).
    async function safeListen<T>(
      event: Parameters<typeof listen<T>>[0],
      handler: Parameters<typeof listen<T>>[1],
    ): Promise<void> {
      try {
        const u = await listen<T>(event, handler);
        if (cancelled) {
          u();
          return;
        }
        unlisteners.push(u);
      } catch (err) {
        console.warn(`[ChatBar] listen("${String(event)}") failed:`, err);
      }
    }

    (async () => {
      await safeListen<string>("chat_stream_chunk", (event) => {
        streamBufferRef.current += event.payload;

        // Streaming TTS: pipe chunks immediately (TTS has its own buffer).
        // Key gate depends on selected provider — Gemini uses the Google key,
        // ElevenLabs uses its own.
        if (settingsRef.current.tts_enabled) {
          const provider = settingsRef.current.tts_provider;
          const hasKey = provider === "gemini"
            ? !!settingsRef.current.api_keys?.google
            : !!settingsRef.current.api_keys?.elevenlabs;
          if (hasKey) {
            sentenceBufferRef.current.push(event.payload);
          }
        }

        // Debounce UI updates — render at most ~12fps instead of on every
        // chunk. On Windows, transparent window repaints are expensive
        // (DWM compositing + backdrop-blur on every frame). Without
        // debouncing, rapid chunks (20-30/sec) freeze the entire window.
        if (!renderTimerRef.current) {
          renderTimerRef.current = setTimeout(() => {
            renderTimerRef.current = null;
            const content = streamBufferRef.current;
            setMessages((prev) => {
              const idx = prev.findIndex((m) => m.id === "streaming");
              if (idx >= 0) {
                const updated = [...prev];
                updated[idx] = { ...updated[idx], content };
                return updated;
              }
              return [
                ...prev,
                {
                  id: "streaming",
                  role: "assistant" as const,
                  content,
                  timestamp: Date.now(),
                },
              ];
            });
          }, 150);
        }
      });

      await safeListen("chat_stream_complete", () => {
        // Cancel any pending debounced render — we'll do a final render now
        if (renderTimerRef.current) {
          clearTimeout(renderTimerRef.current);
          renderTimerRef.current = null;
        }
        const finalContent = streamBufferRef.current;
        setIsStreaming(false);
        streamBufferRef.current = "";

        // Flush any remaining sentence for streaming TTS
        sentenceBufferRef.current.flush();

        // Generate IDs outside the updater — updaters must be pure (React Strict Mode calls them twice)
        const finalizedId = crypto.randomUUID();
        if (!conversationIdRef.current) {
          conversationIdRef.current = crypto.randomUUID();
        }

        // Pure state updater — no side effects
        setMessages((prev) =>
          prev.map((m) =>
            m.id === "streaming"
              ? { ...m, id: finalizedId, content: finalContent }
              : m,
          ),
        );

        // Persist to SQLite outside the updater using refs (no state read needed)
        const userMsg = lastUserMsgRef.current;
        if (userMsg) {
          const convId = conversationIdRef.current!;
          const assistantTimestamp = Date.now();
          // Persist the whole turn atomically so a partial failure
          // can't leave a conversation row with missing messages
          // (which would render as a ghost in History). Errors are
          // surfaced rather than silently swallowed — without the
          // log there's no way to diagnose missing turns from a
          // user bug report.
          saveTurn(convId, null, null, [
            {
              id: userMsg.id,
              role: userMsg.role,
              content: userMsg.content,
              timestamp: userMsg.timestamp,
            },
            {
              id: finalizedId,
              role: "assistant",
              content: finalContent,
              timestamp: assistantTimestamp,
            },
          ]).catch((err) => {
            console.warn("[chat] saveTurn failed; turn not persisted:", err);
          });
        }
      });
      // Listen for tool_use_complete events (Anthropic tool-use pointing).
      // Routing per ADR-033:
      //   - point_at → always cursor_overlay (precise, animated cursor)
      //   - highlight → browser extension when it's connected AND the user
      //     hasn't disabled extension_highlight_enabled; else cursor_overlay
      await safeListen<{ name: string; input: Record<string, unknown> }>(
        "tool_use_complete",
        async (event) => {
          const { name, input } = event.payload;
          const x = Number(input.x);
          const y = Number(input.y);
          const label = String(input.label || "");

          if (name === "point_at") {
            invoke("show_pointer", {
              target: { x, y, label, screen: 0 },
            }).catch(() => {});
            return;
          }

          if (name !== "highlight") return;

          // Default to 120x40 when the model omitted width/height — matches
          // the cursor_overlay spotlight's visual footprint.
          const rawW = Number(input.width);
          const rawH = Number(input.height);
          const width = Number.isFinite(rawW) && rawW > 0 ? rawW : 120;
          const height = Number.isFinite(rawH) && rawH > 0 ? rawH : 40;

          let routedToExtension = false;
          if (settingsRef.current.extension_highlight_enabled !== false) {
            try {
              const status = await invoke<{ connected: boolean }>(
                "get_extension_status",
              );
              if (status.connected) {
                await invoke("extension_highlight", {
                  rect: {
                    x: Math.round(x - width / 2),
                    y: Math.round(y - height / 2),
                    w: Math.round(width),
                    h: Math.round(height),
                  },
                  label,
                });
                routedToExtension = true;
              }
            } catch {
              // Fall through to cursor_overlay on any probe/queue failure
            }
          }

          if (!routedToExtension) {
            invoke("show_pointer", {
              target: { x, y, label, screen: 0 },
            }).catch(() => {});
          }
        },
      );
      // Overlay handshake: when the cursor_overlay window mounts (which
      // may be later than the main window on cold start), it signals
      // `overlay_ready`. Re-emit the last known screenshot dimensions so
      // its coord mapping has a reference even if the original emit at
      // submit time fired before the listener was registered.
      await safeListen("overlay_ready", () => {
        const dims = lastEmittedDimsRef.current;
        if (dims) {
          emit("screenshot_dims", dims).catch(() => {});
        }
      });

      // `external-question`: a local tool (Wotch's "Ask WorkBuddy"
      // command palette entry) posted a question to our /ask endpoint.
      // Surface a confirmation banner instead of auto-submitting —
      // S1 audit threat: any local process holding the extension token
      // could otherwise drive the user's LLM quota silently. The
      // banner shows the source + question and requires an explicit
      // Submit click before the prompt reaches handleSubmit.
      await safeListen<{ source: string; question: string; context?: string }>(
        "external-question",
        async (event) => {
          const { source, question, context } = event.payload;
          if (!question || !question.trim()) return;
          // Surface the UI even if WorkBuddy was hidden/minimised — the
          // student needs to see the confirmation banner.
          try {
            await invoke("show_main_window");
          } catch (err) {
            console.warn("[ChatBar] show_main_window failed:", err);
          }
          setIsExpanded(true);
          setExternalQuestion({
            source: source || "external",
            question: question.trim(),
            context: typeof context === "string" ? context.trim() : undefined,
            receivedAt: Date.now(),
          });
        },
      );
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
      // Clear any pending debounced render — otherwise the timer fires
      // into a stale closure after unmount and calls setMessages on a
      // potentially gone component tree.
      if (renderTimerRef.current) {
        clearTimeout(renderTimerRef.current);
        renderTimerRef.current = null;
      }
    };
  }, [setMessages, setIsStreaming]);

  const lastDetectedElementsRef = useRef<string | null>(null);

  // Probe Wotch availability once on mount. Re-probes when the user
  // toggles `wotch_integration_enabled` on.
  useEffect(() => {
    if (!settings.wotch_integration_enabled) {
      setWotchInstalled(false);
      return;
    }
    let cancelled = false;
    invoke<{ installed: boolean }>("wotch_status")
      .then((s) => { if (!cancelled) setWotchInstalled(!!s.installed); })
      .catch(() => { if (!cancelled) setWotchInstalled(false); });
    return () => { cancelled = true; };
  }, [settings.wotch_integration_enabled]);

  const handleOpenInWotch = useCallback(() => {
    const prompt = input.trim();
    invoke<{ spawned: boolean; prompt_pushed: boolean; message: string }>(
      "launch_wotch",
      { initialPrompt: prompt || null },
    )
      .then((r) => {
        if (r.prompt_pushed) {
          // Clear the input — the prompt is now queued in Wotch.
          setInput("");
        }
      })
      .catch((e) => {
        console.error("[ChatBar] launch_wotch failed:", e);
      });
  }, [input]);

  // Global shortcut listeners: trigger-screenshot and focus-text-input
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    // Same M3 audit fix as the chat-stream block above: a single
    // failed listen() must not skip the rest, leak the earlier
    // unlisteners, OR surface as an unhandled rejection.
    async function safeListen<T>(
      event: Parameters<typeof listen<T>>[0],
      handler: Parameters<typeof listen<T>>[1],
    ): Promise<void> {
      try {
        const u = await listen<T>(event, handler);
        if (cancelled) {
          u();
          return;
        }
        unlisteners.push(u);
      } catch (err) {
        console.warn(`[ChatBar] listen("${String(event)}") failed:`, err);
      }
    }

    (async () => {
      await safeListen("trigger-screenshot", async () => {
        try {
          const result = await invoke<CaptureResult>("capture_to_base64");
          lastScreenshotRef.current = result.base64;
          lastScreenshotDimsRef.current = { width: result.width, height: result.height };
          lastDetectedElementsRef.current = result.detected_elements;
          setHasScreenshot(true);
        } catch (e) {
          console.error("[ChatBar] trigger-screenshot capture failed:", e);
        }
        inputRef.current?.focus();
      });

      await safeListen("focus-text-input", () => {
        inputRef.current?.focus();
      });

      await safeListen("push-to-talk", () => {
        // Toggle recording on/off (global shortcuts don't have keyup)
        if (isRecordingRef.current) {
          stopRecordingRef.current();
        } else {
          startRecordingRef.current();
        }
      });
    })();

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  const takeScreenshot = useCallback(async (skipOcr = false) => {
    const result = await invoke<CaptureResult>("capture_to_base64", {
      skipOcr: skipOcr || undefined,
    });
    lastScreenshotRef.current = result.base64;
    lastScreenshotDimsRef.current = { width: result.width, height: result.height };
    lastDetectedElementsRef.current = result.detected_elements;
    setHasScreenshot(true);
    return result.base64;
  }, []);

  const handleSubmit = useCallback(async (overrideText?: string) => {
    const text = (overrideText || input).trim();
    if (!text || isStreaming || submittingRef.current) return;
    submittingRef.current = true;

    setInput("");
    setIsExpanded(true);
    setIsStreaming(true);
    streamBufferRef.current = "";
    if (renderTimerRef.current) { clearTimeout(renderTimerRef.current); renderTimerRef.current = null; }
    // Reset streaming TTS for new response
    ttsQueueRef.current.reset();
    sentenceBufferRef.current.reset();

    // Add user message
    const userMsg = {
      id: crypto.randomUUID(),
      role: "user" as const,
      content: text,
      timestamp: Date.now(),
    };
    lastUserMsgRef.current = userMsg;
    setMessages((prev) => [...prev, userMsg]);

    // Smart OCR skipping: only run OCR when the user asks about pointing/clicking/UI
    const pointingWords = /\bpoint\b|\bclick\b|\bwhere\b|\bfind\b|\bshow me\b|\bbutton\b|\bsign up\b|\blogin\b|\bjoin\b|\btap\b|\bhighlight\b/i;
    const needsOcr = pointingWords.test(text);

    // Capture screenshot if auto-screenshot is on or user manually took one
    let screenshot: string | null = lastScreenshotRef.current;
    if (settings.auto_screenshot && !screenshot) {
      try {
        screenshot = await takeScreenshot(!needsOcr);
      } catch (e) {
        // Show capture errors in the chat so user can report them
        setMessages((prev) => [...prev, {
          id: crypto.randomUUID(),
          role: "assistant" as const,
          content: `[Screenshot failed: ${e}]`,
          timestamp: Date.now(),
        }]);
      }
    }
    // Store dims in context for CursorOverlay coordinate mapping
    if (lastScreenshotDimsRef.current) {
      setScreenshotDims(lastScreenshotDimsRef.current);
      // Broadcast to the overlay window for full-screen cursor pointing,
      // and retain a copy so an `overlay_ready` late-handshake can re-emit
      // without needing to re-capture.
      lastEmittedDimsRef.current = lastScreenshotDimsRef.current;
      emit("screenshot_dims", lastScreenshotDimsRef.current).catch(() => {});
    }
    // Capture values BEFORE clearing refs
    const detectedElements = lastDetectedElementsRef.current || "";
    const capturedDims = lastScreenshotDimsRef.current;
    setHasScreenshot(false);
    lastScreenshotRef.current = null;
    lastDetectedElementsRef.current = null;
    lastScreenshotDimsRef.current = null;

    // Build conversation history — use ref to avoid stale closure
    const history = messagesRef.current
      .filter((m) => m.id !== "streaming")
      .slice(-10)
      .map((m) => ({
        role: m.role,
        content: m.content,
      }));

    // Search RAG index for relevant documentation
    let ragContext = "";
    try {
      const chunks = await invoke<Array<{ source_file: string; content: string; score: number }>>(
        "search_docs",
        { query: text, topK: 5 },
      );
      if (chunks.length > 0) {
        ragContext = chunks
          .map((c) => `[Source: ${c.source_file}]\n${c.content}`)
          .join("\n\n");
      }
    } catch {
      // RAG search failed or not indexed — continue without it
    }

    // Attached journal day ("Ask about this day") rides along as system
    // context until the user dismisses the chip. Read via ref so the
    // submit closure can't go stale.
    const journalCtx = journalChatContextRef.current;
    const systemPrompt = buildSystemPrompt(
      ragContext,
      settings.tutor_mode,
      !!screenshot,
      detectedElements,
      capturedDims?.width ?? 0,
      capturedDims?.height ?? 0,
      journalCtx ? `Day: ${journalCtx.day}\n${journalCtx.text}` : "",
    );

    try {
      await invoke("stream_response", {
        systemPrompt,
        userMessage: text,
        screenshotBase64: screenshot,
        conversationHistory: history,
        model: settings.model,
        provider: settings.provider,
        usePointingTools: settings.cursor_overlay_enabled,
      });
    } catch (e) {
      setIsStreaming(false);
      streamBufferRef.current = "";
      // Clean up any orphaned streaming message and add error message
      setMessages((prev) => {
        const cleaned = prev.filter((m) => m.id !== "streaming");
        return [
          ...cleaned,
          {
            id: crypto.randomUUID(),
            role: "assistant" as const,
            content: friendlyStreamError(e),
            timestamp: Date.now(),
          },
        ];
      });
    } finally {
      submittingRef.current = false;
    }
  }, [input, isStreaming, settings, setIsExpanded, setIsStreaming, setMessages, takeScreenshot, setScreenshotDims]);

  // Keep ref in sync for mic auto-submit callback
  handleSubmitRef.current = handleSubmit;
  // Expose to ResponsePanel so the externalQuestion banner can fire
  // the prompt (S1 audit — confirmation-gated, never auto-submitted).
  submitExternalRef.current = (composed: string) => handleSubmit(composed);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    // Skip the Enter that confirms an IME composition (CJK / Korean /
    // Vietnamese etc.) — submitting there would send a half-typed
    // question. Browsers report keyCode 229 during composition and
    // expose `isComposing` on the underlying KeyboardEvent.
    if (e.nativeEvent.isComposing || e.keyCode === 229) return;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  // Context badge text — the detected foreground app/window, when known
  const contextLabel = currentContext?.title || null;

  // Show TTS controls only when an API key is configured for the selected
  // provider. Without a key the buttons would do nothing but still take up
  // toolbar space.
  const hasTtsKey =
    settings.tts_provider === "gemini"
      ? !!settings.api_keys?.google
      : !!settings.api_keys?.elevenlabs;

  // Mute toggle: persistent setting + immediate effect on the in-flight queue.
  // When muting mid-stream we cancel the queue so the user gets silence right
  // away; the persistent flag keeps future responses silent until unmuted.
  const handleMuteToggle = () => {
    const next = !settings.tts_enabled;
    if (!next) {
      ttsQueueRef.current.cancel();
      sentenceBufferRef.current.reset();
    }
    updateSettings({ tts_enabled: next });
  };

  const handlePauseToggle = () => {
    const queue = ttsQueueRef.current;
    if (queue.isPaused) queue.resume();
    else queue.pause();
  };

  return (
    <div
      className="flex items-center gap-2 px-3 h-[54px] min-h-[54px]"
      data-tauri-drag-region
    >
      {/* Logo + expand toggle */}
      <div className="flex items-center gap-1 shrink-0">
        <Briefcase size={20} className="text-accent" />
        <span className="text-sm font-heading font-semibold text-accent hidden sm:inline">
          WorkBuddy
        </span>
        {recorderRunning && (
          <span
            title="Work journal is recording"
            aria-label="Work journal is recording"
            className="w-1.5 h-1.5 rounded-full bg-red-500 animate-pulse shrink-0"
          />
        )}
        {messages.length > 0 && (
          <button
            onClick={() => setIsExpanded(!isExpanded)}
            title={isExpanded ? "Minimize" : "Expand messages"}
            aria-label={isExpanded ? "Minimize" : "Expand messages"}
            className="p-0.5 rounded text-zinc-500 hover:text-zinc-300 transition-colors"
          >
            {isExpanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
          </button>
        )}
      </div>

      {/* Context badge */}
      {contextLabel && (
        <span className="text-[10px] px-2 py-0.5 rounded-full bg-accent/15 text-accent shrink-0 capitalize hidden sm:inline">
          {contextLabel}
        </span>
      )}

      {/* Attached journal day (chat-with-journal) */}
      {journalChatContext && (
        <span className="text-[10px] px-2 py-0.5 rounded-full bg-emerald-500/15 text-emerald-400 shrink-0 flex items-center gap-1">
          <CalendarDays size={10} />
          {journalChatContext.day}
          <button
            onClick={() => setJournalChatContext(null)}
            aria-label="Detach journal context"
            title="Detach journal context"
            className="hover:text-emerald-200"
          >
            <X size={10} />
          </button>
        </span>
      )}

      {/* Input */}
      <div className="flex-1 relative">
        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={settings.tutor_mode ? "Ready to learn \u2014 ask a question or say what you see..." : "Ask about what's on your screen..."}
          disabled={isStreaming}
          aria-label="Question input"
          className="w-full bg-zinc-900/80 border border-zinc-700/50 rounded-lg px-3 py-1.5 text-sm text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/25 disabled:opacity-50"
        />
        {hasScreenshot && (
          <div className="absolute right-2 top-1/2 -translate-y-1/2">
            <Camera size={14} className="text-accent" />
          </div>
        )}
      </div>

      {/* Actions */}
      <div className="flex items-center gap-1 shrink-0">
        <button
          onClick={() => takeScreenshot()}
          title="Take screenshot"
          aria-label="Take screenshot"
          className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 transition-colors"
        >
          <Camera size={16} />
        </button>

        <button
          // U1 audit: push-to-talk via Pointer Events (covers mouse +
          // touch + pen) AND Space/Enter for keyboard users. Without
          // the keydown/keyup handlers, a tab-focused mic button is
          // unreachable for blind / keyboard-only students.
          onPointerDown={(e) => {
            // Capture the pointer so a drag off the button still
            // releases stop_mic_capture cleanly.
            e.currentTarget.setPointerCapture(e.pointerId);
            startRecording();
          }}
          onPointerUp={(e) => {
            try {
              e.currentTarget.releasePointerCapture(e.pointerId);
            } catch {
              /* releasing an already-released capture is fine */
            }
            stopRecording();
          }}
          onPointerCancel={() => {
            if (isRecording) stopRecording();
          }}
          onKeyDown={(e) => {
            // Space and Enter both activate; preventDefault on Space
            // so the page doesn't scroll while the user holds it.
            if ((e.key === " " || e.key === "Enter") && !e.repeat) {
              e.preventDefault();
              if (!isRecording) startRecording();
            }
          }}
          onKeyUp={(e) => {
            if (e.key === " " || e.key === "Enter") {
              e.preventDefault();
              if (isRecording) stopRecording();
            }
          }}
          onBlur={() => {
            // Releasing focus while recording (e.g. alt-tab) should
            // not leave the mic stream open.
            if (isRecording) stopRecording();
          }}
          title={isRecording ? "Release to stop recording" : "Hold to talk (Space)"}
          aria-label={isRecording ? "Stop recording" : "Push to talk (Space or Enter to record)"}
          aria-pressed={isRecording}
          className={`p-1.5 rounded-md transition-colors relative ${
            isRecording
              ? "bg-red-500/20 text-red-400"
              : "hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200"
          }`}
        >
          {isRecording && (
            <span className="absolute inset-0 rounded-md bg-red-500/20 animate-pulse-ring" />
          )}
          <Mic size={16} />
        </button>

        {hasTtsKey && (
          <button
            onClick={handleMuteToggle}
            title={settings.tts_enabled ? "Mute voice output" : "Unmute voice output"}
            aria-label={settings.tts_enabled ? "Mute voice output" : "Unmute voice output"}
            aria-pressed={!settings.tts_enabled}
            className={`p-1.5 rounded-md transition-colors ${
              settings.tts_enabled
                ? "hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200"
                : "bg-zinc-800 text-zinc-500"
            }`}
          >
            {settings.tts_enabled ? <Volume2 size={16} /> : <VolumeX size={16} />}
          </button>
        )}

        {hasTtsKey && settings.tts_enabled && ttsPlaybackState.active && (
          <button
            onClick={handlePauseToggle}
            title={ttsPlaybackState.paused ? "Resume voice output" : "Pause voice output"}
            aria-label={ttsPlaybackState.paused ? "Resume voice output" : "Pause voice output"}
            className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 transition-colors"
          >
            {ttsPlaybackState.paused ? <Play size={16} /> : <Pause size={16} />}
          </button>
        )}

        <button
          onClick={() => updateSettings({ tutor_mode: !settings.tutor_mode })}
          title={settings.tutor_mode ? "Tutor mode ON — click to disable" : "Enable tutor mode"}
          aria-label={settings.tutor_mode ? "Disable tutor mode" : "Enable tutor mode"}
          className={`p-1.5 rounded-md transition-colors ${
            settings.tutor_mode
              ? "bg-amber-500/20 text-amber-400"
              : "hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200"
          }`}
        >
          <BookOpen size={16} />
        </button>

        <button
          onClick={() => handleSubmit()}
          disabled={!input.trim() || isStreaming}
          title="Send"
          aria-label="Send message"
          className="p-1.5 rounded-md bg-accent/20 hover:bg-accent/30 text-accent disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
        >
          {isStreaming ? (
            <Loader2 size={16} className="animate-spin" />
          ) : (
            <Send size={16} />
          )}
        </button>

        {settings.wotch_integration_enabled && wotchInstalled && (
          <button
            onClick={handleOpenInWotch}
            title={
              input.trim()
                ? "Open in Wotch — run this prompt via Claude Code"
                : "Open Wotch terminal"
            }
            aria-label="Open in Wotch"
            className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 transition-colors"
          >
            <Terminal size={16} />
          </button>
        )}

        <button
          onClick={() => setCurrentPage("journal")}
          title="Work journal"
          aria-label="Work journal"
          className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 transition-colors"
        >
          <CalendarDays size={16} />
        </button>

        <button
          onClick={() => setCurrentPage("settings")}
          title="Settings"
          aria-label="Settings"
          className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 transition-colors"
        >
          <Settings size={16} />
        </button>

        {messages.length > 0 && (
          <button
            onClick={clearMessages}
            title="Clear messages"
            aria-label="Clear messages"
            className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200 transition-colors"
          >
            <Trash2 size={16} />
          </button>
        )}

        <button
          onClick={() => invoke("toggle_visibility").catch(() => {})}
          title="Hide to tray (tray icon or Ctrl+Shift+S to bring back)"
          aria-label="Hide WorkBuddy"
          className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-500 hover:text-zinc-200 transition-colors"
        >
          <Minus size={16} />
        </button>

        <button
          onClick={() => exit(0)}
          title="Close WorkBuddy"
          aria-label="Close"
          className="p-1.5 rounded-md hover:bg-red-900/30 text-zinc-500 hover:text-red-400 transition-colors"
        >
          <X size={16} />
        </button>
      </div>
    </div>
  );
}
