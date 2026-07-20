//! AI usage monitor — Tauri backend entry point.
//!
//! Module layout follows the design doc §11 layered architecture:
//! - `models`: shared data structures
//! - `db`: SQLite connection, schema, migrations, pricing seed
//! - (later) `parsers`, `indexer`, `query`

pub mod db;
pub mod ccswitch;
pub mod indexer;
pub mod models;
pub mod parsers;
pub mod query;
pub mod taskbar;
pub mod window_drag;
pub mod widget_mouse;
pub mod windows_topmost;

use models::Pricing;
use tauri::Manager;

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
        "SELECT model, in_per_mtok, out_per_mtok, cache_read_per_mtok, cache_create_per_mtok, builtin
         FROM pricing ORDER BY model",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |r| {
        let builtin_val: i64 = r.get(5)?;
        Ok(Pricing {
            model: r.get::<_, String>(0)?,
            in_per_mtok: r.get::<_, f64>(1)?,
            out_per_mtok: r.get::<_, f64>(2)?,
            cache_read_per_mtok: r.get::<_, f64>(3)?,
            cache_create_per_mtok: r.get::<_, f64>(4)?,
            builtin: builtin_val != 0,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

#[tauri::command]
fn refresh_taskbar(state: tauri::State<'_, db::Db>, app: tauri::AppHandle) {
    taskbar::update_taskbar(&state, &app);
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
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
        .manage(db.clone())
        .setup(move |app| {
            // Apply extended window styles once at startup, then keep them
            // pinned with a low-frequency pump. These floating windows are
            // `WS_EX_NOACTIVATE`, which means Windows never auto-promotes them
            // to top-of-zorder (it only raises windows it activates), so
            // without periodic re-stacking the widget sinks behind the taskbar
            // and other always-on-top windows — that was the "拖到任务栏就不置顶
            // / 点别处就消失" bug.
            //
            // The pump inside `keep_floating_windows_topmost` checks the
            // `DRAGGING` flag and is a no-op while the user is mid-drag, so it
            // no longer causes the drag lag that made us delete it before. A
            // short 250ms tick recovers quickly after taskbar/search steals
            // the topmost band; the pump still skips entirely during drag.
            windows_topmost::keep_floating_windows_topmost(app.handle());
            widget_mouse::install_widget_mouse_hook(app.handle().clone());
            let floating_handle = app.handle().clone();
            std::thread::spawn(move || loop {
                windows_topmost::keep_floating_windows_topmost(&floating_handle);
                std::thread::sleep(std::time::Duration::from_millis(250));
            });

            // Kick off the first-ever full index on a background thread. It emits
            // `index-progress` events to the frontend; after it finishes, a loop
            // re-scans "today" on the user-configured interval (default 30s).
            let db = app.state::<db::Db>().inner().clone();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // Sync pricing from cc-switch BEFORE indexing so the first full
                // index already has accurate costs (163+ models from cc-switch's
                // model_pricing table, including glm-5.x, gpt-5.x, etc.).
                let synced = {
                    let conn = db.lock();
                    let prices = ccswitch::read_ccswitch_pricing();
                    let n = prices.len();
                    for p in &prices {
                        let _ = conn.execute(
                            "INSERT INTO pricing (model, in_per_mtok, out_per_mtok, cache_read_per_mtok, cache_create_per_mtok, builtin)
                             VALUES (?1,?2,?3,?4,?5,1)
                             ON CONFLICT(model) DO UPDATE SET
                                in_per_mtok=excluded.in_per_mtok,
                                out_per_mtok=excluded.out_per_mtok,
                                cache_read_per_mtok=excluded.cache_read_per_mtok,
                                cache_create_per_mtok=excluded.cache_create_per_mtok,
                                builtin=1",
                            rusqlite::params![p.model, p.in_per_mtok, p.out_per_mtok, p.cache_read_per_mtok, p.cache_create_per_mtok],
                        );
                    }
                    n
                };
                if synced > 0 {
                    tracing::info!("synced {synced} model prices from cc-switch");
                }
                indexer::initial_full_index(db.clone(), handle.clone());
                taskbar::update_taskbar(&db, &handle);
                loop {
                    // Read the interval fresh each tick so settings changes apply.
                    let secs = {
                        let conn = db.lock();
                        conn.query_row(
                            "SELECT value FROM settings WHERE key='scan_interval_sec'",
                            [],
                            |r| r.get::<_, String>(0),
                        )
                        .ok()
                        .and_then(|s| s.parse::<u64>().ok())
                        .filter(|&n| n > 0)
                        .unwrap_or(30)
                    };
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                    let snapshot = db.clone();
                    let taskbar_db = db.clone();
                    let taskbar_handle = handle.clone();
                    std::thread::spawn(move || {
                        indexer::incremental_scan(snapshot);
                        taskbar::update_taskbar(&taskbar_db, &taskbar_handle);
                    });
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            db_status,
            list_pricing,
            refresh_taskbar,
            quit_app,
            window_drag::start_window_drag,
            window_drag::hide_window_native,
            window_drag::show_window_native,
            window_drag::place_and_show_window,
            ccswitch::get_current_provider_usage,
            query::get_today_summary,
            query::get_range_summary,
            query::get_history,
            query::get_daily_sessions,
            query::get_projects,
            query::get_project_sessions,
            query::get_by_model,
            query::get_today_by_model,
            query::recompute_cost,
            query::sync_pricing_from_ccswitch,
            query::set_pricing,
            query::delete_pricing,
            query::get_unpriced_models,
            query::get_setting,
            query::set_setting,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
