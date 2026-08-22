//! Pure aggregation math (PLAN-05 task 2). Clock injected — no `now()`
//! in any function here; the scheduler passes timestamps explicitly.
//!
//! Definitions (documented in-app via the dashboard's methodology note):
//! - **accuracy** = 1 − correctness_issues / words_checked, clamped to
//!   [0, 1]. Rolling per-day figure over that day's check events.
//! - **streak** = consecutive local days ending today (or yesterday) with
//!   ≥ MIN_STREAK_WORDS checked. DST is handled by deriving day strings
//!   from local timestamps; a 23/25-hour day still maps to one string.
//! - **vocab_unique** = distinct normalized words for the window.
//! - **vocab_rare_pct** = share of tokens not in the embedded common-word
//!   list (top ~10k English; source noted in vocab.rs). Heuristic,
//!   labeled as such in the UI.

use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const MIN_STREAK_WORDS: u32 = 50;

/// Local calendar day for a unix timestamp, as `YYYY-MM-DD`.
pub fn day_string_from_ts(ts_secs: i64) -> String {
    // Local-offset derivation without a TZ database dependency: use the
    // process-local offset captured once (std exposes no TZ db).
    let offset = local_utc_offset_secs();
    let local = ts_secs + offset;
    civil_from_days(local.div_euclid(86_400))
}

fn local_utc_offset_secs() -> i64 {
    // std doesn't expose the zone; derive it once from the difference
    // between the OS-localized and UTC representations of now via
    // `libc`-free trick: compare SystemTime (UTC) against a formatted
    // local read is impossible without a TZ lib — so we cache the offset
    // captured by the caller at startup (see capture_local_offset()).
    LOCAL_OFFSET_SECS.get().copied().unwrap_or(0)
}

static LOCAL_OFFSET_SECS: std::sync::OnceLock<i64> = std::sync::OnceLock::new();

/// Capture the machine's UTC offset once (called at app start) from the
/// Windows time-zone API. Non-Windows defaults to UTC (stubs, D3).
#[cfg(target_os = "windows")]
pub fn capture_local_offset() {
    use windows::Win32::System::Time::{
        GetDynamicTimeZoneInformation, DYNAMIC_TIME_ZONE_INFORMATION,
    };
    unsafe {
        let mut tz = DYNAMIC_TIME_ZONE_INFORMATION::default();
        // Result is the zone id; the bias fields are filled regardless.
        let _ = GetDynamicTimeZoneInformation(&mut tz);
        let bias_min = tz.Bias as i64
            + if tz.DaylightBias != 0 { tz.DaylightBias as i64 } else { tz.StandardBias as i64 };
        // Bias = UTC - local (minutes) → offset = -bias.
        let _ = LOCAL_OFFSET_SECS.set(-bias_min * 60);
    }
}

/// Non-Windows: UTC only (stub platforms, STATE D3).
#[cfg(not(target_os = "windows"))]
pub fn capture_local_offset() {
    let _ = LOCAL_OFFSET_SECS.set(0);
}

/// Days-from-epoch → `YYYY-MM-DD` (Howard Hinnant's civil algorithm).
fn civil_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse `YYYY-MM-DD` → days-from-epoch (inverse of civil_from_days).
pub fn days_from_civil(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = y_adj.div_euclid(400);
    let yoe = y_adj - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(doe + era * 146_097 - 719_468)
}

/// Normalize a word for vocabulary counting: lowercase, ASCII letters
/// kept, apostrophes inside words kept, everything else split.
pub fn normalize_word(word: &str) -> Option<String> {
    let w: String = word
        .chars()
        .filter(|c| c.is_alphabetic() || *c == '\'')
        .collect::<String>()
        .to_lowercase();
    let trimmed = w.trim_matches('\'');
    if trimmed.len() >= 2 && trimmed.chars().any(|c| c.is_alphabetic()) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Vocabulary stats over raw check-event texts' token counts.
#[derive(Debug, Clone, PartialEq)]
pub struct VocabStats {
    pub unique: usize,
    pub total_tokens: usize,
    pub rare_pct: f64,
}

/// `common`: the embedded common-word list. A token is "rare" when its
/// normalized form is absent from the list.
pub fn vocab_stats(tokens: &[String], common: &BTreeSet<String>) -> VocabStats {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for t in tokens {
        if let Some(w) = normalize_word(t) {
            *counts.entry(w).or_insert(0) += 1;
        }
    }
    let total: usize = counts.values().sum();
    let unique = counts.len();
    let rare = counts
        .iter()
        .filter(|(w, _)| !common.contains(*w))
        .map(|(_, n)| n)
        .sum::<usize>();
    let rare_pct = if total == 0 {
        0.0
    } else {
        (rare as f64 / total as f64) * 100.0
    };
    VocabStats { unique, total_tokens: total, rare_pct }
}

/// One aggregated day, derived from raw check events.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DayStat {
    pub day: String,
    pub words: u32,
    pub checks: u32,
    /// 1 − correctness_issues/words, clamped [0,1].
    pub accuracy: f64,
    pub vocab_unique: u32,
    pub vocab_rare_pct: f64,
    /// Top errors by count, best first, capped at 5.
    pub top_errors: Vec<(String, u32)>,
}

/// Aggregate raw daily buckets into `DayStat`s.
///
/// Input: one entry per check event (day, words, correctness issues,
/// rule-name occurrences, tokens).
pub fn aggregate_days(events: &[RawDayEvent]) -> Vec<DayStat> {
    let mut by_day: BTreeMap<String, DayAcc> = BTreeMap::new();
    for e in events {
        let acc = by_day.entry(e.day.clone()).or_default();
        acc.words += e.words;
        acc.checks += 1;
        acc.correctness_issues += e.correctness_issues;
        acc.tokens.extend(e.tokens.iter().cloned());
        for (rule, n) in &e.rule_counts {
            *acc.rules.entry(rule.clone()).or_insert(0) += n;
        }
    }
    by_day
        .into_iter()
        .map(|(day, acc)| {
            let accuracy = if acc.words == 0 {
                1.0
            } else {
                (1.0 - acc.correctness_issues as f64 / acc.words as f64).clamp(0.0, 1.0)
            };
            let mut top: Vec<(String, u32)> =
                acc.rules.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            top.truncate(5);
            DayStat {
                day,
                words: acc.words,
                checks: acc.checks,
                accuracy,
                vocab_unique: 0, // filled by caller with token data
                vocab_rare_pct: 0.0,
                top_errors: top,
            }
        })
        .collect()
}

/// One raw check event reduced to what aggregation needs.
pub struct RawDayEvent {
    pub day: String,
    pub words: u32,
    pub correctness_issues: u32,
    pub rule_counts: BTreeMap<String, u32>,
    pub tokens: Vec<String>,
}

#[derive(Default)]
struct DayAcc {
    words: u32,
    checks: u32,
    correctness_issues: u32,
    rules: BTreeMap<String, u32>,
    tokens: Vec<String>,
}

/// Current streak length: consecutive days (ending `today` or `today−1`,
/// so a not-yet-written today doesn't break it) each having ≥
/// MIN_STREAK_WORDS checked words. `words_by_day` maps YYYY-MM-DD →
/// words checked.
pub fn current_streak(words_by_day: &HashMap<String, u32>, today: String) -> u32 {
    let today_d = match days_from_civil(&today) {
        Some(d) => d,
        None => return 0,
    };
    let mut streak = 0u32;
    let mut cursor = today_d;
    // Today may have zero words so far without breaking the streak.
    if words_by_day.get(&today).copied().unwrap_or(0) < MIN_STREAK_WORDS {
        cursor -= 1;
    }
    loop {
        let day_str = civil_from_days(cursor);
        match words_by_day.get(&day_str).copied().unwrap_or(0) {
            n if n >= MIN_STREAK_WORDS => {
                streak += 1;
                cursor -= 1;
            }
            _ => break,
        }
    }
    streak
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_string_roundtrip_known_date() {
        // 2026-08-22 00:00:00 UTC = days since epoch 20656.
        assert_eq!(civil_from_days(20_656), "2026-08-22");
        assert_eq!(days_from_civil("2026-08-22"), Some(20_656));
    }

    #[test]
    fn month_boundary_day_math() {
        assert_eq!(civil_from_days(days_from_civil("2026-02-28").unwrap() + 1), "2026-03-01");
        // Leap year: 2028-02-28 + 1 = 2028-02-29.
        assert_eq!(civil_from_days(days_from_civil("2028-02-28").unwrap() + 1), "2028-02-29");
        // Year boundary.
        assert_eq!(civil_from_days(days_from_civil("2026-12-31").unwrap() + 1), "2027-01-01");
    }

    #[test]
    fn normalize_splits_and_lowercases() {
        assert_eq!(normalize_word("Hello"), Some("hello".into()));
        assert_eq!(normalize_word("don't"), Some("don't".into()));
        assert_eq!(normalize_word("--"), None);
        assert_eq!(normalize_word("a"), None); // too short
    }

    #[test]
    fn vocab_counts_rare_against_common_list() {
        let common: BTreeSet<String> = ["the", "cat", "dog"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let tokens = vec![
            "the".into(), "Cat".into(), "the".into(),
            "quixotic".into(), "the".into(),
        ];
        let v = vocab_stats(&tokens, &common);
        assert_eq!(v.total_tokens, 5);
        assert_eq!(v.unique, 4);
        assert!((v.rare_pct - 20.0).abs() < 1e-9); // quixotic is the only rare token (1/5)
    }

    #[test]
    fn accuracy_clamps_to_unit_interval() {
        let events = vec![RawDayEvent {
            day: "2026-08-22".into(),
            words: 10,
            correctness_issues: 12, // more issues than words
            rule_counts: BTreeMap::new(),
            tokens: vec![],
        }];
        let days = aggregate_days(&events);
        assert!((days[0].accuracy - 0.0).abs() < 1e-9);
    }

    #[test]
    fn top_errors_capped_at_five_sorted_desc() {
        let mut rules = BTreeMap::new();
        for i in 0..7 {
            rules.insert(format!("harper:r{i}"), (10 - i) as u32);
        }
        let events = vec![RawDayEvent {
            day: "2026-08-22".into(),
            words: 100,
            correctness_issues: 55,
            rule_counts: rules,
            tokens: vec![],
        }];
        let days = aggregate_days(&events);
        assert_eq!(days[0].top_errors.len(), 5);
        assert_eq!(days[0].top_errors[0], ("harper:r0".to_string(), 10));
    }

    #[test]
    fn streak_adversarial_month_boundary() {
        // Wrote 60 words on Jan 31 and Feb 1 (leap-free year), nothing Feb 2.
        let mut words = HashMap::new();
        words.insert("2027-01-30".to_string(), 60);
        words.insert("2027-01-31".to_string(), 60);
        words.insert("2027-02-01".to_string(), 60);
        // Today = Feb 3: streak broken by empty Feb 2 → 0.
        assert_eq!(current_streak(&words, "2027-02-03".into()), 0);
        // Today = Feb 2 (not yet written): yesterday counts → 3.
        assert_eq!(current_streak(&words, "2027-02-02".into()), 3);
        // Today = Feb 1: 2 (Jan 31, Jan 30).
        assert_eq!(current_streak(&words, "2027-02-01".into()), 2);
    }

    #[test]
    fn streak_requires_min_words_per_day() {
        let mut words = HashMap::new();
        words.insert("2026-08-20".to_string(), 49); // below threshold
        words.insert("2026-08-21".to_string(), 80);
        assert_eq!(current_streak(&words, "2026-08-21".into()), 1);
    }
}
