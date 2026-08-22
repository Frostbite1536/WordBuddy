//! Markdown export of timeline cards (Phase 3). Shape follows Dayflow's
//! TimelineClipboardFormatter (numbered bold entries with Summary/Details
//! bullets) but is rendered fresh here from our own card rows.

use super::db;

fn hhmm_of(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts, 0)
        .earliest()
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// Render one day's cards as a markdown section. Pure — unit-testable.
pub fn render_day_markdown(day: &str, cards: &[db::TimelineCardRow]) -> String {
    let mut out = format!("## WorkBuddy journal · {day}\n");
    if cards.is_empty() {
        out.push_str("\n_No activity recorded for this day._\n");
        return out;
    }
    for (i, c) in cards.iter().enumerate() {
        out.push('\n');
        out.push_str(&format!(
            "{}. **{} – {} — {}**\n",
            i + 1,
            hhmm_of(c.start_ts),
            hhmm_of(c.end_ts),
            c.title.trim()
        ));
        let mut meta = c.category.trim().to_string();
        if !c.subcategory.trim().is_empty() {
            meta.push_str(&format!(" • {}", c.subcategory.trim()));
        }
        if !meta.is_empty() {
            out.push_str(&format!("   - _{meta}_\n"));
        }
        if !c.summary.trim().is_empty() {
            out.push_str(&format!("   - Summary: {}\n", c.summary.trim()));
        }
        if !c.detailed_summary.trim().is_empty() && c.detailed_summary.trim() != c.summary.trim() {
            out.push_str(&format!("   - Details: {}\n", c.detailed_summary.trim()));
        }
    }
    out
}

/// All local days in `[from_day, to_day]` inclusive. Bounded to 62 days so
/// a swapped/garbage range can't spin forever.
fn days_between(from_day: &str, to_day: &str) -> Result<Vec<String>, String> {
    use chrono::NaiveDate;
    let from = NaiveDate::parse_from_str(from_day, "%Y-%m-%d")
        .map_err(|e| format!("Invalid from_day '{from_day}': {e}"))?;
    let to = NaiveDate::parse_from_str(to_day, "%Y-%m-%d")
        .map_err(|e| format!("Invalid to_day '{to_day}': {e}"))?;
    if to < from {
        return Err("to_day is before from_day".to_string());
    }
    let mut days = Vec::new();
    let mut d = from;
    while d <= to {
        days.push(d.format("%Y-%m-%d").to_string());
        if days.len() > 62 {
            return Err("Export range exceeds 62 days".to_string());
        }
        d = d.succ_opt().ok_or_else(|| "Date overflow".to_string())?;
    }
    Ok(days)
}

/// Export the cards for a day range as one markdown document. Days with no
/// cards are omitted (except when the whole range is empty, which yields a
/// single "no activity" section for the range's first day).
#[tauri::command]
pub async fn journal_export_markdown(
    app: tauri::AppHandle,
    from_day: String,
    to_day: String,
) -> Result<String, String> {
    let days = days_between(&from_day, &to_day)?;
    tokio::task::spawn_blocking(move || {
        let conn = db::open(&app)?;
        let mut sections: Vec<String> = Vec::new();
        for day in &days {
            let cards = db::list_cards_for_day(&conn, day)?;
            if !cards.is_empty() {
                sections.push(render_day_markdown(day, &cards));
            }
        }
        if sections.is_empty() {
            return Ok(render_day_markdown(&days[0], &[]));
        }
        Ok(sections.join("\n"))
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(start_ts: i64, end_ts: i64, title: &str, summary: &str) -> db::TimelineCardRow {
        db::TimelineCardRow {
            id: 1,
            batch_id: 1,
            start_ts,
            end_ts,
            day: "2026-07-03".into(),
            title: title.into(),
            summary: summary.into(),
            category: "engineering".into(),
            subcategory: "rust".into(),
            detailed_summary: "Wrote the analyzer module and its tests.".into(),
            metadata_json: None,
        }
    }

    #[test]
    fn render_empty_day() {
        let md = render_day_markdown("2026-07-03", &[]);
        assert!(md.contains("2026-07-03"));
        assert!(md.contains("No activity recorded"));
    }

    #[test]
    fn render_numbers_cards_with_meta_and_details() {
        let cards = vec![
            card(1_700_000_000, 1_700_001_800, "Built analyzer", "Wrote batch assembly."),
            card(1_700_001_800, 1_700_003_600, "Code review", "Reviewed PR #12."),
        ];
        let md = render_day_markdown("2026-07-03", &cards);
        assert!(md.starts_with("## WorkBuddy journal · 2026-07-03"));
        assert!(md.contains("1. **"));
        assert!(md.contains("2. **"));
        assert!(md.contains("Built analyzer"));
        assert!(md.contains("_engineering • rust_"));
        assert!(md.contains("Summary: Wrote batch assembly."));
        assert!(md.contains("Details: Wrote the analyzer module"));
    }

    #[test]
    fn days_between_inclusive_and_validated() {
        let days = days_between("2026-07-01", "2026-07-03").unwrap();
        assert_eq!(days, vec!["2026-07-01", "2026-07-02", "2026-07-03"]);
        assert_eq!(days_between("2026-07-03", "2026-07-03").unwrap().len(), 1);
        assert!(days_between("2026-07-03", "2026-07-01").is_err());
        assert!(days_between("garbage", "2026-07-01").is_err());
        assert!(days_between("2026-01-01", "2026-12-31").is_err()); // > 62 days
    }
}
