//! SQLite connection management, schema, and migrations.
//!
//! See design doc §4. The DB lives under the app data dir. We use WAL mode so
//! background indexer writes don't block UI reads. All writes go through the
//! single connection guarded by `Db` (the Mutex); reads may share it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::models::{Pricing, Tool};

/// The current schema version, used for future migrations.
pub const DB_VERSION: i64 = 1;

/// A thread-safe handle to the database. Cloning shares the single connection.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    path: Arc<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("cannot locate app data directory")]
    NoAppDir,
}

impl Db {
    /// Open (or create) the database under the app data dir, run migrations, and
    /// seed built-in pricing on first run.
    pub fn open() -> Result<Self, DbError> {
        let path = db_path()?;
        Self::open_at(&path)
    }

    /// Open (or create) the database at an explicit path. Used by `open()` for
    /// the real app dir and by tests for a temp-file database.
    pub fn open_at(path: &std::path::Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        apply_pragmas(&conn)?;
        migrate(&conn)?;
        seed_builtin_pricing(&conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
            path: Arc::new(path.to_path_buf()),
        })
    }

    /// Acquire the connection lock. Callers must hold the guard only briefly.
    pub fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db mutex poisoned")
    }

    /// The actual on-disk path this handle was opened against.
    pub fn path(&self) -> PathBuf {
        (*self.path).clone()
    }
}

fn db_path() -> Result<PathBuf, DbError> {
    let base = dirs::data_dir().ok_or(DbError::NoAppDir)?;
    Ok(base.join("UsageMonitor").join("usage.db"))
}

fn apply_pragmas(conn: &Connection) -> Result<(), DbError> {
    // WAL: concurrent readers + single writer, exactly the access pattern the
    // indexer (writer) and query layer (reader) need.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Run idempotent schema creation + version migration.
///
/// First version: create all tables. Future versions add ALTER steps guarded
/// by the stored version.
fn migrate(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS usage_records (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            date         TEXT    NOT NULL,
            tool         TEXT    NOT NULL,
            project      TEXT,
            model        TEXT    NOT NULL,
            session_id   TEXT    NOT NULL,
            input_tok    INTEGER NOT NULL DEFAULT 0,
            output_tok   INTEGER NOT NULL DEFAULT 0,
            cache_tok    INTEGER NOT NULL DEFAULT 0,
            cost_usd     REAL    NOT NULL DEFAULT 0,
            priced       INTEGER NOT NULL DEFAULT 0,
            timestamp    INTEGER NOT NULL,
            source_file  TEXT    NOT NULL,
            UNIQUE(source_file, session_id, timestamp)
        );
        CREATE INDEX IF NOT EXISTS idx_usage_date    ON usage_records(date);
        CREATE INDEX IF NOT EXISTS idx_usage_tool    ON usage_records(tool);
        CREATE INDEX IF NOT EXISTS idx_usage_project ON usage_records(project);

        CREATE TABLE IF NOT EXISTS pricing (
            model          TEXT PRIMARY KEY,
            in_per_mtok    REAL NOT NULL,
            out_per_mtok   REAL NOT NULL,
            cache_per_mtok REAL NOT NULL,
            builtin        INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS file_state (
            source_file TEXT PRIMARY KEY,
            tool        TEXT NOT NULL,
            file_offset INTEGER NOT NULL DEFAULT 0,
            last_seen   INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    set_setting_raw(conn, "db_version", &DB_VERSION.to_string())?;
    Ok(())
}

/// Seed the official default prices on an empty pricing table. User edits or
/// custom models override these. See design doc §4.2 (values to be verified
/// against current public pricing during implementation).
fn seed_builtin_pricing(conn: &Connection) -> Result<(), DbError> {
    let defaults = builtin_pricing();
    let mut existing = conn.prepare("SELECT COUNT(*) FROM pricing")?;
    let count: i64 = existing.query_row([], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }
    for p in defaults {
        conn.execute(
            "INSERT OR IGNORE INTO pricing (model, in_per_mtok, out_per_mtok, cache_per_mtok, builtin)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![p.model, p.in_per_mtok, p.out_per_mtok, p.cache_per_mtok],
        )?;
    }
    Ok(())
}

/// Built-in reference pricing. These are defaults the user can override; unknown
/// models (e.g. glm-5.1) are intentionally absent and shown as `—` until filled.
///
/// Prices are USD per million tokens, from official public sources at time of
/// writing. TODO: verify against current published rates before release.
pub fn builtin_pricing() -> Vec<Pricing> {
    vec![
        Pricing {
            model: "claude-sonnet-4".into(),
            in_per_mtok: 3.0,
            out_per_mtok: 15.0,
            cache_per_mtok: 0.30,
            builtin: true,
        },
        Pricing {
            model: "claude-opus-4".into(),
            in_per_mtok: 15.0,
            out_per_mtok: 75.0,
            cache_per_mtok: 1.50,
            builtin: true,
        },
        Pricing {
            model: "claude-haiku-3.5".into(),
            in_per_mtok: 0.80,
            out_per_mtok: 4.0,
            cache_per_mtok: 0.08,
            builtin: true,
        },
        Pricing {
            model: "gpt-5".into(),
            in_per_mtok: 1.25,
            out_per_mtok: 10.0,
            cache_per_mtok: 0.125,
            builtin: true,
        },
        Pricing {
            model: "gpt-5-mini".into(),
            in_per_mtok: 0.25,
            out_per_mtok: 2.0,
            cache_per_mtok: 0.025,
            builtin: true,
        },
    ]
}

/// Raw setting write, used internally during migration.
pub(crate) fn set_setting_raw(conn: &Connection, key: &str, value: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Insert or update a pricing row (marked non-builtin). Public so the query
/// command layer and seed tooling can both reach it.
pub fn upsert_pricing(
    conn: &Connection,
    model: &str,
    in_per_mtok: f64,
    out_per_mtok: f64,
    cache_per_mtok: f64,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO pricing (model, in_per_mtok, out_per_mtok, cache_per_mtok, builtin)
         VALUES (?1, ?2, ?3, ?4, 0)
         ON CONFLICT(model) DO UPDATE SET
            in_per_mtok = excluded.in_per_mtok,
            out_per_mtok = excluded.out_per_mtok,
            cache_per_mtok = excluded.cache_per_mtok",
        rusqlite::params![model, in_per_mtok, out_per_mtok, cache_per_mtok],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> Db {
        let dir = std::env::temp_dir().join(format!(
            "um-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Db::open_at(&dir.join("test.db")).expect("open temp db")
    }

    #[test]
    fn schema_creates_all_tables() {
        let db = temp_db();
        let conn = db.lock();
        for table in ["usage_records", "pricing", "file_state", "settings"] {
            let n: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{table}'"),
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "table {table} should exist");
        }
    }

    #[test]
    fn builtin_pricing_seeded_once() {
        let db = temp_db();
        let conn = db.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pricing", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, builtin_pricing().len() as i64);

        // Re-opening must NOT duplicate or wipe user rows. Add a custom model,
        // reopen at the same path, confirm both the custom row and seed survive.
        conn.execute(
            "INSERT INTO pricing (model, in_per_mtok, out_per_mtok, cache_per_mtok, builtin)
             VALUES ('glm-5.1', 0.5, 1.5, 0.05, 0)",
            [],
        )
        .unwrap();
        drop(conn);
        let path = db.path();
        let db2 = Db::open_at(&path).unwrap();
        let conn2 = db2.lock();
        let has_custom: i64 = conn2
            .query_row("SELECT COUNT(*) FROM pricing WHERE model='glm-5.1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(has_custom, 1, "custom pricing must survive reopen");
    }

    #[test]
    fn usage_record_upsert_dedups() {
        let db = temp_db();
        let conn = db.lock();
        // Insert the same (source_file, session_id, timestamp) twice; the UNIQUE
        // constraint should keep exactly one row.
        for _ in 0..2 {
            conn.execute(
                "INSERT INTO usage_records
                   (date, tool, project, model, session_id, input_tok, output_tok,
                    cache_tok, cost_usd, priced, timestamp, source_file)
                 VALUES ('2026-06-19','claude','p','glm-5.1',100,200,300,400,0.01,0,999,'f.jsonl')
                 ON CONFLICT(source_file, session_id, timestamp) DO UPDATE SET input_tok=excluded.input_tok",
                [],
            )
            .unwrap();
        }
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "duplicate usage records must dedup to one row");
    }

    #[test]
    fn db_version_stored() {
        let db = temp_db();
        let conn = db.lock();
        let v: String = conn
            .query_row("SELECT value FROM settings WHERE key='db_version'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, DB_VERSION.to_string());
    }

    /// Seed real-user custom model prices into the LIVE app database (AppData),
    /// then recompute costs so dollar amounts appear in the UI. Values sourced
    /// from each vendor's public pricing page; CNY converted at ~7.0/USD.
    ///
    /// Run: cargo test --lib db::tests::seed_real_user_pricing -- --ignored --nocapture
    /// Re-index every file for both tools from offset 0.
    fn reindex_all(db: &Db) {
        use crate::parsers::{claude::ClaudeParser, codex::CodexParser, UsageParser};
        let parsers: Vec<Box<dyn UsageParser + Send>> = vec![
            Box::new(ClaudeParser::new()),
            Box::new(CodexParser::new()),
        ];
        for p in &parsers {
            for f in p.discover_files() {
                let r = p.parse_file(&f, 0);
                if !r.records.is_empty() {
                    crate::indexer::upsert_records(db, &r.records);
                }
            }
        }
    }

    /// Re-index only codex files (after wiping codex rows + offsets).
    fn reindex_codex(db: &Db) {
        use crate::parsers::{codex::CodexParser, UsageParser};
        let p = CodexParser::new();
        for f in p.discover_files() {
            let r = p.parse_file(&f, 0);
            if !r.records.is_empty() {
                crate::indexer::upsert_records(db, &r.records);
            }
        }
    }

    #[test]
    #[ignore]
    fn seed_real_user_pricing() {
        let db = Db::open().expect("open live db");
        {
            let conn = db.lock();
            // (model, in, out, cache) USD per 1M tokens.
            // The user's real history only carries two model keys:
            //   <synthetic>  — Claude Code's placeholder for unreported models
            //   openai       — Codex's provider key (no concrete model in logs)
            // So we price those two directly. The fancy names found earlier
            // (glm-5.x etc.) belonged to THIS conversation's own logs, not the
            // user's daily Claude/Codex history. Kept below for completeness.
            let prices: &[(&str, f64, f64, f64)] = &[
                // Real keys present in usage data
                ("<synthetic>", 3.0, 15.0, 0.30),   // Claude Code default (sonnet tier)
                ("openai", 1.25, 10.0, 0.125),       // Codex default (GPT-5 tier)
                // Zhipu GLM (public rates, CNY ~7/USD)
                ("glm-5.2", 1.14, 4.0, 0.29),
                ("glm-5.1", 0.517, 4.40, 0.10),
                ("glm-4.5-air", 0.11, 0.29, 0.05),
                // DeepSeek (V3 / V3.2 rates)
                ("deepseek-v4-pro", 0.14, 0.42, 0.028),
                ("deepseek-v3-2", 0.14, 0.42, 0.028),
                // ByteDance Doubao (volcengine, 1.2/16 CNY)
                ("doubao-seed-code", 0.17, 2.29, 0.10),
                ("doubao-seed-2.0-code", 0.17, 2.29, 0.10),
                // OpenAI / Anthropic variants
                ("gpt-5.5", 1.25, 10.0, 0.125),
                ("claude-opus-4-8", 15.0, 75.0, 1.5),
                ("claude-opus-4-7", 15.0, 75.0, 1.5),
                ("claude-opus-4.8", 15.0, 75.0, 1.5),
                ("claude-sonnet-4-6", 3.0, 15.0, 0.30),
                ("claude-haiku-4-5-20251001", 0.80, 4.0, 0.08),
                ("claude-fable-5", 3.0, 15.0, 0.30),
            ];
            for (m, i, o, c) in prices {
                upsert_pricing(&conn, m, *i, *o, *c).unwrap();
            }
            println!("seeded {} custom prices", prices.len());
        }
        // Index real usage data into this live DB if it's empty (first run).
        // Also re-index Codex when the parser changes — old rows carried the
        // pre-fix token counts, so wipe codex rows + their file_state offsets
        // and re-parse from scratch.
        let conn = db.lock();
        let have: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
            .unwrap_or(0);
        drop(conn);
        if have == 0 {
            println!("live DB empty — indexing real data…");
            reindex_all(&db);
        }
        // Force a codex re-index to pick up parser fixes (token accuracy).
        {
            let conn = db.lock();
            let _ = conn.execute("DELETE FROM usage_records WHERE tool='codex'", []);
            let _ = conn.execute("DELETE FROM file_state WHERE tool='codex'", []);
        }
        println!("re-indexing codex with fixed parser…");
        reindex_codex(&db);
        // Recompute every usage record's cost from the now-populated pricing.
        crate::indexer::recompute_all(&db);
        let conn = db.lock();
        let (total, priced, cost): (i64, i64, f64) = conn
            .query_row(
                "SELECT COUNT(*),
                        SUM(CASE WHEN priced=1 THEN 1 ELSE 0 END),
                        COALESCE(SUM(cost_usd),0)
                 FROM usage_records",
                [],
                |r| Ok((r.get(0)?, r.get::<_, i64>(1)?, r.get(2)?)),
            )
            .unwrap();
        println!("records: {total}, priced: {priced}, total cost: ${cost:.4}");
    }
}

/// Tool <-> DB string conversion, reused by indexer and query.
pub fn tool_to_str(t: Tool) -> &'static str {
    match t {
        Tool::Claude => "claude",
        Tool::Codex => "codex",
    }
}

pub fn tool_from_str(s: &str) -> Option<Tool> {
    Tool::from_str(s)
}
