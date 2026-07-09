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
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect, useLayoutEffect, useRef, useState } from "react";

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
const HOVER_HEIGHT_MIN = 180;
const HOVER_HEIGHT_MAX = 420;
const MAX_VISIBLE_TOOL_ROWS = 2;
const PROVIDER_REFRESH_MS = 30_000;

function visibleToolRows(summary: Summary | null) {
  return (summary?.tools ?? []).filter((tool) => totalTokens(tool) > 0);
}

export function HoverDetailWindow() {
  const [summary, setSummary] = useState<Summary | null>(null);
  const [providers, setProviders] = useState<ProviderUsage[]>(EMPTY_PROVIDERS);
  const [loading, setLoading] = useState(true);
  const cardRef = useRef<HTMLDivElement | null>(null);
  const toolListRef = useRef<HTMLDivElement | null>(null);

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

  const tokens = summary ? totalTokens(summary) : 0;
  const cost = summary?.cost_usd ?? 0;
  const priced = summary?.fully_priced ?? true;
  const visibleToolCount = visibleToolRows(summary).length;
  const lastHeightRef = useRef(0);

  useLayoutEffect(() => {
    if (loading && !summary) return;
    const toolListHeight = applyToolListMaxHeight(toolListRef.current, visibleToolCount);
    const card = cardRef.current;
    const measuredHeight = card ? measureNaturalCardHeight(card, toolListHeight) : HOVER_HEIGHT_MIN;
    const height = clamp(measuredHeight, HOVER_HEIGHT_MIN, HOVER_HEIGHT_MAX);
    // Skip the whole measure→report cycle when height is unchanged. The effect
    // deps include `providers` which flips every 30s refresh even though the
    // row count (and thus height) is identical — re-measuring + re-emitting on
    // every refresh caused a background flicker as the widget re-setSize'd.
    if (height === lastHeightRef.current) return;
    lastHeightRef.current = height;
    // Only MEASURE + REPORT the height here. The widget window is the sole
    // owner of setSize/setPosition — it receives this height and does a single
    // `place_and_show_window` IPC. Previously this window ALSO called setSize
    // on itself, racing the widget's setSize and causing a visible jump.
    emitTo("widget", "hover-detail-height", height).catch(() => {});
  }, [visibleToolCount, loading, summary, providers]);

  return (
    <div className="hover-card" ref={cardRef}>
      <div className="hover-head">
        <div>
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
      <div
        className="hover-tool-list"
        ref={toolListRef}
      >
        {visibleToolRows(summary).map((tool) => {
          const toolTokens = totalTokens(tool);
          return (
            <div className="hover-tool" key={tool.tool}>
              <div className="hover-tool-top">
                <span className="hover-dot" style={{ background: TOOL_COLOR[tool.tool] ?? "#888" }} />
                <span className="hover-tool-name">{TOOL_LABEL[tool.tool] ?? tool.tool}</span>
                <div className="hover-tool-amounts">
                  <span className="hover-tool-tokens">{fmtTokens(toolTokens)}</span>
                  <span className="hover-tool-cost">{tool.fully_priced ? fmtUsd(tool.cost_usd) : `${fmtUsd(tool.cost_usd)}*`}</span>
                </div>
              </div>
              <div className="hover-tool-grid">
                <MetricPill label="输入" value={fmtTokens(tool.input_tok)} />
                <MetricPill label="输出" value={fmtTokens(tool.output_tok)} />
                <MetricPill label="命中" value={fmtTokens(tool.cache_tok)} />
                <MetricPill label="写入" value={fmtTokens(tool.cache_create_tok)} />
                <MetricPill label="命中率" value={fmtPercent(cacheHitRate(tool))} highlight />
              </div>
            </div>
          );
        })}
        {!loading && visibleToolRows(summary).length === 0 && (
          <div className="hover-empty">今天还没有记录</div>
        )}
      </div>
    </div>
  );
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, max));
}

function measureNaturalCardHeight(card: HTMLElement, toolListHeight: number | null): number {
  const styles = window.getComputedStyle(card);
  const paddingY = Number.parseFloat(styles.paddingTop || "0") + Number.parseFloat(styles.paddingBottom || "0");

  const contentHeight = Array.from(card.children).reduce((sum, child) => {
    const element = child as HTMLElement;
    const childStyles = window.getComputedStyle(element);
    const marginY =
      Number.parseFloat(childStyles.marginTop || "0") + Number.parseFloat(childStyles.marginBottom || "0");
    let height: number;
    if (element.classList.contains("hover-tool-list")) {
      // The tool list is `flex: 1 1 auto`, so getBoundingClientRect() returns
      // the flex-constrained height, not the true content height. Use the
      // pre-measured value (based on scrollHeight) when available, otherwise
      // fall back to scrollHeight directly.
      height = toolListHeight ?? element.scrollHeight;
    } else {
      height = element.getBoundingClientRect().height;
    }
    return sum + height + marginY;
  }, 0);

  return Math.ceil(paddingY + contentHeight);
}

function applyToolListMaxHeight(list: HTMLElement | null, visibleToolCount: number): number | null {
  if (!list) return null;
  if (visibleToolCount <= MAX_VISIBLE_TOOL_ROWS) {
    // No cap needed — let the list be its natural size. Clear any prior cap and
    // report the full content height (scrollHeight, NOT getBoundingClientRect
    // which reflects flex-constrained layout).
    list.style.maxHeight = "";
    return Math.ceil(list.scrollHeight);
  }

  const rows = Array.from(list.querySelectorAll<HTMLElement>(".hover-tool"));
  const visibleRows = rows.slice(0, MAX_VISIBLE_TOOL_ROWS);
  if (visibleRows.length < MAX_VISIBLE_TOOL_ROWS) return null;

  const styles = window.getComputedStyle(list);
  const gap = Number.parseFloat(styles.rowGap || styles.gap || "0") || 0;
  // Measure each visible row by its scrollHeight (natural content height,
  // immune to flex shrink/grow), then cap the list so extras scroll.
  const height = Math.ceil(
    visibleRows.reduce((sum, row) => sum + row.scrollHeight, 0) +
      gap * (visibleRows.length - 1)
  );
  list.style.maxHeight = `${height}px`;
  return height;
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

function MetricPill({
  label,
  value,
  highlight = false,
}: {
  label: string;
  value: string;
  highlight?: boolean;
}) {
  return (
    <div className={`hover-pill ${highlight ? "highlight" : ""}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
