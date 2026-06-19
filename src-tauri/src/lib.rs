//! AI usage monitor — Tauri backend entry point.
//!
//! Module layout follows the design doc §11 layered architecture:
//! - `models`: shared data structures
//! - `db`: SQLite connection, schema, migrations, pricing seed
//! - (later) `parsers`, `indexer`, `query`

pub mod db;
pub mod models;

use models::Pricing;

/// Report DB readiness + path so the frontend can confirm the backend booted
/// and the database was created. Used as a smoke check in phase 1.
#[tauri::command]
fn db_status(state: tauri::State<'_, db::Db>) -> serde_json::Value {
    let path = state.path().to_string_lossy().into_owned();
    let count = {
        let conn = state.lock();
        conn.prepare("SELECT COUNT(*) FROM pricing")
            .and_then(|mut s| s.query_row([], |r| r.get::<_, i64>(0)))
            .unwrap_or(-1)
    };
    serde_json::json!({ "ok": true, "path": path, "pricing_rows": count })
}

/// Return the built-in pricing list (sanity-check that seed data loaded).
#[tauri::command]
fn list_pricing(state: tauri::State<'_, db::Db>) -> Vec<Pricing> {
    let conn = state.lock();
    let mut stmt = match conn.prepare(
        "SELECT model, in_per_mtok, out_per_mtok, cache_per_mtok, builtin FROM pricing ORDER BY model",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |r| {
        let builtin_val: i64 = r.get(4)?;
        Ok(Pricing {
            model: r.get::<_, String>(0)?,
            in_per_mtok: r.get::<_, f64>(1)?,
            out_per_mtok: r.get::<_, f64>(2)?,
            cache_per_mtok: r.get::<_, f64>(3)?,
            builtin: builtin_val != 0,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize structured logging to a file for debugging parser/indexer issues.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    let db = match db::Db::open() {
        Ok(db) => {
            tracing::info!("database opened at {:?}", db.path());
            db
        }
        Err(e) => {
            tracing::error!("failed to open database: {e}");
            panic!("database initialization failed: {e}");
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(db)
        .invoke_handler(tauri::generate_handler![db_status, list_pricing])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
