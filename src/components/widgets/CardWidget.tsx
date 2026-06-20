// Card widget: the default compact form. Vertical glass card showing today's
// token total large, with a one-line breakdown per tool.
//
// Layering for input: the outer div carries data-tauri-drag-region so the user
// can drag the window by the card edges. The inner content layer has NO
// drag-region and uses a plain onClick — that way a tap on the content always
// opens detail and is never swallowed by the drag gesture (which is what made
// "click to open" feel unresponsive).

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
    <div className="glass-card widget-card" data-tauri-drag-region>
      <div className="label-tiny" data-tauri-drag-region>今日</div>
      <div className="big-num clickable" onClick={onOpenDetail} title="点击查看详情">
        {loading ? "…" : fmtTokens(tokens)}
        <small className="big-unit">tokens</small>
      </div>
      <div className="token-total" data-tauri-drag-region>
        花费 <span className="cost-small">{priced ? fmtUsd(cost) : `${fmtUsd(cost)}*`}</span>
      </div>

      <div className="tool-rows" data-tauri-drag-region>
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
    </div>
  );
}
