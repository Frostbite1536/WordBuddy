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
    // std doesn't expose the zone; the offset is captured from the OS
    // time-zone API at startup AND re-captured on every nightly
    // aggregation pass so crossing a DST transition can't leave the
    // app with a stale one-hour skew (audit M9).
    LOCAL_OFFSET_SECS.load(std::sync::atomic::Ordering::Relaxed)
}

static LOCAL_OFFSET_SECS: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

pub fn local_offset_secs() -> i64 {
    local_utc_offset_secs()
}

/// Pure mapping from Windows TZ-bias fields to a UTC offset in seconds.
/// Bias values are UTC−local minutes, so the result negates them.
/// The DaylightBias applies ONLY while DST is actually active (audit
/// M9: it used to be applied whenever nonzero, which most zones are
/// year-round, shifting standard-time buckets by one hour).
pub fn offset_secs_from_bias(
    bias_min: i64,
    standard_bias_min: i64,
    daylight_bias_min: i64,
    daylight_active: bool,
) -> i64 {
    let total = if daylight_active {
        bias_min + daylight_bias_min
    } else {
        bias_min + standard_bias_min
    };
    -total * 60
}

/// Capture the machine's UTC offset from the Windows time-zone API.
/// Called at app start and re-called by the nightly scheduler so a DST
/// transition mid-run is picked up (audit M9). Non-Windows: UTC (stubs, D3).
#[cfg(target_os = "windows")]
pub fn capture_local_offset() {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    unsafe {
        let mut tz = TIME_ZONE_INFORMATION::default();
        // Returns the DST-in-effect verdict — exactly the signal the
        // bias arithmetic was missing. 1 = standard, 2 = daylight
        // (windows-0.58 exposes only TIME_ZONE_ID_INVALID as a const).
        let id = GetTimeZoneInformation(&mut tz);
        let secs = offset_secs_from_bias(
            tz.Bias as i64,
            tz.StandardBias as i64,
            tz.DaylightBias as i64,
            id == 2, // TIME_ZONE_ID_DAYLIGHT
        );
        LOCAL_OFFSET_SECS.store(secs, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Non-Windows (PLAN-08 step 6): real local offset via the `time` crate.
/// `now_local()` can fail on some platforms when the global TZ state is
/// being read concurrently — degrade to UTC exactly like the old stub
/// rather than panic or mis-bucket by an hour. The nightly scheduler
/// re-calls this, so a transient failure self-heals within a day and a
/// DST transition mid-run is still picked up (audit M9 hook preserved).
#[cfg(not(target_os = "windows"))]
pub fn capture_local_offset() {
    let secs = time::OffsetDateTime::now_local()
        .map(|t| i64::try_from(t.offset().whole_seconds()).unwrap_or(0))
        .unwrap_or(0);
    LOCAL_OFFSET_SECS.store(secs, std::sync::atomic::Ordering::Relaxed);
}

/// Days-from-epoch → `YYYY-MM-DD` (Howard Hinnant's civil algorithm).
pub fn civil_from_days(days: i64) -> String {
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
/// rule-name occurrences, per-event vocabulary metrics).
pub fn aggregate_days(events: &[RawDayEvent]) -> Vec<DayStat> {
    let mut by_day: BTreeMap<String, DayAcc> = BTreeMap::new();
    for e in events {
        let acc = by_day.entry(e.day.clone()).or_default();
        acc.words += e.words;
        acc.checks += e.events;
        acc.correctness_issues += e.correctness_issues;
        if e.vocab_unique > acc.vocab_unique_max {
            acc.vocab_unique_max = e.vocab_unique;
        }
        acc.vocab_rare_weighted += e.vocab_rare_pct * f64::from(e.words);
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
                vocab_unique: acc.vocab_unique_max,
                vocab_rare_pct: if acc.words == 0 {
                    0.0
                } else {
                    acc.vocab_rare_weighted / f64::from(acc.words)
                },
                top_errors: top,
            }
        })
        .collect()
}

/// One raw check event reduced to what aggregation needs.
///
/// Vocabulary metrics are the per-event values computed at record time
/// from the actual text (then discarded, INV-PRIV-002). Per-day union of
/// distinct words is not recoverable without tokens, so `vocab_unique`
/// aggregates as the day's max event value — a lower bound on the true
/// union; summing would double-count across events of the same day.
pub struct RawDayEvent {
    pub day: String,
    pub words: u32,
    pub correctness_issues: u32,
    pub rule_counts: BTreeMap<String, u32>,
    pub vocab_unique: u32,
    pub vocab_rare_pct: f64,
    /// Number of check events folded into this bucket. jobs.rs groups
    /// per day, so `checks` must come from here — counting entries
    /// would report one check per DAY (pre-fix behavior).
    pub events: u32,
}

#[derive(Default)]
struct DayAcc {
    words: u32,
    checks: u32,
    correctness_issues: u32,
    rules: BTreeMap<String, u32>,
    vocab_unique_max: u32,
    /// Σ(rare_pct × words) — divided by Σwords for a word-weighted mean.
    vocab_rare_weighted: f64,
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
        // 2026-08-22 00:00:00 UTC = day 20687 since epoch.
        assert_eq!(civil_from_days(20_687), "2026-08-22");
        assert_eq!(days_from_civil("2026-08-22"), Some(20_687));
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
        assert_eq!(v.unique, 3); // the, cat, quixotic
        assert!((v.rare_pct - 20.0).abs() < 1e-9); // quixotic is the only rare token (1/5)
    }

    #[test]
    fn accuracy_clamps_to_unit_interval() {
        let events = vec![RawDayEvent {
            day: "2026-08-22".into(),
            words: 10,
            correctness_issues: 12, // more issues than words
            rule_counts: BTreeMap::new(),
            vocab_unique: 0,
            vocab_rare_pct: 0.0,
            events: 1,
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
            vocab_unique: 0,
            vocab_rare_pct: 0.0,
            events: 1,
        }];
        let days = aggregate_days(&events);
        assert_eq!(days[0].top_errors.len(), 5);
        assert_eq!(days[0].top_errors[0], ("harper:r0".to_string(), 10));
    }

    #[test]
    fn vocab_metrics_aggregate_from_per_event_values() {
        let events = vec![
            RawDayEvent {
                day: "2026-08-22".into(),
                words: 10,
                correctness_issues: 0,
                rule_counts: BTreeMap::new(),
                vocab_unique: 6,
                vocab_rare_pct: 20.0,
                events: 1,
            },
            RawDayEvent {
                day: "2026-08-22".into(),
                words: 30,
                correctness_issues: 0,
                rule_counts: BTreeMap::new(),
                vocab_unique: 20,
                vocab_rare_pct: 40.0,
                events: 1,
            },
        ];

        let days = aggregate_days(&events);
        assert_eq!(days[0].vocab_unique, 20); // max, not sum (union lower bound)
        // Word-weighted mean: (20×10 + 40×30) / 40 = 35.
        assert!((days[0].vocab_rare_pct - 35.0).abs() < 1e-9);
    }
    #[test]
    fn offset_applies_daylight_bias_only_when_dst_active() {
        // A typical US Eastern zone: base bias 300, standard 0,
        // daylight −60. DaylightBias is NONZERO year-round — the M9 bug
        // applied it even in standard time.
        let standard = offset_secs_from_bias(300, 0, -60, false);
        let daylight = offset_secs_from_bias(300, 0, -60, true);
        assert_eq!(standard, -5 * 3600); // EST: UTC−5
        assert_eq!(daylight, -4 * 3600); // EDT: UTC−4

        // Southern-hemisphere style zone with positive standard bias.
        let std_plus = offset_secs_from_bias(-60, 60, 0, false);
        assert_eq!(std_plus, 0); // UTC+1 standard, no DST right now
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
        // Today = Feb 1: 3 (Feb 1 itself, Jan 31, Jan 30 — all ≥50).
        assert_eq!(current_streak(&words, "2027-02-01".into()), 3);
    }

    #[test]
    fn streak_requires_min_words_per_day() {
        let mut words = HashMap::new();
        words.insert("2026-08-20".to_string(), 49); // below threshold
        words.insert("2026-08-21".to_string(), 80);
        assert_eq!(current_streak(&words, "2026-08-21".into()), 1);
    }
}
