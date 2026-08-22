import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, RefreshCw, FileDown, Flame, Gauge, PenLine, BookOpen } from "lucide-react";
import { useApp } from "../contexts/app.context";

interface DayStat {
  day: string;
  words: number;
  checks: number;
  accuracy: number;
  vocab_unique: number;
  vocab_rare_pct: number;
  top_errors: [string, number][];
}

interface WeekSummary {
  weekStart: string;
  days: DayStat[];
  words: number;
  checks: number;
  wordsDeltaVsPrior: number;
  accuracy: number;
  streak: number;
  vocabUnique: number;
  vocabRarePct: number;
  topErrors: [string, number][];
  tone: unknown;
}

function humanizeRule(rule: string): string {
  const tail = rule.split(":").pop() ?? rule;
  return tail.replace(/[_-]/g, " ");
}

export default function Stats() {
  const { setCurrentPage } = useApp();
  const [weekOffset, setWeekOffset] = useState(0);
  const [summary, setSummary] = useState<WeekSummary | null>(null);
  const [reportMd, setReportMd] = useState<string | null>(null);
  const [exportPath, setExportPath] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const todayOfOffset = (): string => {
    const d = new Date();
    d.setDate(d.getDate() + weekOffset * 7);
    return d.toISOString().slice(0, 10);
  };

  const load = () => {
    setBusy(true);
    invoke<WeekSummary>("analytics_summary", { today: todayOfOffset() })
      .then(setSummary)
      .catch(() => setSummary(null))
      .finally(() => setBusy(false));
  };

  useEffect(load, [weekOffset]);

  const refresh = async () => {
    setBusy(true);
    try {
      await invoke("analytics_aggregate_now");
    } finally {
      load();
    }
  };

  const openReport = async () => {
    if (!summary) return;
    setBusy(true);
    try {
      const md = await invoke<string>("analytics_report_markdown", {
        weekStart: summary.weekStart,
      });
      setReportMd(md);
    } finally {
      setBusy(false);
    }
  };

  const exportReport = async () => {
    if (!summary) return;
    setBusy(true);
    try {
      const path = await invoke<string>("analytics_export_report", {
        weekStart: summary.weekStart,
      });
      setExportPath(path);
    } catch (e) {
      setExportPath(`Failed: ${String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="h-full bg-background-primary text-zinc-100 overflow-y-auto">
      <div className="max-w-lg mx-auto p-6 space-y-6">
        <div className="flex items-center gap-3">
          <button
            onClick={() => setCurrentPage("chat")}
            aria-label="Back to chat"
            className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400"
          >
            <ArrowLeft size={18} />
          </button>
          <h1 className="text-lg font-heading font-semibold">Your writing stats</h1>
          <div className="ml-auto flex items-center gap-1.5">
            <button
              onClick={() => setWeekOffset((w) => w - 1)}
              aria-label="Previous week"
              className="px-2 py-1 rounded-md bg-zinc-800 text-xs hover:bg-zinc-700"
            >
              ‹
            </button>
            <button
              onClick={() => setWeekOffset((w) => Math.min(0, w + 1))}
              disabled={weekOffset >= 0}
              aria-label="Next week"
              className="px-2 py-1 rounded-md bg-zinc-800 text-xs hover:bg-zinc-700 disabled:opacity-30"
            >
              ›
            </button>
            <button
              onClick={() => void refresh()}
              title="Recompute stats"
              aria-label="Recompute stats"
              className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400"
            >
              <RefreshCw size={14} />
            </button>
          </div>
        </div>

        {!summary ? (
          <p className="text-sm text-zinc-500">
            No stats yet — write something with checking active and come back.
          </p>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-3">
              <Card icon={<PenLine size={14} />} label="Words this week" value={String(summary.words)}
                delta={`${summary.wordsDeltaVsPrior >= 0 ? "+" : ""}${summary.wordsDeltaVsPrior} vs prior`} />
              <Card icon={<Gauge size={14} />} label="Accuracy"
                value={`${(summary.accuracy * 100).toFixed(1)}%`}
                delta="1 − correctness issues ÷ words" />
              <Card icon={<Flame size={14} />} label="Streak" value={`${summary.streak} day(s)`}
                delta="≥50 words/day" />
              <Card icon={<BookOpen size={14} />} label="Vocabulary" value={String(summary.vocabUnique)}
                delta={`${summary.vocabRarePct.toFixed(0)}% uncommon (heuristic)`} />
            </div>

            <section className="space-y-2">
              <h2 className="text-sm font-semibold text-zinc-400">Top errors this week</h2>
              {summary.topErrors.length === 0 ? (
                <p className="text-xs text-zinc-600">None recorded.</p>
              ) : (
                <ul className="space-y-1">
                  {summary.topErrors.map(([rule, n]) => (
                    <li key={rule} className="flex justify-between text-xs">
                      <span className="text-zinc-300">{humanizeRule(rule)}</span>
                      <span className="text-zinc-500">{n}×</span>
                    </li>
                  ))}
                </ul>
              )}
              <p className="text-[10px] text-zinc-600">
                How this is computed: local counts of rule names from your own
                checks; nothing leaves your machine.
              </p>
            </section>

            <section className="space-y-2">
              <h2 className="text-sm font-semibold text-zinc-400">Weekly report</h2>
              <div className="flex gap-2">
                <button
                  onClick={() => void openReport()}
                  disabled={busy}
                  className="px-3 py-1.5 bg-accent/20 text-accent rounded-lg text-xs hover:bg-accent/30 disabled:opacity-40"
                >
                  View report
                </button>
                <button
                  onClick={() => void exportReport()}
                  disabled={busy}
                  className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 text-zinc-200 rounded-lg text-xs hover:bg-zinc-700 disabled:opacity-40"
                >
                  <FileDown size={12} /> Export markdown
                </button>
              </div>
              {reportMd && (
                <pre className="bg-zinc-900 border border-zinc-700 rounded-lg p-3 text-[11px] whitespace-pre-wrap overflow-x-auto max-h-64 overflow-y-auto">
                  {reportMd}
                </pre>
              )}
              {exportPath && <p className="text-[11px] text-accent break-all">{exportPath}</p>}
            </section>
          </>
        )}
      </div>
    </div>
  );
}

function Card({
  icon,
  label,
  value,
  delta,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  delta?: string;
}) {
  return (
    <div className="bg-zinc-900 border border-zinc-700/60 rounded-xl p-3 space-y-1">
      <div className="flex items-center gap-1.5 text-zinc-400 text-[11px]">
        {icon}
        {label}
      </div>
      <div className="text-xl font-heading font-semibold">{value}</div>
      {delta && <div className="text-[10px] text-zinc-500">{delta}</div>}
    </div>
  );
}
