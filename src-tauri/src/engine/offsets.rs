//! UTF-16 offset conversion (INV-OFFSET-001).
//!
//! Harper's `Span<char>` indices count **Unicode scalar values** (chars).
//! JavaScript strings — and therefore the extension, the widget, and every
//! `TextIssue` that crosses the IPC boundary — index **UTF-16 code units**.
//! The two disagree exactly when the text contains characters outside the
//! Basic Multilingual Plane (emoji, some CJK extensions): one astral char is
//! 1 char but 2 UTF-16 units.
//!
//! Every conversion between the two worlds goes through this module. A wrong
//! conversion fails the test vectors below loudly — including the astral and
//! combining sequences the CONTRACTS registry demands.

/// Prefix-sum index mapping char positions → UTF-16 code-unit offsets.
///
/// `map[i]` is the UTF-16 offset of char `i`; `map[chars.len()]` is the
/// total UTF-16 length.
pub struct Utf16Index {
    map: Vec<usize>,
}

impl Utf16Index {
    /// Build the index for a char slice. O(n).
    pub fn build(chars: &[char]) -> Self {
        let mut map = Vec::with_capacity(chars.len() + 1);
        let mut acc = 0usize;
        map.push(0);
        for &c in chars {
            acc += c.len_utf16();
            map.push(acc);
        }
        Self { map }
    }

    /// UTF-16 code-unit offset for a char index. Panics if out of bounds
    /// (an out-of-range harper span is a bug we want to see, not pad over).
    pub fn to_utf16(&self, char_index: usize) -> usize {
        self.map
            .get(char_index)
            .copied()
            .unwrap_or_else(|| panic!("char index {char_index} out of range"))
    }

    /// Total UTF-16 code units in the source text.
    pub fn utf16_len(&self) -> usize {
        *self.map.last().expect("index always has at least one entry")
    }
}

/// Slice `text` by UTF-16 code-unit offsets with JavaScript string
/// semantics: never splits a surrogate pair (a range ending between the
/// high and low half of a pair excludes the whole character) and never
/// emits a lone surrogate half.
///
/// This mirrors what `text.substring(start, end)` yields in the frontend,
/// which is what INV-CHECK-002's `original == text[start..end]` means.
pub fn slice_utf16(text: &str, start: usize, end: usize) -> String {
    let mut out = String::new();
    let mut unit = 0usize;
    for c in text.chars() {
        let char_start = unit;
        let char_end = unit + c.len_utf16();
        unit = char_end;
        // Include the char only if it lies fully inside [start, end).
        if char_start >= start && char_end <= end {
            out.push(c);
        }
        if unit >= end {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_offsets_are_identity() {
        let text = "hello world";
        let chars: Vec<char> = text.chars().collect();
        let idx = Utf16Index::build(&chars);
        assert_eq!(idx.to_utf16(0), 0);
        assert_eq!(idx.to_utf16(5), 5);
        assert_eq!(idx.to_utf16(11), 11);
        assert_eq!(idx.utf16_len(), 11);
        assert_eq!(slice_utf16(text, 0, 5), "hello");
    }

    #[test]
    fn emoji_counts_two_units() {
        // 🚀 is U+1F680: one char, one Unicode scalar, TWO UTF-16 units.
        let text = "a🚀b";
        let chars: Vec<char> = text.chars().collect();
        let idx = Utf16Index::build(&chars);
        assert_eq!(chars.len(), 3);
        assert_eq!(idx.to_utf16(0), 0); // 'a'
        assert_eq!(idx.to_utf16(1), 1); // '🚀' starts at unit 1
        assert_eq!(idx.to_utf16(2), 3); // 'b' starts at unit 3
        assert_eq!(idx.utf16_len(), 4);
        assert_eq!(slice_utf16(text, 1, 3), "🚀");
        assert_eq!(slice_utf16(text, 0, 4), text);
    }

    #[test]
    fn combining_sequence_counts_its_own_units() {
        // 'e' + U+0301 (combining acute): two chars, two UTF-16 units,
        // but ONE grapheme. Harper spans chars, so both count separately.
        let text = "cafe\u{0301} naive";
        let chars: Vec<char> = text.chars().collect();
        let idx = Utf16Index::build(&chars);
        assert_eq!(chars.len(), 11);
        assert_eq!(idx.utf16_len(), 11); // all BMP: units == chars
        assert_eq!(idx.to_utf16(4), 4); // combining mark after 'e'
        // Slicing through the combining sequence keeps it intact.
        assert_eq!(slice_utf16(text, 3, 5), "e\u{0301}");
    }

    #[test]
    fn cjk_bmp_counts_one_unit() {
        let text = "你好世界";
        let chars: Vec<char> = text.chars().collect();
        let idx = Utf16Index::build(&chars);
        assert_eq!(idx.utf16_len(), 4);
        assert_eq!(slice_utf16(text, 1, 3), "好世");
    }

    #[test]
    fn verifier_hand_check_vector_emoji_before_span() {
        // The exact class PLAN-01's verification gate names: an astral
        // character BEFORE the span, so a char-counted span offset would
        // drift by one. "🚀 teh recieve" — the word "recieve" starts at
        // char index 6 but UTF-16 unit 7.
        let text = "\u{1F680} teh recieve";
        let chars: Vec<char> = text.chars().collect();
        let idx = Utf16Index::build(&chars);
        assert_eq!(idx.to_utf16(6), 7); // 'r' of "recieve"
        // Original text for the UTF-16 span [7, 14) must be "recieve".
        assert_eq!(slice_utf16(text, 7, 14), "recieve");
        // And the char-index span [6, 13) covers the same word.
        let span_chars: String = chars[6..13].iter().collect();
        assert_eq!(span_chars, "recieve");
    }

    #[test]
    fn slice_never_emits_lone_surrogate() {
        // Range [1, 2) lands between the high and low halves of 🚀.
        let out = slice_utf16("\u{1F680}x", 1, 2);
        assert_eq!(out, "", "must not emit a lone surrogate half");
    }
}
