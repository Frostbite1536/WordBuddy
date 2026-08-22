//! Style-pass prompt construction (PLAN-01 task 3).
//!
//! The style pass asks one LLM question — "what would improve this text's
//! clarity, engagement, and delivery?" — and demands a JSON answer in the
//! CONTRACTS §1 `TextIssue` shape. The parser in `style.rs` is the only
//! consumer of this format; change both together.

use super::WritingGoals;

/// System prompt for the style pass. The JSON contract is repeated in the
/// retry loop's error feedback verbatim, so keep it in one string.
pub fn style_system_prompt(goals: &WritingGoals) -> String {
    format!(
        "You are a writing style reviewer. Analyze the user's text and respond ONLY with \
a single JSON object, no prose, no markdown fences, with exactly three keys \
\"clarity\", \"engagement\", \"delivery\". Each maps to an array (possibly empty) of \
issue objects with these fields:\n\
{{\"start\": <integer>, \"end\": <integer>, \"original\": \"<exact substring>\", \
\"message\": \"<short human explanation>\", \"replacements\": [\"<better wording>\", ...], \
\"rule_id\": \"llm:<short-slug>\"}}\n\
Rules:\n\
- start/end are UTF-16 code-unit offsets into the text, end-exclusive.\n\
- original must be EXACTLY the substring text[start..end].\n\
- Only flag spans that are actually in the text; never invent quotes.\n\
- Keep spans short (a word or phrase), not whole sentences.\n\
- Order each array by start offset.\n\
Writing goals for this pass: {goals_description}",
        goals_description = describe_goals(goals)
    )
}

/// Human/LLM-readable rendering of the writing goals. `intent` is unused by
/// harper (correctness pass) but prefixes LLM prompts per CONTRACTS §1.
fn describe_goals(goals: &WritingGoals) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(match goals.formality {
        crate::engine::Formality::Informal => "informal register".into(),
        crate::engine::Formality::Neutral => "neutral register".into(),
        crate::engine::Formality::Formal => "formal register".into(),
    });
    parts.push(match goals.audience {
        crate::engine::Audience::General => "general audience".into(),
        crate::engine::Audience::Knowledgeable => "knowledgeable audience".into(),
        crate::engine::Audience::Expert => "expert audience".into(),
    });
    parts.push(format!("domain: {:?}", goals.domain));
    if let Some(intent) = &goals.intent {
        parts.push(format!("intent: {:?}", intent));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_contains_json_contract_and_goals() {
        let goals = WritingGoals {
            dialect: crate::engine::Dialect::EnUs,
            domain: crate::engine::Domain::Academic,
            formality: crate::engine::Formality::Formal,
            audience: crate::engine::Audience::Expert,
            intent: Some(crate::engine::Intent::Convince),
        };
        let p = style_system_prompt(&goals);
        assert!(p.contains("\"clarity\""));
        assert!(p.contains("UTF-16 code-unit offsets"));
        assert!(p.contains("formal register"));
        assert!(p.contains("expert audience"));
        assert!(p.contains("domain: Academic"));
        assert!(p.contains("intent: Convince"));
    }

    #[test]
    fn prompt_without_intent_omits_it() {
        let goals = WritingGoals {
            dialect: crate::engine::Dialect::EnUs,
            domain: crate::engine::Domain::General,
            formality: crate::engine::Formality::Neutral,
            audience: crate::engine::Audience::General,
            intent: None,
        };
        let p = style_system_prompt(&goals);
        assert!(p.contains("neutral register"));
        assert!(p.contains("general audience"));
        assert!(!p.contains("intent:"));
    }
}
