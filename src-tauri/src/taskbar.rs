//! Windows taskbar integration.
//!
//! The widget window is the taskbar entry. In "live_number" mode we set the
//! window title to today's token total so Windows can show it as the taskbar
//! button label when labels are enabled.

use crate::db::Db;
use chrono::Local;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy)]
struct TokenTotals {
    total: u64,
    claude: u64,
    codex: u64,
}

pub fn update_taskbar(db: &Db, app: &AppHandle) {
    let Some(window) = app.get_webview_window("widget") else {
        return;
    };
    let _ = window.set_skip_taskbar(true);
    let mode = read_mode(db);

    match mode.as_str() {
        "live_number" => {
            let totals = today_totals(db);
            let label = compact_token_label(totals.total);
            let title = format!(
                "{} tokens | Claude {} | Codex {}",
                label,
                compact_token_label(totals.claude),
                compact_token_label(totals.codex)
            );
            let _ = window.set_title(&title);
            if let Some(icon) = app.default_window_icon() {
                let _ = window.set_icon(icon.clone());
            }
        }
        _ => {
            let _ = window.set_title("UsageMonitor");
            if let Some(icon) = app.default_window_icon() {
                let _ = window.set_icon(icon.clone());
            }
        }
    }
}

fn read_mode(db: &Db) -> String {
    let conn = db.lock();
    conn.query_row(
        "SELECT value FROM settings WHERE key='taskbar_mode'",
        [],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| "off".to_string())
}

fn today_totals(db: &Db) -> TokenTotals {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let conn = db.lock();
    let mut out = TokenTotals {
        total: 0,
        claude: 0,
        codex: 0,
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT tool,
                COALESCE(SUM(CASE WHEN input_tok > cache_tok THEN input_tok - cache_tok ELSE 0 END
                    + output_tok + cache_tok + cache_create_tok),0)
         FROM usage_records
         WHERE date = ?1
         GROUP BY tool",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map(rusqlite::params![today], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?.max(0) as u64))
    }) else {
        return out;
    };
    for row in rows.flatten() {
        match row.0.as_str() {
            "claude" => out.claude = row.1,
            "codex" => out.codex = row.1,
            _ => {}
        }
        out.total += row.1;
    }
    out
}

fn compact_token_label(tokens: u64) -> String {
    if tokens >= 1_000_000_000 {
        format!("{:.1}B", tokens as f64 / 1_000_000_000.0)
    } else if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}
