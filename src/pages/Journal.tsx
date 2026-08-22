import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowLeft,
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  Loader2,
  RefreshCw,
  Copy,
  Video,
  VideoOff,
  ChevronDown,
  ChevronUp,
  Globe,
} from "lucide-react";
import { MessageSquare, Sparkles } from "lucide-react";
import { useApp } from "../contexts/app.context";
import {
  type TimelineCard,
  type StandupPayload,
  localDayString,
  shiftDay,
  timeRange,
  durationMinutes,
  categoryChipClasses,
  cardMetadata,
  cardsToContextText,
  standupToMarkdown,
} from "../lib/journal";

const JOURNAL_HEIGHT = 600;

interface ObservationRow {
  id: number;
  batch_id: number;
  start_ts: number;
  end_ts: number;
  observation: string;
}

interface ScreenshotRow {
  id: number;
  captured_at: number;
  file_path: string;
  file_size: number;
  idle_seconds: number;
  window_title: string;
}

interface RecorderStatus {
  running: boolean;
  shots_taken: number;
  last_error: string | null;
}

/// Expanded-card detail: detailed summary, distractions, observations and
/// a strip of the batch's frames (lazily loaded via journal_read_screenshot).
function CardDetail({ card, dayShots }: { card: TimelineCard; dayShots: ScreenshotRow[] }) {
  const [observations, setObservations] = useState<ObservationRow[] | null>(null);
  const [frames, setFrames] = useState<Array<{ id: number; b64: string }>>([]);

  useEffect(() => {
    let cancelled = false;
    invoke<ObservationRow[]>("journal_list_observations", { batchId: card.batch_id })
      .then((o) => { if (!cancelled) setObservations(o); })
      .catch(() => { if (!cancelled) setObservations([]); });

    // Up to 4 frames spread across the card's time range.
    const inRange = dayShots.filter(
      (s) => s.captured_at >= card.start_ts && s.captured_at <= card.end_ts
    );
    const picks: ScreenshotRow[] = [];
    if (inRange.length > 0) {
      const count = Math.min(4, inRange.length);
      for (let k = 0; k < count; k++) {
        const idx = Math.round((k * (inRange.length - 1)) / Math.max(1, count - 1));
        if (!picks.includes(inRange[idx])) picks.push(inRange[idx]);
      }
    }
    Promise.all(
      picks.map((s) =>
        invoke<string>("journal_read_screenshot", { id: s.id })
          .then((b64) => ({ id: s.id, b64 }))
          .catch(() => null)
      )
    ).then((results) => {
      if (!cancelled) setFrames(results.filter((r): r is { id: number; b64: string } => !!r));
    });
    return () => { cancelled = true; };
  }, [card.id, card.batch_id, card.start_ts, card.end_ts, dayShots]);

  const meta = cardMetadata(card);

  return (
    <div className="mt-2 pt-2 border-t border-zinc-800/60 space-y-3">
      {card.detailed_summary && (
        <p className="text-xs text-zinc-400 whitespace-pre-wrap">{card.detailed_summary}</p>
      )}

      {(meta.appSites.primary || meta.appSites.secondary) && (
        <p className="text-xs text-zinc-500 flex items-center gap-1.5">
          <Globe size={12} />
          {[meta.appSites.primary, meta.appSites.secondary].filter(Boolean).join(" · ")}
        </p>
      )}

      {meta.distractions.length > 0 && (
        <div className="space-y-1">
          <p className="text-[11px] uppercase tracking-wide text-zinc-600">Distractions</p>
          {meta.distractions.map((d, i) => (
            <p key={i} className="text-xs text-zinc-500">
              {[d.start && d.end ? `${d.start}–${d.end}` : null, d.title, d.summary]
                .filter(Boolean)
                .join(" — ")}
            </p>
          ))}
        </div>
      )}

      {frames.length > 0 && (
        <div className="flex gap-2 overflow-x-auto">
          {frames.map((f) => (
            <img
              key={f.id}
              src={`data:image/jpeg;base64,${f.b64}`}
              alt="Captured frame"
              className="h-20 rounded-md border border-zinc-800 shrink-0"
            />
          ))}
        </div>
      )}

      {observations === null ? (
        <p className="text-xs text-zinc-600 flex items-center gap-1.5">
          <Loader2 size={12} className="animate-spin" /> Loading observations…
        </p>
      ) : observations.length > 0 ? (
        <div className="space-y-1">
          <p className="text-[11px] uppercase tracking-wide text-zinc-600">Observations</p>
          {observations.map((o) => (
            <p key={o.id} className="text-xs text-zinc-500">
              <span className="text-zinc-600">{timeRange(o.start_ts, o.end_ts)}</span>{" "}
              {o.observation}
            </p>
          ))}
        </div>
      ) : null}
    </div>
  );
}

/// Standup tab: saved payload for the day, generate/regenerate via LLM,
/// copy as markdown.
function StandupTab({ day, onNotice }: { day: string; onNotice: (m: string) => void }) {
  const [standup, setStandup] = useState<StandupPayload | null | undefined>(undefined);
  const [generating, setGenerating] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setStandup(undefined);
    invoke<StandupPayload | null>("journal_get_standup", { day })
      .then((s) => { if (!cancelled) setStandup(s); })
      .catch(() => { if (!cancelled) setStandup(null); });
    return () => { cancelled = true; };
  }, [day]);

  const handleGenerate = async () => {
    setGenerating(true);
    try {
      const s = await invoke<StandupPayload>("journal_generate_standup", { day });
      setStandup(s);
    } catch (e) {
      onNotice(`Standup failed: ${e}`);
    }
    setGenerating(false);
  };

  const sections: Array<{ title: string; items: string[] }> = standup
    ? [
        { title: "Highlights", items: standup.highlights },
        { title: "Today", items: standup.tasks },
        { title: "Blockers", items: standup.blockers },
        { title: "Next", items: standup.next },
      ]
    : [];

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <button
          onClick={handleGenerate}
          disabled={generating}
          className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-accent/20 text-accent hover:bg-accent/30 disabled:opacity-40"
        >
          {generating ? (
            <Loader2 size={13} className="animate-spin" />
          ) : (
            <Sparkles size={13} />
          )}
          {standup ? "Regenerate" : "Generate standup"}
        </button>
        {standup && (
          <button
            onClick={async () => {
              await navigator.clipboard.writeText(standupToMarkdown(day, standup));
              onNotice("Copied standup as markdown.");
            }}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-zinc-800 text-zinc-300 hover:bg-zinc-700"
          >
            <Copy size={13} /> Copy as markdown
          </button>
        )}
      </div>

      {standup === undefined ? (
        <p className="text-xs text-zinc-500 flex items-center gap-2">
          <Loader2 size={14} className="animate-spin" /> Loading…
        </p>
      ) : standup === null ? (
        <p className="text-xs text-zinc-600 pt-2">
          No standup for this day yet. Generate one from the timeline cards of
          this day and the day before.
        </p>
      ) : (
        <div className="space-y-3">
          {sections
            .filter((s) => s.items.length > 0)
            .map((s) => (
              <div key={s.title} className="space-y-1">
                <p className="text-[11px] uppercase tracking-wide text-zinc-600">{s.title}</p>
                {s.items.map((item, i) => (
                  <p key={i} className="text-xs text-zinc-300">• {item}</p>
                ))}
              </div>
            ))}
        </div>
      )}
    </div>
  );
}

interface WeekSummary {
  days: Array<{
    day: string;
    category_minutes: Array<[string, number]>;
    total_minutes: number;
    distraction_minutes: number;
  }>;
  total_minutes: number;
  focus_minutes: number;
  distraction_minutes: number;
  top_apps: Array<[string, number]>;
}

/// Bar color per category — solid variants of the chip palette.
function categoryBarColor(category: string): string {
  switch (category) {
    case "engineering": return "bg-sky-500";
    case "design": return "bg-purple-500";
    case "communication": return "bg-emerald-500";
    case "research": return "bg-amber-500";
    case "admin": return "bg-zinc-400";
    case "distraction": return "bg-red-500";
    default: return "bg-zinc-600";
  }
}

function fmtMinutes(mins: number): string {
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

/// Week tab: pure aggregation from the Rust side — stacked CSS bars, no
/// chart library.
function WeekTab({ endDay }: { endDay: string }) {
  const [week, setWeek] = useState<WeekSummary | null>(null);

  useEffect(() => {
    let cancelled = false;
    setWeek(null);
    invoke<WeekSummary>("journal_week_summary", { endDay })
      .then((w) => { if (!cancelled) setWeek(w); })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [endDay]);

  if (!week) {
    return (
      <p className="text-xs text-zinc-500 flex items-center gap-2">
        <Loader2 size={14} className="animate-spin" /> Loading…
      </p>
    );
  }
  if (week.total_minutes === 0) {
    return (
      <p className="text-xs text-zinc-600">
        Nothing analyzed in the 7 days ending {endDay}.
      </p>
    );
  }
  const maxDay = Math.max(...week.days.map((d) => d.total_minutes), 1);

  return (
    <div className="space-y-4">
      <div className="flex gap-4 text-xs">
        <span className="text-zinc-300">Total {fmtMinutes(week.total_minutes)}</span>
        <span className="text-emerald-400">Focus {fmtMinutes(week.focus_minutes)}</span>
        <span className="text-red-400">Distraction {fmtMinutes(week.distraction_minutes)}</span>
      </div>

      {/* Per-day horizontal stacked bars */}
      <div className="space-y-1.5">
        {week.days.map((d) => (
          <div key={d.day} className="flex items-center gap-2">
            <span className="text-[10px] text-zinc-500 w-20 shrink-0 tabular-nums">
              {d.day.slice(5)}
            </span>
            <div className="flex h-3 rounded-sm overflow-hidden bg-zinc-900 flex-1">
              {d.category_minutes.map(([cat, mins]) => (
                <div
                  key={cat}
                  title={`${cat}: ${fmtMinutes(mins)}`}
                  className={categoryBarColor(cat)}
                  style={{ width: `${(mins / maxDay) * 100}%` }}
                />
              ))}
            </div>
            <span className="text-[10px] text-zinc-500 w-12 text-right shrink-0 tabular-nums">
              {d.total_minutes > 0 ? fmtMinutes(d.total_minutes) : ""}
            </span>
          </div>
        ))}
      </div>

      {week.top_apps.length > 0 && (
        <div className="space-y-1">
          <p className="text-[11px] uppercase tracking-wide text-zinc-600">Top apps & sites</p>
          {week.top_apps.map(([app, mins]) => (
            <div key={app} className="flex items-center justify-between text-xs">
              <span className="text-zinc-400">{app}</span>
              <span className="text-zinc-600 tabular-nums">{fmtMinutes(mins)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default function Journal() {
  const { settings, updateSettings, setCurrentPage, isExpanded, setJournalChatContext } = useApp();
  const [day, setDay] = useState(() => localDayString(new Date()));
  const [tab, setTab] = useState<"timeline" | "standup" | "week">("timeline");
  const [cards, setCards] = useState<TimelineCard[] | null>(null);
  const [dayShots, setDayShots] = useState<ScreenshotRow[]>([]);
  const [analyzing, setAnalyzing] = useState(false);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [recorder, setRecorder] = useState<RecorderStatus | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    invoke("set_window_height", { height: JOURNAL_HEIGHT }).catch(() => {});
  }, []);

  const loadDay = useCallback((d: string) => {
    invoke<TimelineCard[]>("journal_list_cards", { day: d })
      .then(setCards)
      .catch(() => setCards([]));
    invoke<ScreenshotRow[]>("journal_list_screenshots", { day: d })
      .then(setDayShots)
      .catch(() => setDayShots([]));
  }, []);

  useEffect(() => {
    setCards(null);
    setExpandedId(null);
    loadDay(day);
  }, [day, loadDay]);

  // Recorder status for the mirror control.
  useEffect(() => {
    let cancelled = false;
    const poll = () => {
      invoke<RecorderStatus>("recorder_status")
        .then((s) => { if (!cancelled) setRecorder(s); })
        .catch(() => {});
    };
    poll();
    const t = setInterval(poll, 10000);
    return () => { cancelled = true; clearInterval(t); };
  }, []);

  const today = localDayString(new Date());
  const isToday = day === today;

  const handleAnalyzeNow = async () => {
    setAnalyzing(true);
    setNotice(null);
    try {
      const summary = await invoke<{
        batches_created: number;
        batches_analyzed: number;
        batches_failed: number;
        cards_written: number;
      }>("journal_analyze_now");
      if (summary.batches_analyzed === 0 && summary.batches_created === 0) {
        setNotice("Nothing new to analyze yet — capture at least ~15 minutes first.");
      } else if (summary.batches_failed > 0) {
        setNotice(
          `Analyzed ${summary.batches_analyzed} batch(es); ${summary.batches_failed} failed — see Settings → Diagnostics.`
        );
      }
      loadDay(day);
    } catch (e) {
      setNotice(`Analysis failed: ${e}`);
    }
    setAnalyzing(false);
  };

  const handleToggleRecorder = async () => {
    const next = !(recorder?.running ?? false);
    updateSettings({ recorder_enabled: next });
    try {
      const s = await invoke<RecorderStatus>(next ? "recorder_start" : "recorder_stop");
      setRecorder(s);
    } catch { /* status poll will catch up */ }
  };

  const handleCopyMarkdown = async () => {
    try {
      const md = await invoke<string>("journal_export_markdown", {
        fromDay: day,
        toDay: day,
      });
      await navigator.clipboard.writeText(md);
      setNotice("Copied day as markdown.");
      setTimeout(() => setNotice(null), 2500);
    } catch (e) {
      setNotice(`Export failed: ${e}`);
    }
  };

  return (
    <div className="h-full bg-background-primary text-zinc-100 overflow-y-auto">
      <div className="max-w-lg mx-auto p-6 space-y-4">
        {/* Header */}
        <div className="flex items-center gap-3">
          <button
            onClick={() => {
              invoke("set_window_height", { height: isExpanded ? 600 : 54 }).catch(() => {});
              setCurrentPage("chat");
            }}
            aria-label="Back to chat"
            className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400"
          >
            <ArrowLeft size={18} />
          </button>
          <h1 className="text-lg font-heading font-semibold flex items-center gap-2">
            <CalendarDays size={18} className="text-accent" /> Journal
          </h1>
        </div>

        {/* Date navigation */}
        <div className="flex items-center justify-between">
          <button
            onClick={() => setDay(shiftDay(day, -1))}
            aria-label="Previous day"
            className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400"
          >
            <ChevronLeft size={16} />
          </button>
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">{isToday ? `Today · ${day}` : day}</span>
            {!isToday && (
              <button
                onClick={() => setDay(today)}
                className="text-xs px-2 py-0.5 rounded-md bg-zinc-800 text-zinc-400 hover:bg-zinc-700"
              >
                Today
              </button>
            )}
          </div>
          <button
            onClick={() => setDay(shiftDay(day, 1))}
            disabled={isToday}
            aria-label="Next day"
            className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 disabled:opacity-30"
          >
            <ChevronRight size={16} />
          </button>
        </div>

        {/* Tabs */}
        <div className="flex gap-1 border-b border-zinc-800/60">
          {([
            ["timeline", "Timeline"],
            ["standup", "Standup"],
            ["week", "Week"],
          ] as const).map(([id, label]) => (
            <button
              key={id}
              onClick={() => setTab(id)}
              className={`px-3 py-1.5 text-xs rounded-t-md transition-colors ${
                tab === id
                  ? "bg-zinc-800/80 text-zinc-200 border-b-2 border-accent"
                  : "text-zinc-500 hover:text-zinc-300"
              }`}
            >
              {label}
            </button>
          ))}
        </div>

        {/* Controls */}
        {tab === "timeline" && (
        <div className="flex flex-wrap items-center gap-2">
          <button
            onClick={handleAnalyzeNow}
            disabled={analyzing}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-accent/20 text-accent hover:bg-accent/30 disabled:opacity-40"
          >
            {analyzing ? (
              <Loader2 size={13} className="animate-spin" />
            ) : (
              <RefreshCw size={13} />
            )}
            Analyze now
          </button>
          <button
            onClick={handleToggleRecorder}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-zinc-800 text-zinc-300 hover:bg-zinc-700"
          >
            {recorder?.running ? <VideoOff size={13} /> : <Video size={13} />}
            {recorder?.running ? "Stop recording" : "Start recording"}
          </button>
          <button
            onClick={handleCopyMarkdown}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-zinc-800 text-zinc-300 hover:bg-zinc-700"
          >
            <Copy size={13} /> Copy as markdown
          </button>
          {cards !== null && cards.length > 0 && (
            <button
              onClick={() => {
                setJournalChatContext({ day, text: cardsToContextText(cards) });
                setCurrentPage("chat");
              }}
              title="Attach this day's timeline to chat and ask about it"
              className="flex items-center gap-1.5 px-3 py-1.5 text-xs rounded-md bg-zinc-800 text-zinc-300 hover:bg-zinc-700"
            >
              <MessageSquare size={13} /> Ask about this day
            </button>
          )}
        </div>
        )}

        {notice && <p className="text-xs text-accent">{notice}</p>}

        {tab === "standup" ? (
          <StandupTab day={day} onNotice={setNotice} />
        ) : tab === "week" ? (
          <WeekTab endDay={day} />
        ) : cards === null ? (
          <p className="text-xs text-zinc-500 flex items-center gap-2 pt-4">
            <Loader2 size={14} className="animate-spin" /> Loading…
          </p>
        ) : cards.length === 0 ? (
          <div className="pt-6 text-center space-y-3">
            {!settings.recorder_enabled && !(recorder?.running ?? false) ? (
              <>
                <p className="text-sm text-zinc-400">The work journal recorder is off.</p>
                <p className="text-xs text-zinc-600">
                  Turn it on and WorkBuddy will quietly capture your screen every few
                  seconds and write this timeline for you.
                </p>
                <button
                  onClick={handleToggleRecorder}
                  className="px-3 py-1.5 text-xs rounded-md bg-accent/20 text-accent hover:bg-accent/30"
                >
                  Start recording
                </button>
              </>
            ) : analyzing ? (
              <p className="text-sm text-zinc-400 flex items-center justify-center gap-2">
                <Loader2 size={14} className="animate-spin" /> Analyzing your day…
              </p>
            ) : (
              <>
                <p className="text-sm text-zinc-400">Nothing analyzed for this day yet.</p>
                {isToday && (
                  <p className="text-xs text-zinc-600">
                    Analysis runs automatically every 10 minutes while recording
                    {dayShots.length > 0 ? ` — ${dayShots.length} frames captured so far` : ""}.
                  </p>
                )}
                <button
                  onClick={handleAnalyzeNow}
                  className="px-3 py-1.5 text-xs rounded-md bg-accent/20 text-accent hover:bg-accent/30"
                >
                  Analyze now
                </button>
              </>
            )}
          </div>
        ) : (
          <div className="space-y-2">
            {cards.map((c) => {
              const expanded = expandedId === c.id;
              return (
                <div
                  key={c.id}
                  className="rounded-lg border border-zinc-800 bg-zinc-900/50 px-3 py-2"
                >
                  <button
                    onClick={() => setExpandedId(expanded ? null : c.id)}
                    className="w-full text-left"
                    aria-expanded={expanded}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-[11px] text-zinc-500 tabular-nums shrink-0">
                        {timeRange(c.start_ts, c.end_ts)} ·{" "}
                        {durationMinutes(c.start_ts, c.end_ts)}m
                      </span>
                      <span
                        className={`text-[10px] px-2 py-0.5 rounded-full shrink-0 ${categoryChipClasses(c.category)}`}
                      >
                        {c.category}
                      </span>
                    </div>
                    <div className="flex items-start justify-between gap-2 mt-1">
                      <div>
                        <h3 className="text-sm font-medium text-zinc-200">{c.title}</h3>
                        {c.summary && (
                          <p className="text-xs text-zinc-500 mt-0.5">{c.summary}</p>
                        )}
                      </div>
                      <span className="text-zinc-600 mt-0.5 shrink-0">
                        {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
                      </span>
                    </div>
                  </button>
                  {expanded && <CardDetail card={c} dayShots={dayShots} />}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
