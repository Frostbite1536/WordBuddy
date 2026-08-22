// Pure helpers for the journal timeline UI. Kept out of the component so
// vitest can cover the date math and formatting without a DOM.

export interface TimelineCard {
  id: number;
  batch_id: number;
  start_ts: number;
  end_ts: number;
  day: string;
  title: string;
  summary: string;
  category: string;
  subcategory: string;
  detailed_summary: string;
  metadata_json: string | null;
}

/// Local YYYY-MM-DD for a Date (NOT toISOString — that's UTC and shifts
/// the day for anyone west of Greenwich).
export function localDayString(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/// The day string shifted by `delta` calendar days.
export function shiftDay(day: string, delta: number): string {
  const [y, m, d] = day.split("-").map((s) => parseInt(s, 10));
  const date = new Date(y, (m || 1) - 1, d || 1);
  date.setDate(date.getDate() + delta);
  return localDayString(date);
}

/// "14:05" local for a unix-seconds timestamp.
export function hhmm(ts: number): string {
  const d = new Date(ts * 1000);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/// "14:05 – 14:35" for a card.
export function timeRange(startTs: number, endTs: number): string {
  return `${hhmm(startTs)} – ${hhmm(endTs)}`;
}

/// Minutes between two unix-second timestamps, floored, never negative.
export function durationMinutes(startTs: number, endTs: number): number {
  return Math.max(0, Math.floor((endTs - startTs) / 60));
}

/// Tailwind classes per category — chip background/text. Categories come
/// from the fixed starter set in journal/prompts.rs; unknown → zinc.
export function categoryChipClasses(category: string): string {
  switch (category) {
    case "engineering":
      return "bg-sky-500/15 text-sky-400";
    case "design":
      return "bg-purple-500/15 text-purple-400";
    case "communication":
      return "bg-emerald-500/15 text-emerald-400";
    case "research":
      return "bg-amber-500/15 text-amber-400";
    case "admin":
      return "bg-zinc-500/20 text-zinc-300";
    case "distraction":
      return "bg-red-500/15 text-red-400";
    default:
      return "bg-zinc-600/20 text-zinc-400";
  }
}

/// Render a day's cards as plain text for chat context ("Ask about this
/// day"). Detailed summaries included — the prompt builder caps length.
export function cardsToContextText(cards: TimelineCard[]): string {
  if (cards.length === 0) return "(no activity recorded)";
  return cards
    .map((c) => {
      const lines = [
        `[${timeRange(c.start_ts, c.end_ts)}] ${c.title} (${c.category})`,
      ];
      if (c.summary) lines.push(`  ${c.summary}`);
      if (c.detailed_summary && c.detailed_summary !== c.summary) {
        lines.push(`  ${c.detailed_summary}`);
      }
      return lines.join("\n");
    })
    .join("\n");
}

export interface StandupPayload {
  highlights: string[];
  tasks: string[];
  blockers: string[];
  next: string[];
}

/// Standup payload → copy-ready markdown.
export function standupToMarkdown(day: string, s: StandupPayload): string {
  const section = (title: string, items: string[]) =>
    items.length > 0 ? `**${title}**\n${items.map((i) => `- ${i}`).join("\n")}` : "";
  const sections = [
    section("Highlights", s.highlights),
    section("Today", s.tasks),
    section("Blockers", s.blockers.length > 0 ? s.blockers : []),
    section("Next", s.next),
  ].filter(Boolean);
  return [`## Standup · ${day}`, ...sections].join("\n\n");
}

/// Parse a card's metadata_json defensively (LLM-derived content).
export function cardMetadata(card: TimelineCard): {
  distractions: Array<{ start?: string; end?: string; title?: string; summary?: string }>;
  appSites: { primary?: string; secondary?: string };
} {
  const fallback = { distractions: [], appSites: {} };
  if (!card.metadata_json) return fallback;
  try {
    const parsed = JSON.parse(card.metadata_json);
    const distractions = Array.isArray(parsed?.distractions) ? parsed.distractions : [];
    const appSites =
      parsed?.app_sites && typeof parsed.app_sites === "object" && !Array.isArray(parsed.app_sites)
        ? parsed.app_sites
        : {};
    return { distractions, appSites };
  } catch {
    return fallback;
  }
}
