//! Weekly report generation (PLAN-05 task 3): pure payload assembly +
//! markdown rendering, unit-tested; the LLM tone pass is optional
//! (snippet retention OFF by default) and degrades to a documented
//! placeholder line.

use serde::Serialize;

use super::aggregate::DayStat;

#[derive(Debug, Clone, Serialize)]
pub struct WeekPayload {
    pub week_start: String,
    pub days: Vec<DayStat>,
    pub words: u32,
    pub checks: u32,
    /// Words this week vs the prior week (may be negative).
    pub words_delta_vs_prior: i64,
    pub accuracy: f64,
    pub streak: u32,
    pub vocab_unique: u32,
    pub vocab_rare_pct: f64,
    pub top_errors: Vec<(String, u32)>,
    /// Tone distribution from the LLM pass when snippet retention is ON.
    pub tone: Option<ToneDistribution>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToneDistribution {
    pub formal: f64,
    pub neutral: f64,
    pub friendly: f64,
    pub confident: f64,
}

/// Render a weekly report as markdown. Pure.
pub fn render_markdown(p: &WeekPayload) -> String {
    let mut md = String::new();
    md.push_str(&format!(
        "# WordBuddy Weekly Report — week of {}\n\n",
        p.week_start
    ));
    md.push_str(&format!(
        "- **Words checked:** {} ({} vs prior week)\n",
        p.words,
        if p.words_delta_vs_prior >= 0 {
            format!("+{}", p.words_delta_vs_prior)
        } else {
            format!("{}", p.words_delta_vs_prior)
        }
    ));
    md.push_str(&format!("- **Accuracy:** {:.1}%\n", p.accuracy * 100.0));
    md.push_str(&format!("- **Current streak:** {} day(s)\n", p.streak));
    md.push_str(&format!(
        "- **Vocabulary:** {} unique words ({:.1}% uncommon)\n",
        p.vocab_unique, p.vocab_rare_pct
    ));

    if p.top_errors.is_empty() {
        md.push_str("\n## Top errors\n\nNone recorded — clean week.\n");
    } else {
        md.push_str("\n## Top errors\n\n");
        for (rule, n) in &p.top_errors {
            md.push_str(&format!("- {} ({}×)\n", humanize_rule(rule), n));
        }
    }

    match &p.tone {
        Some(t) => {
            md.push_str("\n## Tone distribution\n\n");
            for (label, v) in [
                ("Formal", t.formal),
                ("Neutral", t.neutral),
                ("Friendly", t.friendly),
                ("Confident", t.confident),
            ] {
                md.push_str(&format!("- {label}: {:.0}%\n", v * 100.0));
            }
        }
        None => {
            md.push_str(
                "\n## Tone distribution\n\nEnable snippet retention in Settings to unlock tone analysis.\n",
            );
        }
    }

    md.push_str("\n---\n*Generated locally by WordBuddy. No data left your machine.*\n");
    md
}

/// "harper:SpellCheck" → "SpellCheck"; "llm:wordy_sentence" → "Wordy Sentence".
fn humanize_rule(rule: &str) -> String {
    let tail = rule.rsplit(':').next().unwrap_or(rule);
    let spaced = tail.replace(['_', '-'], " ");
    let mut out = String::new();
    for word in spaced.split_whitespace() {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
            out.push(' ');
        }
    }
    out.trim_end().to_string()
}

/// Parse an LLM tone JSON payload: {"formal":0..1,...}. Validated like
/// the engine style pass (bounded, descriptive errors).
#[cfg_attr(not(test), allow(dead_code))]
pub fn parse_tone(raw: &str) -> Result<ToneDistribution, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("tone JSON invalid: {e}"))?;
    let get = |k: &str| -> Result<f64, String> {
        v[k].as_f64()
            .ok_or_else(|| format!("tone field '{k}' missing or not numeric"))
    };
    let t = ToneDistribution {
        formal: get("formal")?,
        neutral: get("neutral")?,
        friendly: get("friendly")?,
        confident: get("confident")?,
    };
    for (k, val) in [
        ("formal", t.formal),
        ("neutral", t.neutral),
        ("friendly", t.friendly),
        ("confident", t.confident),
    ] {
        if !(0.0..=1.0).contains(&val) {
            return Err(format!("tone field '{k}' must be within [0,1]"));
        }
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> WeekPayload {
        WeekPayload {
            week_start: "2026-08-17".into(),
            days: vec![],
            words: 1200,
            checks: 34,
            words_delta_vs_prior: -150,
            accuracy: 0.93,
            streak: 4,
            vocab_unique: 380,
            vocab_rare_pct: 12.5,
            top_errors: vec![("harper:SpellCheck".into(), 9)],
            tone: None,
        }
    }

    #[test]
    fn markdown_contains_sections_and_honest_tone_note() {
        let md = render_markdown(&sample());
        assert!(md.contains("# WordBuddy Weekly Report — week of 2026-08-17"));
        assert!(md.contains("1200 (-150 vs prior week)"));
        assert!(md.contains("93.0%"));
        assert!(md.contains("SpellCheck (9×)"));
        assert!(md.contains("nable snippet retention"));
    }

    #[test]
    fn humanize_maps_rule_names() {
        assert_eq!(super::humanize_rule("harper:SpellCheck"), "SpellCheck");
        assert_eq!(super::humanize_rule("llm:wordy_sentence"), "Wordy Sentence");
    }

    #[test]
    fn tone_parse_rejects_out_of_range() {
        assert!(
            parse_tone(r#"{"formal":0.5,"neutral":0.2,"friendly":0.3,"confident":0.0}"#).is_ok()
        );
        assert!(parse_tone(r#"{"formal":1.5,"neutral":0,"friendly":0,"confident":0}"#).is_err());
        assert!(parse_tone(r#"{"formal":0.5}"#).is_err());
    }
}
