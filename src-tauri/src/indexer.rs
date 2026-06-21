//! Indexer: drives parsers and persists their output into SQLite.
//!
//! Two modes (design doc §6):
//! - **Initial full index**: scanned once on first launch across every file,
//!   emitting progress events so the UI can show "indexing N/M".
//! - **Incremental**: re-scans only "today" files on a timer, resuming each
//!   file from its stored byte offset.
//!
//! Cost is computed at write time from the pricing table, and the `priced`
//! flag records whether the model had a price row so the query layer/UI can
//! render `—` for unpriced models.

use std::collections::HashMap;
use std::path::PathBuf;

use tauri::{AppHandle, Emitter};

use rusqlite::Connection;

use crate::db::{self, Db};
use crate::models::{Tool, UsageRecord};
use crate::parsers::{claude::ClaudeParser, codex::CodexParser, UsageParser};

/// Progress reported during the initial full index. Sent to the frontend as a
/// Tauri event so the detail panel can show "indexing 45/129".
#[derive(Clone, serde::Serialize)]
pub struct IndexProgress {
    pub indexed: usize,
    pub total: usize,
    pub done: bool,
}

/// Run the first-ever full index across all parsers. Idempotent: if already
/// indexed (settings.indexed=1), returns immediately. Emits `index-progress`
/// events to `app` as it goes.
pub fn initial_full_index(db: Db, app: AppHandle) {
    if is_indexed(&db) {
        // One-time data fixes for older parser versions. Rebuild the whole
        // index if either condition holds:
        //  - Claude rows still carry the "<synthetic>" placeholder model
        //  - Codex rows predate the cache-read de-duplication (v2)
        let (synthetic, dedup_marker): (i64, i64) = {
            let conn = db.lock();
            let s = conn
                .query_row(
                    "SELECT COUNT(*) FROM usage_records WHERE model='<synthetic>' AND tool='claude'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let m = conn
                .query_row(
                    "SELECT COUNT(*) FROM settings WHERE key='codex_dedup_v2'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0);
            (s, m)
        };
        if synthetic > 0 || dedup_marker == 0 {
            tracing::info!("rebuilding index (synthetic={synthetic}, dedup_v2={dedup_marker})");
            {
                let conn = db.lock();
                let _ = conn.execute("DELETE FROM usage_records", []);
                let _ = conn.execute("DELETE FROM file_state", []);
                let _ = conn.execute(
                    "INSERT INTO settings(key,value) VALUES('codex_dedup_v2','1')
                     ON CONFLICT(key) DO UPDATE SET value='1'",
                    [],
                );
            }
        } else {
            let _ = app.emit("index-progress", IndexProgress { indexed: 0, total: 0, done: true });
            return;
        }
    }

    let parsers: Vec<Box<dyn UsageParser + Send>> = vec![
        Box::new(ClaudeParser::new()),
        Box::new(CodexParser::new()),
    ];

    // Collect all files up front so we can report a real total.
    let mut files: Vec<(Box<dyn UsageParser + Send>, PathBuf)> = Vec::new();
    for p in parsers {
        let tool = p.tool();
        for f in p.discover_files() {
            files.push((p.boxed_for(tool), f));
        }
    }
    let total = files.len();

    let mut indexed = 0usize;
    for (parser, file) in &files {
        let start = stored_offset(&db, parser.tool(), &file);
        let result = parser.parse_file(file, start);
        if !result.records.is_empty() {
            upsert_records(&db, &result.records);
        }
        set_file_state(&db, parser.tool(), file, result.new_offset);
        indexed += 1;
        if indexed % 10 == 0 || indexed == total {
            let _ = app.emit(
                "index-progress",
                IndexProgress { indexed, total, done: false },
            );
        }
    }

    mark_indexed(&db);
    let _ = app.emit(
        "index-progress",
        IndexProgress { indexed, total, done: true },
    );
}

/// Incremental scan: re-parse every "today" file from its stored offset.
/// Cheap because today's files are few and we resume mid-file.
pub fn incremental_scan(db: Db) {
    let parsers: Vec<Box<dyn UsageParser + Send>> = vec![
        Box::new(ClaudeParser::new()),
        Box::new(CodexParser::new()),
    ];
    for parser in parsers {
        for file in today_files(parser.as_ref()) {
            let start = stored_offset(&db, parser.tool(), &file);
            let result = parser.parse_file(&file, start);
            if !result.records.is_empty() {
                upsert_records(&db, &result.records);
            }
            set_file_state(&db, parser.tool(), &file, result.new_offset);
        }
    }
}

/// Files whose modification time is today. Used by the incremental scan so we
/// don't re-stat the whole tree every tick.
fn today_files(parser: &dyn UsageParser) -> Vec<PathBuf> {
    use chrono::{Local, TimeZone};
    let today = Local::now().date_naive();
    parser
        .discover_files()
        .into_iter()
        .filter(|f| {
            std::fs::metadata(f)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| Local.timestamp_opt(d.as_secs() as i64, 0).single())
                .map(|t| t.date_naive() >= today)
                .unwrap_or(true) // if mtime unreadable, include it to be safe
        })
        .collect()
}

fn is_indexed(db: &Db) -> bool {
    let conn = db.lock();
    get_setting(&conn, "indexed") == Some("1".into())
}

fn mark_indexed(db: &Db) {
    let conn = db.lock();
    let _ = db::set_setting_raw(&conn, "indexed", "1");
}

fn stored_offset(db: &Db, _tool: Tool, file: &std::path::Path) -> u64 {
    let conn = db.lock();
    conn.query_row(
        "SELECT file_offset FROM file_state WHERE source_file = ?1",
        rusqlite::params![file.to_string_lossy()],
        |r| r.get::<_, i64>(0),
    )
    .map(|v| v.max(0) as u64)
    .unwrap_or(0)
}

fn set_file_state(db: &Db, tool: Tool, file: &std::path::Path, offset: u64) {
    let conn = db.lock();
    let _ = conn.execute(
        "INSERT INTO file_state (source_file, tool, file_offset, last_seen)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(source_file) DO UPDATE SET file_offset = excluded.file_offset, last_seen = excluded.last_seen",
        rusqlite::params![
            file.to_string_lossy(),
            db::tool_to_str(tool),
            offset as i64,
            chrono::Local::now().timestamp(),
        ],
    );
}

/// Persist records, computing each one's cost from the pricing table. Bulk-load
/// pricing once per call to avoid a query per record.
pub(crate) fn upsert_records(db: &Db, records: &[UsageRecord]) {
    if records.is_empty() {
        return;
    }
    let mut conn = db.lock();
    let pricing = load_pricing_map(&conn);
    let tx = conn.transaction().unwrap();
    {
        for r in records {
            let (cost, priced) = compute_cost(&pricing, &r.model, r.input_tok, r.output_tok, r.cache_tok);
            let _ = tx.execute(
                "INSERT INTO usage_records
                   (date, tool, project, model, session_id, input_tok, output_tok,
                    cache_tok, cost_usd, priced, timestamp, source_file)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(source_file, session_id, timestamp) DO UPDATE SET
                    input_tok=excluded.input_tok, output_tok=excluded.output_tok,
                    cache_tok=excluded.cache_tok, cost_usd=excluded.cost_usd,
                    priced=excluded.priced",
                rusqlite::params![
                    r.date,
                    db::tool_to_str(r.tool),
                    r.project,
                    r.model,
                    r.session_id,
                    r.input_tok as i64,
                    r.output_tok as i64,
                    r.cache_tok as i64,
                    cost,
                    if priced { 1 } else { 0 },
                    r.timestamp,
                    r.source_file.to_string_lossy(),
                ],
            );
        }
    }
    let _ = tx.commit();
}

fn load_pricing_map(conn: &Connection) -> HashMap<String, (f64, f64, f64)> {
    let mut map = HashMap::new();
    let Ok(mut stmt) = conn.prepare("SELECT model, in_per_mtok, out_per_mtok, cache_per_mtok FROM pricing")
    else {
        return map;
    };
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, f64>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?),
            ))
        })
        .ok();
    if let Some(rows) = rows {
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
    }
    map
}

/// cost = input/1e6*in + output/1e6*out + cache/1e6*cache. Returns (cost, priced).
/// Unknown model -> (0.0, false); the UI then shows `—`.
fn compute_cost(
    pricing: &HashMap<String, (f64, f64, f64)>,
    model: &str,
    input: u64,
    output: u64,
    cache: u64,
) -> (f64, bool) {
    let Some((in_p, out_p, cache_p)) = pricing.get(model) else {
        return (0.0, false);
    };
    let cost = input as f64 / 1_000_000.0 * in_p
        + output as f64 / 1_000_000.0 * out_p
        + cache as f64 / 1_000_000.0 * cache_p;
    (cost, true)
}

/// Re-derive every usage record's cost from the current pricing table. Called
/// when the user edits/adds prices. A single UPDATE beats per-row Rust loops.
pub fn recompute_all(db: &Db) {
    let conn = db.lock();
    let _ = conn.execute(
        "UPDATE usage_records SET
            cost_usd = input_tok/1000000.0 * (SELECT in_per_mtok   FROM pricing p WHERE p.model = usage_records.model)
                      + output_tok/1000000.0 * (SELECT out_per_mtok  FROM pricing p WHERE p.model = usage_records.model)
                      + cache_tok/1000000.0  * (SELECT cache_per_mtok FROM pricing p WHERE p.model = usage_records.model),
            priced = CASE WHEN EXISTS(SELECT 1 FROM pricing p WHERE p.model = usage_records.model) THEN 1 ELSE 0 END",
        [],
    );
}

fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// Helper trait so we can clone a boxed parser while keeping its tool identity
/// when splitting files across the loop. Box<dyn UsageParser> isn't Clone, so we
/// re-create per file from a known tool — cheap, parsers are stateless handles.
trait ParserBoxExt {
    fn boxed_for(&self, tool: Tool) -> Box<dyn UsageParser + Send>;
}

// UsageParser is implemented for the concrete parsers; to re-box we just match
// on tool and construct fresh. This keeps the index loop ownership simple.
impl ParserBoxExt for Box<dyn UsageParser + Send> {
    fn boxed_for(&self, tool: Tool) -> Box<dyn UsageParser + Send> {
        match tool {
            Tool::Claude => Box::new(ClaudeParser::new()),
            Tool::Codex => Box::new(CodexParser::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_known_model_matches_formula() {
        let mut p = HashMap::new();
        p.insert("gpt-5".into(), (1.25, 10.0, 0.125));
        // 2M input, 0.5M output, 1M cache
        let (cost, priced) = compute_cost(&p, "gpt-5", 2_000_000, 500_000, 1_000_000);
        assert!(priced);
        // 2 * 1.25 + 0.5 * 10 + 1 * 0.125 = 2.5 + 5 + 0.125 = 7.625
        assert!((cost - 7.625).abs() < 1e-9, "cost was {cost}");
    }

    #[test]
    fn cost_unknown_model_is_unpriced() {
        let p = HashMap::<String, (f64, f64, f64)>::new();
        let (cost, priced) = compute_cost(&p, "glm-5.1", 1_000_000, 1_000_000, 1_000_000);
        assert!(!priced);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn upsert_then_query_roundtrip() {
        let dir = std::env::temp_dir().join(format!("idx-test-{}", std::process::id()));
        let db = db::Db::open_at(&dir.join("t.db")).unwrap();
        let rec = UsageRecord {
            date: "2026-06-19".into(),
            tool: Tool::Claude,
            project: Some("P".into()),
            model: "claude-sonnet-4".into(),
            session_id: "s".into(),
            input_tok: 1_000_000,
            output_tok: 200_000,
            cache_tok: 0,
            timestamp: 100,
            source_file: PathBuf::from("f.jsonl"),
        };
        upsert_records(&db, std::slice::from_ref(&rec));
        // Insert again — dedup should keep one row.
        upsert_records(&db, std::slice::from_ref(&rec));
        let conn = db.lock();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let (cost, priced): (f64, i64) = conn
            .query_row(
                "SELECT cost_usd, priced FROM usage_records WHERE session_id='s'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(priced, 1, "claude-sonnet-4 has a builtin price");
        // 1M*3 + 0.2M*15 = 3 + 3 = 6.0
        assert!((cost - 6.0).abs() < 1e-9, "cost was {cost}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// End-to-end against real ~/.claude and ~/.codex: parse -> index -> count.
    /// Skips the AppHandle-based `initial_full_index` (no event sink in tests)
    /// and drives parsers + upsert directly. Run with:
    ///   cargo test --lib indexer::tests::real_full_pipeline -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_full_pipeline() {
        let dir = std::env::temp_dir().join(format!("idx-real-{}", std::process::id()));
        let db = db::Db::open_at(&dir.join("real.db")).unwrap();

        let parsers: Vec<Box<dyn UsageParser + Send>> = vec![
            Box::new(crate::parsers::claude::ClaudeParser::new()),
            Box::new(crate::parsers::codex::CodexParser::new()),
        ];
        let mut total_records = 0usize;
        let mut files = 0usize;
        for p in &parsers {
            for f in p.discover_files() {
                let r = p.parse_file(&f, 0);
                files += 1;
                if !r.records.is_empty() {
                    total_records += r.records.len();
                    upsert_records(&db, &r.records);
                }
            }
        }
        let conn = db.lock();
        let (n, cost): (i64, f64) = conn
            .query_row("SELECT COUNT(*), COALESCE(SUM(cost_usd),0) FROM usage_records", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        println!("files: {files}, parsed: {total_records}, in db: {n}, cost: ${cost:.4}");
        assert!(n > 0, "expected real records indexed");
        std::fs::remove_dir_all(&dir).ok();
    }
}
