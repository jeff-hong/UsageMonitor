// 默认的悬浮胶囊组件：展示今日 token 总量和 Claude/Codex 当前额度。
//
// 拖拽处理说明：
//
// 左键 mousedown 直接调用 Rust 的 start_window_drag，让 Windows 进入原生标题栏拖拽。
// Rust 侧 widget_mouse.rs 只保留右键菜单和 hover 轮询，不发起左键拖动。
//
// 注意：不要用 setPointerCapture / 移动阈值方案。原生拖拽（WM_NCLBUTTONDOWN）会让
// 操作系统接管指针，JS 的 pointer capture + pointerup 会彻底失效，导致拖拽结束后
// widget 锁死、无法再交互（"卡死用不了"）。立即进入原生拖拽是参考实现验证过可靠的
// 方式；双击打开详情靠 onDoubleClick（原生拖拽不移动鼠标时不会阻止 dblclick）。

import {
  fmtTokens,
  totalTokens,
  type ProviderUsage,
  type Summary,
  api,
} from "../../lib/api";
import { useEffect, useRef, useState } from "react";
import { startNativeDrag } from "../../lib/drag";

const EMPTY_PROVIDERS: ProviderUsage[] = [
  emptyProvider("claude"),
  emptyProvider("codex"),
];
const PROVIDER_REFRESH_MS = 5_000;

export function CardWidget({
  summary,
  loading,
  onOpenDetail,
  onHoverChange,
  onPressStart,
  onDragStart,
  onDragEnd,
}: {
  summary: Summary | null;
  loading: boolean;
  onOpenDetail: () => void;
  onHoverChange: (expanded: boolean) => void;
  onPressStart: () => void;
  onDragStart: () => void;
  onDragEnd: () => void;
}) {
  const tokens = summary ? totalTokens(summary) : 0;
  const [providers, setProviders] = useState<ProviderUsage[]>(EMPTY_PROVIDERS);
  const draggingRef = useRef(false);

  useEffect(() => {
    let alive = true;
    const refresh = () => {
      Promise.all([
        api.getCurrentProviderUsage("claude").catch(() => emptyProvider("claude", "失败")),
        api.getCurrentProviderUsage("codex").catch(() => emptyProvider("codex", "失败")),
      ]).then(([claude, codex]) => {
        if (alive) {
          setProviders([
            claude ?? emptyProvider("claude", "无数据"),
            codex ?? emptyProvider("codex", "无数据"),
          ]);
        }
      });
    };

    refresh();
    const id = window.setInterval(refresh, PROVIDER_REFRESH_MS);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  return (
    <div
      className="glass-card widget-card dock-widget"
      draggable={false}
      onPointerDownCapture={(event) => {
        if (event.button !== 0 || draggingRef.current) return;
        const target = event.target as HTMLElement | null;
        if (target?.closest("button, input, textarea, select, a, [role='button'], [data-no-drag]")) return;

        event.preventDefault();
        draggingRef.current = true;
        onPressStart();
        document.documentElement.classList.add("is-native-dragging");
        onDragStart();
        startNativeDrag("widget")
          .catch(() => {})
          .finally(() => {
            draggingRef.current = false;
            document.documentElement.classList.remove("is-native-dragging");
            onDragEnd();
          });
      }}
      onPointerEnter={() => onHoverChange(true)}
      onPointerLeave={() => onHoverChange(false)}
      onDoubleClick={onOpenDetail}
    >
      <div className="dock-compact">
        <div>
          <div className="dock-total">{loading ? "..." : fmtTokens(tokens)}</div>
        </div>
        <div className="dock-balances">
          <BalanceLine appType="claude" provider={providers.find((p) => p.app_type === "claude")} />
          <BalanceLine appType="codex" provider={providers.find((p) => p.app_type === "codex")} />
        </div>
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

function BalanceLine({
  appType,
  provider,
}: {
  appType: "claude" | "codex";
  provider?: ProviderUsage;
}) {
  const label = appType === "claude" ? "C" : "X";
  return (
    <div className={`dock-balance-line ${appType}`}>
      <span>{label}</span>
      <strong>{provider ? compactProviderValue(provider) : "..."}</strong>
    </div>
  );
}

function compactProviderValue(provider: ProviderUsage): string {
  if (provider.mode === "plan") {
    const first = provider.primary_value.replace("%", "");
    const second = (provider.secondary_value ?? "").replace("%", "");
    if (first && second) return `${first}/${second}%`;
    return provider.primary_value;
  }

  const unit = provider.secondary_label ?? "";
  if (unit.toUpperCase() === "USD" && provider.ok) {
    return `$${provider.primary_value}`;
  }
  return provider.secondary_label ? `${provider.primary_value}${provider.secondary_label}` : provider.primary_value;
}
