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

use rusqlite::{Connection, OpenFlags};

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
        let (
            synthetic,
            dedup_marker,
            cache_tiers_marker,
            ccswitch_token_marker,
            ccswitch_project_marker,
        ): (i64, i64, i64, i64, i64) = {
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
            let c = conn
                .query_row(
                    "SELECT COUNT(*) FROM settings WHERE key='cache_tiers_v1'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let t = conn
                .query_row(
                    "SELECT COUNT(*) FROM settings WHERE key='ccswitch_token_v2'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0);
            let p = conn
                .query_row(
                    "SELECT COUNT(*) FROM settings WHERE key='ccswitch_project_v1'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0);
            (s, m, c, t, p)
        };
        if synthetic > 0
            || dedup_marker == 0
            || cache_tiers_marker == 0
            || ccswitch_token_marker == 0
            || ccswitch_project_marker == 0
        {
            tracing::info!("rebuilding index (synthetic={synthetic}, dedup_v2={dedup_marker}, cache_tiers_v1={cache_tiers_marker}, ccswitch_token_v2={ccswitch_token_marker}, ccswitch_project_v1={ccswitch_project_marker})");
            {
                let conn = db.lock();
                let _ = conn.execute("DELETE FROM usage_records", []);
                let _ = conn.execute("DELETE FROM file_state", []);
                let _ = conn.execute(
                    "INSERT INTO settings(key,value) VALUES('codex_dedup_v2','1')
                     ON CONFLICT(key) DO UPDATE SET value='1'",
                    [],
                );
                let _ = conn.execute(
                    "INSERT INTO settings(key,value) VALUES('cache_tiers_v1','1')
                     ON CONFLICT(key) DO UPDATE SET value='1'",
                    [],
                );
                let _ = conn.execute(
                    "INSERT INTO settings(key,value) VALUES('ccswitch_token_v2','1')
                     ON CONFLICT(key) DO UPDATE SET value='1'",
                    [],
                );
                let _ = conn.execute(
                    "INSERT INTO settings(key,value) VALUES('ccswitch_project_v1','1')
                     ON CONFLICT(key) DO UPDATE SET value='1'",
                    [],
                );
            }
        } else {
            let _ = app.emit(
                "index-progress",
                IndexProgress {
                    indexed: 0,
                    total: 0,
                    done: true,
                },
            );
            return;
        }
    }

    if import_ccswitch_all(&db) {
        mark_indexed(&db);
        let _ = app.emit(
            "index-progress",
            IndexProgress {
                indexed: 1,
                total: 1,
                done: true,
            },
        );
        return;
    }

    let parsers: Vec<Box<dyn UsageParser + Send>> =
        vec![Box::new(ClaudeParser::new()), Box::new(CodexParser::new())];

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
                IndexProgress {
                    indexed,
                    total,
                    done: false,
                },
            );
        }
    }

    mark_indexed(&db);
    let _ = app.emit(
        "index-progress",
        IndexProgress {
            indexed,
            total,
            done: true,
        },
    );
}

/// Incremental scan: re-parse every "today" file from its stored offset.
/// Cheap because today's files are few and we resume mid-file.
pub fn incremental_scan(db: Db) {
    if import_ccswitch_today(&db) {
        return;
    }

    let parsers: Vec<Box<dyn UsageParser + Send>> =
        vec![Box::new(ClaudeParser::new()), Box::new(CodexParser::new())];
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
    let _ = db::set_setting_raw(&conn, "codex_dedup_v2", "1");
    let _ = db::set_setting_raw(&conn, "cache_tiers_v1", "1");
    let _ = db::set_setting_raw(&conn, "ccswitch_token_v2", "1");
    let _ = db::set_setting_raw(&conn, "ccswitch_project_v1", "1");
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

fn ccswitch_db_path() -> Option<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".cc-switch").join("cc-switch.db"))
        .filter(|p| p.exists())
}

pub(crate) fn import_ccswitch_all(db: &Db) -> bool {
    let Some(path) = ccswitch_db_path() else {
        return false;
    };
    let Ok(cc_conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return false;
    };
    let rows = read_ccswitch_rows(&cc_conn, None);
    if rows.is_empty() {
        return false;
    }
    let mut conn = db.lock();
    let _ = conn.execute("DELETE FROM usage_records", []);
    let _ = conn.execute("DELETE FROM file_state", []);
    insert_ccswitch_rows(&mut conn, &rows);
    true
}

pub(crate) fn import_ccswitch_today(db: &Db) -> bool {
    let Some(path) = ccswitch_db_path() else {
        return false;
    };
    let Ok(cc_conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return false;
    };
    let start = local_day_start_epoch();
    let rows = read_ccswitch_rows(&cc_conn, Some(start));
    if rows.is_empty() {
        return true;
    }
    let today = epoch_to_local_date(start);
    let mut conn = db.lock();
    let _ = conn.execute(
        "DELETE FROM usage_records WHERE source_file LIKE 'cc-switch:%' AND date = ?1",
        rusqlite::params![today],
    );
    insert_ccswitch_rows(&mut conn, &rows);
    true
}

#[derive(Debug)]
struct CcSwitchRow {
    request_id: String,
    app_type: String,
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    total_cost_usd: f64,
    session_id: String,
    created_at: i64,
}

fn read_ccswitch_rows(conn: &Connection, since_epoch: Option<i64>) -> Vec<CcSwitchRow> {
    let where_clause = since_epoch.map(|_| "WHERE created_at >= ?1").unwrap_or("");
    let sql = format!(
        "SELECT request_id, app_type, model, request_model,
                COALESCE(input_tokens,0), COALESCE(output_tokens,0),
                COALESCE(cache_read_tokens,0), COALESCE(cache_creation_tokens,0),
                COALESCE(CAST(total_cost_usd AS REAL),0),
                session_id, created_at
         FROM proxy_request_logs {where_clause}
         ORDER BY created_at, request_id"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return vec![];
    };
    let map_row = |r: &rusqlite::Row<'_>| {
        let model = r
            .get::<_, Option<String>>(2)?
            .or(r.get::<_, Option<String>>(3)?)
            .unwrap_or_else(|| "unknown".to_string());
        Ok(CcSwitchRow {
            request_id: r.get::<_, String>(0)?,
            app_type: r
                .get::<_, Option<String>>(1)?
                .unwrap_or_else(|| "unknown".to_string()),
            model,
            input_tokens: r.get::<_, i64>(4)?.max(0) as u64,
            output_tokens: r.get::<_, i64>(5)?.max(0) as u64,
            cache_read_tokens: r.get::<_, i64>(6)?.max(0) as u64,
            cache_creation_tokens: r.get::<_, i64>(7)?.max(0) as u64,
            total_cost_usd: r.get::<_, f64>(8)?,
            session_id: r
                .get::<_, Option<String>>(9)?
                .unwrap_or_else(|| r.get::<_, String>(0).unwrap_or_default()),
            created_at: r.get::<_, i64>(10)?,
        })
    };
    let rows = if let Some(since) = since_epoch {
        stmt.query_map(rusqlite::params![since], map_row)
    } else {
        stmt.query_map([], map_row)
    };
    rows.map(|rs| rs.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

fn insert_ccswitch_rows(conn: &mut Connection, rows: &[CcSwitchRow]) {
    let tx = conn.transaction().unwrap();
    for r in rows {
        let tool = match r.app_type.as_str() {
            "claude" => "claude",
            "codex" => "codex",
            _ => continue,
        };
        let project = ccswitch_project_label(tool);
        let _ = tx.execute(
            "INSERT INTO usage_records
               (date, tool, project, model, session_id, input_tok, output_tok,
                cache_tok, cache_create_tok, cost_usd, priced, timestamp, source_file)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,1,?11,?12)
             ON CONFLICT(source_file, session_id, timestamp) DO UPDATE SET
                project=excluded.project, model=excluded.model, input_tok=excluded.input_tok,
                output_tok=excluded.output_tok, cache_tok=excluded.cache_tok,
                cache_create_tok=excluded.cache_create_tok, cost_usd=excluded.cost_usd,
                priced=1",
            rusqlite::params![
                epoch_to_local_date(r.created_at),
                tool,
                project,
                r.model,
                r.session_id,
                r.input_tokens as i64,
                r.output_tokens as i64,
                r.cache_read_tokens as i64,
                r.cache_creation_tokens as i64,
                r.total_cost_usd,
                r.created_at,
                format!("cc-switch:{}", r.request_id),
            ],
        );
    }
    let _ = tx.commit();
}

fn ccswitch_project_label(tool: &str) -> &'static str {
    match tool {
        "claude" => "Claude Code",
        "codex" => "Codex",
        _ => "未分组",
    }
}

fn local_day_start_epoch() -> i64 {
    use chrono::{Local, Timelike};
    Local::now()
        .with_hour(0)
        .and_then(|d| d.with_minute(0))
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .map(|d| d.timestamp())
        .unwrap_or(0)
}

fn epoch_to_local_date(epoch: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(epoch, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
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
            let (cost, priced) = compute_cost(
                &pricing,
                &r.model,
                r.input_tok,
                r.output_tok,
                r.cache_tok,
                r.cache_create_tok,
            );
            let _ = tx.execute(
                "INSERT INTO usage_records
                   (date, tool, project, model, session_id, input_tok, output_tok,
                    cache_tok, cache_create_tok, cost_usd, priced, timestamp, source_file)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                 ON CONFLICT(source_file, session_id, timestamp) DO UPDATE SET
                    input_tok=excluded.input_tok, output_tok=excluded.output_tok,
                    cache_tok=excluded.cache_tok, cache_create_tok=excluded.cache_create_tok,
                    cost_usd=excluded.cost_usd, priced=excluded.priced",
                rusqlite::params![
                    r.date,
                    db::tool_to_str(r.tool),
                    r.project,
                    r.model,
                    r.session_id,
                    r.input_tok as i64,
                    r.output_tok as i64,
                    r.cache_tok as i64,
                    r.cache_create_tok as i64,
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

fn load_pricing_map(conn: &Connection) -> HashMap<String, (f64, f64, f64, f64)> {
    let mut map = HashMap::new();
    // cache_read_per_mtok may be absent on very old DBs (pre-4-tier); fall back
    // to cache_create column name then 0.
    let sql = "SELECT model, in_per_mtok, out_per_mtok,
                      COALESCE(cache_read_per_mtok, 0),
                      COALESCE(cache_create_per_mtok, 0)
               FROM pricing";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return map;
    };
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (
                    r.get::<_, f64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, f64>(4)?,
                ),
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

/// cost = billable_input*in + output*out + cache_read*cr + cache_create*cc (per 1M).
/// Returns (cost, priced). Unknown model -> (0.0, false); UI shows `—`.
fn compute_cost(
    pricing: &HashMap<String, (f64, f64, f64, f64)>,
    model: &str,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_create: u64,
) -> (f64, bool) {
    let Some((in_p, out_p, cr_p, cc_p)) = pricing.get(model) else {
        return (0.0, false);
    };
    let m = 1_000_000.0;
    let billable_input = input.saturating_sub(cache_read);
    let cost = billable_input as f64 / m * in_p
        + output as f64 / m * out_p
        + cache_read as f64 / m * cr_p
        + cache_create as f64 / m * cc_p;
    (cost, true)
}

/// Re-derive every usage record's cost from the current pricing table. Called
/// when the user edits/adds prices. A single UPDATE beats per-row Rust loops.
pub fn recompute_all(db: &Db) {
    let conn = db.lock();
    let _ = conn.execute(
        "UPDATE usage_records SET
            cost_usd = (CASE WHEN input_tok > cache_tok THEN input_tok - cache_tok ELSE 0 END)/1000000.0
                        * (SELECT in_per_mtok         FROM pricing p WHERE p.model = usage_records.model)
                      + output_tok/1000000.0       * (SELECT out_per_mtok        FROM pricing p WHERE p.model = usage_records.model)
                      + cache_tok/1000000.0        * (SELECT cache_read_per_mtok FROM pricing p WHERE p.model = usage_records.model)
                      + cache_create_tok/1000000.0 * (SELECT cache_create_per_mtok FROM pricing p WHERE p.model = usage_records.model),
            priced = CASE WHEN EXISTS(SELECT 1 FROM pricing p WHERE p.model = usage_records.model) THEN 1 ELSE 0 END
         WHERE source_file NOT LIKE 'cc-switch:%'",
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
        p.insert("gpt-5".into(), (1.25, 10.0, 0.125, 1.25));
        // 2M raw input, 0.5M output, 1M cache read, 0.4M cache write
        let (cost, priced) = compute_cost(&p, "gpt-5", 2_000_000, 500_000, 1_000_000, 400_000);
        assert!(priced);
        // (2-1)*1.25 + 0.5*10 + 1*0.125 + 0.4*1.25 = 6.875
        assert!((cost - 6.875).abs() < 1e-9, "cost was {cost}");
    }

    #[test]
    fn cost_unknown_model_is_unpriced() {
        let p = HashMap::<String, (f64, f64, f64, f64)>::new();
        let (cost, priced) =
            compute_cost(&p, "glm-5.1", 1_000_000, 1_000_000, 1_000_000, 1_000_000);
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
            cache_create_tok: 0,
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
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(cost_usd),0) FROM usage_records",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        println!("files: {files}, parsed: {total_records}, in db: {n}, cost: ${cost:.4}");
        assert!(n > 0, "expected real records indexed");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Import cc-switch's own accounting DB into the live app database.
    /// Run with:
    ///   cargo test --lib indexer::tests::sync_live_ccswitch -- --ignored --nocapture
    #[test]
    #[ignore]
    fn sync_live_ccswitch() {
        let db = db::Db::open().unwrap();
        assert!(
            import_ccswitch_all(&db),
            "expected cc-switch rows to import"
        );
        mark_indexed(&db);
        let conn = db.lock();
        let (rows, cost): (i64, f64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(cost_usd),0) FROM usage_records",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        println!("cc-switch rows imported: {rows}, total cost: ${cost:.4}");
    }
}
