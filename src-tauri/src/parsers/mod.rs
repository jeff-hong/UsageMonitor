//! Parsers for each AI tool's local usage logs.
//!
//! See design doc §5. Every tool implements [`UsageParser`], which turns raw
//! JSONL on disk into normalized [`UsageRecord`]s. The indexer (phase 3) is the
//! only caller; it drives all parsers the same way and persists their output.

pub mod claude;
pub mod codex;

use std::path::{Path, PathBuf};

use crate::models::{Tool, UsageRecord};

/// A normalized token counter. Codex's `last_token_usage` already holds the
/// per-turn delta, and Claude's usage is per assistant turn, so we never need
/// to diff across records — both map cleanly into these four buckets.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache: u64,
}

impl TokenCounts {
    pub fn total(self) -> u64 {
        self.input + self.output + self.cache
    }
}

/// Result of parsing one file from a given byte offset: the records found and
/// the new offset to resume from next time.
pub struct ParseResult {
    pub records: Vec<UsageRecord>,
    pub new_offset: u64,
}

/// Pluggable source of usage data. Each AI tool is one implementation.
///
/// The indexer loops: discover files -> for each, resume from its stored
/// `file_state.file_offset` -> parse -> persist -> store the new offset.
pub trait UsageParser {
    fn tool(&self) -> Tool;

    /// Root directory holding this tool's logs (e.g. ~/.claude/projects).
    fn source_root(&self) -> Option<PathBuf>;

    /// All log files under the root, newest-first is fine. Empty if the tool
    /// isn't installed (root missing) — callers treat that as "no data".
    fn discover_files(&self) -> Vec<PathBuf>;

    /// Parse one file starting at `start_offset`. Must be tolerant: a single
    /// malformed line is skipped, never aborts the whole file.
    fn parse_file(&self, path: &Path, start_offset: u64) -> ParseResult;
}

/// Parse an ISO-8601 timestamp (e.g. `2026-05-21T07:43:44.373Z`) into Unix
/// seconds. Returns 0 on failure so a bad timestamp never drops a record.
pub(crate) fn parse_iso_to_epoch(ts: &str) -> i64 {
    use chrono::{DateTime, Utc};
    DateTime::parse_from_rfc3339(ts)
        .map(|d| d.with_timezone(&Utc).timestamp())
        .unwrap_or(0)
}

/// Convert a Unix epoch second to the local YYYY-MM-DD date string.
pub(crate) fn epoch_to_local_date(epoch: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}

#[cfg(test)]
mod integration {
    //! Real-data smoke tests against the user's actual ~/.claude and ~/.codex.
    //! Run with: cargo test --lib parsers::integration -- --ignored --nocapture
    use super::*;
    use crate::parsers::{claude::ClaudeParser, codex::CodexParser};

    #[test]
    #[ignore]
    fn real_claude_parses() {
        let p = ClaudeParser::new();
        let files = p.discover_files();
        println!("claude files found: {}", files.len());
        let mut total = 0;
        let mut tokens = 0u64;
        for f in files.iter().take(50) {
            let r = p.parse_file(f, 0);
            for rec in &r.records {
                tokens += rec.input_tok + rec.output_tok + rec.cache_tok;
            }
            total += r.records.len();
        }
        println!("claude records (first 50 files): {total}, tokens: {tokens}");
        assert!(!files.is_empty(), "expected real claude data");
    }

    #[test]
    #[ignore]
    fn real_codex_parses() {
        let p = CodexParser::new();
        let files = p.discover_files();
        println!("codex files found: {}", files.len());
        let mut total = 0;
        let mut tokens = 0u64;
        for f in files.iter().take(50) {
            let r = p.parse_file(f, 0);
            for rec in &r.records {
                tokens += rec.input_tok + rec.output_tok + rec.cache_tok;
            }
            total += r.records.len();
        }
        println!("codex records (first 50 files): {total}, tokens: {tokens}");
        assert!(!files.is_empty(), "expected real codex data");
    }
}
