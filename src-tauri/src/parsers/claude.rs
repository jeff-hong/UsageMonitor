//! Parser for Claude Code's session logs.
//!
//! Source: `~/.claude/projects/<encoded-cwd>/<sessionId>.jsonl`
//!
//! Each line is a JSON object. We keep `type:"assistant"` lines and read
//! `message.usage` (input/output/cache tokens) and `message.model`. The
//! project comes from the record's own `cwd` field when present — that's the
//! real working directory and beats reverse-decoding the folder name.

use std::path::{Path, PathBuf};

use super::{epoch_to_local_date, parse_iso_to_epoch, ParseResult, UsageParser};
use crate::models::{Tool, UsageRecord};

pub struct ClaudeParser {
    root: PathBuf,
}

impl ClaudeParser {
    /// Locate via the user's home dir. Returns None-equivalent (empty root) if
    /// the dir is absent, which discover_files turns into an empty list.
    pub fn new() -> Self {
        let root = dirs::home_dir()
            .map(|h| h.join(".claude").join("projects"))
            .unwrap_or_default();
        ClaudeParser { root }
    }

    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        ClaudeParser { root }
    }
}

impl UsageParser for ClaudeParser {
    fn tool(&self) -> Tool {
        Tool::Claude
    }

    fn source_root(&self) -> Option<PathBuf> {
        if self.root.as_os_str().is_empty() || !self.root.exists() {
            None
        } else {
            Some(self.root.clone())
        }
    }

    fn discover_files(&self) -> Vec<PathBuf> {
        let Some(root) = self.source_root() else {
            return vec![];
        };
        let mut out = Vec::new();
        // <root>/<project>/<sessionId>.jsonl — two levels deep.
        walk_jsonl(&root, &mut out);
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
        // If the file shrank (truncated/rotated), restart from the top.
        let start = if start_offset > total {
            0
        } else {
            start_offset
        };
        let slice = &bytes[start as usize..];
        let text = String::from_utf8_lossy(slice);

        let mut records = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Tolerant: skip any line that isn't a valid assistant record.
            let Some(value) = parse_json(line) else {
                continue;
            };
            let obj = match value.as_object() {
                Some(o) => o,
                None => continue,
            };
            if obj.get("type").and_then(|v| v.as_str()) != Some("assistant") {
                continue;
            }
            let Some(message) = obj.get("message").and_then(|v| v.as_object()) else {
                continue;
            };
            let usage = match message.get("usage").and_then(|v| v.as_object()) {
                Some(u) => u,
                None => continue,
            };
            let input = get_u64(usage, "input_tokens");
            let output = get_u64(usage, "output_tokens");
            let cache_create = get_u64(usage, "cache_creation_input_tokens");
            let cache_read = get_u64(usage, "cache_read_input_tokens");
            let model = message
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let ts_str = obj.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            let timestamp = parse_iso_to_epoch(ts_str);
            let session_id = obj
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let project = obj.get("cwd").and_then(|v| v.as_str()).map(str::to_string);
            records.push(UsageRecord {
                date: epoch_to_local_date(timestamp),
                tool: Tool::Claude,
                project,
                model,
                session_id,
                input_tok: input,
                output_tok: output,
                cache_tok: cache_read,
                cache_create_tok: cache_create,
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

fn parse_json(line: &str) -> Option<serde_json::Value> {
    serde_json::from_str(line).ok()
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
    fn parses_assistant_turn_usage() {
        let tmp = std::env::temp_dir().join(format!("claude-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let proj = tmp.join("E--Idea-test");
        std::fs::create_dir_all(&proj).unwrap();
        let line = r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-05-21T07:43:44.373Z","cwd":"E:\\Idea\\test","message":{"model":"claude-sonnet-4","usage":{"input_tokens":24633,"cache_creation_input_tokens":100,"cache_read_input_tokens":200,"output_tokens":411}}}"#;
        let file = write_jsonl(&proj, "s1.jsonl", &[line]);

        let parser = ClaudeParser::with_root(tmp.clone());
        let res = parser.parse_file(&file, 0);
        assert_eq!(res.records.len(), 1);
        let r = &res.records[0];
        assert_eq!(r.tool, Tool::Claude);
        assert_eq!(r.model, "claude-sonnet-4");
        assert_eq!(r.input_tok, 24633);
        assert_eq!(r.output_tok, 411);
        assert_eq!(r.cache_tok, 200);
        assert_eq!(r.cache_create_tok, 100);
        assert_eq!(r.project.as_deref(), Some("E:\\Idea\\test"));
        assert_eq!(r.session_id, "s1");
        assert_eq!(r.date, "2026-05-21"); // local day (UTC+8 would be 05-21 still here)
        assert!(res.new_offset > 0);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn skips_non_assistant_and_malformed_lines() {
        let tmp = std::env::temp_dir().join(format!("claude-test2-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let lines = [
            r#"{"type":"queue-operation","operation":"enqueue"}"#, // not assistant
            "this is not json at all",                             // malformed
            "",                                                    // empty
            r#"{"type":"assistant","message":{"model":"m","usage":{"input_tokens":5,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}},"sessionId":"s2","timestamp":"2026-06-01T00:00:00Z","cwd":"C:\\p"}"#,
        ];
        let file = write_jsonl(&tmp, "x.jsonl", &lines);

        let parser = ClaudeParser::with_root(tmp.clone());
        let res = parser.parse_file(&file, 0);
        assert_eq!(res.records.len(), 1, "only the one assistant line counts");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn resume_from_offset_only_reads_new_bytes() {
        let tmp = std::env::temp_dir().join(format!("claude-test3-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let l1 = r#"{"type":"assistant","sessionId":"s","timestamp":"2026-06-01T00:00:00Z","cwd":"C:\\p","message":{"model":"m","usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
        let l2 = r#"{"type":"assistant","sessionId":"s","timestamp":"2026-06-01T01:00:00Z","cwd":"C:\\p","message":{"model":"m","usage":{"input_tokens":2,"output_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
        let file = write_jsonl(&tmp, "s.jsonl", &[l1, l2]);

        let parser = ClaudeParser::with_root(tmp.clone());
        let first = parser.parse_file(&file, 0);
        assert_eq!(first.records.len(), 2);
        let mid = first.new_offset;
        // Re-parse from the end offset: nothing new -> 0 records, same offset.
        let second = parser.parse_file(&file, mid);
        assert_eq!(second.records.len(), 0);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn handles_truncated_file_by_restarting() {
        let tmp = std::env::temp_dir().join(format!("claude-test4-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let line = r#"{"type":"assistant","sessionId":"s","timestamp":"2026-06-01T00:00:00Z","cwd":"C:\\p","message":{"model":"m","usage":{"input_tokens":9,"output_tokens":9,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#;
        let file = write_jsonl(&tmp, "s.jsonl", &[line]);

        let parser = ClaudeParser::with_root(tmp.clone());
        // Pretend we'd read past the old EOF (offset larger than current size).
        let res = parser.parse_file(&file, 999_999);
        assert_eq!(res.records.len(), 1, "truncated file should restart from 0");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
