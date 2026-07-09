//! Core data structures shared across parsers, indexer, and query layers.

use std::path::PathBuf;

/// Which AI tool produced a usage record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tool {
    Claude,
    Codex,
}

impl std::fmt::Display for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tool::Claude => write!(f, "claude"),
            Tool::Codex => write!(f, "codex"),
        }
    }
}

impl Tool {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Tool::Claude),
            "codex" => Some(Tool::Codex),
            _ => None,
        }
    }
}

/// A single normalized token-usage record, the common currency all parsers emit.
///
/// See design doc §5. One record = one billable token chunk (a Claude assistant
/// turn or a Codex token_count delta), not necessarily one session.
#[derive(Debug, Clone)]
pub struct UsageRecord {
    /// YYYY-MM-DD, derived from the record timestamp (local day).
    pub date: String,
    pub tool: Tool,
    pub project: Option<String>,
    pub model: String,
    pub session_id: String,
    pub input_tok: u64,
    pub output_tok: u64,
    /// Cache-read tokens (cache hits).
    pub cache_tok: u64,
    /// Cache-write tokens (cache_creation), billed separately from cache hits.
    pub cache_create_tok: u64,
    /// Unix seconds.
    pub timestamp: i64,
    pub source_file: PathBuf,
}

/// Pricing for one model, per million tokens, in USD. Four cost tiers matching
/// cc-switch's pricing model: input, output, cache read (hit), cache create
/// (write). Unknown models have no row here; the query layer then reports
/// `priced=false` and the UI shows `—`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Pricing {
    pub model: String,
    pub in_per_mtok: f64,
    pub out_per_mtok: f64,
    #[serde(rename = "cache_read_per_mtok")]
    pub cache_read_per_mtok: f64,
    #[serde(rename = "cache_create_per_mtok")]
    pub cache_create_per_mtok: f64,
    pub builtin: bool,
}

/// A queryable time range, used by the detail panel and history views.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Range {
    Today,
    Week,
    Month,
    All,
}
