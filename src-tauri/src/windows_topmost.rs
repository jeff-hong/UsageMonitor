#[cfg(windows)]
use tauri::{AppHandle, Manager};

#[cfg(windows)]
use crate::window_drag::DRAGGING;

#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, IsWindowVisible, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_FRAMECHANGED, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

#[cfg(windows)]
pub fn keep_floating_windows_topmost(app: &AppHandle) {
    if DRAGGING.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    for label in ["widget", "hover_detail"] {
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };

        if let Ok(hwnd) = window.hwnd() {
            if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
                continue;
            }

            let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
            let capture_friendly = cfg!(debug_assertions)
                && std::env::var_os("USAGE_MONITOR_CAPTURE_FRIENDLY").is_some();
            let mut wanted = style | WS_EX_TOPMOST.0 as isize;
            if capture_friendly {
                wanted = (wanted | WS_EX_APPWINDOW.0 as isize) & !(WS_EX_TOOLWINDOW.0 as isize);
            } else {
                wanted = (wanted | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize);
            }
            if label == "hover_detail" {
                wanted |= WS_EX_NOACTIVATE.0 as isize;
            } else {
                wanted &= !(WS_EX_NOACTIVATE.0 as isize);
            }
            if wanted != style {
                // Style drifted (e.g. another app or the OS reset our extended
                // styles). Re-apply and flush with SWP_FRAMECHANGED — without
                // that flag Win32 doesn't fully commit the new ex-style, so the
                // next tick would see `wanted != style` again and re-write it
                // every 250ms (steady-state churn / subtle compositing jitter).
                let _ = unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted) };
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED;
                let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) };
            } else {
                // Style already correct — only nudge the z-order back to
                // HWND_TOPMOST (these are WS_EX_NOACTIVATE windows that Windows
                // won't auto-raise). NOMOVE|NOSIZE|NOACTIVATE makes this cheap.
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
                let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) };
            }
        }
    }
}

#[cfg(not(windows))]
pub fn keep_floating_windows_topmost(_: &tauri::AppHandle) {}
