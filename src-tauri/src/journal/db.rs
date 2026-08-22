//! Rust-side SQLite store for the work journal (screenshots → batches →
//! observations → timeline cards). Lives in `<app_data_dir>/journal.sqlite`,
//! separate from the frontend's `workbuddy.db` (conversations, via
//! tauri-plugin-sql) and `rag_vectors.db` — the recorder must not depend on
//! the webview being alive.
//!
//! All tables are created up front (Phase 1) so Phases 2–4 are schema-stable.

use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;
use tauri::Manager;

/// Resolve `<app_data_dir>` for on-disk journal artifacts, creating it if
/// needed. Uses Tauri's identifier-scoped dir (Roaming on Windows).
pub fn app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create app data dir: {e}"))?;
    Ok(dir)
}

/// Directory screenshot JPEGs are written to.
pub fn recordings_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_data_dir(app)?.join("recordings");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create recordings dir: {e}"))?;
    Ok(dir)
}

/// Open the journal DB with WAL mode. Connections are opened per operation
/// (same pattern as rag.rs) — operations are short and WAL tolerates
/// concurrent readers alongside the single writer.
pub fn open(app: &tauri::AppHandle) -> Result<Connection, String> {
    let path = app_data_dir(app)?.join("journal.sqlite");
    let conn = Connection::open(&path)
        .map_err(|e| format!("Failed to open journal db: {e}"))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .map_err(|e| format!("Failed to set WAL mode: {e}"))?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Create every journal table + index. Idempotent. Split out so tests can
/// run it against an in-memory connection.
pub fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS screenshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            captured_at INTEGER NOT NULL,          -- unix seconds
            file_path TEXT NOT NULL,
            file_size INTEGER NOT NULL DEFAULT 0,
            idle_seconds INTEGER NOT NULL DEFAULT 0,
            window_title TEXT NOT NULL DEFAULT '',
            is_deleted INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_screenshots_captured_at
            ON screenshots(captured_at);

        CREATE TABLE IF NOT EXISTS analysis_batches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_start_ts INTEGER NOT NULL,
            batch_end_ts INTEGER NOT NULL,
            -- pending | processing | done | failed | skipped_short | skipped_idle
            status TEXT NOT NULL DEFAULT 'pending',
            reason TEXT,
            llm_metadata TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_batches_start_ts
            ON analysis_batches(batch_start_ts);

        CREATE TABLE IF NOT EXISTS batch_screenshots (
            batch_id INTEGER NOT NULL,
            screenshot_id INTEGER NOT NULL,
            PRIMARY KEY (batch_id, screenshot_id)
        );

        CREATE TABLE IF NOT EXISTS observations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_id INTEGER NOT NULL,
            start_ts INTEGER NOT NULL,
            end_ts INTEGER NOT NULL,
            observation TEXT NOT NULL,
            llm_model TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_observations_start_ts
            ON observations(start_ts);

        CREATE TABLE IF NOT EXISTS timeline_cards (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            batch_id INTEGER NOT NULL,
            start_ts INTEGER NOT NULL,
            end_ts INTEGER NOT NULL,
            day TEXT NOT NULL,                      -- local YYYY-MM-DD
            title TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            category TEXT NOT NULL DEFAULT 'other',
            subcategory TEXT NOT NULL DEFAULT '',
            detailed_summary TEXT NOT NULL DEFAULT '',
            metadata_json TEXT,
            is_deleted INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_timeline_cards_day
            ON timeline_cards(day);
        CREATE INDEX IF NOT EXISTS idx_timeline_cards_start_ts
            ON timeline_cards(start_ts);

        CREATE TABLE IF NOT EXISTS llm_calls (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            created_at INTEGER NOT NULL,
            batch_id INTEGER,
            attempt INTEGER NOT NULL DEFAULT 1,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            operation TEXT NOT NULL,               -- transcription | activity_cards | standup
            status TEXT NOT NULL,                  -- ok | error
            latency_ms INTEGER,
            error_message TEXT
        );

        CREATE TABLE IF NOT EXISTS daily_standup_entries (
            standup_day TEXT PRIMARY KEY,          -- local YYYY-MM-DD
            payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        "#,
    )
    .map_err(|e| format!("Failed to create journal schema: {e}"))
}

/// Current unix time in seconds.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Local-midnight unix bounds `[start, end)` for a `YYYY-MM-DD` day string.
/// Uses the machine's local timezone — the journal is a personal artifact,
/// so "a day" means the user's wall-clock day.
pub fn day_bounds_local(day: &str) -> Result<(i64, i64), String> {
    use chrono::{Local, NaiveDate, TimeZone};
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|e| format!("Invalid day '{day}': {e}"))?;
    let start_naive = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| format!("Invalid day '{day}'"))?;
    let end_naive = date
        .succ_opt()
        .ok_or_else(|| format!("Day '{day}' has no successor"))?
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| format!("Invalid day '{day}'"))?;
    // earliest() handles DST gaps (a midnight that doesn't exist locally).
    let start = Local
        .from_local_datetime(&start_naive)
        .earliest()
        .ok_or_else(|| format!("Cannot resolve local midnight for '{day}'"))?
        .timestamp();
    let end = Local
        .from_local_datetime(&end_naive)
        .earliest()
        .ok_or_else(|| format!("Cannot resolve local midnight after '{day}'"))?
        .timestamp();
    Ok((start, end))
}

/// Local `YYYY-MM-DD` for a unix timestamp.
pub fn day_of_ts(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(ts, 0).earliest() {
        Some(dt) => dt.format("%Y-%m-%d").to_string(),
        None => "1970-01-01".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Screenshot rows
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct ScreenshotRow {
    pub id: i64,
    pub captured_at: i64,
    pub file_path: String,
    pub file_size: i64,
    pub idle_seconds: i64,
    pub window_title: String,
}

pub fn insert_screenshot(
    conn: &Connection,
    captured_at: i64,
    file_path: &str,
    file_size: i64,
    idle_seconds: i64,
    window_title: &str,
) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO screenshots (captured_at, file_path, file_size, idle_seconds, window_title)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![captured_at, file_path, file_size, idle_seconds, window_title],
    )
    .map_err(|e| format!("Failed to insert screenshot row: {e}"))?;
    Ok(conn.last_insert_rowid())
}

pub fn list_screenshots_between(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<ScreenshotRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, captured_at, file_path, file_size, idle_seconds, window_title
             FROM screenshots
             WHERE captured_at >= ?1 AND captured_at < ?2 AND is_deleted = 0
             ORDER BY captured_at ASC",
        )
        .map_err(|e| format!("Query prepare failed: {e}"))?;
    let rows = stmt
        .query_map([start_ts, end_ts], |r| {
            Ok(ScreenshotRow {
                id: r.get(0)?,
                captured_at: r.get(1)?,
                file_path: r.get(2)?,
                file_size: r.get(3)?,
                idle_seconds: r.get(4)?,
                window_title: r.get(5)?,
            })
        })
        .map_err(|e| format!("Query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn get_screenshot(conn: &Connection, id: i64) -> Result<ScreenshotRow, String> {
    conn.query_row(
        "SELECT id, captured_at, file_path, file_size, idle_seconds, window_title
         FROM screenshots WHERE id = ?1 AND is_deleted = 0",
        [id],
        |r| {
            Ok(ScreenshotRow {
                id: r.get(0)?,
                captured_at: r.get(1)?,
                file_path: r.get(2)?,
                file_size: r.get(3)?,
                idle_seconds: r.get(4)?,
                window_title: r.get(5)?,
            })
        },
    )
    .map_err(|e| format!("Screenshot {id} not found: {e}"))
}

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

/// Rows older than `cutoff_ts` whose files should be deleted.
/// Selection is separated from deletion so it can be unit-tested and so the
/// caller can attempt file removal before dropping the row.
pub fn select_purge_candidates(
    conn: &Connection,
    cutoff_ts: i64,
) -> Result<Vec<(i64, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT id, file_path FROM screenshots WHERE captured_at < ?1")
        .map_err(|e| format!("Query prepare failed: {e}"))?;
    let rows = stmt
        .query_map([cutoff_ts], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| format!("Query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Delete screenshot rows by id (files already handled by the caller).
/// Analysis products (observations, timeline_cards) are intentionally kept —
/// they ARE the journal; only the raw frames expire.
pub fn delete_screenshot_rows(conn: &Connection, ids: &[i64]) -> Result<usize, String> {
    let mut deleted = 0;
    for id in ids {
        deleted += conn
            .execute("DELETE FROM screenshots WHERE id = ?1", [id])
            .map_err(|e| format!("Delete failed: {e}"))?;
        // Drop any batch membership rows so joins stay clean.
        let _ = conn.execute(
            "DELETE FROM batch_screenshots WHERE screenshot_id = ?1",
            [id],
        );
    }
    Ok(deleted)
}

// ---------------------------------------------------------------------------
// Analysis batches
// ---------------------------------------------------------------------------

/// Screenshots that are not yet a member of any analysis batch, oldest
/// first. These are the assembler's raw material.
pub fn list_unbatched_screenshots(conn: &Connection) -> Result<Vec<ScreenshotRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.captured_at, s.file_path, s.file_size, s.idle_seconds, s.window_title
             FROM screenshots s
             LEFT JOIN batch_screenshots bs ON bs.screenshot_id = s.id
             WHERE bs.batch_id IS NULL AND s.is_deleted = 0
             ORDER BY s.captured_at ASC",
        )
        .map_err(|e| format!("Query prepare failed: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ScreenshotRow {
                id: r.get(0)?,
                captured_at: r.get(1)?,
                file_path: r.get(2)?,
                file_size: r.get(3)?,
                idle_seconds: r.get(4)?,
                window_title: r.get(5)?,
            })
        })
        .map_err(|e| format!("Query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Create a batch row + membership rows in one transaction.
pub fn create_batch(
    conn: &mut Connection,
    start_ts: i64,
    end_ts: i64,
    status: &str,
    reason: Option<&str>,
    screenshot_ids: &[i64],
) -> Result<i64, String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("Transaction failed: {e}"))?;
    tx.execute(
        "INSERT INTO analysis_batches (batch_start_ts, batch_end_ts, status, reason)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![start_ts, end_ts, status, reason],
    )
    .map_err(|e| format!("Failed to insert batch: {e}"))?;
    let batch_id = tx.last_insert_rowid();
    for sid in screenshot_ids {
        tx.execute(
            "INSERT OR IGNORE INTO batch_screenshots (batch_id, screenshot_id) VALUES (?1, ?2)",
            rusqlite::params![batch_id, sid],
        )
        .map_err(|e| format!("Failed to insert batch membership: {e}"))?;
    }
    tx.commit().map_err(|e| format!("Commit failed: {e}"))?;
    Ok(batch_id)
}

pub fn set_batch_status(
    conn: &Connection,
    batch_id: i64,
    status: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    conn.execute(
        "UPDATE analysis_batches SET status = ?2, reason = ?3 WHERE id = ?1",
        rusqlite::params![batch_id, status, reason],
    )
    .map_err(|e| format!("Failed to update batch status: {e}"))?;
    Ok(())
}

/// Screenshot rows belonging to a batch, oldest first.
pub fn batch_screenshots(conn: &Connection, batch_id: i64) -> Result<Vec<ScreenshotRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.captured_at, s.file_path, s.file_size, s.idle_seconds, s.window_title
             FROM screenshots s
             JOIN batch_screenshots bs ON bs.screenshot_id = s.id
             WHERE bs.batch_id = ?1 AND s.is_deleted = 0
             ORDER BY s.captured_at ASC",
        )
        .map_err(|e| format!("Query prepare failed: {e}"))?;
    let rows = stmt
        .query_map([batch_id], |r| {
            Ok(ScreenshotRow {
                id: r.get(0)?,
                captured_at: r.get(1)?,
                file_path: r.get(2)?,
                file_size: r.get(3)?,
                idle_seconds: r.get(4)?,
                window_title: r.get(5)?,
            })
        })
        .map_err(|e| format!("Query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Observations
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct ObservationRow {
    pub id: i64,
    pub batch_id: i64,
    pub start_ts: i64,
    pub end_ts: i64,
    pub observation: String,
}

pub fn insert_observation(
    conn: &Connection,
    batch_id: i64,
    start_ts: i64,
    end_ts: i64,
    observation: &str,
    llm_model: &str,
) -> Result<i64, String> {
    conn.execute(
        "INSERT INTO observations (batch_id, start_ts, end_ts, observation, llm_model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![batch_id, start_ts, end_ts, observation, llm_model, now_secs()],
    )
    .map_err(|e| format!("Failed to insert observation: {e}"))?;
    Ok(conn.last_insert_rowid())
}

pub fn list_observations_for_batch(
    conn: &Connection,
    batch_id: i64,
) -> Result<Vec<ObservationRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, batch_id, start_ts, end_ts, observation
             FROM observations WHERE batch_id = ?1 ORDER BY start_ts ASC",
        )
        .map_err(|e| format!("Query prepare failed: {e}"))?;
    let rows = stmt
        .query_map([batch_id], |r| {
            Ok(ObservationRow {
                id: r.get(0)?,
                batch_id: r.get(1)?,
                start_ts: r.get(2)?,
                end_ts: r.get(3)?,
                observation: r.get(4)?,
            })
        })
        .map_err(|e| format!("Query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Timeline cards
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct TimelineCardRow {
    pub id: i64,
    pub batch_id: i64,
    pub start_ts: i64,
    pub end_ts: i64,
    pub day: String,
    pub title: String,
    pub summary: String,
    pub category: String,
    pub subcategory: String,
    pub detailed_summary: String,
    pub metadata_json: Option<String>,
}

fn card_from_row(r: &rusqlite::Row) -> rusqlite::Result<TimelineCardRow> {
    Ok(TimelineCardRow {
        id: r.get(0)?,
        batch_id: r.get(1)?,
        start_ts: r.get(2)?,
        end_ts: r.get(3)?,
        day: r.get(4)?,
        title: r.get(5)?,
        summary: r.get(6)?,
        category: r.get(7)?,
        subcategory: r.get(8)?,
        detailed_summary: r.get(9)?,
        metadata_json: r.get(10)?,
    })
}

const CARD_COLUMNS: &str = "id, batch_id, start_ts, end_ts, day, title, summary, category, \
                            subcategory, detailed_summary, metadata_json";

pub fn list_cards_for_day(conn: &Connection, day: &str) -> Result<Vec<TimelineCardRow>, String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {CARD_COLUMNS} FROM timeline_cards
             WHERE day = ?1 AND is_deleted = 0 ORDER BY start_ts ASC"
        ))
        .map_err(|e| format!("Query prepare failed: {e}"))?;
    let rows = stmt
        .query_map([day], |r| card_from_row(r))
        .map_err(|e| format!("Query failed: {e}"))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Replace a day's cards with a new set (Stage 2 output covers the whole
/// day-so-far, so the previous cards are its draft, not history). Runs in
/// one transaction; old cards are soft-deleted so a bug is recoverable by
/// hand until the next replace.
pub struct NewCard {
    pub start_ts: i64,
    pub end_ts: i64,
    pub day: String,
    pub title: String,
    pub summary: String,
    pub category: String,
    pub subcategory: String,
    pub detailed_summary: String,
    pub metadata_json: Option<String>,
}

pub fn replace_cards_for_day(
    conn: &mut Connection,
    day: &str,
    batch_id: i64,
    cards: &[NewCard],
) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("Transaction failed: {e}"))?;
    tx.execute(
        "UPDATE timeline_cards SET is_deleted = 1 WHERE day = ?1 AND is_deleted = 0",
        [day],
    )
    .map_err(|e| format!("Failed to clear day cards: {e}"))?;
    for c in cards {
        tx.execute(
            "INSERT INTO timeline_cards (batch_id, start_ts, end_ts, day, title, summary,
             category, subcategory, detailed_summary, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                batch_id,
                c.start_ts,
                c.end_ts,
                c.day,
                c.title,
                c.summary,
                c.category,
                c.subcategory,
                c.detailed_summary,
                c.metadata_json
            ],
        )
        .map_err(|e| format!("Failed to insert card: {e}"))?;
    }
    tx.commit().map_err(|e| format!("Commit failed: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Daily standup entries
// ---------------------------------------------------------------------------

pub fn upsert_standup(conn: &Connection, day: &str, payload_json: &str) -> Result<(), String> {
    let now = now_secs();
    conn.execute(
        "INSERT INTO daily_standup_entries (standup_day, payload_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(standup_day) DO UPDATE SET payload_json = ?2, updated_at = ?3",
        rusqlite::params![day, payload_json, now],
    )
    .map_err(|e| format!("Failed to upsert standup: {e}"))?;
    Ok(())
}

pub fn get_standup(conn: &Connection, day: &str) -> Result<Option<String>, String> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT payload_json FROM daily_standup_entries WHERE standup_day = ?1",
        [day],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| format!("Failed to read standup: {e}"))
}

// ---------------------------------------------------------------------------
// LLM call audit log
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn log_llm_call(
    conn: &Connection,
    batch_id: Option<i64>,
    attempt: i64,
    provider: &str,
    model: &str,
    operation: &str,
    status: &str,
    latency_ms: i64,
    error_message: Option<&str>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO llm_calls (created_at, batch_id, attempt, provider, model, operation,
         status, latency_ms, error_message)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            now_secs(),
            batch_id,
            attempt,
            provider,
            model,
            operation,
            status,
            latency_ms,
            error_message
        ],
    )
    .map_err(|e| format!("Failed to log llm call: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn schema_creates_all_tables() {
        let conn = mem_conn();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for expected in [
            "analysis_batches",
            "batch_screenshots",
            "daily_standup_entries",
            "llm_calls",
            "observations",
            "screenshots",
            "timeline_cards",
        ] {
            assert!(names.iter().any(|n| n == expected), "missing table {expected}");
        }
    }

    #[test]
    fn schema_is_idempotent() {
        let conn = mem_conn();
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
    }

    #[test]
    fn insert_and_list_screenshots_in_range() {
        let conn = mem_conn();
        insert_screenshot(&conn, 1000, "a.jpg", 10, 0, "Editor").unwrap();
        insert_screenshot(&conn, 2000, "b.jpg", 20, 5, "Browser").unwrap();
        insert_screenshot(&conn, 3000, "c.jpg", 30, 0, "Terminal").unwrap();
        let rows = list_screenshots_between(&conn, 1500, 3000).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_path, "b.jpg");
        assert_eq!(rows[0].window_title, "Browser");
    }

    #[test]
    fn get_screenshot_roundtrip() {
        let conn = mem_conn();
        let id = insert_screenshot(&conn, 1234, "x.jpg", 42, 7, "App").unwrap();
        let row = get_screenshot(&conn, id).unwrap();
        assert_eq!(row.captured_at, 1234);
        assert_eq!(row.file_size, 42);
        assert_eq!(row.idle_seconds, 7);
    }

    #[test]
    fn retention_selects_only_old_rows_and_deletes_them() {
        let conn = mem_conn();
        let old_id = insert_screenshot(&conn, 100, "old.jpg", 1, 0, "").unwrap();
        insert_screenshot(&conn, 200, "new.jpg", 1, 0, "").unwrap();
        let candidates = select_purge_candidates(&conn, 150).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0], (old_id, "old.jpg".to_string()));

        let ids: Vec<i64> = candidates.iter().map(|(id, _)| *id).collect();
        let deleted = delete_screenshot_rows(&conn, &ids).unwrap();
        assert_eq!(deleted, 1);
        let remaining = list_screenshots_between(&conn, 0, 10_000).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].file_path, "new.jpg");
    }

    #[test]
    fn day_bounds_are_a_calendar_day_apart() {
        // Non-DST-transition date in most zones; assert basic sanity rather
        // than an exact 86400 (DST days are 23h/25h and both are correct).
        let (start, end) = day_bounds_local("2026-07-03").unwrap();
        assert!(end > start);
        let span = end - start;
        assert!((23 * 3600..=25 * 3600).contains(&span), "span was {span}");
        assert_eq!(day_of_ts(start), "2026-07-03");
        assert_eq!(day_of_ts(end - 1), "2026-07-03");
        assert_eq!(day_of_ts(end), "2026-07-04");
    }

    #[test]
    fn unbatched_excludes_batch_members() {
        let mut conn = mem_conn();
        let a = insert_screenshot(&conn, 100, "a.jpg", 1, 0, "").unwrap();
        let b = insert_screenshot(&conn, 200, "b.jpg", 1, 0, "").unwrap();
        assert_eq!(list_unbatched_screenshots(&conn).unwrap().len(), 2);
        create_batch(&mut conn, 100, 150, "pending", None, &[a]).unwrap();
        let unbatched = list_unbatched_screenshots(&conn).unwrap();
        assert_eq!(unbatched.len(), 1);
        assert_eq!(unbatched[0].id, b);
    }

    #[test]
    fn batch_membership_roundtrip_and_status() {
        let mut conn = mem_conn();
        let a = insert_screenshot(&conn, 100, "a.jpg", 1, 0, "T1").unwrap();
        let b = insert_screenshot(&conn, 200, "b.jpg", 1, 0, "T2").unwrap();
        let batch = create_batch(&mut conn, 100, 200, "pending", None, &[a, b]).unwrap();
        let members = batch_screenshots(&conn, batch).unwrap();
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].window_title, "T1");
        set_batch_status(&conn, batch, "done", None).unwrap();
        let status: String = conn
            .query_row("SELECT status FROM analysis_batches WHERE id = ?1", [batch], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "done");
    }

    #[test]
    fn replace_cards_soft_deletes_previous_set() {
        let mut conn = mem_conn();
        let mk = |title: &str| NewCard {
            start_ts: 1000,
            end_ts: 2000,
            day: "2026-07-03".into(),
            title: title.into(),
            summary: String::new(),
            category: "engineering".into(),
            subcategory: String::new(),
            detailed_summary: String::new(),
            metadata_json: None,
        };
        replace_cards_for_day(&mut conn, "2026-07-03", 1, &[mk("v1")]).unwrap();
        replace_cards_for_day(&mut conn, "2026-07-03", 2, &[mk("v2a"), mk("v2b")]).unwrap();
        let cards = list_cards_for_day(&conn, "2026-07-03").unwrap();
        assert_eq!(cards.len(), 2);
        assert!(cards.iter().all(|c| c.title.starts_with("v2")));
        // Soft-deleted v1 row still exists for manual recovery.
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM timeline_cards", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 3);
    }

    #[test]
    fn standup_upsert_overwrites_and_get_roundtrips() {
        let conn = mem_conn();
        assert_eq!(get_standup(&conn, "2026-07-03").unwrap(), None);
        upsert_standup(&conn, "2026-07-03", r#"{"highlights":["a"]}"#).unwrap();
        upsert_standup(&conn, "2026-07-03", r#"{"highlights":["b"]}"#).unwrap();
        let payload = get_standup(&conn, "2026-07-03").unwrap().unwrap();
        assert!(payload.contains("\"b\""));
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_standup_entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn observations_and_llm_calls_roundtrip() {
        let mut conn = mem_conn();
        let batch = create_batch(&mut conn, 0, 100, "processing", None, &[]).unwrap();
        insert_observation(&conn, batch, 10, 50, "Editing db.rs", "test-model").unwrap();
        insert_observation(&conn, batch, 50, 90, "Reading docs", "test-model").unwrap();
        let obs = list_observations_for_batch(&conn, batch).unwrap();
        assert_eq!(obs.len(), 2);
        assert_eq!(obs[0].observation, "Editing db.rs");
        log_llm_call(&conn, Some(batch), 1, "anthropic", "m", "transcription", "ok", 1234, None)
            .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM llm_calls WHERE batch_id = ?1", [batch], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn day_bounds_rejects_garbage() {
        assert!(day_bounds_local("not-a-day").is_err());
        assert!(day_bounds_local("2026-13-40").is_err());
        assert!(day_bounds_local("").is_err());
    }
}
