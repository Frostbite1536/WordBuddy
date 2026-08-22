//! Work-journal subsystem (Dayflow-style): background recorder → SQLite
//! storage → analysis pipeline → timeline/standup surfaces. See ADR-042.

pub mod analyzer;
pub mod db;
pub mod export;
pub mod prompts;
pub mod recorder;
pub mod standup;
