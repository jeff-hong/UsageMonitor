// Pill widget: horizontal capsule suited to docking at a screen edge. Cost in
// the lead segment, each tool as its own segment.

import { type Summary, fmtUsd, fmtTokens } from "../../lib/api";

const TOOL_COLOR: Record<string, string> = {
  claude: "#ff8c42",
  codex: "#34c759",
};
const TOOL_LABEL: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
};

export function PillWidget({
  summary,
  loading,
  onOpenDetail,
}: {
  summary: Summary | null;
  loading: boolean;
  onOpenDetail: () => void;
}) {
  const cost = summary?.cost_usd ?? 0;
  return (
    <div className="glass-card widget-pill" data-tauri-drag-region>
      <div className="widget-clickable" onClick={onOpenDetail}>
        <div className="pill-seg">
          <span className="label-tiny">花费</span>
          <span className="pill-val">{loading ? "…" : fmtUsd(cost)}</span>
        </div>
        {(summary?.tools ?? []).map((t) => {
          const tt = t.input_tok + t.output_tok + t.cache_tok;
          if (tt === 0) return null;
          return (
            <div className="pill-seg" key={t.tool}>
              <span className="divider" />
              <span className="label-tiny" style={{ color: TOOL_COLOR[t.tool] }}>
                {TOOL_LABEL[t.tool] ?? t.tool}
              </span>
              <span className="pill-val">{fmtTokens(tt)}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
