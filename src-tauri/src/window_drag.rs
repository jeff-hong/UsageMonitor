//! Native borderless-window dragging on Windows, with a global "is the user
//! currently dragging?" guard that the topmost pump reads.
//!
//! Why this exists: Tauri's built-in drag (`startDragging()` IPC +
//! `data-tauri-drag-region`) routes the pointer through the webview and is
//! buggy on Windows — tauri issue #10767. On `transparent` + `focusable:false`
//! windows the symptoms are: dragging works a few times, then the webview
//! loses the mouseup and the window gets STUCK mid-drag (follows the cursor
//! until the next click), plus focus-toggling stutter.
//!
//! The fix is to stop using Tauri's drag path entirely and drive the move at
//! the Win32 level, exactly like a native title bar: release any capture, then
//! `SendMessage(WM_NCLBUTTONDOWN, HTCAPTION)`. Windows takes over the pointer
//! from that point — no webview involvement, so the stuck-drag bug structurally
//! cannot happen. Same trick Electron, Chromium's HWND handler and countless
//! native apps use.
//!
//! The `DRAGGING` flag is the second half of the fix. These floating windows
//! are `WS_EX_NOACTIVATE` so Windows does NOT re-raise them to top-of-zorder
//! automatically the way it does for activated windows. To stay above the
//! taskbar / other always-on-top windows we have to nudge them back to
//! `HWND_TOPMOST` periodically. BUT doing that `SetWindowPos` while the user is
//! mid-drag fights the OS drag loop and causes exactly the lag/stutter that
//! made dragging feel rough. So: the poll runs at idle, and the moment a drag
//! starts it skips — `SetWindowPos` is deferred until the drag ends. That
//! gives us both smooth dragging AND a window that never sinks behind the
//! taskbar.
//!
//! Refs:
//!   - WM_NCLBUTTONDOWN: https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-nclbuttondown
//!   - ReleaseCapture:   https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-releasecapture

use std::sync::atomic::{AtomicBool, Ordering};

/// `true` while any floating window is being dragged natively. The topmost
/// poll thread checks this and skips its `SetWindowPos` calls while it is set,
/// so re-stacking never interrupts an in-flight OS drag.
pub static DRAGGING: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
use tauri::{AppHandle, Manager};

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SendMessageW, SetWindowPos, ShowWindow, HTCAPTION, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WM_NCLBUTTONDOWN,
};

/// Begin a native drag for a window identified by its Tauri label (e.g.
/// "widget", "detail", "hover_detail"). Called from the frontend on
/// left-button mousedown. The call blocks in Rust until the OS drag loop ends
/// (pointer up), matching `startDragging`'s contract. Sets `DRAGGING` for its
/// duration so the topmost pump gets out of the way.
#[cfg(windows)]
#[tauri::command]
pub fn start_window_drag(app: AppHandle, label: String) {
    let Some(window) = app.get_webview_window(&label) else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };

    // Hide the hover_detail INLINE (not on a spawned thread) so it is gone
    // before the OS drag loop starts — the previous thread spawn let hover
    // linger on screen for a frame after the drag began (visible flash).
    // ShowWindow(SW_HIDE) is a microsecond call, safe to do synchronously.
    if label != "hover_detail" {
        hide_window_hwnd(&app, "hover_detail");
    }

    drag_hwnd(HWND(hwnd.0 as *mut core::ffi::c_void));
}

#[cfg(windows)]
#[tauri::command]
pub fn hide_window_native(app: AppHandle, label: String) {
    hide_window_hwnd(&app, &label);
}

#[cfg(windows)]
#[tauri::command]
pub fn show_window_native(app: AppHandle, label: String) {
    show_window_hwnd(&app, &label);
}

/// Place a floating window next to the widget and (optionally) show it, all in
/// one Win32 round-trip. This collapses what used to be ~5 sequential Tauri
/// IPCs in the frontend (`scaleFactor` / `outerPosition` / `outerSize` /
/// `monitorFromPoint` / `setPosition`) plus a separate `setSize` and
/// `show_window_native` into a single command, so the window appears at its
/// final geometry and z-order atomically — no intermediate paint at the wrong
/// size/position/topmost band.
///
/// Placement mirrors the old frontend `placeWindowNearWidget`:
///   - anchor = the `widget` window's screen rect (read via GetWindowRect)
///   - work area = the monitor under the widget's center (MonitorFromPoint +
///     GetMonitorInfoW rcWork), so multi-monitor + taskbar insets are honored
///   - x: prefer the left gap (widget.left - width - GAP); if that clips the
///     work area, fall back to the right gap (widget.right + GAP); otherwise
///     clamp inside the work area
///   - y: aligned to the widget's top (vertical="top") or bottom, clamped into
///     the work area
/// Then one SetWindowPos applies x/y/w/h + HWND_TOPMOST + (show ? 0 : NOSIZE
/// already applied). When `show`, ShowWindow(SW_SHOWNOACTIVATE) first so the
/// CSS `hover-card-in` entry animation fires on the now-visible surface.
#[cfg(windows)]
#[tauri::command]
pub fn place_and_show_window(
    app: AppHandle,
    label: String,
    width: i32,
    height: i32,
    side: String,
    vertical: String,
    show: bool,
    hide_hover: bool,
) {
    let Some(window) = app.get_webview_window(&label) else {
        return;
    };
    let Ok(target_hwnd) = window.hwnd() else {
        return;
    };
    let target_hwnd = HWND(target_hwnd.0 as *mut core::ffi::c_void);

    if hide_hover {
        hide_window_hwnd(&app, "hover_detail");
    }

    let Some((wx, wy, ww, wh)) = widget_screen_rect(&app) else {
        // No widget to anchor against — just center-ish on the primary screen.
        return;
    };
    let Some(work) = work_area_for_point(wx + ww / 2, wy + wh / 2) else {
        return;
    };

    const GAP: i32 = 12;
    const EDGE: i32 = 12;

    let min_x = work.left + EDGE;
    let max_x = work.right - EDGE - width;
    let left_x = wx - width - GAP;
    let right_x = wx + ww + GAP;
    let has_left = left_x >= min_x;
    let has_right = right_x <= max_x;

    let prefer_left = side == "left";
    let x = if prefer_left {
        if has_left {
            left_x
        } else {
            right_x.clamp(min_x, max_x.max(min_x))
        }
    } else if has_right {
        right_x
    } else if has_left {
        left_x
    } else {
        wx.clamp(min_x, max_x.max(min_x))
    };

    let raw_y = if vertical == "top" { wy } else { wy + wh - height };
    let y = raw_y.clamp(work.top + EDGE, work.bottom - EDGE - height);

    if show {
        // Only the hover_detail (focusable:false, passive preview) is shown
        // through this path. Interactive windows like `detail` are shown +
        // activated via Tauri's window API from the frontend so their clicks
        // work reliably. SW_SHOWNOACTIVATE keeps hover_detail from stealing
        // focus from whatever the user is doing.
        let _ = unsafe { ShowWindow(target_hwnd, SW_SHOWNOACTIVATE) };
    }
    let _ = unsafe {
        SetWindowPos(
            target_hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE,
        )
    };
}

/// Screen rect (left, top, width, height) of the `widget` window, in physical
/// pixels. Returns None if the widget isn't open yet.
#[cfg(windows)]
fn widget_screen_rect(app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
    let window = app.get_webview_window("widget")?;
    let hwnd = window.hwnd().ok()?;
    let mut rect = RECT::default();
    unsafe { GetWindowRect(HWND(hwnd.0 as *mut core::ffi::c_void), &mut rect) }.ok()?;
    Some((rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top))
}

/// Work area (excluding taskbar) of the monitor containing the given screen
/// point, in physical pixels.
#[cfg(windows)]
fn work_area_for_point(x: i32, y: i32) -> Option<RECT> {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool().then_some(info.rcWork)
}

#[cfg(not(windows))]
#[tauri::command]
pub fn hide_window_native(_app: tauri::AppHandle, _label: String) {}

#[cfg(not(windows))]
#[tauri::command]
pub fn show_window_native(_app: tauri::AppHandle, _label: String) {}

#[cfg(not(windows))]
#[tauri::command]
pub fn place_and_show_window(
    _app: tauri::AppHandle,
    _label: String,
    _width: i32,
    _height: i32,
    _side: String,
    _vertical: String,
    _show: bool,
    _hide_hover: bool,
) {
}

#[cfg(windows)]
fn hide_window_hwnd(app: &AppHandle, label: &str) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let _ = unsafe { ShowWindow(HWND(hwnd.0 as *mut core::ffi::c_void), SW_HIDE) };
}

#[cfg(windows)]
fn show_window_hwnd(app: &AppHandle, label: &str) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(hwnd.0 as *mut core::ffi::c_void);
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) };
}

#[cfg(windows)]
pub fn drag_hwnd(hwnd: HWND) {
    DRAGGING.store(true, Ordering::SeqCst);
    begin_native_drag(hwnd);
    // begin_native_drag returns only after the OS drag loop finishes.
    DRAGGING.store(false, Ordering::SeqCst);
}

#[cfg(windows)]
fn begin_native_drag(hwnd: HWND) {
    // 1. Release any current mouse capture. WM_NCLBUTTONDOWN is NOT delivered
    //    while a window has the capture (documented on WM_NCLBUTTONDOWN), so
    //    this is mandatory before sending it.
    let _ = unsafe { ReleaseCapture() };

    // 2. Tell the window the user grabbed the caption (title bar) with the
    //    left button. DefWindowProc interprets this as a title-bar drag and
    //    runs its own move loop, following the mouse until button-up. Entirely
    //    on the OS side, so the webview never has to track the pointer and the
    //    #10767 stuck-drag / focus-toggle bug can't happen.
    let _ = unsafe {
        SendMessageW(
            hwnd,
            WM_NCLBUTTONDOWN,
            Some(WPARAM(HTCAPTION as usize)),
            Some(LPARAM(0)),
        )
    };
}

#[cfg(not(windows))]
#[tauri::command]
pub fn start_window_drag(_app: tauri::AppHandle, _label: String) {}
