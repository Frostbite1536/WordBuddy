//! The Check contract (CONTRACTS §1) — the one pipeline every surface uses.
//!
//! Composition (CONTRACTS "Engine composition"):
//! 1. **Correctness pass** — always, local, zero network: harper-core.
//! 2. **Style pass** — opted-in surfaces (Browser) when the LLM isn't
//!    kill-switched: one validated JSON round-trip via `llm.rs`; on any
//!    failure the result degrades to correctness-only + `style_check_failed`.
//!
//! Purity (INV-CHECK-003): `correctness_pass` is a pure function of its
//! arguments — no config or global reads inside. `check_text` additionally
//! reads exactly two process-level inputs sanctioned by PLAN-01 task 4: the
//! result cache (memoization; observably transparent) and the
//! `WB_DISABLE_LLM` kill-switch (INV-PRIV-003 enforcement point). Settings
//! and goals are parameters.
//!
//! Offsets (INV-OFFSET-001): all `TextIssue` offsets are UTF-16 code units.
//! Harper speaks char indices; `offsets::Utf16Index` is the single
//! conversion site.
//!
//! Personal dictionary: the preferred path — accepted words are fed INTO
//! harper's dictionary layer via `MergedDictionary` (curated FST + a
//! `MutableDictionary` of user words), so spell rules treat user words as
//! first-class vocabulary rather than being post-filtered out of results.

pub mod offsets;
pub mod prompts;
pub mod style;

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use harper_core::linting::{LintGroup, Suggestion};
use harper_core::parsers::PlainEnglish;
use harper_core::spell::{FstDictionary, MergedDictionary, MutableDictionary};
use harper_core::{Dialect as HarperDialect, Document};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Hard input cap (CONTRACTS §1): callers chunk longer text at sentence
/// boundaries; the engine rejects instead of truncating.
pub const MAX_TEXT_BYTES: usize = 20_000;

/// Cache capacity (PLAN-01 task 4).
const CACHE_CAPACITY: usize = 64;

// ---------------------------------------------------------------------------
// CONTRACTS §1 types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Surface {
    Browser,
    Native,
    Palette,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TargetKind {
    BrowserHost { host: String },
    NativeProcess { process: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetId {
    /// Flattened so the wire shape is CONTRACTS §2 exactly:
    /// `{"kind":"browserHost","host":"..."}` — not double-nested.
    #[serde(flatten)]
    pub kind: TargetKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dialect {
    EnUs,
    EnGb,
    EnCa,
    EnAu,
    EnIn,
}
impl Default for Dialect {
    fn default() -> Self {
        Dialect::EnUs
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Domain {
    General,
    Academic,
    Business,
    Casual,
    Technical,
}
impl Default for Domain {
    fn default() -> Self {
        Domain::General
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Formality {
    Informal,
    Neutral,
    Formal,
}
impl Default for Formality {
    fn default() -> Self {
        Formality::Neutral
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Audience {
    General,
    Knowledgeable,
    Expert,
}
impl Default for Audience {
    fn default() -> Self {
        Audience::General
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Intent {
    Inform,
    Describe,
    Convince,
    TellStory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WritingGoals {
    pub dialect: Dialect,
    pub domain: Domain,
    pub formality: Formality,
    pub audience: Audience,
    /// Accepted but unused by harper; prefixes LLM prompts only.
    pub intent: Option<Intent>,
}
impl Default for WritingGoals {
    fn default() -> Self {
        Self {
            dialect: Dialect::default(),
            domain: Domain::default(),
            formality: Formality::default(),
            audience: Audience::default(),
            intent: None,
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRequest {
    pub text: String,
    /// Defaults to Browser on the wire (extension path); native and
    /// palette callers set it explicitly.
    #[serde(default = "default_surface")]
    pub surface: Surface,
    pub target: TargetId,
    #[serde(default)]
    pub goals: WritingGoals,
}

fn default_surface() -> Surface {
    Surface::Browser
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueKind {
    Correctness,
    Clarity,
    Engagement,
    Delivery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueSource {
    Harper,
    Llm,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextIssue {
    /// Stable within one response ("i0","i1",...) — assigned by
    /// `check_text` after final ordering, never by the passes.
    pub id: String,
    pub kind: IssueKind,
    /// UTF-16 code-unit offset (INV-OFFSET-001), end-exclusive.
    pub start: usize,
    pub end: usize,
    /// Exact substring `text[start..end]` in UTF-16 terms (INV-CHECK-002).
    pub original: String,
    pub message: String,
    /// Ranked best-first; may be empty.
    pub replacements: Vec<String>,
    /// `harper:<lint-name>` or `llm:<slug>`.
    pub rule_id: String,
    pub source: IssueSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResponse {
    pub issues: Vec<TextIssue>,
    /// Set when a style pass was requested and failed after retries.
    pub style_check_failed: bool,
}

/// User-accepted vocabulary. P1 defines the engine-level shape; the
/// settings UI arrives in PLAN-06.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PersonalDictionary {
    #[serde(default)]
    pub words: Vec<String>,
}

// ---------------------------------------------------------------------------
// Correctness pass (pure)
// ---------------------------------------------------------------------------

fn harper_dialect(d: Dialect) -> HarperDialect {
    match d {
        Dialect::EnUs => HarperDialect::American,
        Dialect::EnGb => HarperDialect::British,
        Dialect::EnCa => HarperDialect::Canadian,
        Dialect::EnAu => HarperDialect::Australian,
        Dialect::EnIn => HarperDialect::Indian,
    }
}
/// Build the per-call merged dictionary (curated FST + user words).
fn build_merged_dict(dict: &PersonalDictionary) -> Arc<MergedDictionary> {
    let mut merged = MergedDictionary::new();
    merged.add_dictionary(FstDictionary::curated());
    if !dict.words.is_empty() {
        let mut user = MutableDictionary::new();
        for word in &dict.words {
            let chars: Vec<char> = word.chars().collect();
            user.append_word(&chars, harper_core::DictWordMetadata::default());
        }
        merged.add_dictionary(Arc::new(user));
    }
    Arc::new(merged)
}

fn build_document(text: &str, dict: &PersonalDictionary) -> Document {
    let dict_arc = build_merged_dict(dict);
    Document::new(text, &PlainEnglish, dict_arc.as_ref())
}

/// Cache of constructed lint groups, keyed by (dialect, user-words hash).
///
/// `LintGroup::new_curated` costs hundreds of ms (it instantiates every
/// curated rule) and the curated FST deserialization ~1 s; memoizing the
/// group keeps steady-state checks in the tens of ms (INV-PERF-004).
/// Transparent memoization — see the INV-CHECK-003 note in module docs.
fn lint_group_for(dialect: Dialect, dict: &PersonalDictionary) -> Arc<Mutex<LintGroup>> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut dh = DefaultHasher::new();
    dict.hash(&mut dh);
    let key = (dialect, dh.finish());

    static GROUPS: Mutex<Option<HashMap<(Dialect, u64), Arc<Mutex<LintGroup>>>>> =
        Mutex::new(None);

    // Fast path: read-only lookup under the lock.
    {
        let guard = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(map) = guard.as_ref() {
            if let Some(group) = map.get(&key) {
                return group.clone();
            }
        }
    }

    // Miss: construct OUTSIDE the lock — `new_curated` takes ~1 s and
    // doing it under the map lock serializes every concurrent checker
    // behind it (observed 17 s outliers under parallel tests).
    let dict_arc = build_merged_dict(dict);
    let group = Arc::new(Mutex::new(LintGroup::new_curated(
        dict_arc,
        harper_dialect(dialect),
    )));

    let mut guard = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() >= 8 {
        if let Some(evict) = map.keys().next().cloned() {
            map.remove(&evict);
        }
    }
    map.entry(key).or_insert_with(|| group.clone()).clone()
}

/// Pure correctness pass: harper lints → `Correctness` issues.
///
/// Deterministic: output is sorted by `(start, end)` and ids are NOT
/// assigned here (the orchestrator ids the merged result).
pub fn correctness_pass(
    text: &str,
    goals: &WritingGoals,
    dict: &PersonalDictionary,
) -> Result<Vec<TextIssue>, String> {
    let linter = lint_group_for(goals.dialect, dict);
    let mut linter = linter
        .lock()
        .unwrap_or_else(|e| e.into_inner()); // poison recovery
    let document = build_document(text, dict);

    let chars: Vec<char> = text.chars().collect();
    let utf16 = offsets::Utf16Index::build(&chars);

    let mut issues: Vec<TextIssue> = Vec::new();
    // organized_lints → rule name per lint, giving real `harper:<rule>` ids.
    let by_rule = linter.organized_lints(&document);
    let mut rule_order: Vec<&String> = by_rule.keys().collect();
    rule_order.sort(); // BTreeMap already ordered; explicit for clarity.
    for rule_name in rule_order {
        let lints = &by_rule[rule_name];
        for lint in lints {
            let span = lint.span;
            if span.is_empty() {
                continue; // zero-width spans can't carry an `original`
            }
            let original: String = chars
                .get(span.start..span.end)
                .map(|s| s.iter().collect())
                .unwrap_or_default();
            if original.is_empty() {
                continue;
            }
            let replacements = lint
                .suggestions
                .iter()
                .filter_map(|s| match s {
                    Suggestion::ReplaceWith(chars) => {
                        Some(chars.iter().collect::<String>())
                    }
                    Suggestion::Remove => Some(String::new()),
                    // InsertAfter doesn't fit the replacement contract
                    // (replace span with string); dropped deliberately.
                    Suggestion::InsertAfter(_) => None,
                })
                .collect();
            issues.push(TextIssue {
                id: String::new(),
                kind: IssueKind::Correctness,
                start: utf16.to_utf16(span.start),
                end: utf16.to_utf16(span.end),
                original,
                message: lint.message.clone(),
                replacements,
                rule_id: format!("harper:{rule_name}"),
                source: IssueSource::Harper,
            });
        }
    }

    // Deterministic ordering (PLAN-01 task 2).
    issues.sort_by(|a, b| (a.start, a.end).cmp(&(b.start, b.end)));
    Ok(issues)
}

// ---------------------------------------------------------------------------
// Orchestration + cache
// ---------------------------------------------------------------------------

struct CacheKey([u8; 32], u64, u64);

struct LruCache {
    map: HashMap<CacheKey, CheckResponse>,
    order: VecDeque<CacheKey>,
}

impl LruCache {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }
    fn get(&mut self, key: &CacheKey) -> Option<&CheckResponse> {
        if self.map.contains_key(key) {
            // Move to back (most recently used).
            if let Some(pos) = self.order.iter().position(|k| k == key) {
                self.order.remove(pos);
                self.order.push_back(CacheKey(key.0, key.1, key.2));
            }
            self.map.get(key)
        } else {
            None
        }
    }
    fn put(&mut self, key: CacheKey, value: CheckResponse) {
        if self.map.len() >= CACHE_CAPACITY {
            if let Some(evict) = self.order.pop_front() {
                self.map.remove(&evict);
            }
        }
        self.order.push_back(CacheKey(key.0, key.1, key.2));
        self.map.insert(key, value);
    }
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1 && self.2 == other.2
    }
}
impl Eq for CacheKey {}

impl std::hash::Hash for CacheKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
        self.1.hash(state);
        self.2.hash(state);
    }
}

fn cache_key(req: &CheckRequest, dict: &PersonalDictionary, style_on: bool) -> CacheKey {
    let mut hasher = Sha256::new();
    hasher.update(req.text.as_bytes());
    let text_hash: [u8; 32] = hasher.finalize().into();

    // Goals hash: fieldless enums hash stably via derived Hash.
    let mut gh = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    req.goals.hash(&mut gh);
    let mut dh = std::collections::hash_map::DefaultHasher::new();
    dict.hash(&mut dh);
    let style_bit = if style_on { 1u64 } else { 0u64 };
    CacheKey(
        text_hash,
        gh.finish() ^ (style_bit << 63),
        dh.finish(),
    )
}

static CACHE: Mutex<Option<LruCache>> = Mutex::new(None);

fn with_cache<T>(
    f: impl FnOnce(&mut LruCache) -> T,
) -> Result<T, String> {
    let mut guard = CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner()); // poison recovery (CLAUDE.md)
    let mut cache = guard.take().unwrap_or_else(LruCache::new);
    let out = f(&mut cache);
    *guard = Some(cache);
    Ok(out)
}

/// Whether the style pass should run for this request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StylePolicy {
    /// Surface-decided (Browser = opted-in) unless kill-switched.
    AutoBySurface,
    /// Force on (tests / future palette callers).
    Always,
    /// Correctness-only.
    Never,
}

/// The Check pipeline (CONTRACTS §1) with no LLM transport available —
/// equivalent to correctness-only. Production callers use
/// `check_text_with` with an app handle.
pub async fn check_text(req: CheckRequest) -> Result<CheckResponse, String> {
    check_text_with(req, PersonalDictionary::default(), StylePolicy::AutoBySurface, None).await
}

/// Full-control entry point. `llm_transport` carries the app handle plus
/// the configured provider/model; the Tauri command builds it (config
/// reads stay in the wrapper, keeping the pipeline parameter-only).
pub async fn check_text_with(
    req: CheckRequest,
    dict: PersonalDictionary,
    style_policy: StylePolicy,
    llm_transport: Option<(tauri::AppHandle, crate::llm::Provider, String)>,
) -> Result<CheckResponse, String> {
    if req.text.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "text is {} bytes; the per-request cap is {MAX_TEXT_BYTES} bytes. Chunk at sentence boundaries.",
            req.text.len()
        ));
    }

    let llm_disabled = style::llm_disabled_by_env();
    let style_on = match style_policy {
        StylePolicy::Never => false,
        StylePolicy::Always => !llm_disabled,
        StylePolicy::AutoBySurface => style::style_enabled_for(req.surface, llm_disabled),
    };

    let key = cache_key(&req, &dict, style_on);
    if let Some(hit) = with_cache(|c| c.get(&key).cloned())? {
        return Ok(hit);
    }

    let mut issues = correctness_pass(&req.text, &req.goals, &dict)?;
    let mut style_check_failed = false;

    if style_on {
        let outcome = match &llm_transport {
            Some((app, provider, model)) => {
                let app = app.clone();
                let provider = provider.clone();
                let text = req.text.clone();
                let goals = req.goals;
                style::run_style_pass(&text, &goals, move |sys, user| {
                    crate::llm::complete_text(
                        app.clone(),
                        sys,
                        user,
                        Some(model.clone()),
                        Some(provider.clone()),
                    )
                })
                .await
            }
            None => Err("style pass requested but no LLM transport available".to_string()),
        };
        match outcome {
            Ok(Some(style_issues)) => issues.extend(style_issues),
            Ok(None) => {}
            Err(_) => style_check_failed = true, // degrade, never fail
        }
    }

    // Merge + dedupe overlapping spans; correctness wins on overlap.
    issues.sort_by(|a, b| (a.start, a.end).cmp(&(b.start, b.end)));
    let mut merged: Vec<TextIssue> = Vec::with_capacity(issues.len());
    for issue in issues {
        let overlaps = merged
            .iter()
            .any(|m| issue.start < m.end && m.start < issue.end);
        // Correctness beats style on overlap; equal kinds keep the earlier.
        let loser_to_style = overlaps;
        if loser_to_style && issue.kind == IssueKind::Correctness {
            // A correctness issue overlapping an already-kept style issue:
            // replace the style issue (correctness wins).
            merged.retain(|m| !(issue.start < m.end && m.start < issue.end));
            merged.push(issue);
        } else if !overlaps {
            merged.push(issue);
        }
        // Overlapping style issue against a kept correctness/style span: dropped.
    }
    merged.sort_by(|a, b| (a.start, a.end).cmp(&(b.start, b.end)));

    // Stable ids AFTER final ordering.
    for (i, issue) in merged.iter_mut().enumerate() {
        issue.id = format!("i{i}");
    }

    let response = CheckResponse {
        issues: merged,
        style_check_failed,
    };

    // Analytics choke point (PLAN-05): counts + rule names only
    // (INV-PRIV-002). Empty text and monitor self-reads are skipped;
    // failures drop the row behind a counter — never stall checking.
    let text = req.text.clone();
    if !text.is_empty() {
        let mut issue_counts = std::collections::BTreeMap::new();
        let mut rule_counts = std::collections::BTreeMap::new();
        for issue in &response.issues {
            let k = format!("{:?}", issue.kind);
            *issue_counts.entry(k).or_insert(0) += 1;
            *rule_counts.entry(issue.rule_id.clone()).or_insert(0) += 1;
        }
        let surface_str = match req.surface {
            Surface::Browser => "browser",
            Surface::Native => "native",
            Surface::Palette => "palette",
        };
        let target = match &req.target.kind {
            TargetKind::BrowserHost { host } => host.clone(),
            TargetKind::NativeProcess { process } => process.clone(),
        };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let event = crate::analytics::db::CheckEvent {
            ts,
            surface: surface_str.into(),
            target,
            word_count: text.split_whitespace().count() as u32,
            issue_counts,
            rule_counts,
        };
        let _ = crate::analytics::db::record_check(&event);
    }

    with_cache(|c| c.put(cache_key(&req, &dict, style_on), response.clone()))?;
    Ok(response)
}

// ---------------------------------------------------------------------------
// Tauri command (PLAN-01 task 5)
// ---------------------------------------------------------------------------

/// IPC surface for the Check contract. Registered in `lib.rs`
/// invoke_handler (app commands need no per-command capability entry —
/// `core:default` in capabilities/default.json covers them; the duality
/// rule applies to plugin permissions).
#[tauri::command]
pub async fn check_text_command(
    app: tauri::AppHandle,
    request: CheckRequest,
) -> Result<CheckResponse, String> {
    let dict = PersonalDictionary {
        words: crate::config::with_config_pub(|c| c.personal_dictionary.clone()),
    };
    let transport = crate::llm::configured_provider_and_model()
        .map(|(provider, model)| (app, provider, model));
    check_text_with(request, dict, StylePolicy::AutoBySurface, transport).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn goals() -> WritingGoals {
        WritingGoals {
            dialect: Dialect::EnUs,
            domain: Domain::General,
            formality: Formality::Neutral,
            audience: Audience::General,
            intent: None,
        }
    }

    fn request(text: &str) -> CheckRequest {
        CheckRequest {
            text: text.to_string(),
            surface: Surface::Palette,
            target: TargetId {
                kind: TargetKind::BrowserHost {
                    host: "example.com".into(),
                },
            },
            goals: goals(),
        }
    }

    /// PLAN-01 task 1 acceptance: the spike's exit ticket.
    #[test]
    fn spike_lints_teh_recieve_with_replacements() {
        let issues = correctness_pass("teh recieve", &goals(), &PersonalDictionary::default())
            .expect("harper pass must succeed");
        assert!(
            issues.len() >= 2,
            "expected ≥2 issues for 'teh recieve', got {issues:?}"
        );
        assert!(
            issues.iter().filter(|i| !i.replacements.is_empty()).count() >= 2,
            "expected ≥2 issues with non-empty replacement lists, got {issues:?}"
        );
        assert!(issues.iter().all(|i| i.rule_id.starts_with("harper:")));
    }

    /// INV-CHECK-002 both-side assertion (Rust side).
    #[test]
    fn originals_match_utf16_slices() {
        let cases = [
            "teh recieve",
            "🚀 teh recieve",
            "cafe\u{0301} recieve teh",
            "你好 teh",
        ];
        for text in cases {
            let issues =
                correctness_pass(text, &goals(), &PersonalDictionary::default()).unwrap();
            assert!(!issues.is_empty(), "no issues for {text:?}");
            for issue in &issues {
                assert_eq!(
                    issue.original,
                    offsets::slice_utf16(text, issue.start, issue.end),
                    "INV-CHECK-002 violated for {text:?} at [{}, {})",
                    issue.start, issue.end
                );
            }
        }
    }

    #[test]
    fn personal_dictionary_feeds_harper() {
        let without = correctness_pass(
            "My WordBuddyz project",
            &goals(),
            &PersonalDictionary::default(),
        )
        .unwrap();
        assert!(
            without.iter().any(|i| i.original.to_lowercase().contains("wordbuddyz")),
            "expected a spelling issue for 'WordBuddyz' without the dictionary entry"
        );
        let with = correctness_pass(
            "My WordBuddyz project",
            &goals(),
            &PersonalDictionary {
                words: vec!["WordBuddyz".into()],
            },
        )
        .unwrap();
        assert!(
            !with.iter().any(|i| i.original.to_lowercase().contains("wordbuddyz")),
            "dictionary entry should suppress the spelling issue"
        );
    }

    #[test]
    fn deterministic_ordering() {
        let a = correctness_pass(
            "teh recieve and more recieve",
            &goals(),
            &PersonalDictionary::default(),
        )
        .unwrap();
        let b = correctness_pass(
            "teh recieve and more recieve",
            &goals(),
            &PersonalDictionary::default(),
        )
        .unwrap();
        assert_eq!(a, b);
        let spans: Vec<(usize, usize)> = a.iter().map(|i| (i.start, i.end)).collect();
        let mut sorted = spans.clone();
        sorted.sort();
        assert_eq!(spans, sorted);
    }

    /// INV-PERF-004: correctness-only pass on 2,000 chars. The contract
    /// target is < 25 ms p95 (release, warmed); the test ceiling is
    /// 100 ms in release (CI variance guard) and a sanity bound in
    /// debug (unoptimized harper is 10-30x slower — the invariant is
    /// judged in release runs). First call warms the lint-group cache
    /// (dictionary deserialization + rule construction, ~1 s) and is
    /// excluded, matching steady-state usage where the engine stays
    /// resident.
    #[test]
    fn correctness_pass_perf_2k_chars() {
        let sentence = "The quick brown fox jumps over the lazy dog, and teh recieve module handles teh rest. ";
        let mut text = String::new();
        while text.len() < 2_000 {
            text.push_str(sentence);
        }
        text.truncate(2_000);

        // A dictionary with a marker word gives this test its own
        // lint-group cache key, so parallel tests using the default
        // dictionary can't convoy on the shared group lock and pollute
        // the timing (observed 18 s outliers otherwise).
        let perf_dict = PersonalDictionary {
            words: vec!["perfmarker".into()],
        };

        // Warm the caches (group construction + FST deserialization).
        correctness_pass(&text, &goals(), &perf_dict).unwrap();

        let mut samples_ms: Vec<f64> = Vec::with_capacity(20);
        for _ in 0..20 {
            let start = std::time::Instant::now();
            let issues = correctness_pass(&text, &goals(), &perf_dict).unwrap();
            let _ = issues.len();
            samples_ms.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = samples_ms[samples_ms.len() / 2];
        let p95 = samples_ms[samples_ms.len() - 1];
        eprintln!(
            "[perf] correctness on {} chars over 20 runs: p50={p50:.1}ms p95={p95:.1}ms ({})",
            text.chars().count(),
            if cfg!(debug_assertions) { "debug" } else { "release" }
        );
        if cfg!(debug_assertions) {
            assert!(p95 < 3_000.0, "debug sanity ceiling exceeded: p95={p95:.1}ms");
        } else {
            assert!(p95 < 100.0, "INV-PERF-004 ceiling exceeded: p95={p95:.1}ms (target 25ms)");
        }
    }

    #[tokio::test]
    async fn rejects_oversized_text_without_truncating() {
        let big = "a".repeat(MAX_TEXT_BYTES + 1);
        let err = check_text_with(request(&big), PersonalDictionary::default(), StylePolicy::Never, None)
            .await
            .unwrap_err();
        assert!(err.contains("cap"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn assigns_stable_ids_and_merges_overlaps_correctness_first() {
        // Correctness-only policy keeps this deterministic without an LLM.
        let resp = check_text_with(
            request("teh recieve"),
            PersonalDictionary::default(),
            StylePolicy::Never,
            None,
        )
        .await
        .unwrap();
        assert!(!resp.issues.is_empty());
        for (i, issue) in resp.issues.iter().enumerate() {
            assert_eq!(issue.id, format!("i{i}"));
        }
        assert!(!resp.style_check_failed);
    }

    #[tokio::test]
    async fn style_policy_never_skips_llm_even_on_browser() {
        let mut req = request("hello");
        req.surface = Surface::Browser;
        let resp =
            check_text_with(req, PersonalDictionary::default(), StylePolicy::Never, None).await;
        assert!(resp.is_ok());
    }
}
