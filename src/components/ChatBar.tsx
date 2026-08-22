import { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback, type EventName } from "@tauri-apps/api/event";
import {
  BookOpen,
  Send,
  Settings,
  Loader2,
  Briefcase,
  X,
  Minus,
  Trash2,
  ChevronDown,
  ChevronUp,
} from "lucide-react";
import { exit } from "@tauri-apps/plugin-process";
import { useApp } from "../contexts/app.context";
import { buildSystemPrompt } from "../lib/prompts";
import { saveTurn } from "../lib/db";
import { friendlyStreamError } from "../lib/friendlyError";

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
    conversationIdRef,
    externalQuestion,
    setExternalQuestion,
    submitExternalRef,
  } = useApp();
  const [input, setInput] = useState("");
  const streamBufferRef = useRef("");
  const renderTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const submittingRef = useRef(false);
  const lastUserMsgRef = useRef<{ id: string; role: string; content: string; timestamp: number } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const handleSubmitRef = useRef<(overrideText?: string) => void>(() => {});
  // Refs to avoid stale closures in event listeners
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const currentContextRef = useRef(currentContext);
  currentContextRef.current = currentContext;
  const messagesRef = useRef(messages);
  messagesRef.current = messages;

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
      event: EventName,
      handler: EventCallback<T>,
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

      // `external-question`: a local tool posted a question to our /ask
      // endpoint. Surface a confirmation banner instead of auto-submitting —
      // S1 audit threat: any local process holding the extension token
      // could otherwise drive the user's LLM quota silently. The
      // banner shows the source + question and requires an explicit
      // Submit click before the prompt reaches handleSubmit.
      await safeListen<{ source: string; question: string; context?: string }>(
        "external-question",
        async (event) => {
          const { source, question, context } = event.payload;
          if (!question || !question.trim()) return;
          // Surface the UI even if WordBuddy was hidden/minimised — the
          // user needs to see the confirmation banner.
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

  // Global shortcut listener: focus-text-input
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    async function safeListen<T>(
      event: EventName,
      handler: EventCallback<T>,
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
      await safeListen("focus-text-input", () => {
        inputRef.current?.focus();
      });
    })();
    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
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

    // Add user message
    const userMsg = {
      id: crypto.randomUUID(),
      role: "user" as const,
      content: text,
      timestamp: Date.now(),
    };
    lastUserMsgRef.current = userMsg;
    setMessages((prev) => [...prev, userMsg]);

    // Build conversation history — use ref to avoid stale closure
    const history = messagesRef.current
      .filter((m) => m.id !== "streaming")
      .slice(-10)
      .map((m) => ({
        role: m.role,
        content: m.content,
      }));

    const systemPrompt = buildSystemPrompt(settings.tutor_mode);

    try {
      await invoke("stream_response", {
        systemPrompt,
        userMessage: text,
        conversationHistory: history,
        model: settings.model,
        provider: settings.provider,
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
  }, [input, isStreaming, settings, setIsExpanded, setIsStreaming, setMessages]);

  // Keep ref in sync for external-question submit path
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

  return (
    <div
      className="flex items-center gap-2 px-3 h-[54px] min-h-[54px]"
      data-tauri-drag-region
    >
      {/* Logo + expand toggle */}
      <div className="flex items-center gap-1 shrink-0">
        <Briefcase size={20} className="text-accent" />
        <span className="text-sm font-heading font-semibold text-accent hidden sm:inline">
          WordBuddy
        </span>
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

      {/* Input */}
      <div className="flex-1 relative">
        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={settings.tutor_mode ? "Ready to learn \u2014 ask a question..." : "Ask WordBuddy..."}
          disabled={isStreaming}
          aria-label="Question input"
          className="w-full bg-zinc-900/80 border border-zinc-700/50 rounded-lg px-3 py-1.5 text-sm text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/25 disabled:opacity-50"
        />
      </div>

      {/* Actions */}
      <div className="flex items-center gap-1 shrink-0">
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
          aria-label="Hide WordBuddy"
          className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-500 hover:text-zinc-200 transition-colors"
        >
          <Minus size={16} />
        </button>

        <button
          onClick={() => exit(0)}
          title="Close WordBuddy"
          aria-label="Close"
          className="p-1.5 rounded-md hover:bg-red-900/30 text-zinc-500 hover:text-red-400 transition-colors"
        >
          <X size={16} />
        </button>
      </div>
    </div>
  );
}
