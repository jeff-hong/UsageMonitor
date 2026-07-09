#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};

#[cfg(windows)]
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
use crate::window_drag::DRAGGING;

#[cfg(windows)]
use windows::core::w;
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, FindWindowW, GetAncestor, GetCursorPos, GetMessageW,
    GetWindowRect, SetWindowPos, SetWindowsHookExW, ShowWindow, TranslateMessage,
    UnhookWindowsHookEx, WindowFromPoint, GA_ROOT, HC_ACTION, HWND_TOPMOST, MSG, MSLLHOOKSTRUCT,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WH_MOUSE_LL, WM_RBUTTONDOWN,
    WM_RBUTTONUP,
};

#[cfg(windows)]
static APP_HANDLE: OnceLock<Mutex<AppHandle>> = OnceLock::new();
#[cfg(windows)]
static POINTER_INSIDE: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static LEFT_WAS_DOWN: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static RIGHT_WAS_DOWN: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static SUPPRESS_HOVER_UNTIL_MS: AtomicI64 = AtomicI64::new(0);
#[cfg(windows)]
static LAST_WIDGET_RAISE_MS: AtomicI64 = AtomicI64::new(0);
#[cfg(windows)]
static LAST_CONTEXT_MENU_MS: AtomicI64 = AtomicI64::new(0);

#[cfg(windows)]
pub fn install_widget_mouse_hook(app: AppHandle) {
    let _ = APP_HANDLE.set(Mutex::new(app));
    install_low_level_mouse_hook();
    std::thread::spawn(move || loop {
        poll_widget_mouse();
        std::thread::sleep(std::time::Duration::from_millis(4));
    });
}

#[cfg(windows)]
fn poll_widget_mouse() {
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) }.is_err() {
        return;
    }

    let inside = is_inside_widget(point);
    let left_down = key_is_down(VK_LBUTTON.0 as i32);
    let right_down = key_is_down(VK_RBUTTON.0 as i32);
    let left_was_down = LEFT_WAS_DOWN.swap(left_down, Ordering::SeqCst);
    let right_was_down = RIGHT_WAS_DOWN.swap(right_down, Ordering::SeqCst);
    let left_started = left_down && !left_was_down;
    let right_started = right_down && !right_was_down;

    if !inside && (left_started || right_started) && is_inside_taskbar(point) {
        raise_widget_soon();
    }

    if DRAGGING.load(Ordering::SeqCst) || left_down {
        update_hover_state(false);
        return;
    }

    if inside {
        ensure_widget_interactive_at(point);
    }

    let hover_inside = inside && now_ms() >= SUPPRESS_HOVER_UNTIL_MS.load(Ordering::SeqCst);
    update_hover_state(hover_inside);

    // Right-click over the widget is handled entirely by the low-level mouse
    // hook in `low_level_mouse_proc` (it swallows WM_RBUTTONUP and emits
    // `native-widget-context-menu`). The poll-loop right-click branch that used
    // to live here was redundant and could double-emit — removed.
}

#[cfg(windows)]
fn install_low_level_mouse_hook() {
    std::thread::spawn(|| unsafe {
        let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), None, 0) else {
            tracing::warn!("failed to install widget low-level mouse hook");
            return;
        };

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = UnhookWindowsHookEx(hook);
    });
}

#[cfg(windows)]
unsafe extern "system" fn low_level_mouse_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 {
        let mouse = *(lparam.0 as *const MSLLHOOKSTRUCT);
        let message = wparam.0 as u32;

        if message == WM_RBUTTONDOWN || message == WM_RBUTTONUP {
            if let Some((_, rect)) = widget_hwnd_and_rect() {
                if point_in_rect(mouse.pt, rect) {
                    if message == WM_RBUTTONUP {
                        emit_to_widget("native-widget-pointer", "leave");
                        emit_context_menu(mouse.pt);
                    }
                    return LRESULT(1);
                }
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

#[cfg(windows)]
fn key_is_down(vkey: i32) -> bool {
    unsafe { GetAsyncKeyState(vkey) < 0 }
}

#[cfg(windows)]
fn update_hover_state(inside: bool) {
    let was_inside = POINTER_INSIDE.swap(inside, Ordering::SeqCst);
    if inside == was_inside {
        return;
    }

    emit_to_widget(
        "native-widget-pointer",
        if inside { "enter" } else { "leave" },
    );
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(windows)]
fn emit_to_widget(event: &str, payload: &str) {
    let Some(app) = app_handle() else {
        return;
    };
    let _ = app.emit_to("widget", event, payload);
}

#[cfg(windows)]
fn emit_context_menu(point: POINT) {
    let now = now_ms();
    let last = LAST_CONTEXT_MENU_MS.load(Ordering::SeqCst);
    if now - last < 180 {
        return;
    }
    LAST_CONTEXT_MENU_MS.store(now, Ordering::SeqCst);
    emit_to_widget(
        "native-widget-context-menu",
        &format!(r#"{{"x":{},"y":{}}}"#, point.x, point.y),
    );
}

#[cfg(windows)]
fn raise_widget_soon() {
    let Some(app) = app_handle() else {
        return;
    };
    std::thread::spawn(move || {
        for delay_ms in [120, 360, 720] {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            let Some(window) = app.get_webview_window("widget") else {
                return;
            };
            let Ok(hwnd) = window.hwnd() else {
                return;
            };
            show_and_raise_widget(hwnd);
        }
    });
}

#[cfg(windows)]
fn ensure_widget_interactive_at(point: POINT) {
    let Some((hwnd, _)) = widget_hwnd_and_rect() else {
        return;
    };

    let hit = unsafe { WindowFromPoint(point) };
    let hit_root = if hit.is_invalid() {
        hit
    } else {
        unsafe { GetAncestor(hit, GA_ROOT) }
    };
    if hit_root == hwnd {
        return;
    }

    let now = now_ms();
    let last = LAST_WIDGET_RAISE_MS.load(Ordering::SeqCst);
    if now - last < 120 {
        return;
    }
    LAST_WIDGET_RAISE_MS.store(now, Ordering::SeqCst);
    show_and_raise_widget(hwnd);
}

#[cfg(windows)]
fn show_and_raise_widget(hwnd: HWND) {
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
    let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags) };
}

#[cfg(windows)]
fn is_inside_widget(point: POINT) -> bool {
    let Some((_, rect)) = widget_hwnd_and_rect() else {
        return false;
    };

    point_in_rect(point, rect)
}

#[cfg(windows)]
fn point_in_rect(point: POINT, rect: RECT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

#[cfg(windows)]
fn is_inside_taskbar(point: POINT) -> bool {
    let Ok(taskbar) = (unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }) else {
        return false;
    };
    if taskbar.is_invalid() {
        return false;
    }

    let mut rect = RECT::default();
    if unsafe { GetWindowRect(taskbar, &mut rect) }.is_err() {
        return false;
    }

    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

#[cfg(windows)]
fn widget_hwnd_and_rect() -> Option<(HWND, RECT)> {
    let Some(app) = app_handle() else {
        return None;
    };
    let Some(window) = app.get_webview_window("widget") else {
        return None;
    };
    let Ok(hwnd) = window.hwnd() else {
        return None;
    };
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return None;
    }

    Some((hwnd, rect))
}

#[cfg(windows)]
fn app_handle() -> Option<AppHandle> {
    APP_HANDLE
        .get()
        .and_then(|handle| handle.lock().ok().map(|guard| guard.clone()))
}

#[cfg(not(windows))]
pub fn install_widget_mouse_hook(_: tauri::AppHandle) {}
