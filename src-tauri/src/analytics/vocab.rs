//! Vocabulary tokenizer + common-word list (PLAN-05 task 2).
//!
//! The list is the first ~9.9k entries of `google-10000-english`
//! (no-swears variant), MIT license, checked into the repo at
//! `src/analytics/top-english.txt` with source noted here. "Rare" is a
//! heuristic — anything outside this list — and the UI labels it as
//! such (PLAN-05 risk note).

use std::collections::BTreeSet;
use std::sync::LazyLock;

static COMMON: LazyLock<BTreeSet<String>> = LazyLock::new(|| {
    let raw = include_str!("top-english.txt");
    raw.lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .collect()
});

pub fn is_common(word: &str) -> bool {
    COMMON.contains(word)
}

/// The loaded set (for tests and callers that want bulk membership).
pub fn common_set() -> &'static BTreeSet<String> {
    &COMMON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_loads_with_expected_size() {
        assert!(COMMON.len() > 9_000, "common list too small: {}", COMMON.len());
        assert!(COMMON.contains("the"));
        assert!(COMMON.contains("word"));
    }

    #[test]
    fn rare_words_absent_from_list() {
        assert!(!is_common("quixotic"));
    }
}
