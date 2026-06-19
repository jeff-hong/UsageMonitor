// History page: a trend chart over the selected range plus a per-day breakdown
// that drills into that day's sessions. Reached from the detail panel footer.

import { useEffect, useState } from "react";
import { api, fmtUsd, fmtTokens, type DayPoint, type Range, type SessionRow } from "../lib/api";

const TOOL_COLOR: Record<string, string> = { claude: "#ff8c42", codex: "#34c759" };
const TOOL_LABEL: Record<string, string> = { claude: "Claude", codex: "Codex" };

export function HistoryPage({ onBack }: { onBack: () => void }) {
  const [range, setRange] = useState<Range>("month");
  const [days, setDays] = useState<DayPoint[]>([]);
  const [selectedDate, setSelectedDate] = useState<string | null>(null);
  const [sessions, setSessions] = useState<SessionRow[]>([]);

  useEffect(() => {
    api.getHistory(range).then(setDays).catch(() => setDays([]));
  }, [range]);

  useEffect(() => {
    if (selectedDate) {
      api.getDailySessions(selectedDate).then(setSessions).catch(() => setSessions([]));
    } else {
      setSessions([]);
    }
  }, [selectedDate]);

  const total = days.reduce((s, d) => s + d.cost_usd, 0);
  const avg = days.length ? total / days.length : 0;
  const peak = days.reduce((m, d) => Math.max(m, d.cost_usd), 0);

  return (
    <div className="glass-card sub-page">
      <div className="page-head" data-tauri-drag-region>
        <span className="back" onClick={onBack}>‹</span>
        <span className="page-title">历史记录</span>
      </div>

      <div className="seg-tabs">
        {(["week", "month", "all"] as Range[]).map((r) => (
          <div key={r} className={`tab ${range === r ? "active" : ""}`} onClick={() => setRange(r)}>
            {({ week: "7天", month: "30天", all: "全部" } as Record<Range, string>)[r]}
          </div>
        ))}
      </div>

      <div className="hist-chart">
        <div className="section-title">每日花费 ($)</div>
        <LineChart days={days} />
        <div className="hist-stats">
          <Stat k="总计" v={fmtUsd(total)} />
          <Stat k="日均" v={fmtUsd(avg)} />
          <Stat k="峰值" v={fmtUsd(peak)} />
        </div>
      </div>

      <div className="hist-list">
        {[...days].reverse().map((d) => (
          <div key={d.date}>
            <div
              className={`day-head ${selectedDate === d.date ? "open" : ""}`}
              onClick={() => setSelectedDate(selectedDate === d.date ? null : d.date)}
            >
              <span className="day-date">
                {fmtDay(d.date)} <small>{d.session_count} 会话 · {fmtTokens(d.tokens)}</small>
              </span>
              <span className="day-amt">{fmtUsd(d.cost_usd)}</span>
            </div>
            {selectedDate === d.date && (
              <div className="day-sessions">
                {sessions.length === 0 && <div className="empty-mini">加载中…</div>}
                {sessions.map((s, i) => {
                  const tt = s.input_tok + s.output_tok + s.cache_tok;
                  return (
                    <div className="sess-row" key={i}>
                      <span className="sess-dot" style={{ background: TOOL_COLOR[s.tool] ?? "#888" }} />
                      <div className="sess-info">
                        <div className="sess-proj">{s.project ?? "(未知)"}</div>
                        <div className="sess-meta">
                          {TOOL_LABEL[s.tool] ?? s.tool} · {s.model} · {fmtTokens(tt)}
                        </div>
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
        ))}
        {days.length === 0 && <div className="empty-state">该时段暂无使用记录</div>}
      </div>
    </div>
  );
}

function LineChart({ days }: { days: DayPoint[] }) {
  if (days.length < 2) return <div className="chart-empty">数据不足</div>;
  const max = Math.max(...days.map((d) => d.cost_usd), 0.01);
  const W = 300, H = 80;
  const step = W / (days.length - 1);
  const pts = days.map((d, i) => `${i * step},${H - (d.cost_usd / max) * (H - 6) - 3}`);
  const path = "M" + pts.join(" L");
  const area = path + ` L${W},${H} L0,${H} Z`;
  const today = new Date().toISOString().slice(0, 10);
  return (
    <svg className="line-chart" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none">
      <defs>
        <linearGradient id="hg" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#5ac8fa" stopOpacity="0.5" />
          <stop offset="100%" stopColor="#5ac8fa" stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={area} fill="url(#hg)" />
      <path d={path} fill="none" stroke="#5ac8fa" strokeWidth="2" />
      {days.map((d, i) =>
        d.date === today ? (
          <circle key={i} cx={i * step} cy={H - (d.cost_usd / max) * (H - 6) - 3} r="3" fill="#34c759" />
        ) : null
      )}
    </svg>
  );
}

function Stat({ k, v }: { k: string; v: string }) {
  return (
    <div className="stat">
      <div className="stat-k">{k}</div>
      <div className="stat-v">{v}</div>
    </div>
  );
}

function fmtDay(iso: string): string {
  const d = new Date(iso + "T00:00:00");
  const wd = ["日", "一", "二", "三", "四", "五", "六"][d.getDay()];
  return `${d.getMonth() + 1}月${d.getDate()}日 周${wd}`;
}
