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
          {loading ? "…" : fmtTokens(totalTokens)}
          <small className="total-unit">tokens</small>
        </div>
        <div className="total-sub">
          {range === "today" ? "今日" : rangeLabel(range)}花费
          <span className="total-cost">{fmtUsd(summary?.cost_usd ?? 0)}</span>
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

      {/* 7-day trend: each column shows date + bar + tokens + cost */}
      <div className="spark-section">
        <div className="section-title">近 7 天用量</div>
        {history.length < 2 ? (
          <div className="spark-empty">数据不足</div>
        ) : (
          <div className="spark-grid">
            {lastSeven(history).map((d, i) => (
              <SparkCol key={i} day={d} maxTok={maxTok(history)} />
            ))}
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

function lastSeven(history: DayPoint[]): DayPoint[] {
  return history.slice(-7);
}
function maxTok(history: DayPoint[]): number {
  return Math.max(...history.map((d) => d.tokens), 1);
}

// One column of the 7-day trend: date label, a bar whose height reflects token
// volume, then the day's token count and dollar cost.
function SparkCol({
  day,
  maxTok,
}: {
  day: DayPoint;
  maxTok: number;
}) {
  const today = new Date().toISOString().slice(0, 10);
  const isToday = day.date === today;
  const h = Math.max(8, Math.round((day.tokens / maxTok) * 100));
  return (
    <div className="spark-col">
      <div className="spark-col-cost">{fmtUsd(day.cost_usd)}</div>
      <div className="spark-col-tok">{fmtTokens(day.tokens)}</div>
      <div className="spark-col-bar-wrap">
        <div
          className={`spark-bar ${isToday ? "today" : ""}`}
          style={{ height: `${h}%` }}
        />
      </div>
      <div className={`spark-col-date ${isToday ? "today" : ""}`}>
        {day.date.slice(5).replace("-", "/")}
      </div>
    </div>
  );
}
