// Detail panel: the main interactive surface. Opened from any widget by
// double-click. Shows today's total, a per-tool breakdown with progress bars,
// and a 7-day sparkline. The history/projects sub-pages land in phase 5.

import { useEffect, useState } from "react";
import {
  api,
  fmtUsd,
  fmtTokens,
  type DayPoint,
  type Range,
  type Summary,
} from "../lib/api";
import { HistoryPage } from "./HistoryPage";
import { ProjectsPage } from "./ProjectsPage";

const TOOL_COLOR: Record<string, string> = {
  claude: "#ff8c42",
  codex: "#34c759",
};
const TOOL_LABEL: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
};
const TOOL_ICON: Record<string, string> = { claude: "✦", codex: "◈" };

type View = "overview" | "history" | "projects";

export function DetailPanel({ onClose }: { onClose: () => void }) {
  const [view, setView] = useState<View>("overview");
  const [range, setRange] = useState<Range>("today");
  const [summary, setSummary] = useState<Summary | null>(null);
  const [history, setHistory] = useState<DayPoint[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    Promise.all([api.getRangeSummary(range), api.getHistory("week")])
      .then(([s, h]) => {
        setSummary(s);
        setHistory(h);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, [range]);

  const totalTokens = summary
    ? summary.input_tok + summary.output_tok + summary.cache_tok
    : 0;
  const todayLabel = new Date().toLocaleDateString("zh-CN", {
    weekday: "long",
    month: "long",
    day: "numeric",
  });

  if (view === "history") {
    return <HistoryPage onBack={() => setView("overview")} />;
  }
  if (view === "projects") {
    return <ProjectsPage onBack={() => setView("overview")} />;
  }

  return (
    <div className="glass-card detail-panel">
      <div className="panel-header" data-tauri-drag-region>
        <span className="date-label">{todayLabel}</span>
        <div className="header-actions">
          <button className="icon-btn" onClick={() => setView("history")} title="历史记录">📊</button>
          <button className="icon-btn" onClick={() => setView("projects")} title="按项目">📁</button>
          <button className="icon-btn" onClick={() => {
            // open settings via a custom event the App root listens for
            window.dispatchEvent(new CustomEvent("open-settings"));
          }} title="设置">⚙</button>
          <button className="close-btn" onClick={onClose}>✕</button>
        </div>
      </div>

      <div className="total-block">
        <div className="total-num">
          {loading ? "…" : fmtUsd(summary?.cost_usd ?? 0)}
        </div>
        <div className="total-sub">
          {range === "today" ? "今日" : rangeLabel(range)}花费 ·{" "}
          {fmtTokens(totalTokens)} tokens
        </div>
      </div>

      {/* three summary chips */}
      <div className="chip-row">
        <div className="chip">
          <div className="chip-k">输入</div>
          <div className="chip-v">
            {fmtTokens(summary?.input_tok ?? 0)} <small>tok</small>
          </div>
        </div>
        <div className="chip">
          <div className="chip-k">输出</div>
          <div className="chip-v">
            {fmtTokens(summary?.output_tok ?? 0)} <small>tok</small>
          </div>
        </div>
        <div className="chip">
          <div className="chip-k">缓存</div>
          <div className="chip-v">
            {fmtTokens(summary?.cache_tok ?? 0)} <small>tok</small>
          </div>
        </div>
      </div>

      {/* range tabs */}
      <div className="seg-tabs">
        {(["today", "week", "month", "all"] as Range[]).map((r) => (
          <div
            key={r}
            className={`tab ${range === r ? "active" : ""}`}
            onClick={() => setRange(r)}
          >
            {rangeLabel(r)}
          </div>
        ))}
      </div>

      {/* per-tool breakdown */}
      <div className="tool-section">
        {(summary?.tools ?? []).map((t) => {
          const tt = t.input_tok + t.output_tok + t.cache_tok;
          if (tt === 0) return null;
          const pct = totalTokens > 0 ? Math.round((tt / totalTokens) * 100) : 0;
          return (
            <div className="tool-detail" key={t.tool}>
              <div className="tool-head">
                <span
                  className="tool-icon"
                  style={{
                    background: `${TOOL_COLOR[t.tool]}33`,
                    color: TOOL_COLOR[t.tool],
                  }}
                >
                  {TOOL_ICON[t.tool]}
                </span>
                <div className="tool-meta">
                  <div className="tool-title">
                    {TOOL_LABEL[t.tool] ?? t.tool}
                  </div>
                  <div className="tool-stats">
                    {fmtTokens(tt)} tokens · {t.session_count} 会话
                    {!t.fully_priced && (
                      <span className="unpriced-tag">部分未定价</span>
                    )}
                  </div>
                </div>
                <div className="tool-amt">
                  <div className="amt-cost">{fmtUsd(t.cost_usd)}</div>
                  <div className="amt-pct">{pct}%</div>
                </div>
              </div>
              <div className="tool-bar">
                <div
                  className="tool-fill"
                  style={{ width: `${pct}%`, background: TOOL_COLOR[t.tool] }}
                />
              </div>
            </div>
          );
        })}
        {summary && summary.tools.every((t) => t.input_tok + t.output_tok + t.cache_tok === 0) && (
          <div className="empty-state">该时段暂无使用记录</div>
        )}
      </div>

      {/* 7-day sparkline */}
      <div className="spark-section">
        <div className="section-title">近 7 天花费</div>
        <div className="spark">
          {buildSparkBars(history)}
        </div>
        {history.length >= 2 && (
          <div className="spark-axis">
            <span>{history[0]?.date.slice(5)}</span>
            <span>{history[history.length - 1]?.date.slice(5)}</span>
          </div>
        )}
      </div>

      <div className="panel-footer">
        <button className="footer-btn" onClick={() => setView("history")}>
          📊 历史记录
        </button>
        <button className="footer-btn" onClick={() => setView("projects")}>
          📁 按项目
        </button>
      </div>
    </div>
  );
}

function rangeLabel(r: Range): string {
  return { today: "今日", week: "本周", month: "本月", all: "全部" }[r];
}

function buildSparkBars(history: DayPoint[]) {
  // Take the last 7 days, normalize heights to the max.
  const last7 = history.slice(-7);
  if (last7.length === 0) {
    return <div className="spark-empty">暂无数据</div>;
  }
  const max = Math.max(...last7.map((d) => d.cost_usd), 0.01);
  const today = new Date().toISOString().slice(0, 10);
  return last7.map((d, i) => {
    const h = Math.max(6, Math.round((d.cost_usd / max) * 100));
    return (
      <div
        key={i}
        className={`spark-bar ${d.date === today ? "today" : ""}`}
        style={{ height: `${h}%` }}
        title={`${d.date}: ${fmtUsd(d.cost_usd)}`}
      />
    );
  });
}
