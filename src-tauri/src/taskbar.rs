use tauri::WebviewWindow;

#[cfg(windows)]
use windows::core::w;
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, RECT};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetParent, GetWindowLongPtrW, GetWindowRect, SetParent,
    SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, WS_CHILD, WS_POPUP,
};

const TASKBAR_GAP_LOGICAL: f64 = 6.0;

// A floating HWND_TOPMOST window can still fall behind the taskbar, which is
// also topmost. Reassert its order only when an overlapping taskbar is above it.
#[cfg(windows)]
pub fn maintain_floating_topmost(window: &WebviewWindow) -> Result<(), String> {
    use crate::settings::TaskbarSettings;
    use std::sync::RwLock;
    use tauri::Manager;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindow, IsWindowVisible, GW_HWNDPREV, HWND_TOPMOST, SWP_ASYNCWINDOWPOS,
    };

    let state = window.state::<RwLock<TaskbarSettings>>();
    let Ok(settings) = state.try_read() else {
        return Ok(());
    };
    let enabled = settings.window_mode == "floating" && settings.always_on_top;
    drop(settings);
    if !enabled {
        return Ok(());
    }
    let hwnd = native_hwnd(window)?;
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { GetParent(hwnd) }.is_ok() {
        return Ok(());
    }
    let floating = window_rect(hwnd)?;
    let mut cursor = hwnd;
    for _ in 0..4096 {
        let Ok(above) = (unsafe { GetWindow(cursor, GW_HWNDPREV) }) else {
            break;
        };
        cursor = above;
        let mut class = [0u16; 64];
        let length = unsafe { GetClassNameW(above, &mut class) } as usize;
        let class = String::from_utf16_lossy(&class[..length]);
        if !matches!(class.as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
            || !unsafe { IsWindowVisible(above) }.as_bool()
        {
            continue;
        }
        let Ok(taskbar) = window_rect(above) else {
            continue;
        };
        if floating.left < taskbar.right
            && floating.right > taskbar.left
            && floating.top < taskbar.bottom
            && floating.bottom > taskbar.top
        {
            unsafe {
                SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                )
            }
            .map_err(|e| e.to_string())?;
            break;
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn maintain_floating_topmost(_window: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
mod z_order_events {
    use super::*;
    use std::cell::RefCell;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, EVENT_OBJECT_REORDER,
        EVENT_SYSTEM_FOREGROUND, MSG, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };

    thread_local! { static WINDOW: RefCell<Option<WebviewWindow>> = const { RefCell::new(None) }; }

    unsafe extern "system" fn changed(
        _hook: HWINEVENTHOOK,
        _event: u32,
        _hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        WINDOW.with(|slot| {
            if let Some(window) = slot.borrow().as_ref() {
                let _ = maintain_floating_topmost(window);
            }
        });
    }

    pub fn spawn(window: WebviewWindow) {
        std::thread::spawn(move || {
            WINDOW.with(|slot| *slot.borrow_mut() = Some(window));
            let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
            // Out-of-context events are delivered on this thread's message
            // queue. Ignore our own reordering to avoid a feedback loop.
            let foreground = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(changed),
                    0,
                    0,
                    flags,
                )
            };
            let reorder = unsafe {
                SetWinEventHook(
                    EVENT_OBJECT_REORDER,
                    EVENT_OBJECT_REORDER,
                    None,
                    Some(changed),
                    0,
                    0,
                    flags,
                )
            };
            if !foreground.0.is_null() || !reorder.0.is_null() {
                let mut message = MSG::default();
                while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
                    unsafe {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            }
            if !foreground.0.is_null() {
                unsafe {
                    let _ = UnhookWinEvent(foreground);
                }
            }
            if !reorder.0.is_null() {
                unsafe {
                    let _ = UnhookWinEvent(reorder);
                }
            }
            WINDOW.with(|slot| *slot.borrow_mut() = None);
        });
    }
}

pub fn watch_z_order(window: WebviewWindow) {
    #[cfg(windows)]
    z_order_events::spawn(window);
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(windows)]
fn native_hwnd(window: &WebviewWindow) -> Result<HWND, String> {
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    Ok(hwnd)
}

#[cfg(windows)]
fn window_rect(hwnd: HWND) -> Result<RECT, String> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.map_err(|error| error.to_string())?;
    Ok(rect)
}

#[cfg(windows)]
pub fn embed_taskbar(
    window: &WebviewWindow,
    offset: i32,
    width_logical: f64,
    right_anchor: bool,
    secondary_taskbar: bool,
) -> Result<(), String> {
    let hwnd = native_hwnd(window)?;
    let taskbar_class = if secondary_taskbar {
        w!("Shell_SecondaryTrayWnd")
    } else {
        w!("Shell_TrayWnd")
    };
    let taskbar = unsafe { FindWindowW(taskbar_class, None) }
        .map_err(|_| "未找到 Windows 任务栏".to_string())?;

    unsafe {
        if GetParent(hwnd).ok() != Some(taskbar) {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let child_style = ((style as u32) | WS_CHILD.0) & !WS_POPUP.0;
            SetWindowLongPtrW(hwnd, GWL_STYLE, child_style as isize);
            // SetParent returns null both for an unparented window and on failure;
            // GetParent below is the reliable success check for the first embedding.
            let _ = SetParent(hwnd, Some(taskbar));
        }
    }
    if unsafe { GetParent(hwnd) }.ok() != Some(taskbar) {
        return Err("无法将 Token Speed 嵌入 Windows 任务栏".to_string());
    }

    let taskbar_rect = window_rect(taskbar)?;
    let notification_area =
        unsafe { FindWindowExW(Some(taskbar), None, w!("TrayNotifyWnd"), None) };
    let notification_rect = match notification_area {
        Ok(area) => window_rect(area)?,
        Err(_) => taskbar_rect,
    };

    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    let width = (width_logical * scale).round() as i32;
    let gap = (TASKBAR_GAP_LOGICAL * scale).round() as i32;
    let height = taskbar_rect.bottom - taskbar_rect.top;
    let taskbar_width = taskbar_rect.right - taskbar_rect.left;
    let notification_left = notification_rect.left - taskbar_rect.left;
    let anchor = if notification_left > 0 {
        notification_left
    } else {
        taskbar_width
    };
    let preferred = if right_anchor {
        anchor - width - gap + (offset as f64 * scale).round() as i32
    } else {
        gap + (offset as f64 * scale).round() as i32
    };
    let x = preferred.clamp(0, (taskbar_width - width).max(0));

    unsafe {
        SetWindowPos(hwnd, Some(HWND_TOP), x, 0, width, height, SWP_NOACTIVATE)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn embed_taskbar(
    _window: &WebviewWindow,
    _offset: i32,
    _width_logical: f64,
    _right_anchor: bool,
    _secondary_taskbar: bool,
) -> Result<(), String> {
    Err("任务栏嵌入仅支持 Windows".to_string())
}

#[cfg(windows)]
pub fn detach_taskbar(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = native_hwnd(window)?;
    unsafe {
        if GetParent(hwnd).is_ok() {
            let _ = SetParent(hwnd, None);
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_STYLE,
                (((style as u32) & !WS_CHILD.0) | WS_POPUP.0) as isize,
            );
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            )
            .map_err(|e| e.to_string())?;
            // A WS_CHILD window reports the desktop as its parent immediately
            // after SetParent(NULL). Check only after restoring WS_POPUP.
            if GetParent(hwnd).is_ok() {
                return Err("无法分离悬浮窗口".into());
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn detach_taskbar(_window: &WebviewWindow) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub fn settings_lost_focus(window: &tauri::Window) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetForegroundWindow, GA_ROOTOWNER};
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    let foreground = unsafe { GetForegroundWindow() };
    foreground != hwnd && unsafe { GetAncestor(foreground, GA_ROOTOWNER) } != hwnd
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorGeometry {
    pub x: i32,
    pub y: i32,
    pub width: f64,
}

#[cfg(windows)]
pub fn monitor_geometry(window: &WebviewWindow) -> Result<MonitorGeometry, String> {
    let r = window_rect(native_hwnd(window)?)?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    Ok(MonitorGeometry {
        x: r.left,
        y: r.top,
        width: (r.right - r.left) as f64 / scale,
    })
}

#[cfg(not(windows))]
pub fn monitor_geometry(window: &WebviewWindow) -> Result<MonitorGeometry, String> {
    let p = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    Ok(MonitorGeometry {
        x: p.x,
        y: p.y,
        width: size.width as f64,
    })
}

#[cfg(not(windows))]
pub fn settings_lost_focus(window: &tauri::Window) -> bool {
    !window.is_focused().unwrap_or(true)
}
