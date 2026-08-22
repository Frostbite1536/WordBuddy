//! Phase 4: daily standup generation (LLM) + weekly aggregation (pure math,
//! no LLM). Standup payloads persist in `daily_standup_entries`.

use serde::{Deserialize, Serialize};

use super::db;
use super::prompts;

const STANDUP_MAX_ATTEMPTS: usize = 3;

// ---------------------------------------------------------------------------
// Standup
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StandupPayload {
    #[serde(default)]
    pub highlights: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<String>,
    #[serde(default)]
    pub next: Vec<String>,
}

/// Parse + validate a standup LLM response. All four arrays must be
/// present-able (missing → empty), at least one must be non-empty, and
/// every bullet must be a reasonable one-liner.
pub fn parse_standup(raw: &str) -> Result<StandupPayload, String> {
    let payload: StandupPayload =
        serde_json::from_str(super::analyzer::extract_json(raw))
            .map_err(|e| format!("Output was not a valid standup JSON object: {e}"))?;
    let all = [
        &payload.highlights,
        &payload.tasks,
        &payload.blockers,
        &payload.next,
    ];
    if all.iter().all(|v| v.is_empty()) {
        return Err("All standup sections were empty".to_string());
    }
    for (name, list) in ["highlights", "tasks", "blockers", "next"].iter().zip(all) {
        if list.len() > 10 {
            return Err(format!("Section '{name}' has {} bullets; keep it to 2-5", list.len()));
        }
        for b in list {
            if b.trim().is_empty() {
                return Err(format!("Section '{name}' contains an empty bullet"));
            }
            if b.len() > 500 {
                return Err(format!("A bullet in '{name}' is over 500 characters; one line each"));
            }
        }
    }
    Ok(payload)
}

fn cards_as_text(cards: &[db::TimelineCardRow]) -> String {
    if cards.is_empty() {
        return "(no cards)".to_string();
    }
    cards
        .iter()
        .map(|c| {
            format!(
                "- [{} – {}] {} ({}): {}",
                super::db::day_of_ts(c.start_ts),
                super::db::day_of_ts(c.end_ts),
                c.title,
                c.category,
                if c.summary.is_empty() { &c.detailed_summary } else { &c.summary }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate (or regenerate) the standup for a day from that day's and the
/// previous day's cards; persist and return it.
#[tauri::command]
pub async fn journal_generate_standup(
    app: tauri::AppHandle,
    day: String,
) -> Result<StandupPayload, String> {
    // Validate the day early (also gives us the previous day string).
    db::day_bounds_local(&day)?;
    let prev_day = {
        use chrono::NaiveDate;
        NaiveDate::parse_from_str(&day, "%Y-%m-%d")
            .map_err(|e| format!("Invalid day: {e}"))?
            .pred_opt()
            .ok_or_else(|| "Day has no predecessor".to_string())?
            .format("%Y-%m-%d")
            .to_string()
    };

    let (today_cards, yesterday_cards) = {
        let conn = db::open(&app)?;
        (
            db::list_cards_for_day(&conn, &day)?,
            db::list_cards_for_day(&conn, &prev_day)?,
        )
    };
    if today_cards.is_empty() && yesterday_cards.is_empty() {
        return Err("No timeline cards for this day or the previous day yet".to_string());
    }

    let (provider, model) = super::analyzer::analysis_provider_and_model()?;
    let system = prompts::standup_system();
    let base_user = prompts::standup_user(
        &day,
        &cards_as_text(&yesterday_cards),
        &cards_as_text(&today_cards),
    );
    let mut user = base_user.clone();
    let mut last_err = String::new();

    for attempt in 1..=STANDUP_MAX_ATTEMPTS {
        let started = std::time::Instant::now();
        let result =
            crate::llm::complete_with_images(&app, &provider, &model, &system, &user, &[]).await;
        let latency = started.elapsed().as_millis() as i64;
        let provider_name = format!("{provider:?}").to_ascii_lowercase();
        let err = match result {
            Ok(raw) => match parse_standup(&raw) {
                Ok(payload) => {
                    let conn = db::open(&app)?;
                    let json = serde_json::to_string(&payload)
                        .map_err(|e| format!("Serialize failed: {e}"))?;
                    db::upsert_standup(&conn, &day, &json)?;
                    let _ = db::log_llm_call(
                        &conn, None, attempt as i64, &provider_name, &model, "standup", "ok",
                        latency, None,
                    );
                    return Ok(payload);
                }
                Err(e) => e,
            },
            Err(e) => e,
        };
        if let Ok(conn) = db::open(&app) {
            let _ = db::log_llm_call(
                &conn, None, attempt as i64, &provider_name, &model, "standup", "error", latency,
                Some(&err),
            );
        }
        last_err = err.clone();
        user = format!(
            "{base_user}\n\nYOUR PREVIOUS ATTEMPT WAS REJECTED: {err}\nReturn only the corrected JSON object."
        );
    }
    Err(format!(
        "Standup generation failed after {STANDUP_MAX_ATTEMPTS} attempts: {last_err}"
    ))
}

/// Saved standup for a day, if one was generated.
#[tauri::command]
pub async fn journal_get_standup(
    app: tauri::AppHandle,
    day: String,
) -> Result<Option<StandupPayload>, String> {
    tokio::task::spawn_blocking(move || {
        let conn = db::open(&app)?;
        match db::get_standup(&conn, &day)? {
            Some(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|e| format!("Stored standup is corrupt: {e}")),
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

// ---------------------------------------------------------------------------
// Weekly aggregation (pure math, no LLM)
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
pub struct DayAggregate {
    pub day: String,
    /// (category, minutes), descending minutes.
    pub category_minutes: Vec<(String, i64)>,
    pub total_minutes: i64,
    pub distraction_minutes: i64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct WeekSummary {
    pub days: Vec<DayAggregate>,
    pub total_minutes: i64,
    pub focus_minutes: i64,
    pub distraction_minutes: i64,
    /// (app/site, minutes) from card metadata primaries, descending, top 8.
    pub top_apps: Vec<(String, i64)>,
}

/// Aggregate one day's cards. Pure — unit-tested.
pub fn aggregate_day(day: &str, cards: &[db::TimelineCardRow]) -> DayAggregate {
    use std::collections::HashMap;
    let mut per_cat: HashMap<String, i64> = HashMap::new();
    let mut total = 0i64;
    let mut distraction = 0i64;
    for c in cards {
        let mins = ((c.end_ts - c.start_ts).max(0)) / 60;
        total += mins;
        if c.category == "distraction" {
            distraction += mins;
        }
        *per_cat.entry(c.category.clone()).or_insert(0) += mins;
    }
    let mut category_minutes: Vec<(String, i64)> = per_cat.into_iter().collect();
    category_minutes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    DayAggregate {
        day: day.to_string(),
        category_minutes,
        total_minutes: total,
        distraction_minutes: distraction,
    }
}

/// Aggregate a week's worth of per-day card lists. Pure — unit-tested.
pub fn aggregate_week(days: &[(String, Vec<db::TimelineCardRow>)]) -> WeekSummary {
    use std::collections::HashMap;
    let mut summary = WeekSummary::default();
    let mut apps: HashMap<String, i64> = HashMap::new();
    for (day, cards) in days {
        let agg = aggregate_day(day, cards);
        summary.total_minutes += agg.total_minutes;
        summary.distraction_minutes += agg.distraction_minutes;
        for c in cards {
            let mins = ((c.end_ts - c.start_ts).max(0)) / 60;
            if let Some(meta) = &c.metadata_json {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(meta) {
                    if let Some(primary) = v["app_sites"]["primary"].as_str() {
                        let p = primary.trim().to_ascii_lowercase();
                        if !p.is_empty() {
                            *apps.entry(p).or_insert(0) += mins;
                        }
                    }
                }
            }
        }
        summary.days.push(agg);
    }
    summary.focus_minutes = summary.total_minutes - summary.distraction_minutes;
    let mut top: Vec<(String, i64)> = apps.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top.truncate(8);
    summary.top_apps = top;
    summary
}

/// Week summary for the 7 days ending at `end_day` (inclusive).
#[tauri::command]
pub async fn journal_week_summary(
    app: tauri::AppHandle,
    end_day: String,
) -> Result<WeekSummary, String> {
    use chrono::NaiveDate;
    let end = NaiveDate::parse_from_str(&end_day, "%Y-%m-%d")
        .map_err(|e| format!("Invalid end_day '{end_day}': {e}"))?;
    tokio::task::spawn_blocking(move || {
        let conn = db::open(&app)?;
        let mut days: Vec<(String, Vec<db::TimelineCardRow>)> = Vec::with_capacity(7);
        for offset in (0..7).rev() {
            let d = end - chrono::Duration::days(offset);
            let ds = d.format("%Y-%m-%d").to_string();
            let cards = db::list_cards_for_day(&conn, &ds)?;
            days.push((ds, cards));
        }
        Ok(aggregate_week(&days))
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(
        start_min: i64,
        end_min: i64,
        category: &str,
        primary: Option<&str>,
    ) -> db::TimelineCardRow {
        db::TimelineCardRow {
            id: 0,
            batch_id: 0,
            start_ts: start_min * 60,
            end_ts: end_min * 60,
            day: "2026-07-03".into(),
            title: "t".into(),
            summary: String::new(),
            category: category.into(),
            subcategory: String::new(),
            detailed_summary: String::new(),
            metadata_json: primary
                .map(|p| format!(r#"{{"app_sites": {{"primary": "{p}"}}, "distractions": []}}"#)),
        }
    }

    // ── parse_standup ────────────────────────────────────────────

    #[test]
    fn standup_parses_valid_payload() {
        let raw = r#"{"highlights": ["Shipped the analyzer"], "tasks": ["Timeline UI"],
                      "blockers": [], "next": ["Weekly view"]}"#;
        let p = parse_standup(raw).unwrap();
        assert_eq!(p.highlights.len(), 1);
        assert!(p.blockers.is_empty());
    }

    #[test]
    fn standup_tolerates_fences_and_missing_sections() {
        let raw = "```json\n{\"highlights\": [\"a\"]}\n```";
        let p = parse_standup(raw).unwrap();
        assert_eq!(p.highlights, vec!["a"]);
        assert!(p.tasks.is_empty());
    }

    #[test]
    fn standup_rejects_empty_and_garbage() {
        assert!(parse_standup(r#"{"highlights": [], "tasks": []}"#).is_err());
        assert!(parse_standup("not json").is_err());
        assert!(parse_standup(r#"{"highlights": ["  "]}"#).is_err());
    }

    // ── aggregation ──────────────────────────────────────────────

    #[test]
    fn day_aggregate_sums_minutes_per_category() {
        let cards = vec![
            card(0, 30, "engineering", None),
            card(30, 90, "engineering", None),
            card(90, 100, "distraction", None),
        ];
        let agg = aggregate_day("2026-07-03", &cards);
        assert_eq!(agg.total_minutes, 100);
        assert_eq!(agg.distraction_minutes, 10);
        assert_eq!(agg.category_minutes[0], ("engineering".to_string(), 90));
        assert_eq!(agg.category_minutes[1], ("distraction".to_string(), 10));
    }

    #[test]
    fn week_aggregate_totals_focus_and_top_apps() {
        let days = vec![
            (
                "2026-07-02".to_string(),
                vec![card(0, 60, "engineering", Some("github.com"))],
            ),
            (
                "2026-07-03".to_string(),
                vec![
                    card(0, 30, "engineering", Some("github.com")),
                    card(30, 50, "distraction", Some("x.com")),
                ],
            ),
        ];
        let week = aggregate_week(&days);
        assert_eq!(week.total_minutes, 110);
        assert_eq!(week.distraction_minutes, 20);
        assert_eq!(week.focus_minutes, 90);
        assert_eq!(week.days.len(), 2);
        assert_eq!(week.top_apps[0], ("github.com".to_string(), 90));
        assert_eq!(week.top_apps[1], ("x.com".to_string(), 20));
    }

    #[test]
    fn week_aggregate_survives_missing_or_garbage_metadata() {
        let mut bad = card(0, 30, "engineering", None);
        bad.metadata_json = Some("not json".into());
        let days = vec![("2026-07-03".to_string(), vec![bad, card(30, 60, "admin", None)])];
        let week = aggregate_week(&days);
        assert_eq!(week.total_minutes, 60);
        assert!(week.top_apps.is_empty());
    }
}
