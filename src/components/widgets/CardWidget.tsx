// Card widget: the default compact form. Vertical glass card showing today's
// cost large, with a one-line breakdown per tool. Drag handle is the whole card.

import { useRef } from "react";
import { type Summary, fmtUsd, fmtTokens } from "../../lib/api";

const TOOL_COLOR: Record<string, string> = {
  claude: "#ff8c42",
  codex: "#34c759",
};
const TOOL_LABEL: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
};

export function CardWidget({
  summary,
  loading,
  onOpenDetail,
}: {
  summary: Summary | null;
  loading: boolean;
  onOpenDetail: () => void;
}) {
  const tokens = summary ? summary.input_tok + summary.output_tok + summary.cache_tok : 0;
  const cost = summary?.cost_usd ?? 0;
  const priced = summary?.fully_priced ?? true;

  // Distinguish a click from a drag: if the pointer moved more than a few px
  // between mousedown and mouseup, treat it as a drag and don't open detail.
  const down = useRef<{ x: number; y: number } | null>(null);
  const onMouseDown = (e: React.MouseEvent) => {
    down.current = { x: e.clientX, y: e.clientY };
  };
  const onMouseUp = (e: React.MouseEvent) => {
    if (!down.current) return;
    const dx = Math.abs(e.clientX - down.current.x);
    const dy = Math.abs(e.clientY - down.current.y);
    down.current = null;
    if (dx < 5 && dy < 5) onOpenDetail();
  };

  return (
    <div
      className="glass-card widget-card"
      data-tauri-drag-region
      onMouseDown={onMouseDown}
      onMouseUp={onMouseUp}
    >
      <div className="label-tiny" data-tauri-drag-region>
        今日
      </div>
      <div className="big-num" data-tauri-drag-region>
        {loading ? "…" : fmtTokens(tokens)}
        <small className="big-unit">tokens</small>
      </div>
      <div className="token-total" data-tauri-drag-region>
        花费 <span className="cost-small">{priced ? fmtUsd(cost) : `${fmtUsd(cost)}*`}</span>
      </div>

      <div className="tool-rows">
        {(summary?.tools ?? []).map((t) => {
          const tt = t.input_tok + t.output_tok + t.cache_tok;
          if (tt === 0) return null;
          return (
            <div className="tool-row" key={t.tool}>
              <span className="tool-name">
                <span
                  className="dot"
                  style={{ background: TOOL_COLOR[t.tool] ?? "#888" }}
                />
                {TOOL_LABEL[t.tool] ?? t.tool}
              </span>
              <span className="tool-tok">{fmtTokens(tt)}</span>
            </div>
          );
        })}
      </div>
      <div className="hint">点击查看详情</div>
    </div>
  );
}
