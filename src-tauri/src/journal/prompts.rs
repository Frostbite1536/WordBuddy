//! Prompt builders for the two-stage journal analysis pipeline.
//!
//! Stage 1 (transcription): sampled frames + timestamps + window titles →
//! 3–8 timestamped observation segments.
//! Stage 2 (activity cards): observations (+ the day's previous cards) →
//! merged timeline cards.
//!
//! The structure follows Dayflow's audited pipeline (MIT), but the text is
//! written fresh for WorkBuddy, and — unlike Dayflow's compressed-video
//! timeline — we have REAL per-frame timestamps, so Stage 1 works in plain
//! seconds-from-batch-start offsets with no compression math.

/// Fixed starter category set (user-editable later).
pub const CATEGORIES: &[&str] = &[
    "engineering",
    "design",
    "communication",
    "research",
    "admin",
    "distraction",
    "other",
];

/// Metadata for one sampled frame, in the order the images are attached.
pub struct FrameMeta {
    /// Seconds after the batch start this frame was captured.
    pub offset_secs: i64,
    /// Local wall-clock (HH:MM:SS) for readability.
    pub clock: String,
    /// Foreground window title at capture time (may be empty).
    pub window_title: String,
}

pub fn stage1_system() -> String {
    "You are the transcription stage of a private, local work journal. You \
     receive screenshots sampled from a short span of one person's screen \
     activity and produce a factual activity log. You only describe what is \
     visibly on screen — never guess app names you cannot read, never invent \
     URLs, file names, or numbers. Output pure JSON with no markdown fences \
     and no commentary."
        .to_string()
}

pub fn stage1_user(frames: &[FrameMeta], span_secs: i64) -> String {
    let mut frame_lines = String::new();
    for (i, f) in frames.iter().enumerate() {
        frame_lines.push_str(&format!(
            "Frame {} — at +{}s ({}) — foreground window: {}\n",
            i + 1,
            f.offset_secs,
            f.clock,
            if f.window_title.is_empty() {
                "(unknown)"
            } else {
                &f.window_title
            }
        ));
    }
    format!(
        "The attached images are frames sampled from {span_secs} seconds of screen \
activity, in chronological order. Frame metadata (offsets are seconds from the \
start of the span):

{frame_lines}
Write an activity log detailed enough that the person could reconstruct the \
session tomorrow. For each segment ask: what EXACTLY were they doing? What \
specific file names, URLs, page titles, people, or numbers are visible?

Bad: \"Checked email\"
Good: \"Gmail: read 'RE: Q3 budget' from dana@, replied 'looks good'\"
Bad: \"Working on code\"
Good: \"Editing recorder.rs in VS Code — writing an idle-detection function; \
terminal shows cargo test passing\"

Rules:
- 3 to 8 segments covering the FULL span 0..{span_secs}s — no gaps, no overlaps, \
chronological.
- Use 1 segment only if the activity never changes.
- Group by goal, not by app (IDE + terminal + browser for one task = one segment).
- Use the window titles above to identify apps; if unreadable, say \
\"a code editor\" / \"a browser\" rather than guessing a product name.

Return ONLY a JSON array in exactly this shape:
[
  {{ \"start_offset_secs\": 0, \"end_offset_secs\": 300, \"observation\": \"1-3 sentences with specifics\" }}
]"
    )
}

pub fn stage2_system() -> String {
    "You are writing someone's personal work journal. You receive raw \
     timestamped observations of their screen activity plus the timeline \
     cards already written for the same day, and you return the REVISED \
     full set of cards. Write like the person jotting notes about their own \
     day — concrete and recognizable, not like a status report. Output pure \
     JSON with no markdown fences and no commentary."
        .to_string()
}

pub fn stage2_user(existing_cards_json: &str, observations_text: &str, day: &str) -> String {
    let categories = CATEGORIES.join(", ");
    format!(
        "Day being journaled: {day} (times below are local, 24-hour HH:MM).

PREVIOUS CARDS for this day (a draft you are revising, not locked history):
{existing_cards_json}

NEW OBSERVATIONS:
{observations_text}

Produce the full revised card set for the day covering the previous cards' \
time range PLUS the new observations.

Card rules:
- Each card is one cohesive chunk of activity, 10–60 minutes.
- DEFAULT TO MERGING. If the previous card's core activity continues in the \
new observations (same feature, same document, same thread), extend that card \
instead of adding a new one. Two adjacent 15-minute cards about the same work \
stream should almost never exist. When unsure, merge.
- A brief (<5 min) unrelated detour is a \"distraction\" entry inside the \
card, not a new card. Googling an error while debugging is NOT a distraction \
— it is part of the work.
- Start a new card only when the core intent changed for 10+ minutes.
- No overlaps. Preserve genuine gaps in the source data (breaks, lock screen) \
as gaps between cards — do not paper over them.
- The last card may be shorter than 10 minutes if the observations end there.
- title: specific enough to trigger \"oh right, that\" tomorrow morning.
- summary: 1–2 sentences, past tense, concrete.
- detailed_summary: 2–5 sentences with the specifics (files, PRs, URLs, people).
- category: exactly one of [{categories}]. subcategory: short free text or \"\".

Return ONLY a JSON array in exactly this shape:
[
  {{
    \"start\": \"14:00\",
    \"end\": \"14:35\",
    \"title\": \"\",
    \"summary\": \"\",
    \"category\": \"engineering\",
    \"subcategory\": \"\",
    \"detailed_summary\": \"\",
    \"distractions\": [ {{ \"start\": \"14:10\", \"end\": \"14:13\", \"title\": \"\", \"summary\": \"\" }} ],
    \"app_sites\": {{ \"primary\": \"\", \"secondary\": \"\" }}
  }}
]"
    )
}

pub fn standup_system() -> String {
    "You write concise daily-standup notes from a person's private work \
     journal. You receive their timeline cards for yesterday and today and \
     distill them into what a teammate would actually want to hear. Be \
     specific (name the features, PRs, documents), never invent work that \
     is not in the cards, and keep each bullet to one line. Output pure \
     JSON with no markdown fences and no commentary."
        .to_string()
}

pub fn standup_user(day: &str, prev_day_cards: &str, day_cards: &str) -> String {
    format!(
        "Standup for {day}.

YESTERDAY'S TIMELINE CARDS:
{prev_day_cards}

TODAY'S TIMELINE CARDS (may be partial — the day might not be over):
{day_cards}

Return ONLY a JSON object in exactly this shape (arrays may be empty, 2-5 \
bullets each, one line per bullet):
{{
  \"highlights\": [\"what was accomplished, most important first\"],
  \"tasks\": [\"concrete work items in progress or done today\"],
  \"blockers\": [\"things that stalled progress; [] if none\"],
  \"next\": [\"what's queued up next, judging from the cards\"]
}}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standup_user_embeds_both_days() {
        let p = standup_user("2026-07-03", "[yesterday cards]", "[today cards]");
        assert!(p.contains("2026-07-03"));
        assert!(p.contains("[yesterday cards]"));
        assert!(p.contains("[today cards]"));
        assert!(p.contains("\"highlights\""));
        assert!(p.contains("\"blockers\""));
    }

    #[test]
    fn stage1_user_lists_every_frame_and_span() {
        let frames = vec![
            FrameMeta { offset_secs: 0, clock: "14:00:00".into(), window_title: "VS Code".into() },
            FrameMeta { offset_secs: 45, clock: "14:00:45".into(), window_title: String::new() },
        ];
        let p = stage1_user(&frames, 900);
        assert!(p.contains("Frame 1"));
        assert!(p.contains("Frame 2"));
        assert!(p.contains("VS Code"));
        assert!(p.contains("(unknown)"));
        assert!(p.contains("0..900s"));
        assert!(p.contains("start_offset_secs"));
    }

    #[test]
    fn stage2_user_embeds_inputs_and_categories() {
        let p = stage2_user("[]", "[14:00 - 14:10]: wrote tests", "2026-07-03");
        assert!(p.contains("2026-07-03"));
        assert!(p.contains("wrote tests"));
        assert!(p.contains("engineering, design, communication"));
        assert!(p.contains("DEFAULT TO MERGING"));
    }
}
