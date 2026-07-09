// Shared native-drag mouse-down handler.
//
// All of this app's windows are frameless, and the Tauri-provided drag path
// (`data-tauri-drag-region` / `startDragging`) hits a known Windows bug
// (tauri#10767) on transparent + `focusable:false` windows: after a few drags
// the webview loses the mouseup and the window gets stuck mid-drag, with focus
// toggling stutter. Instead we call our own Rust command `start_window_drag`,
// which goes straight to Win32 (ReleaseCapture + WM_NCLBUTTONDOWN/HTCAPTION)
// and lets Windows run its own title-bar drag loop. See window_drag.rs.
//
// Attach the returned handler to the drag region element's onMouseDown.

import { invoke } from "@tauri-apps/api/core";
import type { MouseEvent, PointerEvent } from "react";

export function startNativeDrag(label: string): Promise<void> {
  return invoke("start_window_drag", { label }).then(() => undefined);
}

/**
 * Returns an onMouseDown handler that starts a native Win32 drag for the given
 * window label. Only left button triggers it; right/middle are passed through
 * so context menus etc. still work.
 */
export function nativeDragMouseDown(
  label: string,
  hooks: { onDragStart?: () => void; onDragEnd?: () => void } = {}
) {
  let active = false;
  return (event: MouseEvent | PointerEvent) => {
    if (event.button !== 0) return;
    if (active) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("button, input, textarea, select, a, [role='button'], [data-no-drag]")) return;

    event.preventDefault();
    active = true;
    document.documentElement.classList.add("is-native-dragging");
    hooks.onDragStart?.();
    startNativeDrag(label)
      .catch(() => {})
      .finally(() => {
        active = false;
        document.documentElement.classList.remove("is-native-dragging");
        hooks.onDragEnd?.();
      });
  };
}
