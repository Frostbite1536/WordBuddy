import { describe, it, expect } from "vitest";
import {
  localDayString,
  shiftDay,
  hhmm,
  timeRange,
  durationMinutes,
  categoryChipClasses,
  cardMetadata,
  cardsToContextText,
  standupToMarkdown,
  type TimelineCard,
} from "../src/lib/journal";

describe("localDayString / shiftDay", () => {
  it("formats local calendar dates", () => {
    expect(localDayString(new Date(2026, 6, 3))).toBe("2026-07-03");
    expect(localDayString(new Date(2026, 0, 9))).toBe("2026-01-09");
  });

  it("shifts across month and year boundaries", () => {
    expect(shiftDay("2026-07-03", -1)).toBe("2026-07-02");
    expect(shiftDay("2026-07-31", 1)).toBe("2026-08-01");
    expect(shiftDay("2026-01-01", -1)).toBe("2025-12-31");
    expect(shiftDay("2026-07-03", 0)).toBe("2026-07-03");
  });
});

describe("time formatting", () => {
  // Build timestamps from local time so the tests pass in any timezone.
  const ts = (h: number, m: number) => new Date(2026, 6, 3, h, m).getTime() / 1000;

  it("hhmm renders local 24h time", () => {
    expect(hhmm(ts(14, 5))).toBe("14:05");
    expect(hhmm(ts(9, 0))).toBe("09:00");
  });

  it("timeRange joins with an en dash", () => {
    expect(timeRange(ts(14, 0), ts(14, 35))).toBe("14:00 – 14:35");
  });

  it("durationMinutes floors and never goes negative", () => {
    expect(durationMinutes(ts(14, 0), ts(14, 35))).toBe(35);
    expect(durationMinutes(ts(14, 35), ts(14, 0))).toBe(0);
  });
});

describe("categoryChipClasses", () => {
  it("gives each known category a distinct style", () => {
    const cats = ["engineering", "design", "communication", "research", "admin", "distraction"];
    const styles = new Set(cats.map(categoryChipClasses));
    expect(styles.size).toBe(cats.length);
  });

  it("falls back for unknown categories", () => {
    expect(categoryChipClasses("yak-shaving")).toBe(categoryChipClasses("other"));
  });
});

describe("cardsToContextText", () => {
  const ts = (h: number, m: number) => new Date(2026, 6, 3, h, m).getTime() / 1000;

  it("handles the empty day", () => {
    expect(cardsToContextText([])).toBe("(no activity recorded)");
  });

  it("renders time range, title, category and summaries", () => {
    const card: TimelineCard = {
      id: 1,
      batch_id: 1,
      start_ts: ts(14, 0),
      end_ts: ts(14, 30),
      day: "2026-07-03",
      title: "Built the analyzer",
      summary: "Wrote batch assembly.",
      category: "engineering",
      subcategory: "",
      detailed_summary: "Assembler, sampler, and validation with tests.",
      metadata_json: null,
    };
    const text = cardsToContextText([card]);
    expect(text).toContain("[14:00 – 14:30] Built the analyzer (engineering)");
    expect(text).toContain("Wrote batch assembly.");
    expect(text).toContain("Assembler, sampler");
  });
});

describe("standupToMarkdown", () => {
  it("renders sections and skips empty ones", () => {
    const md = standupToMarkdown("2026-07-03", {
      highlights: ["Shipped the analyzer"],
      tasks: ["Timeline UI"],
      blockers: [],
      next: ["Weekly view"],
    });
    expect(md).toContain("## Standup · 2026-07-03");
    expect(md).toContain("**Highlights**\n- Shipped the analyzer");
    expect(md).toContain("**Today**\n- Timeline UI");
    expect(md).not.toContain("**Blockers**");
    expect(md).toContain("**Next**\n- Weekly view");
  });
});

describe("cardMetadata", () => {
  const base: TimelineCard = {
    id: 1,
    batch_id: 1,
    start_ts: 0,
    end_ts: 0,
    day: "2026-07-03",
    title: "t",
    summary: "",
    category: "engineering",
    subcategory: "",
    detailed_summary: "",
    metadata_json: null,
  };

  it("parses distractions and app_sites", () => {
    const card = {
      ...base,
      metadata_json: JSON.stringify({
        distractions: [{ start: "14:10", end: "14:13", title: "X", summary: "scrolled" }],
        app_sites: { primary: "github.com", secondary: "x.com" },
      }),
    };
    const meta = cardMetadata(card);
    expect(meta.distractions).toHaveLength(1);
    expect(meta.appSites.primary).toBe("github.com");
  });

  it("survives null, garbage, and wrong shapes", () => {
    expect(cardMetadata(base)).toEqual({ distractions: [], appSites: {} });
    expect(cardMetadata({ ...base, metadata_json: "not json" })).toEqual({
      distractions: [],
      appSites: {},
    });
    expect(
      cardMetadata({ ...base, metadata_json: JSON.stringify({ distractions: "x", app_sites: [] }) })
    ).toEqual({ distractions: [], appSites: {} });
  });
});
