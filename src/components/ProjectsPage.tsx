// Projects page: projects ordered by latest use, with a per-project
// Claude/Codex split bar. Clicking a project expands its per-model spending
// breakdown.

import { useEffect, useState } from "react";
import {
  api,
  cacheHitRate,
  fmtPercent,
  fmtUsd,
  fmtTokens,
  totalTokens,
  type ModelBreakdown,
  type ProjectRow,
} from "../lib/api";
import { nativeDragMouseDown } from "../lib/drag";

const TOOL_COLOR: Record<string, string> = {
  claude: "#ff8c42",
  codex: "#34c759",
};

// project is stored as the full cwd path (e.g. E:\AI Project\usage-monitoring).
// Show just the last path segment as the display name; keep the full path as
// the key + API argument so drill-down still matches.
function projectName(fullPath: string): string {
  const parts = fullPath.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] || fullPath;
}

export function ProjectsPage({ onBack }: { onBack: () => void }) {
  const [projects, setProjects] = useState<ProjectRow[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [models, setModels] = useState<ModelBreakdown[]>([]);

  useEffect(() => {
    api.getProjects().then(setProjects).catch(() => setProjects([]));
  }, []);

  useEffect(() => {
    if (open) {
      setModels([]);
      api.getProjectByModel(open).then(setModels).catch(() => setModels([]));
    } else {
      setModels([]);
    }
  }, [open]);

  const totalCost = projects.reduce((s, p) => s + p.cost_usd, 0);

  return (
    <div className="glass-card sub-page">
      <div className="page-head" onMouseDown={nativeDragMouseDown("detail")}>
        <span className="back" data-no-drag role="button" onClick={onBack}>‹</span>
        <span className="page-title">按项目</span>
      </div>
      <div className="proj-total">
        累计 <strong>{fmtUsd(totalCost)}</strong> · {projects.length} 个项目
      </div>

      {projects.map((p) => {
        const tokens = totalTokens(p);
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
                <div className="proj-nm" title={p.project}>{projectName(p.project)}</div>
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
                {models.length === 0 && <div className="empty-mini">加载中…</div>}
                {models.map((m) => {
                  const tt = totalTokens(m);
                  if (tt === 0) return null;
                  return (
                    <div className="model-detail" key={m.model + m.tool}>
                      <div className="model-head">
                        <span
                          className="model-dot"
                          style={{ background: TOOL_COLOR[m.tool] ?? "#888" }}
                        />
                        <span className="model-name">
                          {m.model}
                          <small>真实消耗 {fmtTokens(tt)} Tokens</small>
                        </span>
                        {!m.priced && <span className="unpriced-tag">未定价</span>}
                        <span className="model-cost">{fmtUsd(m.cost_usd)}</span>
                      </div>
                      <div className="model-stats">
                        <span>输入 {fmtTokens(m.input_tok)}</span>
                        <span>输出 {fmtTokens(m.output_tok)}</span>
                        <span>命中缓存 {fmtTokens(m.cache_tok)}</span>
                        <span>写入缓存 {fmtTokens(m.cache_create_tok)}</span>
                        <span>命中率 {fmtPercent(cacheHitRate(m))}</span>
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
