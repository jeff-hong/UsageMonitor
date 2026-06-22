//! Query layer: read-only commands the frontend calls for aggregated data.
//!
//! See design doc §7. Each command returns a serializable DTO. The detail
//! panel uses today/range summaries; the history page uses the per-day series;
//! the projects page uses the project ranking. Cost is read from the
//! pre-computed `cost_usd` column (written by the indexer), so queries stay
//! cheap — no per-row pricing lookup.

use crate::db::{self, Db};
use crate::models::Range;

/// One tool's contribution within a summary.
#[derive(serde::Serialize)]
pub struct ToolBreakdown {
    pub tool: String,
    pub cost_usd: f64,
    pub input_tok: u64,
    pub output_tok: u64,
    pub cache_tok: u64,
    pub session_count: usize,
    /// true if every record for this tool had a price; false if any was `—`.
    pub fully_priced: bool,
}

/// Top-level summary for a range — drives the detail panel header + tool rows.
#[derive(serde::Serialize)]
pub struct Summary {
    pub cost_usd: f64,
    pub input_tok: u64,
    pub output_tok: u64,
    pub cache_tok: u64,
    pub session_count: usize,
    pub fully_priced: bool,
    pub tools: Vec<ToolBreakdown>,
}

/// One day in the history trend.
#[derive(serde::Serialize)]
pub struct DayPoint {
    pub date: String,
    pub cost_usd: f64,
    pub tokens: u64,
    pub session_count: usize,
}

/// One row of the projects ranking.
#[derive(serde::Serialize)]
pub struct ProjectRow {
    pub project: String,
    pub cost_usd: f64,
    pub input_tok: u64,
    pub output_tok: u64,
    pub cache_tok: u64,
    pub session_count: usize,
    /// token split per tool, for the segmented progress bar.
    pub claude_tokens: u64,
    pub codex_tokens: u64,
}

/// One session within a day (history drill-down) or project.
#[derive(serde::Serialize)]
pub struct SessionRow {
    pub tool: String,
    pub project: Option<String>,
    pub model: String,
    pub cost_usd: f64,
    pub input_tok: u64,
    pub output_tok: u64,
    pub cache_tok: u64,
    pub timestamp: i64,
    pub priced: bool,
}

fn range_clause(r: Range) -> String {
    match r {
        Range::All => "1=1".to_string(),
        Range::Today => {
            let today = today_str();
            format!("date = '{today}'")
        }
        Range::Week | Range::Month => {
            let days = if matches!(r, Range::Week) { 7 } else { 30 };
            format!("date >= date('now','localtime','-{days} day')")
        }
    }
}

fn today_str() -> String {
    use chrono::Local;
    Local::now().format("%Y-%m-%d").to_string()
}

fn read_summary(conn: &rusqlite::Connection, where_clause: &str) -> Summary {
    let base = format!(
        "SELECT
            COALESCE(SUM(cost_usd),0),
            COALESCE(SUM(input_tok),0),
            COALESCE(SUM(output_tok),0),
            COALESCE(SUM(cache_tok),0),
            COUNT(DISTINCT session_id)
         FROM usage_records WHERE {where_clause}"
    );
    let (cost, input, output, cache, sessions): (f64, i64, i64, i64, i64) = conn
        .query_row(&base, [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap_or((0.0, 0, 0, 0, 0));

    let tools = read_tool_breakdown(conn, where_clause);
    let any_unpriced = tools.iter().any(|t| !t.fully_priced);
    Summary {
        cost_usd: cost,
        input_tok: input as u64,
        output_tok: output as u64,
        cache_tok: cache as u64,
        session_count: sessions as usize,
        fully_priced: !any_unpriced,
        tools,
    }
}

fn read_tool_breakdown(conn: &rusqlite::Connection, where_clause: &str) -> Vec<ToolBreakdown> {
    // Per-tool WHERE needs the tool restriction folded in. We AND the tool with
    // the range clause; the range clause is already a complete boolean expr.
    let sql = |tool: &str| {
        format!(
            "SELECT
                COALESCE(SUM(cost_usd),0),
                COALESCE(SUM(input_tok),0),
                COALESCE(SUM(output_tok),0),
                COALESCE(SUM(cache_tok),0),
                COUNT(DISTINCT session_id),
                CASE WHEN MIN(priced)=1 AND COUNT(*)>0 THEN 1 ELSE 0 END
             FROM usage_records WHERE tool='{tool}' AND ({where_clause})"
        )
    };
    let mut out = Vec::new();
    for tool in ["claude", "codex"] {
        let (cost, input, output, cache, sessions, priced): (f64, i64, i64, i64, i64, i64) = conn
            .query_row(&sql(tool), [], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .unwrap_or((0.0, 0, 0, 0, 0, 0));
        out.push(ToolBreakdown {
            tool: tool.to_string(),
            cost_usd: cost,
            input_tok: input as u64,
            output_tok: output as u64,
            cache_tok: cache as u64,
            session_count: sessions as usize,
            fully_priced: priced == 1,
        });
    }
    out
}

// ---- Commands (exposed to frontend) ---------------------------------------

#[tauri::command]
pub fn get_today_summary(state: tauri::State<'_, Db>) -> Summary {
    let conn = state.lock();
    read_summary(&conn, &format!("date = '{}'", today_str()))
}

#[tauri::command]
pub fn get_range_summary(state: tauri::State<'_, Db>, range: Range) -> Summary {
    let conn = state.lock();
    read_summary(&conn, &range_clause(range))
}

#[tauri::command]
pub fn get_history(state: tauri::State<'_, Db>, range: Range) -> Vec<DayPoint> {
    let conn = state.lock();
    let sql = format!(
        "SELECT date,
                COALESCE(SUM(cost_usd),0),
                COALESCE(SUM(input_tok)+SUM(output_tok)+SUM(cache_tok),0),
                COUNT(DISTINCT session_id)
         FROM usage_records WHERE {} GROUP BY date ORDER BY date",
        range_clause(range)
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return vec![];
    };
    stmt.query_map([], |r| {
        Ok(DayPoint {
            date: r.get(0)?,
            cost_usd: r.get(1)?,
            tokens: r.get::<_, i64>(2)? as u64,
            session_count: r.get::<_, i64>(3)? as usize,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

#[tauri::command]
pub fn get_daily_sessions(state: tauri::State<'_, Db>, date: String) -> Vec<SessionRow> {
    let conn = state.lock();
    let Ok(mut stmt) = conn.prepare(
        "SELECT tool, project, model, COALESCE(SUM(cost_usd),0),
                COALESCE(SUM(input_tok),0), COALESCE(SUM(output_tok),0),
                COALESCE(SUM(cache_tok),0), MAX(timestamp), MIN(priced)
         FROM usage_records
         WHERE date = ?1
         GROUP BY session_id ORDER BY MAX(timestamp) DESC",
    ) else {
        return vec![];
    };
    stmt.query_map(rusqlite::params![date], |r| {
        Ok(SessionRow {
            tool: r.get(0)?,
            project: r.get(1)?,
            model: r.get(2)?,
            cost_usd: r.get(3)?,
            input_tok: r.get::<_, i64>(4)? as u64,
            output_tok: r.get::<_, i64>(5)? as u64,
            cache_tok: r.get::<_, i64>(6)? as u64,
            timestamp: r.get(7)?,
            priced: r.get::<_, i64>(8)? == 1,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

#[tauri::command]
pub fn get_projects(state: tauri::State<'_, Db>) -> Vec<ProjectRow> {
    let conn = state.lock();
    let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(project,'(unknown)'),
                COALESCE(SUM(cost_usd),0),
                COALESCE(SUM(input_tok),0),
                COALESCE(SUM(output_tok),0),
                COALESCE(SUM(cache_tok),0),
                COUNT(DISTINCT session_id),
                COALESCE(SUM(CASE WHEN tool='claude' THEN input_tok+output_tok+cache_tok END),0),
                COALESCE(SUM(CASE WHEN tool='codex'  THEN input_tok+output_tok+cache_tok END),0)
         FROM usage_records GROUP BY COALESCE(project,'(unknown)')
         ORDER BY COALESCE(SUM(cost_usd),0) DESC",
    ) else {
        return vec![];
    };
    stmt.query_map([], |r| {
        Ok(ProjectRow {
            project: r.get(0)?,
            cost_usd: r.get(1)?,
            input_tok: r.get::<_, i64>(2)? as u64,
            output_tok: r.get::<_, i64>(3)? as u64,
            cache_tok: r.get::<_, i64>(4)? as u64,
            session_count: r.get::<_, i64>(5)? as usize,
            claude_tokens: r.get::<_, i64>(6)? as u64,
            codex_tokens: r.get::<_, i64>(7)? as u64,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

#[tauri::command]
pub fn recompute_cost(state: tauri::State<'_, Db>) {
    // Re-derive every row's cost from the current pricing table. Triggered when
    // the user edits prices in settings. Shares the SQL with the seed tooling.
    crate::indexer::recompute_all(&state);
}

/// Insert or update one model's price. Used by the settings page to add custom
/// models (e.g. glm-5.1) or override a builtin.
#[tauri::command]
pub fn set_pricing(
    state: tauri::State<'_, Db>,
    model: String,
    in_per_mtok: f64,
    out_per_mtok: f64,
    cache_per_mtok: f64,
) {
    let conn = state.lock();
    let _ = conn.execute(
        "INSERT INTO pricing (model, in_per_mtok, out_per_mtok, cache_per_mtok, builtin)
         VALUES (?1,?2,?3,?4,0)
         ON CONFLICT(model) DO UPDATE SET
            in_per_mtok=excluded.in_per_mtok,
            out_per_mtok=excluded.out_per_mtok,
            cache_per_mtok=excluded.cache_per_mtok",
        rusqlite::params![model, in_per_mtok, out_per_mtok, cache_per_mtok],
    );
}

/// Delete a custom (non-builtin) pricing row. Builtins can't be removed.
#[tauri::command]
pub fn delete_pricing(state: tauri::State<'_, Db>, model: String) -> bool {
    let conn = state.lock();
    conn.execute(
        "DELETE FROM pricing WHERE model = ?1 AND builtin = 0",
        rusqlite::params![model],
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// Models seen in usage data but missing a price row — the settings page lists
/// these so the user knows what to fill in (e.g. their glm-5.1).
#[derive(serde::Serialize)]
pub struct UnpricedModel {
    pub model: String,
    pub tokens: u64,
}

#[tauri::command]
pub fn get_unpriced_models(state: tauri::State<'_, Db>) -> Vec<UnpricedModel> {
    let conn = state.lock();
    let Ok(mut stmt) = conn.prepare(
        "SELECT model, COALESCE(SUM(input_tok)+SUM(output_tok)+SUM(cache_tok),0)
         FROM usage_records
         WHERE model NOT IN (SELECT model FROM pricing)
         GROUP BY model ORDER BY 2 DESC",
    ) else {
        return vec![];
    };
    stmt.query_map([], |r| {
        Ok(UnpricedModel {
            model: r.get(0)?,
            tokens: r.get::<_, i64>(1)? as u64,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

/// All sessions belonging to one project — for the projects-page drill-down.
#[tauri::command]
pub fn get_project_sessions(state: tauri::State<'_, Db>, project: String) -> Vec<SessionRow> {
    let conn = state.lock();
    let Ok(mut stmt) = conn.prepare(
        "SELECT tool, project, model, COALESCE(SUM(cost_usd),0),
                COALESCE(SUM(input_tok),0), COALESCE(SUM(output_tok),0),
                COALESCE(SUM(cache_tok),0), MAX(timestamp), MIN(priced)
         FROM usage_records
         WHERE COALESCE(project,'(unknown)') = ?1
         GROUP BY session_id ORDER BY MAX(timestamp) DESC",
    ) else {
        return vec![];
    };
    stmt.query_map(rusqlite::params![project], |r| {
        Ok(SessionRow {
            tool: r.get(0)?,
            project: r.get(1)?,
            model: r.get(2)?,
            cost_usd: r.get(3)?,
            input_tok: r.get::<_, i64>(4)? as u64,
            output_tok: r.get::<_, i64>(5)? as u64,
            cache_tok: r.get::<_, i64>(6)? as u64,
            timestamp: r.get(7)?,
            priced: r.get::<_, i64>(8)? == 1,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

/// Today's usage broken down per model — mirrors cc-switch's per-model list.
/// Each row: model, tool, new-input/output/cache tokens, cost, priced flag.
#[derive(serde::Serialize)]
pub struct ModelBreakdown {
    pub model: String,
    pub tool: String,
    pub input_tok: u64,
    pub output_tok: u64,
    pub cache_tok: u64,
    pub cost_usd: f64,
    pub priced: bool,
}

#[tauri::command]
pub fn get_today_by_model(state: tauri::State<'_, Db>) -> Vec<ModelBreakdown> {
    let conn = state.lock();
    let today = today_str();
    let Ok(mut stmt) = conn.prepare(
        "SELECT model, tool,
                COALESCE(SUM(input_tok),0),
                COALESCE(SUM(output_tok),0),
                COALESCE(SUM(cache_tok),0),
                COALESCE(SUM(cost_usd),0),
                CASE WHEN MIN(priced)=1 THEN 1 ELSE 0 END
         FROM usage_records WHERE date = ?1
         GROUP BY model, tool ORDER BY COALESCE(SUM(cost_usd),0) DESC",
    ) else {
        return vec![];
    };
    stmt.query_map(rusqlite::params![today], |r| {
        Ok(ModelBreakdown {
            model: r.get(0)?,
            tool: r.get(1)?,
            input_tok: r.get::<_, i64>(2)? as u64,
            output_tok: r.get::<_, i64>(3)? as u64,
            cache_tok: r.get::<_, i64>(4)? as u64,
            cost_usd: r.get(5)?,
            priced: r.get::<_, i64>(6)? == 1,
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

/// Read a settings value (scan interval, widget/taskbar mode). Empty if unset.
#[tauri::command]
pub fn get_setting(state: tauri::State<'_, Db>, key: String) -> Option<String> {
    let conn = state.lock();
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

/// Write a settings value.
#[tauri::command]
pub fn set_setting(state: tauri::State<'_, Db>, key: String, value: String) {
    let conn = state.lock();
    let _ = db::set_setting_raw(&conn, &key, &value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Tool, UsageRecord};
    use std::path::PathBuf;

    fn seeded_db() -> Db {
        let dir = std::env::temp_dir().join(format!("qry-test-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let db = Db::open_at(&dir.join("t.db")).unwrap();
        let recs = vec![
            UsageRecord {
                date: today_str(), tool: Tool::Claude, project: Some("P1".into()),
                model: "claude-sonnet-4".into(), session_id: "s1".into(),
                input_tok: 1_000_000, output_tok: 0, cache_tok: 0, timestamp: 1,
                source_file: PathBuf::from("f1"),
            },
            UsageRecord {
                date: today_str(), tool: Tool::Codex, project: Some("P1".into()),
                model: "gpt-5".into(), session_id: "s2".into(),
                input_tok: 0, output_tok: 1_000_000, cache_tok: 0, timestamp: 2,
                source_file: PathBuf::from("f2"),
            },
            UsageRecord {
                date: "2020-01-01".into(), tool: Tool::Claude, project: None,
                model: "unknown-model".into(), session_id: "s3".into(),
                input_tok: 5_000_000, output_tok: 0, cache_tok: 0, timestamp: 3,
                source_file: PathBuf::from("f3"),
            },
        ];
        crate::indexer::upsert_records(&db, &recs);
        db
    }

    #[test]
    fn today_summary_only_counts_today() {
        let db = seeded_db();
        let conn = db.lock();
        let s = read_summary(&conn, &format!("date = '{}'", today_str()));
        assert_eq!(s.session_count, 2, "two of three records are today");
        // claude 1M input * 3 = 3.0 ; codex 1M output * 10 = 10.0
        assert!((s.cost_usd - 13.0).abs() < 1e-9, "cost {}", s.cost_usd);
        assert_eq!(s.tools.len(), 2);
    }

    #[test]
    fn all_range_includes_old_unpriced() {
        let db = seeded_db();
        let conn = db.lock();
        let s = read_summary(&conn, "1=1");
        assert_eq!(s.session_count, 3);
        assert!(!s.fully_priced, "the 2020 unknown-model record is unpriced");
    }

    #[test]
    fn projects_ranked_by_cost() {
        let db = seeded_db();
        let conn = db.lock();
        let sql = "SELECT COALESCE(project,'(unknown)'), COALESCE(SUM(cost_usd),0)
                   FROM usage_records GROUP BY COALESCE(project,'(unknown)')
                   ORDER BY COALESCE(SUM(cost_usd),0) DESC";
        let mut stmt = conn.prepare(sql).unwrap();
        let rows: Vec<(String, f64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        // P1 = 13.0 (today) ; (unknown) = 0 (unpriced)
        assert_eq!(rows[0].0, "P1");
        assert!((rows[0].1 - 13.0).abs() < 1e-9);
    }
}
