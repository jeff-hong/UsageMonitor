// Card widget: the default compact form. Vertical glass card showing today's
// cost large, with a one-line breakdown per tool. Drag handle is the whole card.

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

  return (
    <div
      className="glass-card widget-card"
      data-tauri-drag-region
      onDoubleClick={onOpenDetail}
    >
      <div className="label-tiny" data-tauri-drag-region>
        今日
      </div>
      <div className="big-num" data-tauri-drag-region>
        {loading ? "…" : priced ? fmtUsd(cost) : `${fmtUsd(cost)}*`}
      </div>
      <div className="token-total" data-tauri-drag-region>
        {fmtTokens(tokens)} tokens
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
      <div className="hint">双击查看详情</div>
    </div>
  );
}
