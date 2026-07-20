use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, serde::Serialize)]
pub struct ProviderUsage {
    pub app_type: String,
    pub provider_name: String,
    pub provider_id: String,
    pub mode: String,
    pub primary_label: String,
    pub primary_value: String,
    pub primary_updated_text: Option<String>,
    pub secondary_label: Option<String>,
    pub secondary_value: Option<String>,
    pub secondary_updated_text: Option<String>,
    pub updated_text: Option<String>,
    pub ok: bool,
}

#[derive(Debug)]
struct ProviderRow {
    id: String,
    app_type: String,
    name: String,
    settings_config: Value,
    website_url: Option<String>,
    meta: Value,
}

#[tauri::command]
pub async fn get_current_provider_usage(app_type: Option<String>) -> Option<ProviderUsage> {
    let app_type = app_type.unwrap_or_else(|| "codex".to_string());
    // `query_current_provider_usage` does `reqwest::blocking` network calls
    // (up to 10s timeout each). Running that directly in a Tauri command
    // freezes the command thread — and the frontend calls this twice on
    // startup plus every 5s, so a slow/stuck network request made the whole
    // app feel frozen ("打开就卡死"). spawn_blocking moves it onto the
    // blocking thread pool so the UI stays responsive regardless of network.
    let (app_type, inner) =
        tokio::task::spawn_blocking(move || (app_type.clone(), query_current_provider_usage(&app_type)))
            .await
            .unwrap_or_else(|_| (String::new(), Err("join failed".to_string())));
    match inner {
        Ok(Some(usage)) => Some(usage),
        Ok(None) => Some(fallback_missing(&app_type, "未选择")),
        Err(_) => Some(fallback_missing(&app_type, "未配置")),
    }
}

fn query_current_provider_usage(app_type: &str) -> Result<Option<ProviderUsage>, String> {
    let Some(provider) = read_current_provider(app_type)? else {
        return Ok(None);
    };
    let usage_script = provider.meta.get("usage_script").cloned().unwrap_or(Value::Null);
    let template = usage_script
        .get("templateType")
        .and_then(Value::as_str)
        .unwrap_or("general");

    if template == "token_plan" {
        return Ok(Some(query_token_plan(provider, &usage_script)));
    }

    Ok(Some(query_balance_provider(provider, &usage_script)))
}

fn current_provider_id(app_type: &str) -> Result<String, String> {
    let settings = fs::read_to_string(ccswitch_settings_path()).map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_str(&settings).map_err(|e| e.to_string())?;
    let key = match app_type {
        "claude" => "currentProviderClaude",
        "codex" => "currentProviderCodex",
        _ => "currentProviderCodex",
    };
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| format!("missing {key}"))
}

fn find_provider_id_in_db(conn: &Connection, app_type: &str) -> Result<String, String> {
    if let Ok(id) = conn.query_row(
        "SELECT id FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1",
        rusqlite::params![app_type],
        |r| r.get::<_, String>(0),
    ) {
        return Ok(id);
    }

    if let Some(id) = find_provider_id_in_settings_table(conn, app_type)? {
        return Ok(id);
    }

    let count = conn
        .query_row(
            "SELECT COUNT(*) FROM providers WHERE app_type = ?1",
            rusqlite::params![app_type],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    if count == 1 {
        return conn
            .query_row(
                "SELECT id FROM providers WHERE app_type = ?1 LIMIT 1",
                rusqlite::params![app_type],
                |r| r.get::<_, String>(0),
            )
            .map_err(|e| e.to_string());
    }

    conn.query_row(
        "SELECT id FROM providers WHERE app_type = ?1
         ORDER BY COALESCE(sort_index, 999999), COALESCE(created_at, 0) DESC, rowid DESC
         LIMIT 1",
        rusqlite::params![app_type],
        |r| r.get::<_, String>(0),
    )
    .map_err(|e| e.to_string())
}

fn find_provider_id_in_settings_table(
    conn: &Connection,
    app_type: &str,
) -> Result<Option<String>, String> {
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    let needles = match app_type {
        "claude" => ["currentProviderClaude", "current_provider_claude", "claude_provider"],
        "codex" => ["currentProviderCodex", "current_provider_codex", "codex_provider"],
        _ => ["currentProviderCodex", "current_provider_codex", "codex_provider"],
    };
    for row in rows.flatten() {
        let key = row.0.to_lowercase();
        if !needles.iter().any(|needle| key.contains(&needle.to_lowercase())) {
            continue;
        }
        if let Some(id) = extract_provider_id(conn, app_type, &row.1)? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

fn extract_provider_id(
    conn: &Connection,
    app_type: &str,
    raw: &str,
) -> Result<Option<String>, String> {
    if provider_exists(conn, app_type, raw)? {
        return Ok(Some(raw.to_string()));
    }
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Ok(None);
    };
    for pointer in ["/id", "/provider_id", "/providerId", "/current", "/value"] {
        if let Some(id) = value.pointer(pointer).and_then(Value::as_str) {
            if provider_exists(conn, app_type, id)? {
                return Ok(Some(id.to_string()));
            }
        }
    }
    Ok(None)
}

fn provider_exists(conn: &Connection, app_type: &str, id: &str) -> Result<bool, String> {
    if id.trim().is_empty() {
        return Ok(false);
    }
    conn.query_row(
        "SELECT COUNT(*) FROM providers WHERE app_type = ?1 AND id = ?2",
        rusqlite::params![app_type, id],
        |r| r.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .map_err(|e| e.to_string())
}

fn read_provider_from_conn(
    conn: &Connection,
    provider_id: &str,
    app_type: &str,
) -> Result<Option<ProviderRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, app_type, name, settings_config, website_url, meta
             FROM providers WHERE id = ?1 AND app_type = ?2",
        )
        .map_err(|e| e.to_string())?;
    let row = stmt
        .query_row(rusqlite::params![provider_id, app_type], |r| {
            let settings: String = r.get(3)?;
            let meta: String = r.get(5)?;
            Ok(ProviderRow {
                id: r.get(0)?,
                app_type: r.get(1)?,
                name: r.get(2)?,
                settings_config: serde_json::from_str(&settings).unwrap_or(Value::Null),
                website_url: r.get(4)?,
                meta: serde_json::from_str(&meta).unwrap_or(Value::Null),
            })
        })
        .ok();
    Ok(row)
}

fn read_current_provider(app_type: &str) -> Result<Option<ProviderRow>, String> {
    let conn = open_ccswitch_db()?;
    let provider_id = current_provider_id(app_type).or_else(|_| find_provider_id_in_db(&conn, app_type))?;
    if let Some(provider) = read_provider_from_conn(&conn, &provider_id, app_type)? {
        return Ok(Some(provider));
    }
    read_current_provider_row_from_db(&conn, app_type)
}

fn open_ccswitch_db() -> Result<Connection, String> {
    Connection::open_with_flags(ccswitch_db_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())
}

fn read_current_provider_row_from_db(
    conn: &Connection,
    app_type: &str,
) -> Result<Option<ProviderRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, app_type, name, settings_config, website_url, meta
             FROM providers WHERE app_type = ?1 AND is_current = 1 LIMIT 1",
        )
        .map_err(|e| e.to_string())?;
    let provider = stmt
        .query_row(rusqlite::params![app_type], |r| {
            let settings: String = r.get(3)?;
            let meta: String = r.get(5)?;
            Ok(ProviderRow {
                id: r.get(0)?,
                app_type: r.get(1)?,
                name: r.get(2)?,
                settings_config: serde_json::from_str(&settings).unwrap_or(Value::Null),
                website_url: r.get(4)?,
                meta: serde_json::from_str(&meta).unwrap_or(Value::Null),
            })
        })
        .ok();
    Ok(provider)
}

fn query_balance_provider(provider: ProviderRow, usage_script: &Value) -> ProviderUsage {
    let Some(api_key) = read_api_key(&provider.settings_config, &provider.app_type) else {
        return fallback_balance(provider, "未配置");
    };
    let base_url = provider
        .website_url
        .clone()
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_string();
    if base_url.is_empty() {
        return fallback_balance(provider, "无地址");
    }

    let url = script_url(usage_script, &base_url);
    let value =
        fetch_json_with_auth(&url, &api_key, AuthMode::Bearer, script_timeout(usage_script)).ok();
    let Some(value) = value else {
        return fallback_balance(provider, "查询失败");
    };

    let remaining = value
        .get("remaining")
        .or_else(|| value.pointer("/quota/remaining"))
        .or_else(|| value.get("balance"))
        .and_then(number_or_string);
    let unit = value
        .get("unit")
        .or_else(|| value.pointer("/quota/unit"))
        .and_then(Value::as_str)
        .unwrap_or("USD")
        .to_string();

    let Some(remaining) = remaining else {
        return fallback_balance(provider, "无数据");
    };

    ProviderUsage {
        app_type: provider.app_type,
        provider_name: provider.name,
        provider_id: provider.id,
        mode: "balance".to_string(),
        primary_label: "剩余".to_string(),
        primary_value: format_balance(remaining),
        primary_updated_text: None,
        secondary_label: Some(unit),
        secondary_value: None,
        secondary_updated_text: None,
        updated_text: None,
        ok: true,
    }
}

fn query_token_plan(provider: ProviderRow, usage_script: &Value) -> ProviderUsage {
    let plan_provider = usage_script
        .get("codingPlanProvider")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if plan_provider != "zhipu" {
        return fallback_plan(provider, None, None, None, None, false);
    }

    let Some(api_key) = read_api_key(&provider.settings_config, &provider.app_type) else {
        return fallback_plan(provider, None, None, None, None, false);
    };
    let value = fetch_json_with_auth(
        "https://open.bigmodel.cn/api/monitor/usage/quota/limit",
        &api_key,
        AuthMode::Raw,
        script_timeout(usage_script),
    )
    .ok();
    let Some(value) = value else {
        return fallback_plan(provider, None, None, None, None, false);
    };

    let limits = value
        .get("data")
        .and_then(|v| v.get("limits"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let five_hour_limit = limits.iter().find(|limit| {
        limit.get("type").and_then(Value::as_str) == Some("TOKENS_LIMIT")
            && limit.get("unit").and_then(Value::as_i64) == Some(3)
    });
    let five_hour = five_hour_limit.and_then(limit_percentage);
    let five_hour_reset_text = five_hour_limit
        .and_then(|limit| limit.get("nextResetTime"))
        .and_then(Value::as_i64)
        .and_then(format_remaining_time);
    let seven_day_limit = limits.iter().find(|limit| {
        limit.get("type").and_then(Value::as_str) == Some("TOKENS_LIMIT")
            && limit.get("unit").and_then(Value::as_i64) == Some(6)
    });
    let seven_day = seven_day_limit.and_then(limit_percentage);
    let reset_text = seven_day_limit
        .and_then(|limit| limit.get("nextResetTime"))
        .and_then(Value::as_i64)
        .and_then(format_remaining_time);

    let ok = five_hour.is_some() || seven_day.is_some();
    fallback_plan(
        provider,
        five_hour,
        five_hour_reset_text,
        seven_day,
        reset_text,
        ok,
    )
}

fn script_url(usage_script: &Value, base_url: &str) -> String {
    let code = usage_script
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if code.contains("/v1/usage") {
        format!("{}/v1/usage", trim_v1(base_url))
    } else if code.contains("/usage") {
        format!("{}/usage", base_url.trim_end_matches('/'))
    } else {
        format!("{}/usage", base_url.trim_end_matches('/'))
    }
}

fn script_timeout(usage_script: &Value) -> u64 {
    usage_script
        .get("timeout")
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
        .unwrap_or(10)
}

enum AuthMode {
    Bearer,
    Raw,
}

fn fetch_json_with_auth(
    url: &str,
    api_key: &str,
    auth_mode: AuthMode,
    timeout_secs: u64,
) -> Result<Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| e.to_string())?;
    let request = client.get(url);
    let request = match auth_mode {
        AuthMode::Bearer => request.bearer_auth(api_key),
        AuthMode::Raw => request.header(reqwest::header::AUTHORIZATION, api_key),
    };
    request
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .map_err(|e| e.to_string())
}

fn read_api_key(settings: &Value, app_type: &str) -> Option<String> {
    if app_type == "claude" {
        first_string(
            settings,
            &[
                "/env/ANTHROPIC_AUTH_TOKEN",
                "/auth/ANTHROPIC_AUTH_TOKEN",
                "/ANTHROPIC_AUTH_TOKEN",
                "/api_key",
                "/apiKey",
            ],
        )
    } else {
        first_string(
            settings,
            &[
                "/auth/OPENAI_API_KEY",
                "/env/OPENAI_API_KEY",
                "/OPENAI_API_KEY",
                "/api_key",
                "/apiKey",
                "/key",
            ],
        )
    }
}

fn first_string(value: &Value, pointers: &[&str]) -> Option<String> {
    pointers
        .iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .filter(|s| !s.trim().is_empty())
        .map(ToString::to_string)
}

fn fallback_balance(provider: ProviderRow, message: &str) -> ProviderUsage {
    ProviderUsage {
        app_type: provider.app_type,
        provider_name: provider.name,
        provider_id: provider.id,
        mode: "balance".to_string(),
        primary_label: "余额".to_string(),
        primary_value: message.to_string(),
        primary_updated_text: None,
        secondary_label: None,
        secondary_value: None,
        secondary_updated_text: None,
        updated_text: None,
        ok: false,
    }
}

fn fallback_missing(app_type: &str, message: &str) -> ProviderUsage {
    ProviderUsage {
        app_type: app_type.to_string(),
        provider_name: "cc-switch".to_string(),
        provider_id: String::new(),
        mode: "balance".to_string(),
        primary_label: "余额".to_string(),
        primary_value: message.to_string(),
        primary_updated_text: None,
        secondary_label: None,
        secondary_value: None,
        secondary_updated_text: None,
        updated_text: None,
        ok: false,
    }
}

fn fallback_plan(
    provider: ProviderRow,
    five_hour: Option<f64>,
    five_hour_reset_text: Option<String>,
    seven_day: Option<f64>,
    reset_text: Option<String>,
    ok: bool,
) -> ProviderUsage {
    ProviderUsage {
        app_type: provider.app_type,
        provider_name: provider.name,
        provider_id: provider.id,
        mode: "plan".to_string(),
        primary_label: "5小时".to_string(),
        primary_value: five_hour
            .map(format_percent)
            .unwrap_or_else(|| "暂无".to_string()),
        primary_updated_text: five_hour_reset_text,
        secondary_label: Some("7天".to_string()),
        secondary_value: Some(
            seven_day
                .map(format_percent)
                .unwrap_or_else(|| "暂无".to_string()),
        ),
        secondary_updated_text: reset_text.clone(),
        updated_text: reset_text,
        ok,
    }
}

fn number_or_string(value: &Value) -> Option<f64> {
    if let Some(n) = value.as_f64() {
        return Some(n);
    }
    value.as_str()?.parse::<f64>().ok()
}

fn limit_percentage(value: &Value) -> Option<f64> {
    value.get("percentage").and_then(number_or_string)
}

fn format_percent(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}%")
    } else {
        format!("{value:.1}%")
    }
}

fn format_remaining_time(next_reset_ms: i64) -> Option<String> {
    let now_ms = chrono::Local::now().timestamp_millis();
    let remaining_ms = next_reset_ms - now_ms;
    if remaining_ms <= 0 {
        return None;
    }
    let total_minutes = remaining_ms / 60_000;
    let total_hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    // >24h (e.g. the 7-day quota reset) → "X天X小时"; ≤24h (e.g. the 5-hour
    // quota reset) → "X小时X分". Keeps larger windows human-readable instead
    // of showing "168小时".
    if total_hours > 24 {
        let days = total_hours / 24;
        let hours = total_hours % 24;
        Some(format!("{days}天{hours}小时"))
    } else {
        Some(format!("{total_hours}小时{minutes}分"))
    }
}

fn format_balance(value: f64) -> String {
    // Always two decimals, rounded (Rust's float formatting is round-half-to-even,
    // i.e. banker's rounding, which is the standard for money display). Replaces
    // the old size-tiered logic (1/2/3 decimals) so every provider balance shows
    // a consistent 2 decimal places, e.g. $12.50, $0.03, $120.00.
    format!("{value:.2}")
}

fn trim_v1(base_url: &str) -> &str {
    base_url.trim_end_matches('/').trim_end_matches("/v1")
}

fn ccswitch_db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cc-switch")
        .join("cc-switch.db")
}

fn ccswitch_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cc-switch")
        .join("settings.json")
}

/// One model's pricing from cc-switch's `model_pricing` table.
pub struct CcSwitchPrice {
    pub model: String,
    pub in_per_mtok: f64,
    pub out_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_create_per_mtok: f64,
}

/// Read ALL model prices from cc-switch's `model_pricing` table. Returns an
/// empty vec if cc-switch isn't installed or the table doesn't exist (graceful
/// degradation — the caller just keeps the built-in prices).
pub fn read_ccswitch_pricing() -> Vec<CcSwitchPrice> {
    let Ok(conn) = open_ccswitch_db() else {
        return vec![];
    };
    // Check the table exists before querying (older cc-switch versions may not
    // have model_pricing).
    let has_table: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='model_pricing'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n != 0)
        .unwrap_or(false);
    if !has_table {
        return vec![];
    }

    let mut stmt = match conn.prepare(
        "SELECT model_id, input_cost_per_million, output_cost_per_million,
                cache_read_cost_per_million, cache_creation_cost_per_million
         FROM model_pricing",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    stmt.query_map([], |r| {
        let parse = |v: String| v.parse::<f64>().unwrap_or(0.0);
        Ok(CcSwitchPrice {
            model: r.get::<_, String>(0)?,
            in_per_mtok: parse(r.get::<_, String>(1)?),
            out_per_mtok: parse(r.get::<_, String>(2)?),
            cache_read_per_mtok: parse(r.get::<_, String>(3)?),
            cache_create_per_mtok: parse(r.get::<_, String>(4)?),
        })
    })
    .map(|rows| rows.filter_map(Result::ok).collect())
    .unwrap_or_default()
}
