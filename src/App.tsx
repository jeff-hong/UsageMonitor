import { useEffect, useRef, useState, type MouseEvent, type MutableRefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Menu } from "@tauri-apps/api/menu";
import {
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
  type Window as TauriWindow,
} from "@tauri-apps/api/window";
import "./App.css";
import { DetailPanel } from "./components/DetailPanel";
import { HoverDetailWindow } from "./components/HoverDetailWindow";
import { SettingsPage } from "./components/SettingsPage";
import { CardWidget } from "./components/widgets/CardWidget";
import { useTodaySummary } from "./hooks/useTodaySummary";

const WIDGET_SIZE = { width: 150, height: 42 };
const DETAIL_SIZE = { width: 360, height: 560 };
const HOVER_DETAIL_SIZE = { width: 360, height: 420 };
const HOVER_DETAIL_MIN_HEIGHT = 180;
const HOVER_OPEN_DELAY_MS = 200;
const HOVER_CLOSE_DELAY_MS = 0;
let cachedWindows: Partial<Record<string, TauriWindow>> = {};

/// Single IPC that places a floating window next to the widget and optionally
/// shows it — the Rust side does GetWindowRect + MonitorFromPoint + SetWindowPos
/// in one round-trip, so the window appears at its final geometry/topmost band
/// atomically. Replaces the old ~5-IPC `placeWindowNearWidget` + separate
/// `setSize`/`show_window_native`.
function placeAndShow(
  label: string,
  size: { width: number; height: number },
  opts: { side?: "left" | "auto"; vertical?: "top" | "bottom"; show?: boolean; hideHover?: boolean } = {}
): Promise<void> {
  return invoke("place_and_show_window", {
    label,
    width: size.width,
    height: size.height,
    side: opts.side ?? "auto",
    vertical: opts.vertical ?? "bottom",
    show: opts.show ?? false,
    hideHover: opts.hideHover ?? false,
  }).then(() => undefined);
}

function App() {
  const label = getCurrentWindow().label;

  if (label === "hover_detail") {
    return <HoverDetailApp />;
  }

  if (label === "detail") {
    return <DetailWindowApp />;
  }

  return <WidgetApp />;
}

function WidgetApp() {
  const { summary, loading } = useTodaySummary();
  const hoverOpenTimerRef = useRef<number | null>(null);
  const hoverCloseTimerRef = useRef<number | null>(null);
  const hoverShowRequestRef = useRef(0);
  const wantsHoverRef = useRef(false);
  const isDraggingRef = useRef(false);
  const pointerInsideWidgetRef = useRef(false);
  const suppressHoverUntilRef = useRef(0);
  const hoverPrepareRequestRef = useRef(0);
  // Start at the max height so the FIRST show doesn't crop the tool list. The
  // hover window's useLayoutEffect measures the real content height and emits
  // `hover-detail-height`, which shrinks the window to fit. If this started at
  // HOVER_DETAIL_MIN_HEIGHT (180) the first placeAndShow would clamp the window
  // to 180px and the "按工具" rows would be clipped off-screen until the
  // measurement raced in — visibly cutting them off.
  const hoverHeightRef = useRef(HOVER_DETAIL_SIZE.height);
  const menuRef = useRef<Menu | null>(null);

  useEffect(() => {
    initWidgetWindow();
    // Pre-place the hover window (hidden) near the widget so the first hover
    // show only needs a show+repos, not a cold placement.
    placeAndShow("hover_detail", { width: HOVER_DETAIL_SIZE.width, height: hoverHeightRef.current }, { side: "left", vertical: "top" }).catch(() => {});
    getWindow("detail").catch(() => null);
  }, []);

  useEffect(() => {
    createWidgetMenu(menuRef, showDetailWindow);
  }, []);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    listen("hover-detail-pointer", (event) => {
      if (event.payload === "enter") {
        wantsHoverRef.current = true;
        clearTimer(hoverCloseTimerRef);
      } else {
        scheduleHoverLeave();
      }
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];

    listen<string>("native-widget-pointer", (event) => {
      if (event.payload === "enter") {
        scheduleHoverShow();
      } else {
        scheduleHoverLeave();
      }
    }).then((fn) => unlisteners.push(fn));

    listen<string>("native-widget-context-menu", (event) => {
      openContextMenu(undefined, parseNativePoint(event.payload));
    }).then((fn) => unlisteners.push(fn));

    listen("native-widget-open-detail", () => {
      showDetailWindow("overview");
    }).then((fn) => unlisteners.push(fn));

    listen<number>("hover-detail-height", async (event) => {
      if (!wantsHoverRef.current || isDraggingRef.current) return;
      const height = normalizeHoverHeight(event.payload);
      hoverHeightRef.current = height;
      // Single IPC re-places the hover window at its new height (no show —
      // it's already visible while content grows/shrinks). This replaces the
      // old placeWindowNearWidget (5 IPCs) + setSize (1 IPC) double-call that
      // caused a visible jump between the two SetWindowPos calls.
      await placeAndShow("hover_detail", { width: HOVER_DETAIL_SIZE.width, height }, { side: "left", vertical: "top" }).catch(() => {});
    }).then((fn) => unlisteners.push(fn));

    return () => {
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  const showDetailWindow = async (mode: "overview" | "settings" = "overview") => {
    wantsHoverRef.current = false;
    clearTimer(hoverOpenTimerRef);
    clearTimer(hoverCloseTimerRef);
    await hideHoverDetail();
    const detail = await getWindow("detail");
    if (!detail) return;
    // Place + size + topmost via one IPC (no show — see below). hide_hover so
    // any lingering hover_detail is gone before we paint detail.
    await placeAndShow("detail", DETAIL_SIZE, { hideHover: true }).catch(() => {});
    // Show + activate via Tauri's own window API rather than Win32
    // SW_SHOWNOACTIVATE. The detail window is an interactive surface: if it is
    // shown without activation the first click on it (e.g. the close button)
    // is swallowed just to activate the window — that was the "叉不掉" bug.
    // Tauri's show() on a focusable window activates it correctly and reliably
    // (unlike raw SetForegroundWindow, which is gated by the foreground lock).
    await detail.show().catch(() => {});
    await detail.setFocus().catch(() => {});
    emitTo("detail", "detail-mode", mode).catch(() => {});
  };

  const showHoverDetail = async () => {
    if (!wantsHoverRef.current || isDraggingRef.current) return;
    if (performance.now() < suppressHoverUntilRef.current) return;
    const requestId = ++hoverShowRequestRef.current;
    // Place + show the hover window in a single IPC (no separate prepare/size
    // round-trips). Bumps hoverPrepareRequestRef so any in-flight prepare is
    // invalidated.
    hoverPrepareRequestRef.current += 1;
    await placeAndShow(
      "hover_detail",
      { width: HOVER_DETAIL_SIZE.width, height: hoverHeightRef.current },
      { side: "left", vertical: "top", show: true }
    ).catch(() => {});
    if (requestId !== hoverShowRequestRef.current || !wantsHoverRef.current || isDraggingRef.current) {
      hideHoverDetail();
      return;
    }
    emitTo("hover_detail", "hover-detail-refresh").catch(() => {});
  };

  const scheduleHoverShow = () => {
    if (isDraggingRef.current) return;
    if (performance.now() < suppressHoverUntilRef.current) return;
    if (pointerInsideWidgetRef.current && wantsHoverRef.current && hoverOpenTimerRef.current !== null) return;
    pointerInsideWidgetRef.current = true;
    wantsHoverRef.current = true;
    clearTimer(hoverCloseTimerRef);
    clearTimer(hoverOpenTimerRef);
    if (HOVER_OPEN_DELAY_MS <= 0) {
      showHoverDetail();
      return;
    }
    hoverOpenTimerRef.current = window.setTimeout(() => {
      hoverOpenTimerRef.current = null;
      if (isDraggingRef.current) return;
      showHoverDetail();
    }, HOVER_OPEN_DELAY_MS);
  };

  const scheduleHoverLeave = () => {
    hoverShowRequestRef.current += 1;
    hoverPrepareRequestRef.current += 1;
    pointerInsideWidgetRef.current = false;
    wantsHoverRef.current = false;
    clearTimer(hoverOpenTimerRef);
    clearTimer(hoverCloseTimerRef);
    if (HOVER_CLOSE_DELAY_MS <= 0) {
      hideHoverDetail();
      return;
    }
    hoverCloseTimerRef.current = window.setTimeout(() => {
      hoverCloseTimerRef.current = null;
      if (!pointerInsideWidgetRef.current && !wantsHoverRef.current) {
        hideHoverDetail();
      }
    }, HOVER_CLOSE_DELAY_MS);
  };

  const openContextMenu = async (ev?: MouseEvent, nativePoint?: { x: number; y: number }) => {
    ev?.preventDefault();
    suppressHoverUntilRef.current = performance.now() + 700;
    wantsHoverRef.current = false;
    clearTimer(hoverOpenTimerRef);
    clearTimer(hoverCloseTimerRef);
    await hideHoverDetail();

    if (!menuRef.current) await createWidgetMenu(menuRef, showDetailWindow);
    const menu = menuRef.current;
    if (!menu) return;

    const win = getCurrentWindow();
    const at = nativePoint ? await screenToWindowLogical(nativePoint, win) : eventPoint(ev);
    await menu.popup(at, win);
  };

  const closeHoverForWidgetPress = () => {
    hoverShowRequestRef.current += 1;
    hoverPrepareRequestRef.current += 1;
    wantsHoverRef.current = false;
    pointerInsideWidgetRef.current = false;
    suppressHoverUntilRef.current = performance.now() + 220;
    clearTimer(hoverOpenTimerRef);
    clearTimer(hoverCloseTimerRef);
  };

  const beginDragging = () => {
    hoverShowRequestRef.current += 1;
    hoverPrepareRequestRef.current += 1;
    pointerInsideWidgetRef.current = false;
    wantsHoverRef.current = false;
    isDraggingRef.current = true;
    suppressHoverUntilRef.current = performance.now() + 180;
    clearTimer(hoverOpenTimerRef);
    clearTimer(hoverCloseTimerRef);
    document.documentElement.classList.add("is-native-dragging");
  };

  const endDragging = () => {
    isDraggingRef.current = false;
    suppressHoverUntilRef.current = performance.now() + 80;
    document.documentElement.classList.remove("is-native-dragging");
    // Don't reposition the (hidden) hover window here — it was hidden on drag
    // start and `wantsHoverRef` is false. The next scheduleHoverShow will place
    // it via placeAndShow. Re-positioning a hidden window was wasted IPCs.
  };

  return (
    <div className="widget-root" onContextMenu={openContextMenu}>
      <CardWidget
        summary={summary}
        loading={loading}
        onOpenDetail={() => showDetailWindow("overview")}
        onHoverChange={(hovering) => (hovering ? scheduleHoverShow() : scheduleHoverLeave())}
        onPressStart={closeHoverForWidgetPress}
        onDragStart={beginDragging}
        onDragEnd={endDragging}
      />
    </div>
  );
}

function DetailWindowApp() {
  const [mode, setMode] = useState<"overview" | "settings">("overview");

  useEffect(() => {
    const win = getCurrentWindow();
    win.setSize(new LogicalSize(DETAIL_SIZE.width, DETAIL_SIZE.height)).catch(() => {});
    win.setSkipTaskbar(true).catch(() => {});

    let unlisten: UnlistenFn | null = null;
    listen<"overview" | "settings">("detail-mode", (event) => {
      setMode(event.payload === "settings" ? "settings" : "overview");
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const h = () => setMode("settings");
    window.addEventListener("open-settings", h);
    return () => window.removeEventListener("open-settings", h);
  }, []);

  if (mode === "settings") {
    return <SettingsPage onBack={() => setMode("overview")} />;
  }

  return (
    <DetailPanel
      onClose={() => {
        getCurrentWindow().hide().catch(() => {});
      }}
    />
  );
}

function HoverDetailApp() {
  useEffect(() => {
    const win = getCurrentWindow();
    win.setSize(new LogicalSize(HOVER_DETAIL_SIZE.width, HOVER_DETAIL_SIZE.height)).catch(() => {});
    win.setResizable(false).catch(() => {});
    win.setSkipTaskbar(true).catch(() => {});
    win.setFocusable(false).catch(() => {});
  }, []);

  // Route hover-window enter/leave through the widget window, which is the
  // single authority for hover state. The widget listens for
  // `hover-detail-pointer` and treats enter as "keep open" / leave as
  // "schedule close". Previously this window also issued its own
  // `hide_window_native` on leave, which raced with the widget's state machine
  // and could hide during a re-enter.
  const setHovering = (hovering: boolean) => {
    emitTo("widget", "hover-detail-pointer", hovering ? "enter" : "leave").catch(() => {});
  };

  return (
    <div
      className="hover-detail-root"
      onMouseEnter={() => setHovering(true)}
      onMouseLeave={() => setHovering(false)}
      onContextMenu={(ev) => {
        ev.preventDefault();
        invoke("quit_app");
      }}
    >
      <HoverDetailWindow />
    </div>
  );
}

function clearTimer(timerRef: MutableRefObject<number | null>) {
  if (timerRef.current !== null) {
    window.clearTimeout(timerRef.current);
    timerRef.current = null;
  }
}

async function hideHoverDetail() {
  await invoke("hide_window_native", { label: "hover_detail" }).catch(async () => {
    const hover = await getWindow("hover_detail");
    await hover?.hide().catch(() => {});
  });
}

function initWidgetWindow() {
  const win = getCurrentWindow();
  win.setSize(new LogicalSize(WIDGET_SIZE.width, WIDGET_SIZE.height)).catch(() => {});
  win.setResizable(false).catch(() => {});
  win.setSkipTaskbar(true).catch(() => {});
  win.setAlwaysOnTop(true).catch(() => {});
  win.setFocusable(true).catch(() => {});
  win.show().catch(() => {});
}

function normalizeHoverHeight(height: number): number {
  return clamp(Math.round(height), HOVER_DETAIL_MIN_HEIGHT, HOVER_DETAIL_SIZE.height);
}

async function getWindow(label: string): Promise<TauriWindow | null> {
  if (cachedWindows[label]) return cachedWindows[label] ?? null;
  const { Window } = await import("@tauri-apps/api/window");
  const win = await Window.getByLabel(label);
  if (win) cachedWindows[label] = win;
  return win;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, max));
}

function parseNativePoint(payload: unknown): { x: number; y: number } | undefined {
  if (typeof payload !== "string" || !payload) return undefined;
  try {
    const point = JSON.parse(payload) as { x?: unknown; y?: unknown };
    if (typeof point.x === "number" && typeof point.y === "number") {
      return { x: point.x, y: point.y };
    }
  } catch {
    return undefined;
  }
  return undefined;
}

function eventPoint(ev?: MouseEvent): LogicalPosition | undefined {
  if (!ev) return undefined;
  return new LogicalPosition(Math.round(ev.clientX), Math.round(ev.clientY));
}

/// Convert a screen-space physical point (from the native right-click hook)
/// into logical coordinates relative to the given window, for menu.popup.
/// The two queries run in parallel — this only runs on right-click, off the
/// hot path.
async function screenToWindowLogical(
  point: { x: number; y: number },
  win: TauriWindow
): Promise<LogicalPosition | undefined> {
  const [pos, scale] = await Promise.all([
    win.outerPosition().catch(() => null),
    win.scaleFactor().catch(() => window.devicePixelRatio || 1),
  ]);
  if (!pos) return undefined;
  return new LogicalPosition(
    Math.round((point.x - pos.x) / scale),
    Math.round((point.y - pos.y) / scale)
  );
}

export default App;

async function createWidgetMenu(
  menuRef: MutableRefObject<Menu | null>,
  showDetailWindow: (mode: "overview" | "settings") => void
) {
  if (menuRef.current) return;
  menuRef.current = await Menu.new({
    items: [
      {
        id: "detail",
        text: "查看详情",
        action: () => showDetailWindow("overview"),
      },
      {
        id: "settings",
        text: "设置",
        action: () => showDetailWindow("settings"),
      },
      { item: "Separator" },
      {
        id: "quit",
        text: "退出",
        action: () => invoke("quit_app"),
      },
    ],
  });
}
