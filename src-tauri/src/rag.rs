use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

use crate::HttpClient;

/// Get the path to the RAG-specific SQLite database.
/// Stored alongside config in the OS config directory.
fn rag_db_path() -> Result<PathBuf, String> {
    let base = dirs_next::config_dir()
        .ok_or_else(|| "Could not determine config directory".to_string())?;
    let dir = base.join("workbuddy");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create config dir: {}", e))?;
    Ok(dir.join("rag_vectors.db"))
}

/// Open the RAG SQLite database with WAL mode enabled.
/// All callers must use this to ensure consistent journal mode.
fn open_rag_db() -> Result<rusqlite::Connection, String> {
    let db_path = rag_db_path()?;
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("Failed to open database: {}", e))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| format!("Failed to set WAL mode: {}", e))?;
    Ok(conn)
}

// Resolve `input` and confirm it lives under the user's home directory.
// Prevents a compromised frontend from ingesting /etc/passwd, SSH keys, etc.
fn canonicalize_under_home(input: &str) -> Result<PathBuf, String> {
    let resolved = Path::new(input)
        .canonicalize()
        .map_err(|e| format!("Invalid path {}: {}", input, e))?;
    let home = dirs_next::home_dir()
        .ok_or_else(|| "Could not determine home directory".to_string())?
        .canonicalize()
        .map_err(|e| format!("Could not canonicalize home directory: {}", e))?;
    if !resolved.starts_with(&home) {
        return Err(format!(
            "Path {} is outside the user home directory and cannot be ingested",
            resolved.display()
        ));
    }
    Ok(resolved)
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocChunk {
    pub source_file: String,
    pub chunk_index: usize,
    pub content: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionStatus {
    pub total_chunks: usize,
    pub source_files: Vec<String>,
    pub last_ingested: Option<u64>,
}

// OpenAI embedding API response types
#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

// ---------------------------------------------------------------------------
// Embedding via OpenAI API
// ---------------------------------------------------------------------------

/// Embed text via OpenAI text-embedding-3-small (1536 dims).
/// Uses the shared HttpClient from Tauri state.
async fn embed_text(
    client: &reqwest::Client,
    api_key: &str,
    text: &str,
) -> Result<Vec<f32>, String> {
    let body = serde_json::json!({
        "model": "text-embedding-3-small",
        "input": text,
    });

    let resp = client
        .post("https://api.openai.com/v1/embeddings")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Embedding request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!("OpenAI embedding API {} — {}", status, body_text));
    }

    let parsed: EmbeddingResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse embedding response: {}", e))?;

    parsed
        .data
        .into_iter()
        .next()
        .map(|d| d.embedding)
        .ok_or_else(|| "No embedding returned".to_string())
}

// ---------------------------------------------------------------------------
// Chunking
// ---------------------------------------------------------------------------

/// Split markdown content into chunks of roughly `max_tokens` tokens.
/// Splits on markdown headers first, then paragraph boundaries.
/// Includes header hierarchy as prefix for context.
fn chunk_document(content: &str, max_tokens: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current_header = String::new();
    let mut current_section = String::new();
    let mut in_code_block = false;

    for line in content.lines() {
        // Track fenced code blocks — don't split headers or paragraphs inside them
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            current_section.push_str(line);
            current_section.push('\n');
            continue;
        }

        // Detect markdown headers (only outside code blocks)
        if !in_code_block && (line.starts_with("## ") || line.starts_with("### ") || line.starts_with("# ")) {
            // Flush previous section
            if !current_section.trim().is_empty() {
                let section_text = if current_header.is_empty() {
                    current_section.trim().to_string()
                } else {
                    format!("{}\n{}", current_header, current_section.trim())
                };
                split_into_chunks(&section_text, max_tokens, &mut chunks);
            }
            current_header = line.to_string();
            current_section.clear();
        } else {
            current_section.push_str(line);
            current_section.push('\n');
        }
    }

    // Flush last section
    if !current_section.trim().is_empty() {
        let section_text = if current_header.is_empty() {
            current_section.trim().to_string()
        } else {
            format!("{}\n{}", current_header, current_section.trim())
        };
        split_into_chunks(&section_text, max_tokens, &mut chunks);
    }

    // Filter out trivial chunks (empty / near-empty). A short-but-informative
    // line like "USDC has 6 decimals." is exactly the kind of high-precision
    // hit we want to keep, so the threshold is deliberately low — just drops
    // whitespace-only or single-char noise.
    chunks.retain(|c| c.trim().len() >= 10);
    chunks
}

/// Split a section into chunks of roughly `max_tokens` tokens, breaking on
/// paragraph boundaries. Rough token estimate: 1 token ≈ 4 characters.
///
/// Fenced code blocks (``` … ```) are kept intact even if they exceed
/// `max_chars` — it's better to emit one oversized chunk than to hand
/// the embedder corrupted half-code. Non-code paragraphs that exceed
/// the budget still fall back to line-by-line splitting.
fn split_into_chunks(text: &str, max_tokens: usize, out: &mut Vec<String>) {
    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        out.push(text.to_string());
        return;
    }

    // Tokenize into blocks that respect fenced-code boundaries. A blank
    // line inside a code block is NOT a paragraph separator — treating
    // it as one (as a naive `split("\n\n")` would) rips code blocks apart
    // and corrupts the embeddings for any doc with a long code example.
    let blocks = tokenize_blocks(text);
    let mut current = String::new();

    for block in blocks {
        if block.len() > max_chars {
            if !current.is_empty() {
                out.push(current.trim().to_string());
                current.clear();
            }
            let trimmed = block.trim();
            let is_fenced_code = trimmed.starts_with("```") && trimmed.ends_with("```");
            if is_fenced_code {
                // Keep the fenced block whole even when oversized.
                out.push(trimmed.to_string());
                continue;
            }
            // Plain prose paragraph — split on line boundaries.
            let mut sub = String::new();
            for line in block.lines() {
                if sub.len() + line.len() + 1 > max_chars && !sub.is_empty() {
                    out.push(sub.trim().to_string());
                    sub.clear();
                }
                if !sub.is_empty() {
                    sub.push('\n');
                }
                sub.push_str(line);
            }
            if !sub.trim().is_empty() {
                out.push(sub.trim().to_string());
            }
            continue;
        }
        if current.len() + block.len() + 2 > max_chars && !current.is_empty() {
            out.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(&block);
    }

    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
}

/// Split `text` into logical blocks. Outside of fenced code, blank lines
/// separate paragraphs. Inside a ``` fence, blank lines are content and
/// the entire fenced region is emitted as a single block (including the
/// opening and closing fences, so callers can detect it with `starts_with`
/// / `ends_with`).
///
/// Fence counts are tracked per CommonMark: a fence opens at N backticks
/// (N ≥ 3) and only closes at a line with ≥ N leading backticks followed
/// by nothing but whitespace. A 3-backtick line inside a 4-backtick block
/// is therefore content, not a close — so docs that embed `` ``` ``
/// examples inside `` ```` `` fences aren't torn apart.
fn tokenize_blocks(text: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut fence_level: Option<usize> = None;

    let flush = |blocks: &mut Vec<String>, current: &mut String| {
        let t = current.trim();
        if !t.is_empty() {
            blocks.push(t.to_string());
        }
        current.clear();
    };

    for line in text.split_inclusive('\n') {
        let trim = line.trim_end_matches(|c| c == '\n' || c == '\r');
        let stripped = trim.trim_start();
        let backtick_count = stripped.chars().take_while(|&c| c == '`').count();
        // CommonMark: a fence line is 3+ leading backticks followed by no
        // further backticks in the rest of the line (open can have an info
        // string; close is just the fence + optional whitespace, neither
        // contains more backticks).
        let is_fence_line = backtick_count >= 3
            && stripped.chars().skip(backtick_count).all(|c| c != '`');

        if is_fence_line {
            match fence_level {
                None => {
                    // Opening fence. Record the required close count and
                    // flush any pending prose.
                    flush(&mut blocks, &mut current);
                    current.push_str(line);
                    fence_level = Some(backtick_count);
                    continue;
                }
                Some(open_count) if backtick_count >= open_count => {
                    // Valid close (count ≥ open count). Emit the block.
                    current.push_str(line);
                    fence_level = None;
                    flush(&mut blocks, &mut current);
                    continue;
                }
                Some(_) => {
                    // A fence-like line with fewer backticks than the open
                    // count — it's inline content of the current code block.
                    // Fall through to append-as-content below.
                }
            }
        }

        if fence_level.is_some() {
            // Inside code — preserve blank lines and inner fences as content.
            current.push_str(line);
        } else if trim.is_empty() {
            // Blank line outside code = paragraph boundary.
            flush(&mut blocks, &mut current);
        } else {
            current.push_str(line);
        }
    }

    // Unterminated code block (missing closing fence) — still emit the
    // partial buffer so no content is silently dropped.
    flush(&mut blocks, &mut current);
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_fenced_code_blocks_intact_even_when_oversized() {
        // Build a single fenced code block that exceeds max_chars.
        let big_code_body: String = "let x = 1;\n".repeat(300); // ~3.3k chars
        let code_block = format!("```rust\n{}\n```", big_code_body);
        let input = format!("Prose paragraph here.\n\n{}\n\nAnother paragraph.", code_block);

        let mut out: Vec<String> = Vec::new();
        split_into_chunks(&input, 200, &mut out); // max_chars = 800

        // The code block should appear as a single chunk with balanced fences.
        let code_chunk = out
            .iter()
            .find(|c| c.starts_with("```"))
            .expect("code block chunk missing");
        assert!(code_chunk.starts_with("```rust"));
        assert!(code_chunk.ends_with("```"));
        // Must not have been split — exactly one leading fence and one
        // trailing fence in the chunk.
        assert_eq!(code_chunk.matches("```").count(), 2);
    }

    #[test]
    fn split_still_splits_oversized_prose() {
        // Non-code long paragraph should fall back to line splitting.
        let long_line = "This is a sentence.\n".repeat(200); // ~4000 chars
        let mut out: Vec<String> = Vec::new();
        split_into_chunks(&long_line, 200, &mut out); // max_chars = 800
        assert!(out.len() > 1, "oversized prose must be split, got {} chunks", out.len());
    }

    #[test]
    fn cosine_similarity_zero_norm_returns_zero() {
        let a = vec![0.0_f32; 4];
        let b = vec![1.0_f32, 2.0, 3.0, 4.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
        assert_eq!(cosine_similarity(&b, &a), 0.0);
    }

    #[test]
    fn tokenize_blocks_keeps_inner_triple_backticks_as_content() {
        // Docs about Markdown itself wrap their ``` examples in 4-backtick
        // fences. A fence-level tracker must not treat the inner 3-backtick
        // line as a premature close — else the outer block gets torn apart.
        let input = "\
Before prose.

````markdown
Here is a fenced block:

```rust
let x = 1;
```

End of example.
````

After prose.
";
        let blocks = tokenize_blocks(input);
        assert_eq!(blocks.len(), 3, "got blocks: {:#?}", blocks);

        // Middle block must contain the entire outer fence, inner fences
        // intact, and end at the 4-backtick closer.
        let code = &blocks[1];
        assert!(code.starts_with("````markdown"));
        assert!(code.ends_with("````"));
        // The inner ```rust … ``` pair must still appear in the block as
        // content, not be torn into separate blocks.
        assert!(code.contains("```rust"));
        assert!(code.contains("let x = 1;"));
        // Exactly four ``` occurrences: outer open (3 of the 4 ticks), outer
        // close (3 of the 4 ticks), inner ```rust, inner closing ```.
        // `matches("```")` counts overlapping, so outer fences contribute
        // their full 4-backtick runs (which each contain one triple) once.
        let triple_count = code.matches("```").count();
        assert_eq!(triple_count, 4, "expected 4 triples in: {}", code);
    }

    #[test]
    fn tokenize_blocks_split_preserves_oversized_outer_fence() {
        // Integration with split_into_chunks: a 4-backtick fenced block
        // containing an inner ```rust sample must survive as one chunk.
        let body: String = "let x = 1;\n".repeat(300);
        let input = format!(
            "Intro.\n\n````markdown\n```rust\n{}\n```\n````\n\nOutro.",
            body
        );
        let mut out: Vec<String> = Vec::new();
        split_into_chunks(&input, 200, &mut out); // max_chars = 800

        let code_chunk = out
            .iter()
            .find(|c| c.starts_with("````"))
            .expect("outer fence chunk missing");
        assert!(code_chunk.ends_with("````"));
        assert!(code_chunk.contains("```rust"));
    }
}

// ---------------------------------------------------------------------------
// Cosine similarity
// ---------------------------------------------------------------------------

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a < f32::EPSILON || norm_b < f32::EPSILON {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

// ---------------------------------------------------------------------------
// Serialization helpers for f32 vectors ↔ BLOB
// ---------------------------------------------------------------------------

fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

// ---------------------------------------------------------------------------
// Helper: get OpenAI API key from config
// ---------------------------------------------------------------------------

fn get_openai_key() -> Result<String, String> {
    let key = crate::config::read_api_key("openai")
        .or_else(|_| crate::config::read_api_key("stt"))
        .map_err(|_| "No OpenAI API key configured. Set one in Settings to enable document indexing.".to_string())?;

    if key.is_empty() {
        return Err("No OpenAI API key configured. Set one in Settings to enable document indexing.".to_string());
    }
    Ok(key)
}

// ---------------------------------------------------------------------------
// Tauri Commands
// ---------------------------------------------------------------------------

/// Ingest a single markdown document: read, chunk, embed, store in SQLite.
#[tauri::command]
pub async fn ingest_document(
    app: AppHandle,
    file_path: String,
) -> Result<usize, String> {
    let api_key = get_openai_key()?;
    let http = app.state::<HttpClient>();
    let client = &http.0;

    // Constrain to home dir and whitelist extensions (defense against compromised frontend).
    let safe_path = canonicalize_under_home(&file_path)?;
    let ext_ok = matches!(
        safe_path.extension().and_then(|e| e.to_str()),
        Some("md" | "txt")
    );
    if !ext_ok {
        return Err(format!(
            "Only .md and .txt files may be ingested (got {})",
            safe_path.display()
        ));
    }

    // Read the file
    let content = tokio::fs::read_to_string(&safe_path)
        .await
        .map_err(|e| format!("Failed to read {}: {}", safe_path.display(), e))?;

    let source_file = safe_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Chunk the document (~500 tokens per chunk)
    let chunks = chunk_document(&content, 500);
    if chunks.is_empty() {
        return Ok(0);
    }

    // Embed all chunks first (async network calls)
    let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(chunks.len());
    for chunk_text in &chunks {
        let embedding = embed_text(client, &api_key, chunk_text).await?;
        embeddings.push(embedding);
    }

    // Move all DB operations to a blocking thread to avoid stalling the Tokio executor
    let chunks_owned = chunks;
    let source_file_owned = source_file;
    tokio::task::spawn_blocking(move || {
        let conn = open_rag_db()?;

        // Ensure table exists
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS doc_chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_file TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                content TEXT NOT NULL,
                embedding BLOB,
                program_hint TEXT,
                created_at INTEGER DEFAULT (strftime('%s', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_doc_chunks_source ON doc_chunks(source_file);"
        ).map_err(|e| format!("Failed to create table: {}", e))?;

        // Delete existing chunks for this source file (re-index)
        conn.execute("DELETE FROM doc_chunks WHERE source_file = ?1", [&source_file_owned])
            .map_err(|e| format!("Failed to delete old chunks: {}", e))?;

        let mut inserted = 0;
        for (idx, (chunk_text, embedding)) in chunks_owned.iter().zip(embeddings.iter()).enumerate() {
            let blob = embedding_to_blob(embedding);

            conn.execute(
                "INSERT INTO doc_chunks (source_file, chunk_index, content, embedding) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![source_file_owned, idx as i64, chunk_text, blob],
            ).map_err(|e| format!("Failed to insert chunk: {}", e))?;

            inserted += 1;
        }

        Ok::<usize, String>(inserted)
    })
    .await
    .map_err(|e| format!("DB task failed: {}", e))?
}

/// Recursively collect all .md and .txt files from a directory and its subdirectories.
fn collect_md_files(dir: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, String>> + Send + '_>> {
    Box::pin(async move {
        let mut files = Vec::new();
        let mut entries = tokio::fs::read_dir(dir)
            .await
            .map_err(|e| format!("Failed to read directory {}: {}", dir, e))?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            let path = entry.path();
            if path.is_dir() {
                if let Some(p) = path.to_str() {
                    let mut sub = collect_md_files(p).await?;
                    files.append(&mut sub);
                }
            } else if matches!(path.extension().and_then(|e| e.to_str()), Some("md" | "txt")) {
                if let Some(p) = path.to_str() {
                    files.push(p.to_string());
                }
            }
        }

        Ok(files)
    })
}

/// Ingest all .md and .txt files in a directory (recursive — includes subfolders).
#[tauri::command]
pub async fn ingest_all_documents(
    app: AppHandle,
    directory: String,
) -> Result<usize, String> {
    if directory.is_empty() {
        return Err("Directory path must not be empty. Provide the path to your Limitless Sources folder.".to_string());
    }

    // Constrain to home dir (defense against compromised frontend).
    let safe_dir = canonicalize_under_home(&directory)?;
    let safe_dir_str = safe_dir
        .to_str()
        .ok_or_else(|| "Directory path is not valid UTF-8".to_string())?
        .to_string();

    let mut total = 0;

    // Collect .md files recursively
    let md_files = collect_md_files(&safe_dir_str).await?;

    for file_path in md_files {
        match ingest_document(app.clone(), file_path.clone()).await {
            Ok(count) => total += count,
            Err(e) => {
                // Log error but continue with other files
                eprintln!("Failed to ingest {}: {}", file_path, e);
            }
        }
    }

    Ok(total)
}

/// Search for document chunks relevant to a query.
/// Embeds the query, then computes cosine similarity against all stored chunks.
/// Returns empty array when no OpenAI key is configured (INV-CURR-004).
#[tauri::command]
pub async fn search_docs(
    app: AppHandle,
    query: String,
    top_k: usize,
) -> Result<Vec<DocChunk>, String> {
    // INV-CURR-004: degrade gracefully without OpenAI key — return empty, not error
    let api_key = match get_openai_key() {
        Ok(key) => key,
        Err(_) => return Ok(Vec::new()),
    };
    let http = app.state::<HttpClient>();
    let client = &http.0;

    // Embed the query (async network call)
    let query_embedding = embed_text(client, &api_key, &query).await?;

    // Move all DB + similarity operations to a blocking thread
    tokio::task::spawn_blocking(move || {
        let conn = open_rag_db()?;

        // Check if table exists
        let table_exists: bool = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='doc_chunks'")
            .and_then(|mut stmt| stmt.exists([]))
            .unwrap_or(false);

        if !table_exists {
            return Ok(Vec::new());
        }

        let mut stmt = conn
            .prepare("SELECT source_file, chunk_index, content, embedding FROM doc_chunks WHERE embedding IS NOT NULL")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let mut scored_chunks: Vec<DocChunk> = Vec::new();

        let rows = stmt
            .query_map([], |row| {
                let source_file: String = row.get(0)?;
                let chunk_index: i64 = row.get(1)?;
                let content: String = row.get(2)?;
                let blob: Vec<u8> = row.get(3)?;
                Ok((source_file, chunk_index as usize, content, blob))
            })
            .map_err(|e| format!("Failed to query chunks: {}", e))?;

        for row in rows {
            let (source_file, chunk_index, content, blob) =
                row.map_err(|e| format!("Row error: {}", e))?;
            let embedding = blob_to_embedding(&blob);
            let score = cosine_similarity(&query_embedding, &embedding);

            scored_chunks.push(DocChunk {
                source_file,
                chunk_index,
                content,
                score,
            });
        }

        // Sort by score descending, take top_k
        scored_chunks.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        scored_chunks.truncate(top_k);

        Ok(scored_chunks)
    })
    .await
    .map_err(|e| format!("DB task failed: {}", e))?
}

/// Get the current ingestion status (how many chunks, which files, when last indexed).
#[tauri::command]
pub async fn get_ingestion_status(_app: AppHandle) -> Result<IngestionStatus, String> {
    let db_path = rag_db_path()?;

    if !db_path.exists() {
        return Ok(IngestionStatus {
            total_chunks: 0,
            source_files: Vec::new(),
            last_ingested: None,
        });
    }

    tokio::task::spawn_blocking(move || {
        let conn = open_rag_db()?;

        // Check if table exists
        let table_exists: bool = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='doc_chunks'")
            .and_then(|mut stmt| stmt.exists([]))
            .unwrap_or(false);

        if !table_exists {
            return Ok(IngestionStatus {
                total_chunks: 0,
                source_files: Vec::new(),
                last_ingested: None,
            });
        }

        let total_chunks: usize = conn
            .query_row("SELECT COUNT(*) FROM doc_chunks", [], |row| row.get(0))
            .unwrap_or(0);

        let mut stmt = conn
            .prepare("SELECT DISTINCT source_file FROM doc_chunks ORDER BY source_file")
            .map_err(|e| format!("Failed to query source files: {}", e))?;

        let source_files: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to map source files: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let last_ingested: Option<u64> = conn
            .query_row("SELECT MAX(created_at) FROM doc_chunks", [], |row| row.get(0))
            .ok();

        Ok(IngestionStatus {
            total_chunks,
            source_files,
            last_ingested,
        })
    })
    .await
    .map_err(|e| format!("DB task failed: {}", e))?
}

/// Clear all indexed document chunks.
#[tauri::command]
pub async fn clear_doc_index(_app: AppHandle) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let conn = open_rag_db()?;

        // Use DROP TABLE IF EXISTS — handles the case where no docs have been indexed yet
        conn.execute_batch("DROP TABLE IF EXISTS doc_chunks")
            .map_err(|e| format!("Failed to clear index: {}", e))?;

        Ok(())
    })
    .await
    .map_err(|e| format!("DB task failed: {}", e))?
}
