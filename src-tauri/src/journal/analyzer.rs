//! Two-stage journal analysis pipeline (Phase 2).
//!
//! Assembles unanalyzed screenshots into 15–30 min batches, samples ≤20
//! frames per batch, sends them as one multi-image LLM request (Stage 1 →
//! timestamped observations), then folds the observations into the day's
//! activity cards (Stage 2, merge-by-default, validated, ≤4 attempts with
//! error feedback). Every attempt is logged to `llm_calls`.
//!
//! All parsing/validation logic is pure and unit-tested with canned LLM
//! outputs — tests never make live API calls.

use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};

use super::db;
use super::db::ScreenshotRow;
use super::prompts;

/// Split a run of screenshots when consecutive shots are further apart
/// than this (the recorder was paused, idle-skipping, or the machine slept).
const GAP_SPLIT_SECS: i64 = 300;
/// A run is "closed" (safe to analyze its tail) once its newest shot is
/// this old — the user moved on and no more frames will join it.
const CLOSE_GRACE_SECS: i64 = 300;
/// Don't cut a batch from a still-open run until it spans this much.
const TARGET_MIN_SECS: i64 = 900;
/// Never let one batch span more than this.
const TARGET_MAX_SECS: i64 = 1800;
/// Closed fragments shorter than this are recorded as skipped_short.
const MIN_BATCH_SECS: i64 = 300;
/// Cap on images per LLM request.
const MAX_IMAGES_PER_REQUEST: usize = 20;
/// Keep a frame when this much time passed since the last kept frame
/// (roughly 1 frame / 45s → a 30-min batch samples ~40, thinned to 20).
const SAMPLE_EVERY_SECS: i64 = 45;

const STAGE1_MAX_ATTEMPTS: usize = 3;
const STAGE2_MAX_ATTEMPTS: usize = 4;

/// Guard so the 10-minute scheduler and `journal_analyze_now` never run
/// two pipelines concurrently (they would race on batch assembly).
static ANALYZING: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Batch assembly (pure)
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Disposition {
    Analyze,
    SkippedShort,
    SkippedIdle,
}

#[derive(Debug)]
pub struct AssembledBatch {
    pub screenshot_ids: Vec<i64>,
    pub start_ts: i64,
    pub end_ts: i64,
    pub disposition: Disposition,
}

/// Group unanalyzed shots into batches. `interval_secs` is the recorder
/// interval — a shot is "idle" when its idle_seconds exceeds it (the user
/// gave no input for the whole frame gap).
pub fn assemble_batches(
    shots: &[ScreenshotRow],
    now: i64,
    interval_secs: i64,
) -> Vec<AssembledBatch> {
    let mut out = Vec::new();
    if shots.is_empty() {
        return out;
    }

    // Split into runs on capture gaps.
    let mut runs: Vec<&[ScreenshotRow]> = Vec::new();
    let mut run_start = 0usize;
    for i in 1..shots.len() {
        if shots[i].captured_at - shots[i - 1].captured_at > GAP_SPLIT_SECS {
            runs.push(&shots[run_start..i]);
            run_start = i;
        }
    }
    runs.push(&shots[run_start..]);

    for run in runs {
        let closed = now - run.last().map(|s| s.captured_at).unwrap_or(now) >= CLOSE_GRACE_SECS;
        let mut i = 0usize;
        while i < run.len() {
            let chunk_start = run[i].captured_at;
            let mut j = i;
            while j < run.len() && run[j].captured_at - chunk_start < TARGET_MAX_SECS {
                j += 1;
            }
            let chunk = &run[i..j];
            let chunk_span = chunk.last().unwrap().captured_at - chunk_start;
            let is_tail = j == run.len();

            if is_tail && !closed && chunk_span < TARGET_MIN_SECS {
                // Still accumulating — leave it for a later pass.
                break;
            }

            let all_idle = chunk.iter().all(|s| s.idle_seconds > interval_secs);
            let disposition = if all_idle {
                Disposition::SkippedIdle
            } else if is_tail && closed && chunk_span < MIN_BATCH_SECS {
                Disposition::SkippedShort
            } else {
                Disposition::Analyze
            };
            out.push(AssembledBatch {
                screenshot_ids: chunk.iter().map(|s| s.id).collect(),
                start_ts: chunk_start,
                end_ts: chunk.last().unwrap().captured_at,
                disposition,
            });
            i = j;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Frame sampling (pure)
// ---------------------------------------------------------------------------

/// Pick the indices of frames to attach: every window-title change plus at
/// least one frame per ~45s, thinned evenly to `max_images` (first and
/// last always survive thinning).
pub fn sample_frame_indices(shots: &[ScreenshotRow], max_images: usize) -> Vec<usize> {
    if shots.is_empty() || max_images == 0 {
        return Vec::new();
    }
    let mut kept: Vec<usize> = vec![0];
    for i in 1..shots.len() {
        let last = *kept.last().unwrap();
        let title_changed = shots[i].window_title != shots[last].window_title;
        let due = shots[i].captured_at - shots[last].captured_at >= SAMPLE_EVERY_SECS;
        if title_changed || due {
            kept.push(i);
        }
    }
    let last_idx = shots.len() - 1;
    if *kept.last().unwrap() != last_idx {
        kept.push(last_idx);
    }
    if kept.len() <= max_images {
        return kept;
    }
    // Thin evenly, preserving the first and last kept frames.
    let mut thinned = Vec::with_capacity(max_images);
    let n = kept.len();
    for k in 0..max_images {
        let pos = (k as f64) * ((n - 1) as f64) / ((max_images - 1) as f64);
        let idx = kept[pos.round() as usize];
        if thinned.last() != Some(&idx) {
            thinned.push(idx);
        }
    }
    thinned
}

// ---------------------------------------------------------------------------
// LLM output parsing + validation (pure)
// ---------------------------------------------------------------------------

/// Strip markdown code fences some models wrap around JSON, and any
/// leading prose before the first bracket.
pub fn extract_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    let inner = if let Some(rest) = trimmed.strip_prefix("```json") {
        rest.strip_suffix("```").unwrap_or(rest)
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.strip_suffix("```").unwrap_or(rest)
    } else {
        trimmed
    };
    let inner = inner.trim();
    // Some models prepend a sentence; recover by slicing from the first
    // JSON bracket to the last one.
    if !(inner.starts_with('[') || inner.starts_with('{')) {
        if let (Some(start), Some(end)) = (inner.find('['), inner.rfind(']')) {
            if start < end {
                return &inner[start..=end];
            }
        }
    }
    inner
}

#[derive(Debug, Clone)]
pub struct ParsedObservation {
    pub start_ts: i64,
    pub end_ts: i64,
    pub text: String,
}

/// Parse + validate Stage 1 output. Offsets must chronologically cover a
/// plausible slice of `[0, span]`; anything else returns a descriptive
/// error that is fed back to the model on retry.
pub fn parse_stage1(raw: &str, batch_start: i64, batch_end: i64) -> Result<Vec<ParsedObservation>, String> {
    #[derive(Deserialize)]
    struct RawObs {
        start_offset_secs: i64,
        end_offset_secs: i64,
        observation: String,
    }
    let span = batch_end - batch_start;
    let parsed: Vec<RawObs> = serde_json::from_str(extract_json(raw))
        .map_err(|e| format!("Output was not a valid JSON array of observation objects: {e}"))?;
    if parsed.is_empty() {
        return Err("Output contained zero observation segments".to_string());
    }
    if parsed.len() > 10 {
        return Err(format!(
            "Output contained {} segments; the maximum is 8",
            parsed.len()
        ));
    }
    const TOLERANCE: i64 = 60;
    let mut obs: Vec<ParsedObservation> = Vec::with_capacity(parsed.len());
    for (i, o) in parsed.iter().enumerate() {
        if o.observation.trim().is_empty() {
            return Err(format!("Segment {} has an empty observation", i + 1));
        }
        if o.start_offset_secs < -TOLERANCE
            || o.end_offset_secs > span + TOLERANCE
            || o.start_offset_secs >= o.end_offset_secs
        {
            return Err(format!(
                "Segment {} has offsets {}..{} outside the valid span 0..{} (or non-increasing)",
                i + 1,
                o.start_offset_secs,
                o.end_offset_secs,
                span
            ));
        }
        obs.push(ParsedObservation {
            start_ts: batch_start + o.start_offset_secs.max(0),
            end_ts: batch_start + o.end_offset_secs.min(span),
            text: o.observation.trim().to_string(),
        });
    }
    obs.sort_by_key(|o| o.start_ts);
    Ok(obs)
}

#[derive(Debug, Clone)]
pub struct ParsedCard {
    pub start_ts: i64,
    pub end_ts: i64,
    pub title: String,
    pub summary: String,
    pub category: String,
    pub subcategory: String,
    pub detailed_summary: String,
    pub metadata_json: Option<String>,
}

/// Convert "HH:MM" (24h; "24:00" allowed as end-of-day) into unix seconds
/// anchored at the local midnight `day_start_ts`.
fn hhmm_to_ts(hhmm: &str, day_start_ts: i64) -> Result<i64, String> {
    let (h, m) = hhmm
        .trim()
        .split_once(':')
        .ok_or_else(|| format!("Time '{hhmm}' is not HH:MM"))?;
    let h: i64 = h.trim().parse().map_err(|_| format!("Bad hour in '{hhmm}'"))?;
    let m: i64 = m.trim().parse().map_err(|_| format!("Bad minute in '{hhmm}'"))?;
    if !(0..=24).contains(&h) || !(0..60).contains(&m) || (h == 24 && m != 0) {
        return Err(format!("Time '{hhmm}' out of range"));
    }
    Ok(day_start_ts + h * 3600 + m * 60)
}

/// Parse Stage 2 output into cards anchored on `day_start_ts`. Shape errors
/// and per-card field problems come back as retryable error text.
pub fn parse_stage2(raw: &str, day: &str, day_start_ts: i64) -> Result<Vec<ParsedCard>, String> {
    #[derive(Deserialize)]
    struct RawCard {
        start: String,
        end: String,
        title: String,
        #[serde(default)]
        summary: String,
        #[serde(default)]
        category: String,
        #[serde(default)]
        subcategory: String,
        #[serde(default)]
        detailed_summary: String,
        #[serde(default)]
        distractions: serde_json::Value,
        #[serde(default)]
        app_sites: serde_json::Value,
    }
    let parsed: Vec<RawCard> = serde_json::from_str(extract_json(raw))
        .map_err(|e| format!("Output was not a valid JSON array of card objects: {e}"))?;
    if parsed.is_empty() {
        return Err("Output contained zero cards".to_string());
    }
    let mut cards = Vec::with_capacity(parsed.len());
    for (i, c) in parsed.iter().enumerate() {
        let start_ts = hhmm_to_ts(&c.start, day_start_ts)
            .map_err(|e| format!("Card {}: {e}", i + 1))?;
        let end_ts = hhmm_to_ts(&c.end, day_start_ts)
            .map_err(|e| format!("Card {}: {e}", i + 1))?;
        if end_ts <= start_ts {
            return Err(format!(
                "Card {} ends ({}) at or before its start ({})",
                i + 1,
                c.end,
                c.start
            ));
        }
        if c.title.trim().is_empty() {
            return Err(format!("Card {} has an empty title", i + 1));
        }
        let category = {
            let cat = c.category.trim().to_ascii_lowercase();
            if prompts::CATEGORIES.contains(&cat.as_str()) {
                cat
            } else {
                "other".to_string()
            }
        };
        let metadata = serde_json::json!({
            "distractions": c.distractions,
            "app_sites": c.app_sites,
        });
        cards.push(ParsedCard {
            start_ts,
            end_ts,
            title: c.title.trim().to_string(),
            summary: c.summary.trim().to_string(),
            category,
            subcategory: c.subcategory.trim().to_string(),
            detailed_summary: c.detailed_summary.trim().to_string(),
            metadata_json: Some(metadata.to_string()),
        });
        let _ = day; // day is carried by the caller when persisting
    }
    cards.sort_by_key(|c| c.start_ts);
    Ok(cards)
}

/// Validate the revised card set against what it must cover.
///
/// Enforced (mirrors Dayflow's validation): no overlaps (>2 min), every
/// required span (previous cards + the new observations) still covered
/// within a 5-minute tolerance, and every card except the last ≥10 min.
/// The 60-minute maximum is prompt guidance, not a hard gate — merged
/// long-focus sessions legitimately exceed it.
pub fn validate_cards(
    cards: &[ParsedCard],
    required_spans: &[(i64, i64)],
) -> Result<(), String> {
    const OVERLAP_TOL: i64 = 120;
    const COVERAGE_TOL: i64 = 300;
    const MIN_CARD_SECS: i64 = 600;

    for w in cards.windows(2) {
        if w[1].start_ts < w[0].end_ts - OVERLAP_TOL {
            return Err(format!(
                "Cards '{}' and '{}' overlap; cards must not overlap",
                w[0].title, w[1].title
            ));
        }
    }
    for (i, c) in cards.iter().enumerate() {
        let is_last = i == cards.len() - 1;
        if !is_last && c.end_ts - c.start_ts < MIN_CARD_SECS {
            return Err(format!(
                "Card '{}' is only {} minutes long; every card except the last must be at least \
                 10 minutes — merge it into a neighboring card",
                c.title,
                (c.end_ts - c.start_ts) / 60
            ));
        }
    }
    for (s, e) in required_spans {
        let mut uncovered = e - s;
        for c in cards {
            let lo = c.start_ts.max(*s);
            let hi = c.end_ts.min(*e);
            if hi > lo {
                uncovered -= hi - lo;
            }
        }
        if uncovered > COVERAGE_TOL {
            return Err(format!(
                "The time range {} – {} from the inputs is not covered by the output cards \
                 ({} minutes missing); do not drop time segments",
                super::db::day_of_ts(*s),
                super::db::day_of_ts(*e),
                uncovered / 60
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

pub(crate) fn analysis_provider_and_model() -> Result<(crate::llm::Provider, String), String> {
    let (ap, am, cp, cm) = crate::config::with_config_pub(|c| {
        (
            c.analysis_provider.clone(),
            c.analysis_model.clone(),
            c.provider.clone(),
            c.model.clone(),
        )
    });
    let provider_str = if ap.trim().is_empty() { cp } else { ap };
    let provider = crate::llm::provider_from_str(&provider_str)
        .ok_or_else(|| format!("Unknown analysis provider '{provider_str}'"))?;
    let model = if am.trim().is_empty() {
        if cm.trim().is_empty() {
            crate::llm::default_model_for(&provider).to_string()
        } else {
            cm
        }
    } else {
        am
    };
    Ok((provider, model))
}

fn clock_of(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts, 0)
        .earliest()
        .map(|dt| dt.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

fn hhmm_of(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts, 0)
        .earliest()
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// Run Stage 1 for one batch: sample frames, call the LLM (with retries +
/// llm_calls logging), persist observations. Returns them for Stage 2.
async fn run_stage1(
    app: &tauri::AppHandle,
    batch_id: i64,
    shots: &[ScreenshotRow],
    provider: &crate::llm::Provider,
    model: &str,
) -> Result<Vec<ParsedObservation>, String> {
    let batch_start = shots.first().map(|s| s.captured_at).unwrap_or(0);
    let batch_end = shots.last().map(|s| s.captured_at).unwrap_or(batch_start);
    let indices = sample_frame_indices(shots, MAX_IMAGES_PER_REQUEST);

    // Read the sampled frames off the async thread. Missing files (user
    // deleted frames, retention raced us) are skipped, not fatal.
    let sampled: Vec<ScreenshotRow> = indices.iter().map(|&i| shots[i].clone()).collect();
    let loaded: Vec<(ScreenshotRow, String)> = tokio::task::spawn_blocking(move || {
        use base64::{engine::general_purpose::STANDARD, Engine};
        sampled
            .into_iter()
            .filter_map(|s| match std::fs::read(&s.file_path) {
                Ok(bytes) => Some((s, STANDARD.encode(bytes))),
                Err(_) => None,
            })
            .collect()
    })
    .await
    .map_err(|e| format!("Frame load task failed: {e}"))?;

    if loaded.is_empty() {
        return Err("No frame files readable for this batch".to_string());
    }

    let frames_meta: Vec<prompts::FrameMeta> = loaded
        .iter()
        .map(|(s, _)| prompts::FrameMeta {
            offset_secs: s.captured_at - batch_start,
            clock: clock_of(s.captured_at),
            window_title: s.window_title.clone(),
        })
        .collect();
    let images: Vec<String> = loaded.iter().map(|(_, b64)| b64.clone()).collect();

    let system = prompts::stage1_system();
    let base_user = prompts::stage1_user(&frames_meta, batch_end - batch_start);
    let mut user = base_user.clone();
    let mut last_err = String::new();

    for attempt in 1..=STAGE1_MAX_ATTEMPTS {
        let started = std::time::Instant::now();
        let result =
            crate::llm::complete_with_images(app, provider, model, &system, &user, &images).await;
        let latency = started.elapsed().as_millis() as i64;
        match result {
            Ok(raw) => match parse_stage1(&raw, batch_start, batch_end) {
                Ok(obs) => {
                    log_call(app, Some(batch_id), attempt as i64, provider, model, "transcription", "ok", latency, None);
                    let conn = db::open(app)?;
                    for o in &obs {
                        db::insert_observation(&conn, batch_id, o.start_ts, o.end_ts, &o.text, model)?;
                    }
                    return Ok(obs);
                }
                Err(e) => {
                    log_call(app, Some(batch_id), attempt as i64, provider, model, "transcription", "error", latency, Some(&e));
                    last_err = e.clone();
                    user = format!(
                        "{base_user}\n\nYOUR PREVIOUS ATTEMPT WAS REJECTED: {e}\nFix this and return only the corrected JSON array."
                    );
                }
            },
            Err(e) => {
                log_call(app, Some(batch_id), attempt as i64, provider, model, "transcription", "error", latency, Some(&e));
                last_err = e;
            }
        }
    }
    Err(format!(
        "Stage 1 failed after {STAGE1_MAX_ATTEMPTS} attempts: {last_err}"
    ))
}

/// Run Stage 2: fold this batch's observations into the day's cards.
async fn run_stage2(
    app: &tauri::AppHandle,
    batch_id: i64,
    observations: &[ParsedObservation],
    provider: &crate::llm::Provider,
    model: &str,
) -> Result<usize, String> {
    let batch_start = observations.first().map(|o| o.start_ts).unwrap_or(0);
    let day = db::day_of_ts(batch_start);
    let (day_start_ts, _) = db::day_bounds_local(&day)?;

    let existing = {
        let conn = db::open(app)?;
        db::list_cards_for_day(&conn, &day)?
    };
    let existing_json = serde_json::to_string_pretty(
        &existing
            .iter()
            .map(|c| {
                serde_json::json!({
                    "start": hhmm_of(c.start_ts),
                    "end": hhmm_of(c.end_ts),
                    "title": c.title,
                    "summary": c.summary,
                    "category": c.category,
                    "subcategory": c.subcategory,
                    "detailed_summary": c.detailed_summary,
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".to_string());

    let obs_text = observations
        .iter()
        .map(|o| format!("[{} - {}]: {}", hhmm_of(o.start_ts), hhmm_of(o.end_ts), o.text))
        .collect::<Vec<_>>()
        .join("\n");

    // What the revised set must still cover: every previous card's span
    // plus the span of the new observations.
    let mut required: Vec<(i64, i64)> = existing.iter().map(|c| (c.start_ts, c.end_ts)).collect();
    if let (Some(first), Some(last)) = (observations.first(), observations.last()) {
        required.push((first.start_ts, last.end_ts));
    }

    let system = prompts::stage2_system();
    let base_user = prompts::stage2_user(&existing_json, &obs_text, &day);
    let mut user = base_user.clone();
    let mut last_err = String::new();

    for attempt in 1..=STAGE2_MAX_ATTEMPTS {
        let started = std::time::Instant::now();
        let result =
            crate::llm::complete_with_images(app, provider, model, &system, &user, &[]).await;
        let latency = started.elapsed().as_millis() as i64;
        let err = match result {
            Ok(raw) => match parse_stage2(&raw, &day, day_start_ts) {
                Ok(cards) => match validate_cards(&cards, &required) {
                    Ok(()) => {
                        log_call(app, Some(batch_id), attempt as i64, provider, model, "activity_cards", "ok", latency, None);
                        let new_cards: Vec<db::NewCard> = cards
                            .iter()
                            .map(|c| db::NewCard {
                                start_ts: c.start_ts,
                                end_ts: c.end_ts,
                                day: day.clone(),
                                title: c.title.clone(),
                                summary: c.summary.clone(),
                                category: c.category.clone(),
                                subcategory: c.subcategory.clone(),
                                detailed_summary: c.detailed_summary.clone(),
                                metadata_json: c.metadata_json.clone(),
                            })
                            .collect();
                        let mut conn = db::open(app)?;
                        db::replace_cards_for_day(&mut conn, &day, batch_id, &new_cards)?;
                        return Ok(new_cards.len());
                    }
                    Err(e) => e,
                },
                Err(e) => e,
            },
            Err(e) => e,
        };
        log_call(app, Some(batch_id), attempt as i64, provider, model, "activity_cards", "error", latency, Some(&err));
        last_err = err.clone();
        user = format!(
            "{base_user}\n\nYOUR PREVIOUS ATTEMPT WAS REJECTED:\n{err}\n\nFix this and return only the corrected JSON array covering all required time ranges."
        );
    }
    Err(format!(
        "Stage 2 failed after {STAGE2_MAX_ATTEMPTS} attempts: {last_err}"
    ))
}

#[allow(clippy::too_many_arguments)]
fn log_call(
    app: &tauri::AppHandle,
    batch_id: Option<i64>,
    attempt: i64,
    provider: &crate::llm::Provider,
    model: &str,
    operation: &str,
    status: &str,
    latency_ms: i64,
    error: Option<&str>,
) {
    let provider_name = format!("{provider:?}").to_ascii_lowercase();
    if let Ok(conn) = db::open(app) {
        let _ = db::log_llm_call(
            &conn, batch_id, attempt, &provider_name, model, operation, status, latency_ms, error,
        );
    }
}

#[derive(serde::Serialize, Debug, Default)]
pub struct AnalyzeSummary {
    pub batches_created: usize,
    pub batches_analyzed: usize,
    pub batches_skipped: usize,
    pub batches_failed: usize,
    pub cards_written: usize,
}

/// Assemble pending batches and analyze each Analyze-disposition batch.
pub async fn analyze_pending(app: &tauri::AppHandle) -> Result<AnalyzeSummary, String> {
    if ANALYZING.swap(true, Ordering::SeqCst) {
        return Err("Analysis already running".to_string());
    }
    let result = analyze_pending_inner(app).await;
    ANALYZING.store(false, Ordering::SeqCst);
    result
}

async fn analyze_pending_inner(app: &tauri::AppHandle) -> Result<AnalyzeSummary, String> {
    let mut summary = AnalyzeSummary::default();
    let interval = crate::config::with_config_pub(|c| c.recorder_interval_secs) as i64;

    // Assemble: read unbatched shots, group, create batch rows.
    let assembled = {
        let conn = db::open(app)?;
        let shots = db::list_unbatched_screenshots(&conn)?;
        assemble_batches(&shots, db::now_secs(), interval)
    };
    let mut to_analyze: Vec<i64> = Vec::new();
    {
        let mut conn = db::open(app)?;
        for b in &assembled {
            let (status, reason) = match b.disposition {
                Disposition::Analyze => ("pending", None),
                Disposition::SkippedShort => ("skipped_short", Some("fragment under 5 minutes")),
                Disposition::SkippedIdle => ("skipped_idle", Some("all frames idle")),
            };
            let id = db::create_batch(&mut conn, b.start_ts, b.end_ts, status, reason, &b.screenshot_ids)?;
            summary.batches_created += 1;
            match b.disposition {
                Disposition::Analyze => to_analyze.push(id),
                _ => summary.batches_skipped += 1,
            }
        }
        // Also pick up batches left pending/failed-transient from earlier
        // sessions (app quit mid-analysis).
        let mut stmt = conn
            .prepare("SELECT id FROM analysis_batches WHERE status = 'pending' ORDER BY batch_start_ts ASC")
            .map_err(|e| format!("Query prepare failed: {e}"))?;
        let pending: Vec<i64> = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| format!("Query failed: {e}"))?
            .filter_map(|r| r.ok())
            .collect();
        for id in pending {
            if !to_analyze.contains(&id) {
                to_analyze.push(id);
            }
        }
    }

    if to_analyze.is_empty() {
        return Ok(summary);
    }
    let (provider, model) = analysis_provider_and_model()?;
    log::info!(
        "[analyzer] analyzing {} batch(es) with {:?}/{}",
        to_analyze.len(),
        provider,
        model
    );

    for batch_id in to_analyze {
        let shots = {
            let conn = db::open(app)?;
            db::set_batch_status(&conn, batch_id, "processing", None)?;
            db::batch_screenshots(&conn, batch_id)?
        };
        if shots.is_empty() {
            let conn = db::open(app)?;
            db::set_batch_status(&conn, batch_id, "failed", Some("no screenshots"))?;
            summary.batches_failed += 1;
            continue;
        }
        let outcome = async {
            let obs = run_stage1(app, batch_id, &shots, &provider, &model).await?;
            run_stage2(app, batch_id, &obs, &provider, &model).await
        }
        .await;
        let conn = db::open(app)?;
        match outcome {
            Ok(cards) => {
                db::set_batch_status(&conn, batch_id, "done", None)?;
                summary.batches_analyzed += 1;
                summary.cards_written += cards;
            }
            Err(e) => {
                log::warn!("[analyzer] batch {batch_id} failed: {e}");
                db::set_batch_status(&conn, batch_id, "failed", Some(&e))?;
                summary.batches_failed += 1;
            }
        }
    }
    Ok(summary)
}

/// Background scheduler: analyze pending batches every 10 minutes while
/// the recorder is running. Manual analysis is always available via
/// `journal_analyze_now`.
pub fn start_scheduler(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            let recorder_on = crate::config::with_config_pub(|c| c.recorder_enabled);
            if !recorder_on {
                continue;
            }
            match analyze_pending(&app).await {
                Ok(s) if s.batches_created + s.batches_analyzed > 0 => {
                    log::info!("[analyzer] scheduled pass: {s:?}");
                }
                Ok(_) => {}
                Err(e) if e == "Analysis already running" => {}
                Err(e) => log::warn!("[analyzer] scheduled pass failed: {e}"),
            }
        }
    });
}

#[tauri::command]
pub async fn journal_analyze_now(app: tauri::AppHandle) -> Result<AnalyzeSummary, String> {
    analyze_pending(&app).await
}

/// Timeline cards for a local day — the journal page's main query.
#[tauri::command]
pub async fn journal_list_cards(
    app: tauri::AppHandle,
    day: String,
) -> Result<Vec<db::TimelineCardRow>, String> {
    tokio::task::spawn_blocking(move || {
        let conn = db::open(&app)?;
        db::list_cards_for_day(&conn, &day)
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

/// Observations for a batch — shown in the card detail expansion.
#[tauri::command]
pub async fn journal_list_observations(
    app: tauri::AppHandle,
    batch_id: i64,
) -> Result<Vec<db::ObservationRow>, String> {
    tokio::task::spawn_blocking(move || {
        let conn = db::open(&app)?;
        db::list_observations_for_batch(&conn, batch_id)
    })
    .await
    .map_err(|e| format!("Task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(id: i64, at: i64, idle: i64, title: &str) -> ScreenshotRow {
        ScreenshotRow {
            id,
            captured_at: at,
            file_path: format!("{id}.jpg"),
            file_size: 1,
            idle_seconds: idle,
            window_title: title.to_string(),
        }
    }

    /// A run of shots every `step` seconds starting at `t0`.
    fn run_of(t0: i64, count: i64, step: i64) -> Vec<ScreenshotRow> {
        (0..count).map(|i| shot(i + 1, t0 + i * step, 0, "App")).collect()
    }

    // ── assemble_batches ─────────────────────────────────────────

    #[test]
    fn assembler_leaves_open_short_run_alone() {
        // 10 minutes of shots, newest one is fresh → still accumulating.
        let shots = run_of(1000, 60, 10);
        let now = shots.last().unwrap().captured_at + 30;
        let batches = assemble_batches(&shots, now, 10);
        assert!(batches.is_empty());
    }

    #[test]
    fn assembler_cuts_15min_from_open_run() {
        // 20 minutes of shots, still open → one batch (span >= 15 min).
        let shots = run_of(1000, 120, 10);
        let now = shots.last().unwrap().captured_at + 30;
        let batches = assemble_batches(&shots, now, 10);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].disposition, Disposition::Analyze);
        assert_eq!(batches[0].screenshot_ids.len(), 120);
    }

    #[test]
    fn assembler_splits_long_run_at_30min() {
        // 50 minutes closed → 30-min batch + 20-min batch.
        let shots = run_of(1000, 300, 10);
        let now = shots.last().unwrap().captured_at + 600;
        let batches = assemble_batches(&shots, now, 10);
        assert_eq!(batches.len(), 2);
        assert!(batches[0].end_ts - batches[0].start_ts < 1800);
        assert_eq!(batches[1].disposition, Disposition::Analyze);
        let total: usize = batches.iter().map(|b| b.screenshot_ids.len()).sum();
        assert_eq!(total, 300);
    }

    #[test]
    fn assembler_splits_on_gap_and_marks_short_fragment() {
        // 16 min of work, a 20-min gap, then 3 min more (closed).
        let mut shots = run_of(1000, 96, 10); // 16 min
        let t2 = shots.last().unwrap().captured_at + 1200;
        let mut tail = run_of(t2, 18, 10); // 3 min
        for (i, s) in tail.iter_mut().enumerate() {
            s.id = 200 + i as i64;
        }
        shots.extend(tail);
        let now = shots.last().unwrap().captured_at + 600;
        let batches = assemble_batches(&shots, now, 10);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].disposition, Disposition::Analyze);
        assert_eq!(batches[1].disposition, Disposition::SkippedShort);
    }

    #[test]
    fn assembler_marks_fully_idle_batch() {
        let shots: Vec<ScreenshotRow> =
            (0..120).map(|i| shot(i + 1, 1000 + i * 10, 170, "App")).collect();
        let now = shots.last().unwrap().captured_at + 600;
        let batches = assemble_batches(&shots, now, 10);
        assert!(!batches.is_empty());
        assert!(batches.iter().all(|b| b.disposition == Disposition::SkippedIdle));
    }

    // ── sample_frame_indices ─────────────────────────────────────

    #[test]
    fn sampler_takes_one_per_45s_and_the_ends() {
        let shots = run_of(0, 90, 10); // 15 min, every 10s
        let idx = sample_frame_indices(&shots, 20);
        assert!(idx.len() <= 20);
        assert_eq!(*idx.first().unwrap(), 0);
        assert_eq!(*idx.last().unwrap(), 89);
        // Roughly 1 per 45s → ~20 candidates for 890s.
        assert!(idx.len() >= 10, "got {}", idx.len());
    }

    #[test]
    fn sampler_prefers_title_changes() {
        let mut shots = run_of(0, 10, 10); // 100s — below the 45s cadence alone
        shots[3].window_title = "Browser".into();
        shots[4].window_title = "Browser".into();
        shots[5].window_title = "App".into();
        let idx = sample_frame_indices(&shots, 20);
        assert!(idx.contains(&3), "title change to Browser must be kept: {idx:?}");
        assert!(idx.contains(&5), "title change back to App must be kept: {idx:?}");
    }

    #[test]
    fn sampler_caps_at_max_images() {
        let shots = run_of(0, 360, 10); // 60 min
        let idx = sample_frame_indices(&shots, 20);
        assert!(idx.len() <= 20);
        assert_eq!(*idx.first().unwrap(), 0);
        assert_eq!(*idx.last().unwrap(), 359);
        // Strictly increasing (no duplicates).
        assert!(idx.windows(2).all(|w| w[0] < w[1]));
    }

    // ── extract_json ─────────────────────────────────────────────

    #[test]
    fn extract_json_strips_fences_and_prose() {
        assert_eq!(extract_json("```json\n[1]\n```"), "[1]");
        assert_eq!(extract_json("```\n[1]\n```"), "[1]");
        assert_eq!(extract_json("[1]"), "[1]");
        assert_eq!(extract_json("Here you go:\n[1, 2]"), "[1, 2]");
    }

    // ── parse_stage1 ─────────────────────────────────────────────

    #[test]
    fn stage1_parses_and_anchors_offsets() {
        let raw = r#"[
            {"start_offset_secs": 0, "end_offset_secs": 400, "observation": "Editing db.rs in VS Code"},
            {"start_offset_secs": 400, "end_offset_secs": 900, "observation": "Reading rusqlite docs in browser"}
        ]"#;
        let obs = parse_stage1(raw, 10_000, 10_900).unwrap();
        assert_eq!(obs.len(), 2);
        assert_eq!(obs[0].start_ts, 10_000);
        assert_eq!(obs[0].end_ts, 10_400);
        assert_eq!(obs[1].end_ts, 10_900);
    }

    #[test]
    fn stage1_rejects_out_of_span_and_empty() {
        let out_of_span = r#"[{"start_offset_secs": 0, "end_offset_secs": 5000, "observation": "x"}]"#;
        assert!(parse_stage1(out_of_span, 0, 900).is_err());
        let empty_text = r#"[{"start_offset_secs": 0, "end_offset_secs": 100, "observation": "  "}]"#;
        assert!(parse_stage1(empty_text, 0, 900).is_err());
        assert!(parse_stage1("[]", 0, 900).is_err());
        assert!(parse_stage1("not json", 0, 900).is_err());
    }

    // ── parse_stage2 + validate_cards ────────────────────────────

    const DAY_START: i64 = 1_700_000_000;

    fn card_json(start: &str, end: &str, title: &str) -> String {
        format!(
            r#"{{"start": "{start}", "end": "{end}", "title": "{title}", "summary": "s",
                "category": "engineering", "subcategory": "", "detailed_summary": "d",
                "distractions": [], "app_sites": {{"primary": "github.com"}}}}"#
        )
    }

    #[test]
    fn stage2_parses_times_against_day_start() {
        let raw = format!("[{}]", card_json("14:00", "14:30", "Built the analyzer"));
        let cards = parse_stage2(&raw, "2026-07-03", DAY_START).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].start_ts, DAY_START + 14 * 3600);
        assert_eq!(cards[0].end_ts, DAY_START + 14 * 3600 + 1800);
        assert_eq!(cards[0].category, "engineering");
        assert!(cards[0].metadata_json.as_deref().unwrap().contains("github.com"));
    }

    #[test]
    fn stage2_maps_unknown_category_to_other() {
        let raw = format!(
            "[{}]",
            card_json("09:00", "09:20", "x").replace("engineering", "yak-shaving")
        );
        let cards = parse_stage2(&raw, "2026-07-03", DAY_START).unwrap();
        assert_eq!(cards[0].category, "other");
    }

    #[test]
    fn stage2_rejects_backwards_or_garbage_times() {
        let backwards = format!("[{}]", card_json("15:00", "14:00", "x"));
        assert!(parse_stage2(&backwards, "2026-07-03", DAY_START).is_err());
        let garbage = format!("[{}]", card_json("25:99", "26:00", "x"));
        assert!(parse_stage2(&garbage, "2026-07-03", DAY_START).is_err());
    }

    fn pc(start_min: i64, end_min: i64, title: &str) -> ParsedCard {
        ParsedCard {
            start_ts: DAY_START + start_min * 60,
            end_ts: DAY_START + end_min * 60,
            title: title.into(),
            summary: String::new(),
            category: "engineering".into(),
            subcategory: String::new(),
            detailed_summary: String::new(),
            metadata_json: None,
        }
    }

    #[test]
    fn validate_accepts_contiguous_covering_cards() {
        let cards = vec![pc(840, 870, "a"), pc(870, 900, "b")];
        let required = vec![(DAY_START + 840 * 60, DAY_START + 900 * 60)];
        assert!(validate_cards(&cards, &required).is_ok());
    }

    #[test]
    fn validate_rejects_overlap() {
        let cards = vec![pc(840, 880, "a"), pc(860, 900, "b")];
        assert!(validate_cards(&cards, &[]).is_err());
    }

    #[test]
    fn validate_rejects_dropped_coverage() {
        // Required span 14:00-15:00 but cards only cover the first half.
        let cards = vec![pc(840, 870, "a")];
        let required = vec![(DAY_START + 840 * 60, DAY_START + 900 * 60)];
        let err = validate_cards(&cards, &required).unwrap_err();
        assert!(err.contains("not covered"));
    }

    #[test]
    fn validate_rejects_short_middle_card_but_allows_short_last() {
        let short_middle = vec![pc(840, 845, "tiny"), pc(845, 900, "b")];
        assert!(validate_cards(&short_middle, &[]).is_err());
        let short_last = vec![pc(840, 880, "a"), pc(880, 885, "tail")];
        assert!(validate_cards(&short_last, &[]).is_ok());
    }

    #[test]
    fn validate_allows_gap_between_cards() {
        // A real break between cards is fine when nothing requires coverage there.
        let cards = vec![pc(600, 660, "morning"), pc(840, 900, "afternoon")];
        let required = vec![
            (DAY_START + 600 * 60, DAY_START + 660 * 60),
            (DAY_START + 840 * 60, DAY_START + 900 * 60),
        ];
        assert!(validate_cards(&cards, &required).is_ok());
    }
}
