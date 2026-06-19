// Gauge widget: circular ring showing today's cost as the focal value. The ring
// fill could represent a monthly budget later; for now it's decorative so the
// tile still reads as "iOS control center" at a glance.

import { type Summary, fmtUsd, fmtTokens } from "../../lib/api";

export function GaugeWidget({
  summary,
  loading,
  onOpenDetail,
}: {
  summary: Summary | null;
  loading: boolean;
  onOpenDetail: () => void;
}) {
  const cost = summary?.cost_usd ?? 0;
  const tokens = summary
    ? summary.input_tok + summary.output_tok + summary.cache_tok
    : 0;
  const priced = summary?.fully_priced ?? true;

  return (
    <div
      className="glass-card widget-gauge"
      data-tauri-drag-region
      onDoubleClick={onOpenDetail}
    >
      <div className="gauge-ring" data-tauri-drag-region>
        <div className="gauge-inner" data-tauri-drag-region>
          <div className="gauge-num">{loading ? "…" : fmtUsd(cost)}</div>
          <div className="gauge-label">今日</div>
        </div>
      </div>
      <div className="gauge-tokens">{fmtTokens(tokens)} tok</div>
      {!priced && <div className="gauge-unpriced">部分未定价</div>}
    </div>
  );
}
