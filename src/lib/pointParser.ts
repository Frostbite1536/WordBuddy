export interface PointTarget {
  x: number;
  y: number;
  label: string;
  screen: number;
}

const POINT_TAG_RE = /\[POINT:(\d+(?:\.\d+)?),(\d+(?:\.\d+)?):([^\]]+)\]/g;

// Split `text` into alternating segments tagged "code" or "prose".
// Code covers fenced blocks (``` … ```) and inline backticks (` … `).
// We only want to extract / strip POINT tags from prose segments —
// inside a code block the model is illustrating the literal syntax
// to the student and stripping it would corrupt the example.
function segmentByCode(text: string): Array<{ kind: "code" | "prose"; value: string }> {
  const segments: Array<{ kind: "code" | "prose"; value: string }> = [];
  let i = 0;
  while (i < text.length) {
    const fenceIdx = text.indexOf("```", i);
    const tickIdx = text.indexOf("`", i);
    // Pick the nearest opener; -1 means "not found", treat as Infinity.
    const fence = fenceIdx === -1 ? Infinity : fenceIdx;
    const tick = tickIdx === -1 ? Infinity : tickIdx;
    const next = Math.min(fence, tick);
    if (next === Infinity) {
      segments.push({ kind: "prose", value: text.slice(i) });
      break;
    }
    if (next > i) {
      segments.push({ kind: "prose", value: text.slice(i, next) });
    }
    if (next === fence) {
      // Fenced block: include the fences in the code segment so a
      // trailing reconstruction round-trips losslessly.
      const close = text.indexOf("```", next + 3);
      if (close === -1) {
        // Unterminated fence — treat the rest as code (matches how
        // Markdown renderers behave on a truncated stream).
        segments.push({ kind: "code", value: text.slice(next) });
        break;
      }
      segments.push({ kind: "code", value: text.slice(next, close + 3) });
      i = close + 3;
    } else {
      // Inline backtick run. Markdown lets you use multiple backticks
      // as the delimiter; mirror that — count opening backticks and
      // look for the matching close run.
      let openLen = 0;
      while (text[next + openLen] === "`") openLen += 1;
      const opener = "`".repeat(openLen);
      const close = text.indexOf(opener, next + openLen);
      if (close === -1) {
        segments.push({ kind: "code", value: text.slice(next) });
        break;
      }
      segments.push({ kind: "code", value: text.slice(next, close + openLen) });
      i = close + openLen;
    }
  }
  return segments;
}

/**
 * Parse [POINT:x,y:label:screenN] tags from response text.
 * TypeScript port of pointer.rs parse_point_tags(). Code blocks
 * (fenced or inline backticks) are excluded so a literal POINT tag
 * inside a code example is preserved verbatim.
 */
export function parsePointTags(text: string): {
  cleanText: string;
  points: PointTarget[];
} {
  const points: PointTarget[] = [];
  const segments = segmentByCode(text);

  const cleanedParts: string[] = [];
  for (const seg of segments) {
    if (seg.kind === "code") {
      cleanedParts.push(seg.value);
      continue;
    }
    POINT_TAG_RE.lastIndex = 0;
    let match;
    while ((match = POINT_TAG_RE.exec(seg.value)) !== null) {
      const x = parseFloat(match[1]);
      const y = parseFloat(match[2]);
      const rest = match[3];

      const parts = rest.split(":");
      let label: string;
      let screen = 0;

      if (parts.length >= 2) {
        const last = parts[parts.length - 1];
        // Accept "screen0", "screen 1", or bare single digit (0-9).
        // Multi-digit bare numbers stay in the label so "Step:42"
        // isn't misread as screen 42.
        const screenMatch = last.match(/^screen\s*(\d+)$/) || last.match(/^(\d)$/);
        if (screenMatch) {
          screen = parseInt(screenMatch[1], 10);
          label = parts.slice(0, -1).join(":");
        } else {
          label = rest;
        }
      } else {
        label = rest;
      }

      points.push({ x, y, label, screen });
    }
    cleanedParts.push(
      seg.value.replace(/\[POINT:\d+(?:\.\d+)?,\d+(?:\.\d+)?:[^\]]+\]/g, ""),
    );
  }

  return { cleanText: cleanedParts.join(""), points };
}
