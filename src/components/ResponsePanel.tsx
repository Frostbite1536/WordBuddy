import React, { useRef, useEffect, useState, useCallback } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Copy, Check, Minus, Volume2, Square, ScrollText, Shield } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import { useApp } from "../contexts/app.context";
import { parsePointTags } from "../lib/pointParser";
import { confirmExternalLink } from "../lib/safeOpen";

// Memoized markdown renderer — only re-renders when content actually changes.
// Without this, every streaming chunk triggers ReactMarkdown re-parse of
// ALL prior messages, which blocks the WebView2 main thread and freezes
// the native window on Windows.
const MemoizedMarkdown = React.memo(function MemoizedMarkdown({
  content,
  ttsAvailable,
  messageId,
  onListen,
  playingId,
}: {
  content: string;
  ttsAvailable: boolean;
  messageId: string;
  onListen: (id: string, content: string) => void;
  playingId: string | null;
}) {
  return (
    <div className="prose text-zinc-300 max-w-full">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          pre({ children, ...props }) {
            let codeText = "";
            if (children && typeof children === "object" && "props" in (children as React.ReactElement)) {
              const codeEl = children as React.ReactElement<{ children?: React.ReactNode }>;
              codeText = String(codeEl.props.children ?? "");
            }
            return (
              <div className="relative group">
                <pre {...props}>{children}</pre>
                <CopyButton code={codeText} />
              </div>
            );
          },
          a({ href, children, ...props }) {
            return (
              <a
                {...props}
                href="#"
                title={href}
                onClick={(e) => {
                  e.preventDefault();
                  if (!href) return;
                  // S4 audit: model-rendered markdown links are a
                  // prompt-injection / phishing vector. Always
                  // confirm the destination before opening in the
                  // system browser so a label like "[Limitless](attacker.com)"
                  // can't silently navigate the student.
                  if (confirmExternalLink(href)) {
                    open(href).catch(() => {});
                  }
                }}
                className="text-accent hover:underline cursor-pointer"
              >
                {children}
              </a>
            );
          },
          code({ className, children, ...props }) {
            const isInline = !className;
            if (isInline) {
              return <code {...props}>{children}</code>;
            }
            return (
              <code className={className} {...props}>
                {children}
              </code>
            );
          },
        }}
      >
        {content}
      </ReactMarkdown>
      {ttsAvailable && messageId !== "streaming" && (
        <button
          onClick={() => onListen(messageId, content)}
          className="mt-1 p-1 rounded hover:bg-zinc-800 text-zinc-500 hover:text-zinc-300 transition-colors"
          title={playingId === messageId ? "Stop" : "Listen"}
          aria-label={playingId === messageId ? "Stop audio" : "Listen to response"}
        >
          {playingId === messageId ? <Square size={14} /> : <Volume2 size={14} />}
        </button>
      )}
    </div>
  );
});

function CopyButton({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard API may fail in some contexts
    }
  };

  return (
    <button
      onClick={handleCopy}
      className="absolute top-2 right-2 p-1 rounded bg-zinc-700/50 hover:bg-zinc-700 text-zinc-400 hover:text-zinc-200 transition-colors"
      title="Copy code"
      aria-label="Copy code to clipboard"
    >
      {copied ? <Check size={14} /> : <Copy size={14} />}
    </button>
  );
}

export default function ResponsePanel() {
  const {
    messages,
    setMessages,
    settings,
    isStreaming,
    setIsExpanded,
    externalQuestion,
    setExternalQuestion,
    submitExternalRef,
  } = useApp();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [playingId, setPlayingId] = useState<string | null>(null);
  const playingIdRef = useRef<string | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  // Use ref for settings to avoid stale closures in async handleListen
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  // Auto-scroll to bottom on new content.
  // Uses rAF to avoid forced synchronous reflows during render — prevents
  // layout thrashing that can freeze transparent WebView2 windows on Win10.
  useEffect(() => {
    const el = scrollRef.current;
    if (el) {
      requestAnimationFrame(() => {
        if (scrollRef.current) {
          scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
      });
    }
  }, [messages]);

  // Use ref for playingId to avoid stale closure in async handleListen
  const handleListen = useCallback(
    async (messageId: string, content: string) => {
      // Stop if already playing this message
      if (playingIdRef.current === messageId) {
        if (audioRef.current) {
          audioRef.current.pause();
          audioRef.current = null;
        }
        playingIdRef.current = null;
        setPlayingId(null);
        return;
      }

      // Stop any currently playing audio
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current = null;
      }

      try {
        playingIdRef.current = messageId;
        setPlayingId(messageId);
        const params: Record<string, string> = { text: content };
        const voice = settingsRef.current.tts_voice;
        if (voice && voice !== "default") {
          params.voiceId = voice;
        }
        const provider = settingsRef.current.tts_provider;
        if (provider) {
          params.provider = provider;
        }
        const base64Audio = await invoke<string>("synthesize_speech", params);
        const mimeType = provider === "gemini" ? "audio/wav" : "audio/mpeg";
        const audio = new Audio(`data:${mimeType};base64,${base64Audio}`);
        audio.onended = () => {
          playingIdRef.current = null;
          setPlayingId(null);
          if (audioRef.current === audio) audioRef.current = null;
        };
        audio.onerror = () => {
          playingIdRef.current = null;
          setPlayingId(null);
          if (audioRef.current === audio) audioRef.current = null;
        };
        // Assign AFTER play() resolves so a synchronous reject (autoplay
        // policy block, decoder failure, malformed MIME) doesn't leave
        // a corpse Audio element in audioRef that the next "stop any
        // currently playing audio" branch tries to .pause().
        await audio.play();
        audioRef.current = audio;
      } catch {
        playingIdRef.current = null;
        setPlayingId(null);
        // Defensive: in case a future refactor sets audioRef before
        // play(), null it on failure so subsequent clicks don't act
        // on a dead element.
        if (audioRef.current && audioRef.current.src.startsWith("data:")) {
          audioRef.current = null;
        }
      }
    },
    [],
  );

  // Clean up audio on unmount
  useEffect(() => {
    return () => {
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current = null;
      }
    };
  }, []);

  // When the user mutes TTS (toolbar Volume button), stop any per-message
  // replay that's currently playing — otherwise mute only silences the
  // streaming-TTS queue and the replay audio keeps going.
  useEffect(() => {
    if (!settings.tts_enabled && audioRef.current) {
      audioRef.current.pause();
      audioRef.current = null;
      playingIdRef.current = null;
      setPlayingId(null);
    }
  }, [settings.tts_enabled]);

  // Parse point tags from finalized assistant messages and emit pointer events.
  // Uses refs to avoid re-triggering on every message array change.
  const prevStreamingRef = useRef(true);
  const processedPointMsgRef = useRef<string | null>(null);
  useEffect(() => {
    if (prevStreamingRef.current && !isStreaming) {
      // Stream just completed — check the last assistant message for point tags
      const lastMsg = messages[messages.length - 1];
      if (
        lastMsg &&
        lastMsg.role === "assistant" &&
        lastMsg.id !== "streaming" &&
        lastMsg.id !== processedPointMsgRef.current
      ) {
        processedPointMsgRef.current = lastMsg.id;
        const { cleanText, points } = parsePointTags(lastMsg.content);
        if (points.length > 0) {
          // Update message content to strip tags
          setMessages((prev) =>
            prev.map((m) =>
              m.id === lastMsg.id ? { ...m, content: cleanText } : m,
            ),
          );
          // Emit pointer events sequentially
          points.forEach((point, i) => {
            setTimeout(() => {
              invoke("show_pointer", { target: point }).catch(() => {});
            }, i * 1500);
          });
        }
      }
    }
    prevStreamingRef.current = isStreaming;
  }, [isStreaming, messages, setMessages]);

  // Filter out duplicate streaming messages for display
  // Keep only the last streaming message (dedup in-progress updates)
  const lastStreamingIdx = (() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].id === "streaming") return i;
    }
    return -1;
  })();
  const displayMessages = messages.filter(
    (m, i) => m.id !== "streaming" || i === lastStreamingIdx,
  );

  const hasTtsKey = settings.tts_provider === "gemini"
    ? !!settings.api_keys?.google
    : !!settings.api_keys?.elevenlabs;
  const ttsAvailable = hasTtsKey && settings.tts_enabled;

  return (
    <div className="flex-1 flex flex-col min-h-0 border-t border-zinc-800/50">
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-zinc-800/30">
        <span className="text-xs text-zinc-500">
          {isStreaming
            ? "Thinking..."
            : `${displayMessages.filter((m) => m.id !== "streaming").length} messages`}
        </span>
        <button
          onClick={() => setIsExpanded(false)}
          className="p-1 rounded hover:bg-zinc-800 text-zinc-500 hover:text-zinc-300"
          title="Minimize"
          aria-label="Minimize response panel"
        >
          <Minus size={14} />
        </button>
      </div>

      {/* Messages */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto px-3 py-2 space-y-3">
        {/* External question confirmation banner (S1 audit). When a
            local tool (e.g. Wotch's "Ask WorkBuddy") posts to the
            /ask endpoint, the question lands here with explicit
            Submit / Discard buttons rather than auto-firing into the
            user's LLM quota. */}
        {externalQuestion && (
          <div
            role="alertdialog"
            aria-live="polite"
            aria-label="External question pending"
            className="bg-amber-500/10 border border-amber-500/40 rounded-lg px-3 py-2 text-xs"
          >
            <div className="flex items-start gap-2">
              <Shield size={14} className="text-amber-400 shrink-0 mt-0.5" />
              <div className="flex-1 min-w-0">
                <p className="text-zinc-300">
                  <span className="text-amber-400 font-medium">{externalQuestion.source}</span>
                  {" "}wants to ask:
                </p>
                <p className="mt-1 text-zinc-200 italic break-words">
                  &ldquo;{externalQuestion.question.length > 240
                    ? externalQuestion.question.slice(0, 240) + "\u2026"
                    : externalQuestion.question}&rdquo;
                </p>
                {externalQuestion.context && (
                  <p className="mt-1 text-[10px] text-zinc-500">
                    Includes terminal context ({externalQuestion.context.length} chars).
                  </p>
                )}
                <div className="flex gap-2 mt-2">
                  <button
                    onClick={() => {
                      const ctx = externalQuestion.context?.trim();
                      const composed = ctx
                        ? `${externalQuestion.question}\n\n[Terminal context from ${externalQuestion.source}]\n\`\`\`\n${ctx}\n\`\`\``
                        : externalQuestion.question;
                      setExternalQuestion(null);
                      submitExternalRef.current(composed);
                    }}
                    className="px-2 py-0.5 rounded bg-amber-500/30 text-amber-200 hover:bg-amber-500/40 transition-colors"
                  >
                    Submit
                  </button>
                  <button
                    onClick={() => setExternalQuestion(null)}
                    className="px-2 py-0.5 rounded bg-zinc-800 text-zinc-300 hover:bg-zinc-700 transition-colors"
                  >
                    Discard
                  </button>
                </div>
              </div>
            </div>
          </div>
        )}

        {displayMessages.map((msg) => (
          <div
            key={msg.id}
            className={msg.role === "user" ? "flex justify-end" : ""}
          >
            {msg.role === "user" ? (
              <div className="max-w-[80%] bg-accent/15 text-accent rounded-lg px-3 py-1.5 text-sm">
                {msg.content}
              </div>
            ) : msg.id === "streaming" ? (
              /* Streaming message: render as plain text to avoid expensive
                 ReactMarkdown re-parsing on every debounced chunk. Markdown
                 formatting is applied once on finalization. */
              <div className="prose text-zinc-300 max-w-full">
                <p className="whitespace-pre-wrap text-sm leading-relaxed">{msg.content}</p>
              </div>
            ) : (
              /* Finalized message: full ReactMarkdown with memoization.
                 React.memo ensures this only re-renders when content changes,
                 not on every streaming tick. */
              <MemoizedMarkdown
                content={msg.content}
                ttsAvailable={ttsAvailable}
                messageId={msg.id}
                onListen={handleListen}
                playingId={playingId}
              />
            )}
          </div>
        ))}

        {isStreaming &&
          displayMessages[displayMessages.length - 1]?.role === "user" && (
            <div className="flex items-center gap-2 text-zinc-500 text-sm">
              <div className="flex gap-1">
                <span
                  className="w-1.5 h-1.5 bg-accent rounded-full animate-bounce"
                  style={{ animationDelay: "0ms" }}
                />
                <span
                  className="w-1.5 h-1.5 bg-accent rounded-full animate-bounce"
                  style={{ animationDelay: "150ms" }}
                />
                <span
                  className="w-1.5 h-1.5 bg-accent rounded-full animate-bounce"
                  style={{ animationDelay: "300ms" }}
                />
              </div>
            </div>
          )}
      </div>
    </div>
  );
}
