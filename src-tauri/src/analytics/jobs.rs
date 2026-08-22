//! Aggregation + report assembly over the real `writing.sqlite`
//! (PLAN-05 tasks 2–3). Pure math lives in `aggregate.rs`; this module
//! bridges the DB to it and owns the schedulers' shared guard.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use super::aggregate;
use super::db;
use super::report::{render_markdown, WeekPayload};

/// Aggregate all raw check events into `daily_stats`. Returns the number
/// of days written. Guarded by ANALYZING-style AtomicBool at the caller
/// (`analytics_aggregate_now`).
pub fn aggregate_and_store(conn: &Connection) -> Result<usize, String> {
    let mut stmt = conn
        .prepare(
            "SELECT ts, surface, target, word_count, issue_counts_json, rule_counts_json
             FROM check_events ORDER BY ts",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    let mut raw: Vec<aggregate::RawDayEvent> = Vec::new();
    // Group key: local day string.
    let mut by_day: HashMap<String, usize> = HashMap::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let ts: i64 = row.get(0).map_err(|e| e.to_string())?;
        let _surface: String = row.get(1).map_err(|e| e.to_string())?;
        let _target: String = row.get(2).map_err(|e| e.to_string())?;
        let words: u32 = row.get(3).map_err(|e| e.to_string())?;
        let issue_json: String = row.get(4).map_err(|e| e.to_string())?;
        let rule_json: String = row.get(5).map_err(|e| e.to_string())?;

        let day = aggregate::day_string_from_ts(ts);
        let idx = match by_day.get(&day) {
            Some(i) => *i,
            None => {
                raw.push(aggregate::RawDayEvent {
                    day: day.clone(),
                    words: 0,
                    correctness_issues: 0,
                    rule_counts: Default::default(),
                    tokens: Vec::new(),
                });
                let i = raw.len() - 1;
                by_day.insert(day, i);
                i
            }
        };
        let ev = &mut raw[idx];
        ev.words += words;

        if let Ok(counts) =
            serde_json::from_str::<std::collections::BTreeMap<String, u32>>(&issue_json)
        {
            ev.correctness_issues += *counts.get("Correctness").unwrap_or(&0);
        }
        if let Ok(rules) =
            serde_json::from_str::<std::collections::BTreeMap<String, u32>>(&rule_json)
        {
            for (r, n) in rules {
                *ev.rule_counts.entry(r).or_insert(0) += n;
            }
        }
        // Tokens are not stored (INV-PRIV-002) — vocabulary is derived
        // per-day at event time is impossible retroactively, so vocab is
        // computed from the day's word stream reconstructed approximately:
        // we store no text, so unique/rare use the day's WORD COUNT only.
        // See the honest-heuristic note in the dashboard.
    }

    let days = aggregate::aggregate_days(&raw);
    let mut count = 0usize;
    for d in &days {
        let top_json = serde_json::to_string(&d.top_errors).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO daily_stats (day, words, checks, accuracy, vocab_unique, vocab_rare_pct, top_errors_json, streak_len)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)
             ON CONFLICT(day) DO UPDATE SET words=?2, checks=?3, accuracy=?4,
               vocab_unique=?5, vocab_rare_pct=?6, top_errors_json=?7",
            rusqlite::params![
                d.day,
                d.words,
                d.checks,
                d.accuracy,
                d.vocab_unique,
                d.vocab_rare_pct,
                top_json
            ],
        )
        .map_err(|e| e.to_string())?;
        count += 1;
    }

    refresh_streaks(conn)?;
    Ok(count)
}

/// Recompute streak_len for each stored day (consecutive days with ≥
/// MIN_STREAK_WORDS ending at that day).
fn refresh_streaks(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT day, words FROM daily_stats ORDER BY day")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut words_by_day: HashMap<String, u32> = HashMap::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let day: String = row.get(0).map_err(|e| e.to_string())?;
        let words: u32 = row.get(1).map_err(|e| e.to_string())?;
        words_by_day.insert(day, words);
    }
    drop(rows);
    for (day, words) in &words_by_day {
        let streak = aggregate::current_streak(&words_by_day, day.clone());
        conn.execute(
            "UPDATE daily_stats SET streak_len = ?1 WHERE day = ?2",
            rusqlite::params![streak, day],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Build the weekly payload for the 7 days starting `week_start`.
/// Reads daily_stats when present, else aggregates raw on the fly.
pub fn build_week_payload(
    conn: &Connection,
    week_start: &str,
    today: &str,
) -> Result<WeekPayload, String> {
    let start_d = aggregate::days_from_civil(week_start)
        .ok_or_else(|| format!("bad week_start '{week_start}'"))?;
    let mut days = Vec::new();
    for offset in 0..7i64 {
        let day = aggregate::civil_from_days(start_d + offset);
        let mut stmt = conn
            .prepare(
                "SELECT words, checks, accuracy, vocab_unique, vocab_rare_pct, top_errors_json
                 FROM daily_stats WHERE day = ?1",
            )
            .map_err(|e| e.to_string())?;
        let row = stmt
            .query_row([day.as_str()], |r| {
                Ok((
                    r.get::<_, u32>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, u32>(3)?,
                    r.get::<_, f64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .optional()
            .map_err(|e| e.to_string())?;
        days.push(match row {
            Some((words, checks, accuracy, vocab_unique, vocab_rare_pct, top_json)) => super::aggregate::DayStat {
                day,
                words,
                checks,
                accuracy,
                vocab_unique: vocab_unique,
                vocab_rare_pct: vocab_rare_pct,
                top_errors: serde_json::from_str(&top_json).unwrap_or_default(),
            },
            None => super::aggregate::DayStat {
                day,
                words: 0,
                checks: 0,
                accuracy: 1.0,
                vocab_unique: 0,
                vocab_rare_pct: 0.0,
                top_errors: vec![],
            },
        });
    }

    let words: u32 = days.iter().map(|d| d.words).sum();
    let checks: u32 = days.iter().map(|d| d.checks).sum();

    // Prior-week words for the delta.
    let prior_start = aggregate::civil_from_days(start_d - 7);
    let prior_end = aggregate::civil_from_days(start_d - 1);
    let mut stmt = conn
        .prepare("SELECT COALESCE(SUM(words),0) FROM daily_stats WHERE day BETWEEN ?1 AND ?2")
        .map_err(|e| e.to_string())?;
    let prior_words: u32 = stmt
        .query_row(rusqlite::params![prior_start, prior_end], |r| r.get(0))
        .map_err(|e| e.to_string())?;

    // Weighted accuracy across days with data.
    let total_words_for_acc: u32 = days.iter().filter(|d| d.checks > 0).map(|d| d.words).sum();
    let acc_sum: f64 = days
        .iter()
        .filter(|d| d.checks > 0)
        .map(|d| d.accuracy * d.words as f64)
        .sum();
    let accuracy = if total_words_for_acc == 0 {
        1.0
    } else {
        acc_sum / total_words_for_acc as f64
    };

    // Top errors across the week.
    let mut week_rules: HashMap<String, u32> = HashMap::new();
    for d in &days {
        for (r, n) in &d.top_errors {
            *week_rules.entry(r.clone()).or_insert(0) += n;
        }
    }
    let mut top_errors: Vec<(String, u32)> = week_rules.into_iter().collect();
    top_errors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top_errors.truncate(5);

    let vocab_unique: u32 = days.iter().map(|d| d.vocab_unique).max().unwrap_or(0);
    let vocab_rare_pct = if days.is_empty() {
        0.0
    } else {
        days.iter()
            .filter(|d| d.checks > 0)
            .map(|d| d.vocab_rare_pct)
            .sum::<f64>()
            / days.iter().filter(|d| d.checks > 0).count().max(1) as f64
    };

    let streak = {
        let mut words_by_day: HashMap<String, u32> = HashMap::new();
        for d in &days {
            words_by_day.insert(d.day.clone(), d.words);
        }
        aggregate::current_streak(&words_by_day, today.to_string())
    };

    Ok(WeekPayload {
        week_start: week_start.to_string(),
        days,
        words,
        checks,
        words_delta_vs_prior: words as i64 - prior_words as i64,
        accuracy,
        streak,
        vocab_unique,
        vocab_rare_pct,
        top_errors,
        tone: None,
    })
}

/// Persist + export a rendered report.
pub fn save_report(
    conn: &Connection,
    week_start: &str,
    markdown: &str,
    payload_json: &str,
    now_secs: i64,
) -> Result<PathBuf2, String> {
    conn.execute(
        "INSERT INTO weekly_reports (week_start, payload_json, created_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(week_start) DO UPDATE SET payload_json=?2, created_at=?3",
        rusqlite::params![week_start, payload_json, now_secs],
    )
    .map_err(|e| e.to_string())?;

    let docs = dirs_next::document_dir().ok_or("no documents dir")?;
    let dir = docs.join("WordBuddy").join("reports");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create reports dir: {e}"))?;
    let path = dir.join(format!("wordbuddy-report-{week_start}.md"));
    std::fs::write(&path, markdown).map_err(|e| format!("write report: {e}"))?;
    Ok(PathBuf2(path))
}

/// Wrapper so callers don't depend on PathBuf naming in signatures.
pub struct PathBuf2(pub std::path::PathBuf);

impl PathBuf2 {
    pub fn as_str(&self) -> &str {
        self.0.to_str().unwrap_or("")
    }
}
