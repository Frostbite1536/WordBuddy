/**
 * Accumulates streamed text chunks and emits complete sentences.
 * Used for streaming TTS — speak sentences as they arrive instead
 * of waiting for the full response.
 */

// Abbreviations that end with a period but aren't sentence boundaries
const ABBREVIATIONS = new Set([
  "dr.", "mr.", "mrs.", "ms.", "prof.", "sr.", "jr.",
  "vs.", "e.g.", "i.e.", "etc.", "approx.", "dept.",
  "est.", "inc.", "ltd.", "no.", "vol.", "fig.",
  "a.m.", "p.m.", "p.s.", "cf.", "ca.", "viz.",
]);

export class SentenceBuffer {
  private buffer = "";
  private onSentence: (sentence: string) => void;
  private minLength: number;

  constructor(onSentence: (sentence: string) => void, minLength = 10) {
    this.onSentence = onSentence;
    this.minLength = minLength;
  }

  /** Add a text chunk from the SSE stream */
  push(chunk: string): void {
    this.buffer += chunk;
    this.extract();
  }

  /** Flush any remaining text as a final sentence */
  flush(): void {
    const remaining = this.buffer.trim();
    if (remaining.length > 0) {
      this.onSentence(remaining);
    }
    this.buffer = "";
  }

  /** Reset the buffer without emitting */
  reset(): void {
    this.buffer = "";
  }

  private extract(): void {
    // Scan for sentence boundaries
    let i = 0;
    while (i < this.buffer.length) {
      const char = this.buffer[i];

      // Check for sentence-ending punctuation followed by whitespace
      if ((char === "." || char === "!" || char === "?") && i + 1 < this.buffer.length) {
        const nextChar = this.buffer[i + 1];
        if (nextChar === " " || nextChar === "\n" || nextChar === "\r") {
          // Check if this is an abbreviation
          if (char === "." && this.isAbbreviation(i)) {
            i++;
            continue;
          }
          // Check if this is a decimal number (e.g., "3.14")
          if (char === "." && this.isDecimal(i)) {
            i++;
            continue;
          }

          // Found a sentence boundary
          const sentence = this.buffer.slice(0, i + 1).trim();

          if (sentence.length >= this.minLength) {
            // Long enough — emit and continue from remaining buffer
            this.buffer = this.buffer.slice(i + 1);
            this.onSentence(sentence);
            i = 0; // Restart scanning from beginning of remaining buffer
            continue;
          }

          // Too short — DON'T slice the buffer. Skip past this punctuation
          // mark so it merges with the next sentence. The old code prepended
          // the short sentence back, which re-created the same boundary and
          // caused an infinite loop (e.g. "Sure. " → extract "Sure." → prepend
          // "Sure. " → find same period → infinite loop).
          i++;
          continue;
        }
      }

      // Also split on double newlines (paragraph boundaries)
      if (char === "\n" && i + 1 < this.buffer.length && this.buffer[i + 1] === "\n") {
        const sentence = this.buffer.slice(0, i).trim();

        if (sentence.length >= this.minLength) {
          this.buffer = this.buffer.slice(i + 2);
          this.onSentence(sentence);
          i = 0;
          continue;
        }

        // Too short — skip past the double newline to merge with next paragraph
        i += 2;
        continue;
      }

      i++;
    }
  }

  private isAbbreviation(dotIndex: number): boolean {
    // Look backwards to find the word containing this dot. Treat any
    // Unicode whitespace as a word boundary, not just ASCII space and
    // newline — otherwise "全角\u3000Dr." would be read as one word
    // and miss the abbreviation set.
    let start = dotIndex;
    while (start > 0 && !WHITESPACE_RE.test(this.buffer[start - 1])) {
      start--;
    }
    const word = this.buffer.slice(start, dotIndex + 1).toLowerCase();
    return ABBREVIATIONS.has(word);
  }

  private isDecimal(dotIndex: number): boolean {
    // A dot is decimal if preceded and immediately followed by ANY
    // Unicode digit category — \p{Nd} catches Arabic-Indic, Devanagari,
    // and other locale digit forms in addition to ASCII 0-9.
    // Only checks the character directly after the dot — no whitespace
    // skipping, because "3. Next" is a sentence boundary, not a decimal.
    if (dotIndex === 0 || dotIndex + 1 >= this.buffer.length) return false;
    const before = this.buffer[dotIndex - 1];
    const after = this.buffer[dotIndex + 1];
    return DIGIT_RE.test(before) && DIGIT_RE.test(after);
  }
}

// Compiled once — matching every dot through `new RegExp` per call
// would dominate the per-token cost on long streams.
const DIGIT_RE = /\p{Nd}/u;
const WHITESPACE_RE = /\s/u;
