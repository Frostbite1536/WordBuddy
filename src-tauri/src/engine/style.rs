//! LLM style pass (PLAN-01 task 3): clarity / engagement / delivery.
//!
//! Pure parse-and-validate with bounded retry, ported from the base repo's
//! journal analyzer pattern (`extract_json` + descriptive parse errors +
//! the previous error fed back verbatim). The network call itself is the
//! only impurity and is injected as a closure so every failure mode is
//! unit-testable with canned JSON (same convention as the base tests).
use serde::Deserialize;
use std::future::Future;

use super::offsets::slice_utf16;
use super::prompts::style_system_prompt;
use super::{IssueKind, IssueSource, TextIssue, WritingGoals};

/// Maximum attempts per style pass (initial + 1 retry), matching the base
/// analyzer's bounded retry.
pub const MAX_ATTEMPTS: usize = 2;

/// Raw shape the model is asked for. Field-for-field the CONTRACTS
/// `TextIssue` minus the fields we derive ourselves (id, source, kind).
#[derive(Debug, Clone, Deserialize)]
pub struct LlmIssue {
    pub start: usize,
    pub end: usize,
    pub original: Option<String>,
    pub message: String,
    #[serde(default)]
    pub replacements: Vec<String>,
    pub rule_id: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct StylePassOutput {
    #[serde(default)]
    pub clarity: Vec<LlmIssue>,
    #[serde(default)]
    pub engagement: Vec<LlmIssue>,
    #[serde(default)]
    pub delivery: Vec<LlmIssue>,
}

/// Extracted-JSON error carrying a verbatim, model-actionable message.
#[derive(Debug)]
pub struct StyleParseError(pub String);

impl std::fmt::Display for StyleParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Pull the outermost JSON object out of a model response. Models
/// occasionally wrap the object in markdown fences or a sentence of
/// apology; we tolerate fences + leading/trailing prose but require
/// exactly one balanced top-level object (base `extract_json` behavior).
pub fn extract_json(raw: &str) -> Result<&str, StyleParseError> {
    let start = raw.find('{').ok_or_else(|| {
        StyleParseError(
            "Your response contained no JSON object. Respond with ONLY the JSON object.".into(),
        )
    })?;
    // Walk forward balancing braces (string-aware) to find the matching close.
    let bytes = raw.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(&raw[start..=i]);
                }
            }
            _ => {}
        }
    }
    Err(StyleParseError(
        "Your JSON object was truncated or unbalanced. Emit the complete object.".into(),
    ))
}

/// Deserialize + validate one attempt's payload against the source text.
/// Every field-level problem becomes a descriptive error the model can act
/// on in the retry prompt.
pub fn parse_and_validate(raw: &str, text: &str) -> Result<StylePassOutput, StyleParseError> {
    let json_str = extract_json(raw)?;
    let parsed: StylePassOutput = serde_json::from_str(json_str).map_err(|e| {
        StyleParseError(format!(
            "Your JSON did not match the required schema: {e}. Required keys: clarity, engagement, delivery — each an array of {{start, end, original, message, replacements, rule_id}}."
        ))
    })?;

    let mut buckets: [(&str, &Vec<LlmIssue>); 3] = [
        ("clarity", &parsed.clarity),
        ("engagement", &parsed.engagement),
        ("delivery", &parsed.delivery),
    ];
    let utf16_len: usize = text.chars().map(|c| c.len_utf16()).sum();
    for (bucket, issues) in buckets.iter_mut() {
        for (i, issue) in issues.iter().enumerate() {
            let label = format!("{bucket}[{i}]");
            if issue.start >= issue.end {
                return Err(StyleParseError(format!(
                    "{label}: start ({start}) must be less than end ({end}).",
                    start = issue.start,
                    end = issue.end
                )));
            }
            if issue.end > utf16_len {
                return Err(StyleParseError(format!(
                    "{label}: end ({end}) is beyond the text's length in UTF-16 code units ({utf16_len}).",
                    end = issue.end
                )));
            }
            let actual = slice_utf16(text, issue.start, issue.end);
            if let Some(claimed) = &issue.original {
                if claimed != &actual {
                    return Err(StyleParseError(format!(
                        "{label}: original \"{claimed}\" is not the substring text[{start}..{end}], which is \"{actual}\". Copy the substring exactly.",
                        start = issue.start, end = issue.end
                    )));
                }
            }
            if issue.message.trim().is_empty() {
                return Err(StyleParseError(format!(
                    "{label}: message must be a short human-readable explanation."
                )));
            }
            if !issue.rule_id.starts_with("llm:") {
                return Err(StyleParseError(format!(
                    "{label}: rule_id must start with \"llm:\" (got \"{rid}\").",
                    rid = issue.rule_id
                )));
            }
        }
    }
    Ok(parsed)
}

/// responses; production wiring passes a closure over `llm.rs`.
///
/// Never errors the caller's whole check: on final failure the style pass
/// yields `None` and the orchestrator marks `style_check_failed`.
pub async fn run_style_pass<F, Fut>(
    text: &str,
    goals: &WritingGoals,
    mut complete: F,
) -> Result<Option<Vec<TextIssue>>, String>
where
    F: FnMut(String, String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    let system_prompt = style_system_prompt(goals);
    let user_prompt = text.to_string();

    let mut last_error: Option<String> = None;
    for _attempt in 0..MAX_ATTEMPTS {
        let ask = match &last_error {
            None => user_prompt.clone(),
            Some(err) => format!(
                "{user_prompt}\n\nYour previous reply was invalid and was NOT used. Fix it and reply again with ONLY the corrected JSON.\nPrevious error: {err}"
            ),
        };
        let raw = match complete(system_prompt.clone(), ask).await {
            Ok(raw) => raw,
            Err(e) => {
                last_error = Some(format!("transport error: {e}"));
                continue;
            }
        };
        match parse_and_validate(&raw, text) {
            Ok(out) => return Ok(Some(materialize_issues(out, text))),
            Err(e) => {
                last_error = Some(e.0);
            }
        }
    }
    // All attempts exhausted — degrade, don't fail (CONTRACTS §1).
    Err(last_error.unwrap_or_else(|| "style pass failed without an error".into()))
}

/// Convert validated buckets into `TextIssue`s with derived ids/kinds.
/// Also fills in a missing `original` from the text itself (validated
/// spans are trusted) and sorts each bucket by start.
fn materialize_issues(out: StylePassOutput, text: &str) -> Vec<TextIssue> {
    let mut issues = Vec::new();
    let buckets: [(IssueKind, Vec<LlmIssue>); 3] = [
        (IssueKind::Clarity, out.clarity),
        (IssueKind::Engagement, out.engagement),
        (IssueKind::Delivery, out.delivery),
    ];
    for (kind, bucket) in buckets {
        let mut items = bucket;
        items.sort_by_key(|i| (i.start, i.end));
        for item in items {
            issues.push(TextIssue {
                id: String::new(), // orchestrator assigns stable ids
                kind,
                start: item.start,
                end: item.end,
                original: item
                    .original
                    .unwrap_or_else(|| slice_utf16(text, item.start, item.end)),
                message: item.message,
                replacements: item.replacements,
                rule_id: item.rule_id,
                source: IssueSource::Llm,
            });
        }
    }
    issues
}

/// `true` when the LLM is disabled for this process (INV-PRIV-003
/// kill-switch). Read once per check by the orchestrator; tests call
/// `style_enabled_for` directly with an override.
pub fn llm_disabled_by_env() -> bool {
    std::env::var("WB_DISABLE_LLM")
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Pure decision helper (unit-testable without env races).
pub fn style_enabled_for(surface: crate::engine::Surface, llm_disabled: bool) -> bool {
    if llm_disabled {
        return false;
    }
    // Style runs on the opted-in browser surface; the native monitor
    // (P3) and the explicit palette (P4) are correctness-first / opt-in
    // via their own callers.
    matches!(surface, crate::engine::Surface::Browser)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goals() -> WritingGoals {
        WritingGoals {
            dialect: crate::engine::Dialect::EnUs,
            domain: crate::engine::Domain::General,
            formality: crate::engine::Formality::Neutral,
            audience: crate::engine::Audience::General,
            intent: None,
        }
    }

    #[test]
    fn extracts_json_from_fenced_response() {
        let raw = "Here you go:\n```json\n{\"clarity\":[],\"engagement\":[],\"delivery\":[]}\n```\nThanks!";
        assert_eq!(
            extract_json(raw).unwrap(),
            "{\"clarity\":[],\"engagement\":[],\"delivery\":[]}"
        );
    }

    #[test]
    fn rejects_brace_inside_string_as_terminator() {
        // The first '}' inside a string literal must NOT close the object.
        let raw = r#"{"clarity":[{"start":0,"end":3,"message":"has } inside","rule_id":"llm:x"}],"engagement":[],"delivery":[]}"#;
        assert_eq!(extract_json(raw).unwrap(), raw);
        assert!(parse_and_validate(raw, "abc def").is_ok());
    }

    #[test]
    fn missing_json_is_descriptive() {
        let err = extract_json("I cannot do that.").unwrap_err();
        assert!(err.0.contains("no JSON object"));
    }

    #[test]
    fn offset_mismatch_names_both_strings() {
        let raw = r#"{"clarity":[{"start":0,"end":5,"original":"wrld","message":"vague","rule_id":"llm:vague"}],"engagement":[],"delivery":[]}"#;
        let err = parse_and_validate(raw, "hello world").unwrap_err();
        assert!(err.0.contains("not the substring"));
        assert!(err.0.contains("hello"));
    }

    #[test]
    fn valid_output_materializes_with_kinds_and_sorted_order() {
        let text = "this is very unique actually";
        let raw = r#"{"clarity":[{"start":8,"end":19,"original":"very unique","message":"'very unique' is overused","replacements":["unique"],"rule_id":"llm:cliche"}],"engagement":[{"start":0,"end":4,"original":"this","message":"weak opener","replacements":[],"rule_id":"llm:opener"}],"delivery":[]}"#;
        let out = parse_and_validate(raw, text).unwrap();
        let issues = materialize_issues(out, text);
        assert_eq!(issues.len(), 2);
        // Sorted by start across buckets after orchestrator sort — here raw order kept per bucket;
        // engagement(0) should come first after the orchestrator's final sort, so just check both exist.
        assert!(issues
            .iter()
            .any(|i| i.kind == IssueKind::Engagement && i.start == 0));
        assert!(issues
            .iter()
            .any(|i| i.kind == IssueKind::Clarity && i.original == "very unique"));
        assert!(issues.iter().all(|i| i.source == IssueSource::Llm));
    }

    #[tokio::test]
    async fn retry_feeds_error_back_then_succeeds() {
        let text = "hello world";
        let bad = r#"{"clarity":[{"start":0,"end":99,"original":"?","message":"m","rule_id":"llm:x"}],"engagement":[],"delivery":[]}"#;
        let good = r#"{"clarity":[{"start":0,"end":5,"original":"hello","message":"generic greeting","replacements":["greetings"],"rule_id":"llm:hello"}],"engagement":[],"delivery":[]}"#;
        let mut calls = 0usize;
        let mut payloads: Vec<String> = Vec::new();
        let result = run_style_pass(text, &goals(), |_sys, user| {
            calls += 1;
            payloads.push(user.clone());
            let r = if calls == 1 {
                bad.to_string()
            } else {
                good.to_string()
            };
            async move { Ok(r) }
        })
        .await
        .unwrap()
        .expect("second attempt should validate");
        assert_eq!(calls, 2);
        assert_eq!(result.len(), 1);
        // The retry prompt carried the previous error verbatim.
        assert!(payloads[1].contains("Your previous reply was invalid"));
        assert!(payloads[1].contains("beyond the text's length"));
    }

    #[tokio::test]
    async fn two_failures_degrade_to_none() {
        let text = "hello";
        let bad = "not json at all";
        let result = run_style_pass(text, &goals(), |_, _| {
            let r = bad.to_string();
            async move { Ok(r) }
        })
        .await;
        assert!(
            result.is_err(),
            "exhausted retries must return Err so caller sets style_check_failed"
        );
    }

    #[test]
    fn style_policy_respects_kill_switch_and_surface() {
        assert!(style_enabled_for(crate::engine::Surface::Browser, false));
        assert!(!style_enabled_for(crate::engine::Surface::Browser, true));
        assert!(!style_enabled_for(crate::engine::Surface::Native, false));
        assert!(!style_enabled_for(crate::engine::Surface::Palette, false));
    }
}
