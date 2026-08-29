// Widget window UI (PLAN-04): suggestion-card mode + selection-rewrite
// palette mode. Runs in the `widget` webview window (label-routed from
// main.tsx). Never steals focus; Esc hides.

import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback, type EventName } from "@tauri-apps/api/event";
import { emit } from "@tauri-apps/api/event";

interface TextIssue {
  id: string;
  kind: "correctness" | "clarity" | "engagement" | "delivery";
  start: number;
  end: number;
  original: string;
  message: string;
  replacements: string[];
  ruleId: string;
  source: string;
}

interface IssuesEvent {
  targetKey: string;
  issues: TextIssue[];
  revoked: boolean;
}

const KIND_COLOR: Record<TextIssue["kind"], string> = {
  correctness: "#ef4444",
  clarity: "#3b82f6",
  engagement: "#22c55e",
  delivery: "#a855f7",
};

// ── safeListen (cancelled-flag pattern, ChatBar convention) ─────────

async function safeListen<T>(
  cancelledRef: { current: boolean },
  unlisteners: Array<() => void>,
  event: EventName,
  handler: EventCallback<T>,
): Promise<void> {
  try {
    const u = await listen<T>(event, handler);
    if (cancelledRef.current) {
      u();
      return;
    }
    unlisteners.push(u);
  } catch (err) {
    console.warn(`[widget] listen("${String(event)}") failed:`, err);
  }
}

// ── Suggestion card mode ────────────────────────────────────────────

function friendlyProcess(targetKey: string): string {
  // "native:notepad.exe@598,164" → "Notepad"
  const raw = targetKey.split("@")[0].replace("native:", "");
  const name = raw.replace(/\.exe$/i, "");
  if (!name) return targetKey;
  return name.charAt(0).toUpperCase() + name.slice(1);
}

function SuggestionCard() {
  const [targetKey, setTargetKey] = useState("");
  const [issues, setIssues] = useState<TextIssue[]>([]);
  const [ignored, setIgnored] = useState<Set<string>>(new Set());
  // Words accepted into the personal dictionary from this card —
  // hidden locally right away; harper stops flagging them on the
  // next check (config-backed via add_dictionary_word).
  const [dictAdded, setDictAdded] = useState<Set<string>>(new Set());
  const [status, setStatus] = useState<string | null>(null);
  const [activeRow, setActiveRow] = useState(0);
  // Text snapshot for apply requests — the LAST issues event carries
  // spans against the field text the monitor read.
  const fieldTextRef = useRef({ targetKey: "", text: "" });
  const targetKeyRef = useRef("");
  const cardRef = useRef<HTMLDivElement>(null);
  const [hasFieldText, setHasFieldText] = useState(false);

  // Persisted rule mutes: Ignore survives restarts (config-backed);
  // "Unmute all" in the footer clears them.
  useEffect(() => {
    invoke<{ ignored_rules?: string[] }>("get_settings")
      .then((cfg) => {
        if (Array.isArray(cfg.ignored_rules)) setIgnored(new Set(cfg.ignored_rules));
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    const cancelled = { current: false };
    const unlisteners: Array<() => void> = [];
    (async () => {
      await safeListen<IssuesEvent>(cancelled, unlisteners, "wb://issues", (event) => {
        const payload = event.payload;
        setTargetKey(payload.targetKey);
        targetKeyRef.current = payload.targetKey ?? "";
        fieldTextRef.current = { targetKey: payload.targetKey ?? "", text: "" };
        setHasFieldText(false);
        setIssues(payload.issues ?? []);
        setActiveRow(0);
        setStatus(null);
      });
      // Field text arrives with the focus event (P3 emits caret=null +
      // rect; the text itself rides a dedicated payload below).
      await safeListen<{ targetKey: string; text?: string }>(
        cancelled,
        unlisteners,
        "wb://field-text",
        (event) => {
          if (
            typeof event.payload.text === "string" &&
            event.payload.targetKey === targetKeyRef.current
          ) {
            fieldTextRef.current = {
              targetKey: event.payload.targetKey,
              text: event.payload.text,
            };
            setHasFieldText(true);
          }
        },
      );
      await safeListen<{ id: string; ok: boolean; error?: string }>(
        cancelled,
        unlisteners,
        "wb://apply-result",
        (event) => {
          setStatus(
            event.payload.ok
              ? "Applied"
              : `Not applied: ${event.payload.error ?? "failed"}`,
          );
        },
      );
      if (!cancelled.current) {
        await emit("wb://widget-mode-ready", { mode: "card" });
      }
    })();
    return () => {
      cancelled.current = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);


  const visible = useMemo(
    () => issues.filter(
      (i) =>
        !ignored.has(i.ruleId) &&
        !(i.source === "harper" && dictAdded.has(i.original.trim())),
    ),
    [dictAdded, ignored, issues],
  );

  // Dismissal: notify the coordinator so it suppresses re-showing the
  // card for this exact issue set (offset-free signature kept in App).
  const hide = () => {
    if (targetKeyRef.current) {
      void emit("wb://widget-dismissed", { targetKey: targetKeyRef.current });
    }
    invoke("widget_hide").catch(() => {});
  };

  useEffect(() => {
    const el = cardRef.current;
    if (!el) return;
    const report = () => {
      const height = Math.ceil(el.getBoundingClientRect().height) + 12;
      invoke("widget_set_size", { width: 340, height }).catch(() => {});
    };
    report();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // Window-level keyboard handling: SetForegroundWindow on the widget
  // window does NOT transfer DOM focus, so key events land on <body>.
  // Listening on window (capture) makes ↑/↓/Enter/Esc work regardless.
  // Rebound on state change — no stale closures, no refs.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        hide();
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveRow((r) => Math.min(r + 1, visible.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveRow((r) => Math.max(0, r - 1));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const row = visible[activeRow];
        if (row?.replacements[0]) {
          void applyIssue(row, row.replacements[0]);
        }
      }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visible, activeRow]);



  const applyIssue = async (issue: TextIssue, replacement: string) => {
    const snapshot = fieldTextRef.current;
    if (snapshot.targetKey !== targetKeyRef.current) {
      setStatus("Waiting for the field snapshot…");
      return;
    }
    setStatus("Applying…");
    try {
      await invoke("apply_fix_command", {
        request: {
          process: friendlyProcess(targetKey) + ".exe", // best-effort reverse map
          originalText: snapshot.text,
          start: issue.start,
          end: issue.end,
          replacement,
        },
      });
    } catch (e) {
      setStatus(`Not applied: ${String(e)}`);
    }
  };


  return (
    <div
      tabIndex={-1}
      ref={cardRef}
      className="w-full bg-zinc-900/95 backdrop-blur-md rounded-xl ring-1 ring-inset ring-zinc-700/60 p-3 pb-2 text-zinc-200 text-xs outline-none"
    >
      <div className="flex items-center justify-between mb-2">
        <div className="flex items-center gap-1.5">
          <span className="font-semibold">{friendlyProcess(targetKey) || "WordBuddy"}</span>
          {(["correctness", "clarity", "engagement", "delivery"] as const)
            .filter((k) => visible.some((i) => i.kind === k))
            .map((k) => (
              <span
                key={k}
                className="inline-block w-2 h-2 rounded-full"
                style={{ backgroundColor: KIND_COLOR[k] }}
                aria-label={k}
              />
            ))}
        </div>
        <button
          onClick={hide}
          aria-label="Hide suggestions"
          className="text-zinc-500 hover:text-zinc-200"
        >
          ✕
        </button>
      </div>

      <div className="space-y-2 overflow-y-auto" style={{ maxHeight: 150 }}>
        {visible.map((issue, idx) => (
          <div
            key={issue.id}
            className={`p-2 rounded-lg border ${
              idx === activeRow
                ? "border-accent/60 bg-accent/10"
                : "border-zinc-700/60 bg-zinc-800/40"
            }`}
            onMouseEnter={() => setActiveRow(idx)}
          >
            <div className="flex items-start justify-between gap-2">
              <p className="flex-1">
                <span
                  className="inline-block w-1.5 h-1.5 rounded-full mr-1.5"
                  style={{ backgroundColor: KIND_COLOR[issue.kind] }}
                />
                {issue.message}
              </p>
              <button
                onClick={() => {
                  setIgnored((prev) => new Set(prev).add(issue.ruleId));
                  invoke("ignore_rule", { ruleId: issue.ruleId }).catch(() => {});
                }}
                title="Never suggest this rule again"
                aria-label="Ignore rule"
                className="text-zinc-600 hover:text-zinc-300 shrink-0"
              >
                ✕
              </button>
            </div>
            {issue.replacements.length > 0 && (
              <div className="flex flex-wrap gap-1.5 mt-1.5">
                {issue.replacements.slice(0, 3).map((rep, rIdx) => (
                  <button
                    key={`${issue.id}-${rIdx}-${rep}`}
                    onClick={() => void applyIssue(issue, rep)}
                    disabled={!hasFieldText}
                    className={`px-2 py-0.5 rounded-full border ${
                      rIdx === 0
                        ? "border-accent bg-accent/20 text-accent"
                        : "border-zinc-600 bg-zinc-800 text-zinc-200"
                    } hover:brightness-125 disabled:cursor-wait disabled:opacity-40`}
                    title={!hasFieldText ? "Waiting for the current field text" : (
                      issue.source === "harper"
                        ? "Replaces the field text (undo history is replaced for simple fields)"
                        : "Replaces the selected span"
                    )}
                  >
                    {rep}
                  </button>
                ))}
                {issue.source === "harper" && (
                  <button
                    onClick={() => {
                      const word = issue.original.trim();
                      if (!word) return;
                      setDictAdded((prev) => new Set(prev).add(word));
                      invoke("add_dictionary_word", { word }).catch(() => {});
                    }}
                    title={`Accept "${issue.original.trim()}" everywhere — it stops being flagged`}
                    className="px-2 py-0.5 rounded-full border border-dashed border-zinc-600 text-zinc-500 hover:text-zinc-200 text-[10px]"
                  >
                    + accept
                  </button>
                )}
              </div>
            )}
          </div>
        ))}
      </div>

      <div className="flex items-center justify-between mt-2 pt-2 border-t border-zinc-700/50">
        <span className="text-[10px] text-zinc-500">
          {status ?? "↑↓ navigate · Enter applies · Esc hides"}
        </span>
        <div className="flex items-center gap-2 shrink-0">
          {ignored.size > 0 && (
            <button
              onClick={() => {
                setIgnored(new Set());
                invoke("reset_ignored_rules").catch(() => {});
              }}
              title="Restore all muted suggestion rules"
              className="text-[10px] text-zinc-500 hover:text-zinc-300"
            >
              Unmute {ignored.size} rule{ignored.size === 1 ? "" : "s"}
            </button>
          )}
          <button
            onClick={() => {
              // INV-EXCL-001 surface: stops ALL reading of this app —
              // no field text, no checks, no widget. Persisted in
              // config; reversible in Settings → excluded processes.
              const raw = targetKey.split("@")[0].replace("native:", "");
              if (raw) invoke("exclude_process", { process: raw }).catch(() => {});
              hide();
            }}
            title={`Never read or suggest in ${friendlyProcess(targetKey)}`}
            className="text-[10px] text-zinc-500 hover:text-zinc-300"
          >
            Don't monitor
          </button>
          <button
            onClick={() => {
              invoke("snooze_monitor", { minutes: 60 }).catch(() => {});
              hide();
            }}
            title="Pause all monitoring for one hour (no reads, no popups)"
            className="text-[10px] text-zinc-500 hover:text-zinc-300"
          >
            Snooze 1 h
          </button>
          <button
            onClick={() => {
              // INV-PRIV-002: never paste ambient text into the editor.
              // Opens the main window's editor surface empty.
              invoke("show_main_window").catch(() => {});
            }}
            className="text-[10px] text-zinc-500 hover:text-zinc-300"
          >
            Open editor
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Selection rewrite palette ───────────────────────────────────────

const REWRITE_ACTIONS = [
  "Proofread",
  "Rewrite",
  "Make concise",
  "Professional",
  "Friendly",
] as const;

function Palette() {
  const [selection, setSelection] = useState("");
  const [instruction, setInstruction] = useState<string>("Proofread");
  const [custom, setCustom] = useState("");
  const [result, setResult] = useState("");
  const [streaming, setStreaming] = useState(false);
  const bufferRef = useRef("");
  const streamingRef = useRef(false);

  useEffect(() => {
    const cancelled = { current: false };
    const unlisteners: Array<() => void> = [];
    (async () => {
      await safeListen<{ text: string }>(cancelled, unlisteners, "palette-open", (event) => {
        setSelection(event.payload.text);
        setResult("");
        bufferRef.current = "";
      });
      await safeListen<string>(cancelled, unlisteners, "chat_stream_chunk", (event) => {
        if (!streamingRef.current) return;
        bufferRef.current += event.payload;
        setResult(bufferRef.current);
      });
      await safeListen<unknown>(cancelled, unlisteners, "chat_stream_complete", () => {
        if (!streamingRef.current) return;
        streamingRef.current = false;
        setStreaming(false);
      });
      if (!cancelled.current) {
        await emit("wb://widget-mode-ready", { mode: "palette" });
      }
    })();
    return () => {
      cancelled.current = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  const run = async () => {
    if (!selection.trim()) return;
    streamingRef.current = true;
    setStreaming(true);
    setResult("");
    bufferRef.current = "";
    const chosen = instruction === "Custom" ? custom : instruction;
    try {
      // Route through the user's configured provider/model — the bare
      // defaults would send non-Anthropic users' rewrites to Anthropic
      // with a hard-coded model (audit M1).
      const cfg = await invoke<{ provider: string; model: string }>("get_settings").catch(
        () => null,
      );
      await invoke("stream_response", {
        systemPrompt:
          `You rewrite selected text. Instruction: ${chosen}. ` +
          `Respond ONLY with the rewritten text — no preamble, no quotes, no markdown fences.`,
        userMessage: selection,
        conversationHistory: [],
        provider: cfg?.provider ?? null,
        model: cfg?.model || null,
      });
    } catch (e) {
      streamingRef.current = false;
      setStreaming(false);
      setResult(`Failed: ${String(e)}`);
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(result);
    } catch {
      // clipboard unavailable — user can select the text manually
    }
  };

  return (
    <div className="w-full h-full bg-zinc-900/95 backdrop-blur-md rounded-xl ring-1 ring-inset ring-zinc-700/60 p-3 text-zinc-200 text-xs flex flex-col">
      <div className="flex items-center justify-between mb-2">
        <span className="font-semibold">Rewrite selection</span>
        <button
          onClick={() => invoke("widget_hide").catch(() => {})}
          aria-label="Close palette"
          className="text-zinc-500 hover:text-zinc-200"
        >
          ✕
        </button>
      </div>
      <p className="text-zinc-500 mb-2 truncate" title={selection}>
        {selection.length > 90 ? selection.slice(0, 90) + "…" : selection}
      </p>
      <div className="flex flex-wrap gap-1.5 mb-2">
        {REWRITE_ACTIONS.map((a) => (
          <button
            key={a}
            onClick={() => setInstruction(a)}
            className={`px-2 py-0.5 rounded-full border ${
              instruction === a
                ? "border-accent bg-accent/20 text-accent"
                : "border-zinc-600 bg-zinc-800 text-zinc-300"
            }`}
          >
            {a}
          </button>
        ))}
        <button
          onClick={() => setInstruction("Custom")}
          className={`px-2 py-0.5 rounded-full border ${
            instruction === "Custom"
              ? "border-accent bg-accent/20 text-accent"
              : "border-zinc-600 bg-zinc-800 text-zinc-300"
          }`}
        >
          Custom…
        </button>
      </div>
      {instruction === "Custom" && (
        <input
          type="text"
          name="rewrite-instruction"
          autoComplete="off"
          value={custom}
          onChange={(e) => setCustom(e.target.value)}
          placeholder="e.g. Make it sound confident…"
          aria-label="Custom rewrite instruction"
          className="mb-2 bg-zinc-800 border border-zinc-700 rounded-lg px-2 py-1 text-xs outline-none focus:border-accent/50"
        />
      )}
      <button
        onClick={() => void run()}
        disabled={streaming || !selection.trim()}
        className="self-start px-3 py-1 rounded-lg bg-accent/20 text-accent hover:bg-accent/30 disabled:opacity-40 mb-2"
      >
        {streaming ? "Rewriting…" : "Rewrite"}
      </button>
      <div className="flex-1 overflow-y-auto whitespace-pre-wrap text-zinc-200">
        {result}
      </div>
      {result && !streaming && (
        <div className="flex gap-2 mt-2 pt-2 border-t border-zinc-700/50">
          <button
            onClick={() => void copy()}
            className="px-2 py-0.5 rounded-lg bg-zinc-800 text-zinc-200 hover:bg-zinc-700"
          >
            Copy
          </button>
        </div>
      )}
    </div>
  );
}

export default function WidgetApp() {
  const [mode, setMode] = useState<"card" | "palette">("card");
  const modeRef = useRef(mode);
  useEffect(() => {
    const cancelled = { current: false };
    const unlisteners: Array<() => void> = [];
    (async () => {
      await safeListen<string>(cancelled, unlisteners, "widget-mode", (event) => {
        const nextMode = event.payload === "palette" ? "palette" : "card";
        modeRef.current = nextMode;
        setMode(nextMode);
      });
      await safeListen(cancelled, unlisteners, "wb://widget-ready-request", () => {
        void emit("wb://widget-ready", { mode: modeRef.current });
      });
      if (!cancelled.current) {
        await emit("wb://widget-ready", { mode: modeRef.current });
      }
    })();
    return () => {
      cancelled.current = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);
  return mode === "palette" ? <Palette /> : <SuggestionCard />;
}
