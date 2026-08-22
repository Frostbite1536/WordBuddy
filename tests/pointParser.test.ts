// Tests for the [POINT:x,y:label] tag parser. Specifically covers
// the M2 audit fix that made segmentation code-fence-aware so a
// literal POINT example inside a fenced code block isn't stripped
// from the rendered output AND isn't dispatched as a real overlay.

import { describe, it, expect } from "vitest";
import { parsePointTags } from "../src/lib/pointParser";

describe("parsePointTags", () => {
  describe("plain prose", () => {
    it("extracts a single POINT tag", () => {
      const r = parsePointTags("Look at [POINT:120,240:Save] now.");
      expect(r.points).toHaveLength(1);
      expect(r.points[0]).toEqual({ x: 120, y: 240, label: "Save", screen: 0 });
      expect(r.cleanText).toBe("Look at  now.");
    });

    it("extracts multiple POINT tags in order", () => {
      const r = parsePointTags("[POINT:10,20:A] and [POINT:30,40:B]");
      expect(r.points.map((p) => p.label)).toEqual(["A", "B"]);
    });

    it("handles fractional coordinates", () => {
      const r = parsePointTags("[POINT:100.5,200.25:X]");
      expect(r.points[0]).toMatchObject({ x: 100.5, y: 200.25 });
    });

    it("parses a trailing screen index", () => {
      const r = parsePointTags("[POINT:10,20:Label:screen2]");
      expect(r.points[0]).toEqual({ x: 10, y: 20, label: "Label", screen: 2 });
    });

    it("parses a bare single-digit screen index", () => {
      const r = parsePointTags("[POINT:10,20:Label:1]");
      expect(r.points[0].screen).toBe(1);
      expect(r.points[0].label).toBe("Label");
    });

    it("treats multi-digit bare numbers as label, not screen", () => {
      const r = parsePointTags("[POINT:10,20:Step:42]");
      expect(r.points[0].screen).toBe(0);
      expect(r.points[0].label).toBe("Step:42");
    });
  });

  describe("fenced code blocks", () => {
    it("preserves a POINT tag inside ``` ... ```", () => {
      const md = "Like this:\n```\n[POINT:100,200:Save]\n```\nMakes sense?";
      const r = parsePointTags(md);
      expect(r.points).toHaveLength(0);
      // The fenced block survives intact.
      expect(r.cleanText).toContain("[POINT:100,200:Save]");
    });

    it("only strips POINT tags outside fenced blocks", () => {
      const md =
        "Click [POINT:50,60:Buy] then later you can write something like:\n```text\n[POINT:1,2:Example]\n```";
      const r = parsePointTags(md);
      expect(r.points).toHaveLength(1);
      expect(r.points[0].label).toBe("Buy");
      // Inside-fence tag preserved.
      expect(r.cleanText).toContain("[POINT:1,2:Example]");
      // Outside-fence tag stripped.
      expect(r.cleanText).not.toContain("[POINT:50,60:Buy]");
    });

    it("handles fences with a language tag", () => {
      const md = "```js\nconst x = '[POINT:1,2:Y]';\n```";
      const r = parsePointTags(md);
      expect(r.points).toHaveLength(0);
      expect(r.cleanText).toContain("[POINT:1,2:Y]");
    });

    it("handles unterminated fence by treating the rest as code", () => {
      // Streaming truncation case — chat_stream_chunk may deliver a
      // partial response with no closing fence.
      const md = "Here:\n```\n[POINT:1,2:Y]";
      const r = parsePointTags(md);
      expect(r.points).toHaveLength(0);
      expect(r.cleanText).toContain("[POINT:1,2:Y]");
    });
  });

  describe("inline backticks", () => {
    it("preserves a POINT tag in an inline `code` span", () => {
      const r = parsePointTags("Try `[POINT:1,2:Demo]` for the syntax.");
      expect(r.points).toHaveLength(0);
      expect(r.cleanText).toContain("[POINT:1,2:Demo]");
    });

    it("supports multi-backtick spans", () => {
      const r = parsePointTags("Use ``[POINT:1,2:Foo]`` here.");
      expect(r.points).toHaveLength(0);
      expect(r.cleanText).toContain("[POINT:1,2:Foo]");
    });

    it("unterminated inline backtick treats remainder as code", () => {
      const r = parsePointTags("Open `[POINT:1,2:Foo]");
      expect(r.points).toHaveLength(0);
    });
  });

  describe("mixed content", () => {
    it("handles fenced + inline + prose POINTs in one message", () => {
      const md =
        "Click [POINT:1,1:A].\n```\n[POINT:2,2:CODE]\n```\nOr inline `[POINT:3,3:INLINE]`. Then [POINT:4,4:B].";
      const r = parsePointTags(md);
      expect(r.points.map((p) => p.label)).toEqual(["A", "B"]);
      expect(r.cleanText).toContain("[POINT:2,2:CODE]");
      expect(r.cleanText).toContain("[POINT:3,3:INLINE]");
      expect(r.cleanText).not.toContain("[POINT:1,1:A]");
      expect(r.cleanText).not.toContain("[POINT:4,4:B]");
    });
  });

  describe("no tags / empty input", () => {
    it("returns the original text and empty points array", () => {
      const r = parsePointTags("Nothing to see here.");
      expect(r.points).toEqual([]);
      expect(r.cleanText).toBe("Nothing to see here.");
    });

    it("handles empty string", () => {
      const r = parsePointTags("");
      expect(r.points).toEqual([]);
      expect(r.cleanText).toBe("");
    });
  });
});
