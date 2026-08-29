//! Atomic persistence for chat turns.
//!
//! `tauri-plugin-sql` exposes individual statements through a pool; a
//! `BEGIN`/`INSERT`/`COMMIT` sequence from the webview is not guaranteed to
//! stay on one SQLite connection. This module owns the one write that must be
//! indivisible: a conversation row and all messages saved for a completed
//! turn.

use rusqlite::Connection;
use serde::Deserialize;

const MAX_MESSAGES_PER_TURN: usize = 100;
const MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_ID_BYTES: usize = 256;
const MAX_CONTEXT_BYTES: usize = 4 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageWrite {
    id: String,
    role: String,
    content: String,
    timestamp: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSaveRequest {
    conversation_id: String,
    program: Option<String>,
    module_id: Option<String>,
    created_at: i64,
    messages: Vec<MessageWrite>,
}

fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS conversations (
           id TEXT PRIMARY KEY,
           created_at INTEGER NOT NULL,
           program TEXT,
           module_id TEXT
         );
         CREATE TABLE IF NOT EXISTS messages (
           id TEXT PRIMARY KEY,
           conversation_id TEXT NOT NULL REFERENCES conversations(id),
           role TEXT NOT NULL,
           content TEXT NOT NULL,
           timestamp INTEGER NOT NULL
         );",
    )
    .map_err(|e| format!("initialize chat database: {e}"))
}

fn validate_turn(turn: &TurnSaveRequest) -> Result<(), String> {
    if turn.conversation_id.trim().is_empty() || turn.conversation_id.len() > MAX_ID_BYTES {
        return Err("conversationId is required".into());
    }
    if turn.messages.is_empty() {
        return Err("a turn must contain at least one message".into());
    }
    if turn.messages.len() > MAX_MESSAGES_PER_TURN {
        return Err(format!(
            "a turn may contain at most {MAX_MESSAGES_PER_TURN} messages"
        ));
    }
    for value in [turn.program.as_deref(), turn.module_id.as_deref()]
        .into_iter()
        .flatten()
    {
        if value.len() > MAX_CONTEXT_BYTES {
            return Err(format!(
                "program and moduleId may not exceed {MAX_CONTEXT_BYTES} bytes"
            ));
        }
    }
    for message in &turn.messages {
        if message.id.trim().is_empty() || message.id.len() > MAX_ID_BYTES {
            return Err(format!("every message id must be 1..={MAX_ID_BYTES} bytes"));
        }
        if !matches!(message.role.as_str(), "user" | "assistant") {
            return Err("message role must be 'user' or 'assistant'".into());
        }
        if message.content.len() > MAX_MESSAGE_BYTES {
            return Err(format!("message content exceeds {MAX_MESSAGE_BYTES} bytes"));
        }
    }
    Ok(())
}

fn save_turn_in(conn: &mut Connection, turn: &TurnSaveRequest) -> Result<(), String> {
    validate_turn(turn)?;
    init_schema(conn)?;
    let tx = conn
        .transaction()
        .map_err(|e| format!("begin chat turn transaction: {e}"))?;
    tx.execute(
        "INSERT OR IGNORE INTO conversations (id, created_at, program, module_id)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            turn.conversation_id,
            turn.created_at,
            turn.program,
            turn.module_id,
        ],
    )
    .map_err(|e| format!("insert conversation: {e}"))?;
    for message in &turn.messages {
        // Conversation text is intentionally persisted for History. Images
        // never cross this command, preserving INV-SEC-004.
        tx.execute(
            "INSERT OR REPLACE INTO messages (id, conversation_id, role, content, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                message.id,
                turn.conversation_id,
                message.role,
                message.content,
                message.timestamp,
            ],
        )
        .map_err(|e| format!("insert message: {e}"))?;
    }
    tx.commit()
        .map_err(|e| format!("commit chat turn transaction: {e}"))
}

#[tauri::command]
pub async fn save_turn_atomic(app: tauri::AppHandle, turn: TurnSaveRequest) -> Result<(), String> {
    use tauri::Manager;

    // The SQL plugin's `sqlite:wordbuddy.db` is app-*config*-relative
    // (tauri-plugin-sql's path_mapper uses `app_config_dir`). Resolve the
    // same concrete file before entering the blocking rusqlite path.
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("resolve app config directory: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create app config directory: {e}"))?;
    let path = dir.join("wordbuddy.db");
    tokio::task::spawn_blocking(move || {
        let mut conn = Connection::open(path).map_err(|e| format!("open chat database: {e}"))?;
        save_turn_in(&mut conn, &turn)
    })
    .await
    .map_err(|e| format!("chat persistence task join: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn() -> TurnSaveRequest {
        TurnSaveRequest {
            conversation_id: "c1".into(),
            program: Some("test".into()),
            module_id: None,
            created_at: 1,
            messages: vec![
                MessageWrite {
                    id: "m1".into(),
                    role: "user".into(),
                    content: "one".into(),
                    timestamp: 1,
                },
                MessageWrite {
                    id: "m2".into(),
                    role: "assistant".into(),
                    content: "two".into(),
                    timestamp: 2,
                },
            ],
        }
    }

    #[test]
    fn turn_is_one_transaction_and_upserts_messages() {
        let mut conn = Connection::open_in_memory().unwrap();
        let mut initial = turn();
        save_turn_in(&mut conn, &initial).unwrap();
        initial.messages[1].content = "updated".into();
        save_turn_in(&mut conn, &initial).unwrap();

        let conversations: i64 = conn
            .query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))
            .unwrap();
        let messages: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        let content: String = conn
            .query_row("SELECT content FROM messages WHERE id = 'm2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            (conversations, messages, content.as_str()),
            (1, 2, "updated")
        );
    }

    #[test]
    fn validation_rejects_ghost_turns_and_untrusted_shape() {
        let mut empty = turn();
        empty.messages.clear();
        assert!(validate_turn(&empty).is_err());

        let mut role = turn();
        role.messages[0].role = "system".into();
        assert!(validate_turn(&role).is_err());

        let mut oversized = turn();
        oversized.conversation_id = "x".repeat(MAX_ID_BYTES + 1);
        assert!(validate_turn(&oversized).is_err());
    }
}
