//! Writing analytics (PLAN-05): local-only stats in `writing.sqlite`.
//!
//! Conventions copied from the base repo's `journal/db.rs`: per-op
//! connections (WAL), idempotent schema init, local-midnight day math.
//!
//! INV-PRIV-002 for analytics: rows carry counts and rule names only —
//! never field text. The optional snippet-retention flag (default OFF)
//! gates the weekly tone pass, not the DB.

pub mod aggregate;
pub mod db;
pub mod report;
pub mod vocab;
