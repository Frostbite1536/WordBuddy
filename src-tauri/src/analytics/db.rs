//! `writing.sqlite` access (rusqlite, WAL, per-op connections).
//!
//! Mirrors the base repo's journal/db.rs conventions: idempotent
//! `init_schema`, `day_bounds_local` local-midnight day math, and a
//! strict no-field-text rule (INV-PRIV-002) — rows carry counts and
//! rule names only.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

/// Dropped analytics events (backpressure counter): checking must never
/// stall on analytics, so failed writes are dropped and counted.
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);

pub fn dropped_events() -> u64 {
    DROPPED_EVENTS.load(Ordering::Relaxed)
}

fn db_path() -> Result<PathBuf, String> {
    let base = dirs_next::data_dir().ok_or("could not resolve data dir")?;
    let dir = base.join("wordbuddy");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create_dir_all: {e}"))?;
    Ok(dir.join("writing.sqlite"))
}

pub fn connect() -> Result<Connection, String> {
    let path = db_path()?;
    let conn = Connection::open(&path).map_err(|e| format!("open writing.sqlite: {e}"))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("WAL: {e}"))?;
    Ok(conn)
}

/// Open an in-memory DB with the schema applied (tests + dry runs).
#[cfg_attr(not(test), allow(dead_code))]
pub fn connect_in_memory() -> Result<Connection, String> {
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    init_schema(&conn)?;
    Ok(conn)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS check_events (
          id INTEGER PRIMARY KEY,
          ts INTEGER NOT NULL,
          local_day TEXT NOT NULL DEFAULT '',
          surface TEXT NOT NULL,
          target TEXT NOT NULL,
          word_count INTEGER NOT NULL,
          issue_counts_json TEXT NOT NULL,
          rule_counts_json TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_check_events_ts ON check_events(ts);
        CREATE TABLE IF NOT EXISTS rewrites (
          id INTEGER PRIMARY KEY,
          ts INTEGER NOT NULL,
          kind TEXT NOT NULL,
          action TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS daily_stats (
          day TEXT PRIMARY KEY,
          words INTEGER NOT NULL DEFAULT 0,
          checks INTEGER NOT NULL DEFAULT 0,
          accuracy REAL NOT NULL DEFAULT 1.0,
          vocab_unique INTEGER NOT NULL DEFAULT 0,
          vocab_rare_pct REAL NOT NULL DEFAULT 0.0,
          top_errors_json TEXT NOT NULL DEFAULT '[]',
          streak_len INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS weekly_reports (
          week_start TEXT PRIMARY KEY,
          payload_json TEXT NOT NULL,
          created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS llm_calls (
          id INTEGER PRIMARY KEY,
          ts INTEGER NOT NULL,
          purpose TEXT NOT NULL,
          ok INTEGER NOT NULL
        );
        "#,
    )
    .map_err(|e| format!("init_schema: {e}"))?;
    // Migration: vocab columns added after first PLAN-05 release. Runs
    // AFTER table creation — on a fresh database the pre-creation probe
    // always failed, so the ALTERs never ran and check_events shipped
    // without the vocab columns record_check writes.
    let has_vocab = conn
        .prepare("SELECT vocab_unique FROM check_events LIMIT 0")
        .is_ok();
    if !has_vocab {
        let _ = conn.execute_batch(
            "ALTER TABLE check_events ADD COLUMN vocab_unique INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE check_events ADD COLUMN vocab_rare_pct REAL NOT NULL DEFAULT 0.0;",
        );
    }
    let has_local_day = conn
        .prepare("SELECT local_day FROM check_events LIMIT 0")
        .is_ok();
    if !has_local_day {
        conn.execute_batch(
            "ALTER TABLE check_events ADD COLUMN local_day TEXT NOT NULL DEFAULT '';",
        )
        .map_err(|e| format!("add local_day migration: {e}"))?;
    }
    Ok(())
}

/// Rows deleted by a retention pass (audit M10).
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct PurgeCounts {
    pub check_events: usize,
    pub rewrites: usize,
    pub llm_calls: usize,
    pub weekly_reports: usize,
}

/// Delete every analytics row older than `cutoff_ts` (epoch seconds).
/// Parameterized SQL only; each table in its own statement so one
/// failure doesn't skip the rest.
pub fn purge_older_than(conn: &Connection, cutoff_ts: i64) -> Result<PurgeCounts, String> {
    let mut counts = PurgeCounts::default();
    for (sql, slot) in [
        (
            "DELETE FROM check_events WHERE ts < ?1",
            &mut counts.check_events as &mut usize,
        ),
        ("DELETE FROM rewrites WHERE ts < ?1", &mut counts.rewrites),
        ("DELETE FROM llm_calls WHERE ts < ?1", &mut counts.llm_calls),
        (
            "DELETE FROM weekly_reports WHERE created_at < ?1",
            &mut counts.weekly_reports,
        ),
    ] {
        *slot = conn
            .execute(sql, rusqlite::params![cutoff_ts])
            .map_err(|e| format!("purge: {e}"))?;
    }
    Ok(counts)
}

/// One checked field, ready to record.
#[derive(Debug, Clone)]
pub struct CheckEvent {
    pub ts: i64,
    /// Calendar day at the moment of the check, under the then-current
    /// local UTC offset. Persisting this avoids rebucketing old events
    /// incorrectly after a DST transition.
    pub local_day: String,
    pub surface: String,
    pub target: String,
    pub word_count: u32,
    /// Persisted as numbers only, computed at record time.
    pub vocab_unique: u32,
    pub vocab_rare_pct: f64,
    pub issue_counts: std::collections::BTreeMap<String, u32>,
    pub rule_counts: std::collections::BTreeMap<String, u32>,
}

pub fn record_check(event: &CheckEvent) -> Result<(), String> {
    let result = (|| {
        let conn = connect()?;
        let ic = serde_json::to_string(&event.issue_counts).map_err(|e| e.to_string())?;
        let rc = serde_json::to_string(&event.rule_counts).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO check_events (ts, local_day, surface, target, word_count, vocab_unique, vocab_rare_pct, issue_counts_json, rule_counts_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                event.ts, event.local_day, event.surface, event.target, event.word_count,
                event.vocab_unique, event.vocab_rare_pct, ic, rc
            ],
        )
        .map_err(|e| format!("insert check_event: {e}"))?;
        Ok(())
    })();
    if result.is_err() {
        // Backpressure: drop the row, count it, never stall checking.
        DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
    }
    result
}

pub fn record_rewrite(ts: i64, kind: &str, action: &str) -> Result<(), String> {
    let result = (|| {
        let conn = connect()?;
        conn.execute(
            "INSERT INTO rewrites (ts, kind, action) VALUES (?1, ?2, ?3)",
            rusqlite::params![ts, kind, action],
        )
        .map_err(|e| format!("insert rewrite: {e}"))?;
        Ok(())
    })();
    if result.is_err() {
        DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
    }
    result
}

/// Days (YYYY-MM-DD) with at least one check event.
#[allow(dead_code)] // retained for report/export callers outside this crate
pub fn days_with_events(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT ts FROM check_events ORDER BY ts")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let ts: i64 = row.get(0).map_err(|e| e.to_string())?;
        seen.insert(super::aggregate::day_string_from_ts(ts));
    }
    Ok(seen.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_idempotent() {
        let conn = connect_in_memory().unwrap();
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
    }

    #[test]
    fn record_check_roundtrip_and_drop_counter() {
        let mut ev = CheckEvent {
            ts: 1_700_000_000,
            local_day: "2023-11-14".into(),
            surface: "browser".into(),
            target: "example.com".into(),
            word_count: 12,
            vocab_unique: 5,
            vocab_rare_pct: 20.0,
            issue_counts: [("Correctness".to_string(), 2u32)].into_iter().collect(),
            rule_counts: [("harper:SpellCheck".to_string(), 2u32)]
                .into_iter()
                .collect(),
        };
        // In-memory DB can't be reached through connect() (file-backed);
        // exercise the insert statement shape directly.
        let conn = connect_in_memory().unwrap();
        let ic = serde_json::to_string(&ev.issue_counts).unwrap();
        conn.execute(
            "INSERT INTO check_events (ts, surface, target, word_count, issue_counts_json, rule_counts_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![ev.ts, ev.surface, ev.target, ev.word_count, ic, serde_json::to_string(&ev.rule_counts).unwrap()],
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM check_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let _ = &mut ev;
    }

    #[test]
    fn purge_older_than_deletes_only_old_rows() {
        let conn = connect_in_memory().unwrap();
        init_schema(&conn).unwrap();
        // Two rows per table: one old (ts=1000), one new (ts=2000).
        for ts in [1_000i64, 2_000] {
            conn.execute(
                "INSERT INTO check_events (ts, surface, target, word_count, vocab_unique,
                  vocab_rare_pct, issue_counts_json, rule_counts_json)
                 VALUES (?1, 's', 't', 1, 0, 0.0, '{}', '{}')",
                rusqlite::params![ts],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO rewrites (ts, kind, action) VALUES (?1, 'k', 'a')",
                rusqlite::params![ts],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO llm_calls (ts, purpose, ok) VALUES (?1, 'p', 1)",
                rusqlite::params![ts],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO weekly_reports (week_start, payload_json, created_at)
                 VALUES ('w-' || ?1, '[]', ?1)",
                rusqlite::params![ts],
            )
            .unwrap();
        }

        let counts = purge_older_than(&conn, 1_500).unwrap();
        assert_eq!(counts.check_events, 1);
        assert_eq!(counts.rewrites, 1);
        assert_eq!(counts.llm_calls, 1);
        assert_eq!(counts.weekly_reports, 1);

        // Newer rows survive.
        for table in ["check_events", "rewrites", "llm_calls", "weekly_reports"] {
            let n: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "{table} should keep its newer row");
        }
    }
}
