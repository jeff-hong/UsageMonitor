//! Parser for Codex's session rollouts.
//!
//! Source: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` plus
//! `~/.codex/archived_sessions/rollout-*.jsonl`
//!
//! Records of interest:
//! - `session_meta` — gives us `cwd` (project) and the session id.
//! - `token_count` events — `payload.info.last_token_usage` is the per-turn
//!   delta (input/output/cached). Using `last_token_usage` avoids
//!   any cross-record differencing and stays correct across log rotation.
//!
//! Codex logs don't name a concrete model, only `model_provider`. We record
//! that as the model so the query layer can still match a user-set price.

use std::path::{Path, PathBuf};

use super::{epoch_to_local_date, parse_iso_to_epoch, ParseResult, UsageParser};
use crate::models::{Tool, UsageRecord};

pub struct CodexParser {
    root: PathBuf,
    archive_root: PathBuf,
}

impl CodexParser {
    pub fn new() -> Self {
        let (root, archive_root) = dirs::home_dir()
            .map(|h| {
                let codex = h.join(".codex");
                (codex.join("sessions"), codex.join("archived_sessions"))
            })
            .unwrap_or_default();
        CodexParser { root, archive_root }
    }

    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        let archive_root = root.join("archived_sessions");
        CodexParser { root, archive_root }
    }
}

impl UsageParser for CodexParser {
    fn tool(&self) -> Tool {
        Tool::Codex
    }

    fn source_root(&self) -> Option<PathBuf> {
        if self.root.as_os_str().is_empty() || !self.root.exists() {
            None
        } else {
            Some(self.root.clone())
        }
    }

    fn discover_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(root) = self.source_root() {
            walk_jsonl(&root, &mut out);
        }
        if self.archive_root.exists() {
            walk_jsonl(&self.archive_root, &mut out);
        }
        out
    }

    fn parse_file(&self, path: &Path, start_offset: u64) -> ParseResult {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                return ParseResult {
                    records: vec![],
                    new_offset: start_offset,
                }
            }
        };
        let total = bytes.len() as u64;
        let start = if start_offset > total {
            0
        } else {
            start_offset
        };
        // session_meta gives us project (cwd) + the session id. The concrete
        // model is emitted by turn_context and may change during a session.
        // Keep gpt-5.5 only as a fallback for old logs without either field.
        let mut project: Option<String> = None;
        let mut model = String::from("gpt-5.5");
        let mut session_id = String::new();

        // 增量扫描通常从 session_meta 之后继续读取，因此先重放已读取前缀中的
        // 元数据，确保新追加的 token 记录仍保留项目、会话和最新模型信息。
        let prefix = &bytes[..start as usize];
        for line in String::from_utf8_lossy(prefix).lines() {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                continue;
            };
            let Some(obj) = value.as_object() else {
                continue;
            };
            apply_session_context(obj, &mut project, &mut model, &mut session_id);
        }

        let slice = &bytes[start as usize..];
        let text = String::from_utf8_lossy(slice);

        let mut records = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some(value) = serde_json::from_str::<serde_json::Value>(line).ok() else {
                continue;
            };
            let Some(obj) = value.as_object() else {
                continue;
            };

            if apply_session_context(obj, &mut project, &mut model, &mut session_id) {
                continue;
            }

            // token_count events carry the per-turn delta in last_token_usage.
            if obj.get("type").and_then(|v| v.as_str()) != Some("event_msg") {
                continue;
            }
            let Some(payload) = obj.get("payload").and_then(|v| v.as_object()) else {
                continue;
            };
            if payload.get("type").and_then(|v| v.as_str()) != Some("token_count") {
                continue;
            }
            let Some(info) = payload.get("info").and_then(|v| v.as_object()) else {
                continue;
            };
            // Prefer the per-turn delta; fall back to total if absent.
            let usage = info
                .get("last_token_usage")
                .and_then(|v| v.as_object())
                .or_else(|| info.get("total_token_usage").and_then(|v| v.as_object()));
            let Some(usage) = usage else { continue };

            let mut input = get_u64(usage, "input_tokens");
            let cached = get_u64(usage, "cached_input_tokens");
            let output = get_u64(usage, "output_tokens");
            // cc-switch 口径：Codex 的 input_tokens 保留原始值（包含 cache hit）。
            // 成本计算会用 input - cache_read 作为新增输入成本，展示总量则与
            // cc-switch 一样把 input/cache_read 分栏相加。
            // Codex often leaves the per-field counters at 0 while reporting
            // the real volume only in `total_tokens`. When that happens, fold
            // the total into input so the session isn't dropped as empty.
            if input + cached + output == 0 {
                let total = get_u64(usage, "total_tokens");
                if total == 0 {
                    continue;
                }
                input = total;
            }

            let ts_str = obj.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            let timestamp = parse_iso_to_epoch(ts_str);
            records.push(UsageRecord {
                date: epoch_to_local_date(timestamp),
                tool: Tool::Codex,
                project: project.clone(),
                model: model.clone(),
                session_id: session_id.clone(),
                input_tok: input,
                output_tok: output,
                cache_tok: cached,
                cache_create_tok: 0, // codex logs don't report cache writes
                timestamp,
                source_file: path.to_path_buf(),
            });
        }

        ParseResult {
            records,
            new_offset: total,
        }
    }
}

fn apply_session_context(
    obj: &serde_json::Map<String, serde_json::Value>,
    project: &mut Option<String>,
    model: &mut String,
    session_id: &mut String,
) -> bool {
    let kind = obj.get("type").and_then(|v| v.as_str());
    let Some(payload) = obj.get("payload").and_then(|v| v.as_object()) else {
        return false;
    };

    match kind {
        Some("session_meta") => {
            if let Some(cwd) = payload.get("cwd").and_then(|v| v.as_str()) {
                *project = Some(cwd.to_string());
            }
            if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
                *session_id = id.to_string();
            }
            // Some older/newer Codex logs put the concrete model here.
            if let Some(value) = payload.get("model").and_then(|v| v.as_str()) {
                if value != "openai" && value != "custom" {
                    *model = value.to_string();
                }
            }
            true
        }
        Some("turn_context") => {
            if let Some(value) = payload.get("model").and_then(|v| v.as_str()) {
                *model = value.to_string();
            }
            if let Some(cwd) = payload.get("cwd").and_then(|v| v.as_str()) {
                *project = Some(cwd.to_string());
            }
            true
        }
        _ => false,
    }
}

fn walk_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            walk_jsonl(&path, out);
        } else if ft.is_file() && path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn get_u64(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> u64 {
    obj.get(key)
        .and_then(|v| v.as_u64())
        .or_else(|| obj.get(key).and_then(|v| v.as_f64()).map(|f| f as u64))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_jsonl(dir: &Path, name: &str, lines: &[&str]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, lines.join("\n")).unwrap();
        p
    }

    #[test]
    fn parses_token_count_delta_from_session() {
        let tmp = std::env::temp_dir().join(format!("codex-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let meta = r#"{"timestamp":"2026-06-01T02:39:19.245Z","type":"session_meta","payload":{"id":"sid-1","cwd":"E:\\Dev\\proj","model":"gpt-5"}}"#;
        let tc = r#"{"timestamp":"2026-06-01T02:40:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":50,"total_tokens":200},"last_token_usage":{"input_tokens":100,"cached_input_tokens":30,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":190}}}}"#;
        let file = write_jsonl(&tmp, "rollout-1.jsonl", &[meta, tc]);

        let parser = CodexParser::with_root(tmp.clone());
        let res = parser.parse_file(&file, 0);
        assert_eq!(res.records.len(), 1);
        let r = &res.records[0];
        assert_eq!(r.tool, Tool::Codex);
        assert_eq!(r.project.as_deref(), Some("E:\\Dev\\proj"));
        assert_eq!(r.model, "gpt-5");
        assert_eq!(r.session_id, "sid-1");
        assert_eq!(r.input_tok, 100);
        assert_eq!(r.cache_tok, 30);
        assert_eq!(r.output_tok, 50);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn keeps_gpt_55_when_codex_logs_provider_placeholder() {
        let tmp = std::env::temp_dir().join(format!("codex-test-provider-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let meta = r#"{"timestamp":"2026-06-01T02:39:19.245Z","type":"session_meta","payload":{"id":"sid-1","cwd":"E:\\Dev\\proj","model":"openai"}}"#;
        let tc = r#"{"timestamp":"2026-06-01T02:40:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"total_tokens":150}}}}"#;
        let file = write_jsonl(&tmp, "rollout-provider.jsonl", &[meta, tc]);

        let parser = CodexParser::with_root(tmp.clone());
        let res = parser.parse_file(&file, 0);
        assert_eq!(res.records.len(), 1);
        assert_eq!(res.records[0].model, "gpt-5.5");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn uses_real_model_from_turn_context() {
        let tmp =
            std::env::temp_dir().join(format!("codex-test-model-context-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let meta = r#"{"timestamp":"2026-06-01T02:39:19.245Z","type":"session_meta","payload":{"id":"sid-1","cwd":"E:\\Dev\\proj","model_provider":"custom"}}"#;
        let context = r#"{"timestamp":"2026-06-01T02:39:20.000Z","type":"turn_context","payload":{"cwd":"E:\\Dev\\proj","model":"gpt-5.6-sol"}}"#;
        let usage = r#"{"timestamp":"2026-06-01T02:40:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"total_tokens":150}}}}"#;
        let file = write_jsonl(&tmp, "rollout-context.jsonl", &[meta, context, usage]);

        let parser = CodexParser::with_root(tmp.clone());
        let res = parser.parse_file(&file, 0);

        assert_eq!(res.records.len(), 1);
        assert_eq!(res.records[0].model, "gpt-5.6-sol");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn skips_zero_total_and_non_token_events() {
        let tmp = std::env::temp_dir().join(format!("codex-test2-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let lines = [
            r#"{"type":"agent_message","payload":{}}"#,
            r#"{"type":"event_msg","payload":{"type":"something_else"}}"#,
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":0,"output_tokens":0,"cached_input_tokens":0}}}}"#,
        ];
        let file = write_jsonl(&tmp, "r.jsonl", &lines);
        let parser = CodexParser::with_root(tmp.clone());
        let res = parser.parse_file(&file, 0);
        assert_eq!(res.records.len(), 0, "no real token deltas to record");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn folds_total_tokens_when_fields_are_zero() {
        // Codex's real-world logs often keep per-field counters at 0 and report
        // the whole volume only in total_tokens. The parser must not drop these.
        let tmp = std::env::temp_dir().join(format!("codex-test5-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":0,"output_tokens":0,"cached_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":88343}}}}"#;
        let file = write_jsonl(&tmp, "r.jsonl", &[line]);
        let parser = CodexParser::with_root(tmp.clone());
        let res = parser.parse_file(&file, 0);
        assert_eq!(res.records.len(), 1);
        assert_eq!(res.records[0].input_tok, 88343, "total folded into input");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn tolerates_malformed_lines() {
        let tmp = std::env::temp_dir().join(format!("codex-test3-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let lines = [
            "garbage { not json",
            "",
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":5,"output_tokens":2,"cached_input_tokens":0}}}}"#,
        ];
        let file = write_jsonl(&tmp, "r.jsonl", &lines);
        let parser = CodexParser::with_root(tmp.clone());
        let res = parser.parse_file(&file, 0);
        assert_eq!(res.records.len(), 1);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resume_skips_already_read_bytes() {
        let tmp = std::env::temp_dir().join(format!("codex-test4-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":5,"output_tokens":2,"cached_input_tokens":0}}}}"#;
        let file = write_jsonl(&tmp, "r.jsonl", &[line]);
        let parser = CodexParser::with_root(tmp.clone());
        let first = parser.parse_file(&file, 0);
        assert_eq!(first.records.len(), 1);
        let second = parser.parse_file(&file, first.new_offset);
        assert_eq!(second.records.len(), 0);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resume_keeps_session_metadata_for_appended_records() {
        let tmp =
            std::env::temp_dir().join(format!("codex-test-resume-meta-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let meta = r#"{"timestamp":"2026-06-01T02:39:19.245Z","type":"session_meta","payload":{"id":"sid-1","cwd":"E:\\Dev\\proj","model_provider":"custom"}}"#;
        let context = r#"{"timestamp":"2026-06-01T02:39:20.000Z","type":"turn_context","payload":{"cwd":"E:\\Dev\\proj","model":"gpt-5.6-sol"}}"#;
        let first_usage = r#"{"timestamp":"2026-06-01T02:40:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":5,"cached_input_tokens":1,"output_tokens":2}}}}"#;
        let next_usage = r#"{"timestamp":"2026-06-01T02:41:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":8,"cached_input_tokens":2,"output_tokens":3}}}}"#;
        let file = write_jsonl(&tmp, "r.jsonl", &[meta, context, first_usage]);
        let parser = CodexParser::with_root(tmp.clone());
        let first = parser.parse_file(&file, 0);

        std::fs::write(&file, [meta, context, first_usage, next_usage].join("\n")).unwrap();
        let second = parser.parse_file(&file, first.new_offset);

        assert_eq!(second.records.len(), 1);
        assert_eq!(second.records[0].project.as_deref(), Some("E:\\Dev\\proj"));
        assert_eq!(second.records[0].session_id, "sid-1");
        assert_eq!(second.records[0].model, "gpt-5.6-sol");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
