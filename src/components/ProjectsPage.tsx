// Projects page: cumulative spending ranked, with a per-project Claude/Codex
// split bar. Clicking a project expands its session history.

import { useEffect, useState } from "react";
import {
  api,
  fmtUsd,
  fmtTokens,
  type ProjectRow,
  type SessionRow,
} from "../lib/api";

export function ProjectsPage({ onBack }: { onBack: () => void }) {
  const [projects, setProjects] = useState<ProjectRow[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [sessions, setSessions] = useState<SessionRow[]>([]);

  useEffect(() => {
    api.getProjects().then(setProjects).catch(() => setProjects([]));
  }, []);

  useEffect(() => {
    if (open) {
      api.getProjectSessions(open).then(setSessions).catch(() => setSessions([]));
    } else {
      setSessions([]);
    }
  }, [open]);

  const totalCost = projects.reduce((s, p) => s + p.cost_usd, 0);

  return (
    <div className="glass-card sub-page">
      <div className="page-head" data-tauri-drag-region>
        <span className="back" onClick={onBack}>‹</span>
        <span className="page-title">按项目</span>
      </div>
      <div className="proj-total">
        累计 <strong>{fmtUsd(totalCost)}</strong> · {projects.length} 个项目
      </div>

      {projects.map((p) => {
        const tokens = p.input_tok + p.output_tok + p.cache_tok;
        const pct = totalCost > 0 ? (p.cost_usd / totalCost) * 100 : 0;
        const totalToolTok = p.claude_tokens + p.codex_tokens;
        const claudeShare = totalToolTok > 0 ? (p.claude_tokens / totalToolTok) * 100 : 0;
        const codexShare = totalToolTok > 0 ? (p.codex_tokens / totalToolTok) * 100 : 0;
        return (
          <div key={p.project}>
            <div
              className={`proj-row ${open === p.project ? "open" : ""}`}
              onClick={() => setOpen(open === p.project ? null : p.project)}
            >
              <div className="proj-ico">📂</div>
              <div className="proj-info">
                <div className="proj-nm">{p.project}</div>
                <div className="proj-mt">
                  {p.session_count} 会话 · {fmtTokens(tokens)} tok
                </div>
                <div className="proj-split">
                  {p.claude_tokens > 0 && (
                    <span><span className="dot" style={{ background: "#ff8c42" }} />Claude {fmtTokens(p.claude_tokens)}</span>
                  )}
                  {p.codex_tokens > 0 && (
                    <span><span className="dot" style={{ background: "#34c759" }} />Codex {fmtTokens(p.codex_tokens)}</span>
                  )}
                </div>
                <div className="proj-bar">
                  <div style={{ width: `${claudeShare}%`, background: "#ff8c42", height: "100%" }} />
                  <div style={{ width: `${codexShare}%`, background: "#34c759", height: "100%" }} />
                </div>
              </div>
              <div className="proj-amt">
                <div className="proj-cost">{fmtUsd(p.cost_usd)}</div>
                <div className="proj-pct">{pct.toFixed(0)}%</div>
              </div>
            </div>
            {open === p.project && (
              <div className="proj-sessions">
                {sessions.length === 0 && <div className="empty-mini">加载中…</div>}
                {sessions.map((s, i) => {
                  const tt = s.input_tok + s.output_tok + s.cache_tok;
                  return (
                    <div className="sess-row" key={i}>
                      <span className="sess-dot" style={{ background: s.tool === "claude" ? "#ff8c42" : "#34c759" }} />
                      <div className="sess-info">
                        <div className="sess-proj">{s.model}</div>
                        <div className="sess-meta">{fmtTokens(tt)} tok · {s.tool}</div>
                      </div>
                      <div className="sess-amt">
                        {fmtUsd(s.cost_usd)}
                        {!s.priced && <small> 未定价</small>}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}
      {projects.length === 0 && <div className="empty-state">暂无项目数据</div>}
    </div>
  );
}
