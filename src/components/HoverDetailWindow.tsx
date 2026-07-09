import {
  api,
  cacheHitRate,
  fmtPercent,
  fmtUsd,
  fmtTokens,
  totalTokens,
  type ProviderUsage,
  type Summary,
} from "../lib/api";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

const TOOL_COLOR: Record<string, string> = {
  claude: "#ff8c42",
  codex: "#34c759",
};

const TOOL_LABEL: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
};

const EMPTY_PROVIDERS: ProviderUsage[] = [
  emptyProvider("claude"),
  emptyProvider("codex"),
];
const PROVIDER_REFRESH_MS = 30_000;

function visibleToolRows(summary: Summary | null) {
  return (summary?.tools ?? []).filter((tool) => totalTokens(tool) > 0);
}

export function HoverDetailWindow() {
  const [summary, setSummary] = useState<Summary | null>(null);
  const [providers, setProviders] = useState<ProviderUsage[]>(EMPTY_PROVIDERS);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let alive = true;
    const refresh = () => {
      Promise.all([
        api.getTodaySummary(),
        api.getCurrentProviderUsage("claude").catch(() => emptyProvider("claude", "失败")),
        api.getCurrentProviderUsage("codex").catch(() => emptyProvider("codex", "失败")),
      ]).then(
        ([s, claudeProvider, codexProvider]) => {
          if (alive) {
            setSummary(s);
            setProviders([
              claudeProvider ?? emptyProvider("claude", "无数据"),
              codexProvider ?? emptyProvider("codex", "无数据"),
            ]);
            setLoading(false);
          }
        },
        () => {
          if (alive) setLoading(false);
        }
      );
    };

    refresh();
    const id = window.setInterval(refresh, PROVIDER_REFRESH_MS);
    let unlisten: UnlistenFn | null = null;
    listen("hover-detail-refresh", refresh).then((fn) => {
      unlisten = fn;
    });
    return () => {
      alive = false;
      window.clearInterval(id);
      unlisten?.();
    };
  }, []);

  // No dynamic height measurement: the window has a FIXED height set in
  // tauri.conf.json + HoverDetailApp, and the card content scrolls if it ever
  // overflows. The old approach (measure content → resize window per tool-row
  // count) was a repeated source of bugs (Codex row clipped, height too tall)
  // because measuring inside a fixed-height window is circular. Fixed height =
  // zero measurement, zero clipping surprises.
  const tokens = summary ? totalTokens(summary) : 0;
  const cost = summary?.cost_usd ?? 0;
  const priced = summary?.fully_priced ?? true;
  const tools = visibleToolRows(summary);

  return (
    <div className="hover-card" >
      <div className="hover-head">
        <div className="hover-head-left">
          <div className="hover-brand">UsageMonitor</div>
          <div className="hover-sub">今日使用量</div>
        </div>
        <div className="hover-total">
          <span className="hover-total-tok">{loading ? "..." : fmtTokens(tokens)}</span>
          <span className="hover-total-cost">{priced ? fmtUsd(cost) : `${fmtUsd(cost)}*`}</span>
        </div>
      </div>

      <ProviderUsageStrip providers={providers} />

      <div className="hover-section-title">按工具</div>
      <div className="hover-tool-list">
        {tools.map((tool) => {
          const toolTokens = totalTokens(tool);
          return (
            <div className="hover-tool" key={tool.tool}>
              <div className="hover-tool-row1">
                <span className="hover-dot" style={{ background: TOOL_COLOR[tool.tool] ?? "#888" }} />
                <span className="hover-tool-name">{TOOL_LABEL[tool.tool] ?? tool.tool}</span>
                <span className="hover-tool-tokens">{fmtTokens(toolTokens)}</span>
                <span className="hover-tool-cost">
                  {tool.fully_priced ? fmtUsd(tool.cost_usd) : `${fmtUsd(tool.cost_usd)}*`}
                </span>
              </div>
              <div className="hover-tool-row2">
                <span>输入 {fmtTokens(tool.input_tok)}</span>
                <span>输出 {fmtTokens(tool.output_tok)}</span>
                <span>缓存 {fmtTokens(tool.cache_tok)}</span>
                <span>写入 {fmtTokens(tool.cache_create_tok ?? 0)}</span>
                <span className="hover-hit">命中率 {fmtPercent(cacheHitRate(tool))}</span>
              </div>
            </div>
          );
        })}
        {!loading && tools.length === 0 && (
          <div className="hover-empty">今天还没有记录</div>
        )}
      </div>
    </div>
  );
}

function emptyProvider(appType: "claude" | "codex", value = "..."): ProviderUsage {
  return {
    app_type: appType,
    provider_name: "cc-switch",
    provider_id: "",
    mode: "balance",
    primary_label: "余额",
    primary_value: value,
    primary_updated_text: null,
    secondary_label: null,
    secondary_value: null,
    secondary_updated_text: null,
    updated_text: null,
    ok: false,
  };
}

function ProviderUsageStrip({ providers }: { providers: ProviderUsage[] }) {
  if (providers.length === 0) return null;

  return (
    <div className="hover-provider-strip">
      {providers.map((provider) => (
        <div className="hover-provider" key={`${provider.app_type}-${provider.provider_id}`}>
          <div className="hover-provider-top">
            <span
              className="hover-provider-dot"
              style={{ background: TOOL_COLOR[provider.app_type] ?? "#8b92a3" }}
            />
            <span className="hover-provider-name">
              {TOOL_LABEL[provider.app_type] ?? provider.app_type}
              <strong>{provider.provider_name}</strong>
            </span>
          </div>
          {provider.mode === "plan" ? (
            <div className="hover-provider-plan">
              <PlanCell
                label={provider.primary_label}
                value={provider.primary_value}
                updatedText={provider.primary_updated_text}
                ok={provider.ok}
              />
              {provider.secondary_label && (
                <PlanCell
                  label={provider.secondary_label}
                  value={provider.secondary_value ?? "暂无"}
                  updatedText={provider.secondary_updated_text ?? provider.updated_text}
                  ok={provider.ok}
                />
              )}
            </div>
          ) : (
            <div className={`hover-provider-balance ${provider.ok ? "" : "muted"}`}>
              <span>{provider.primary_label}</span>
              <strong>{provider.primary_value}</strong>
              {provider.secondary_label && <em>{provider.secondary_label}</em>}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function PlanCell({
  label,
  value,
  updatedText,
  ok,
}: {
  label: string;
  value: string;
  updatedText?: string | null;
  ok: boolean;
}) {
  return (
    <span className={`hover-plan-cell ${ok ? "" : "muted"}`}>
      {label}: <strong>{value}</strong>
      {updatedText && <em>{updatedText}</em>}
    </span>
  );
}
