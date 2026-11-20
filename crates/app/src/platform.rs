//! The few things gpui 0.2 does not do for a borderless mini player: keeping it above other
//! windows, and dragging it by its background. Each is one system call;
//! this is the only module allowed to use `unsafe`.
#![allow(unsafe_code)]
// The objc 0.2 macros test a `feature` cfg that this crate does not declare.
#![allow(unexpected_cfgs)]

use gpui::Window;

/// Keeps the window above all others, or puts it back among them.
#[cfg(target_os = "windows")]
pub fn keep_on_top(window: &Window, on_top: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    if let RawWindowHandle::Win32(win) = handle.as_raw() {
        let hwnd = win.hwnd.get() as windows_sys::Win32::Foundation::HWND;
        let after = if on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY: `hwnd` is this window's live handle, owned by gpui for the window's lifetime.
        unsafe {
            SetWindowPos(
                hwnd,
                after,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

#[cfg(target_os = "macos")]
pub fn keep_on_top(window: &Window, on_top: bool) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    // NSFloatingWindowLevel and NSNormalWindowLevel.
    const FLOATING: isize = 3;
    const NORMAL: isize = 0;
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    if let RawWindowHandle::AppKit(appkit) = handle.as_raw() {
        let view = appkit.ns_view.as_ptr() as *mut Object;
        // SAFETY: the view belongs to this live window.
        unsafe {
            let ns_window: *mut Object = msg_send![view, window];
            if !ns_window.is_null() {
                let level = if on_top { FLOATING } else { NORMAL };
                let _: () = msg_send![ns_window, setLevel: level];
            }
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn keep_on_top(_window: &Window, _on_top: bool) {}

/// Starts moving the window with the mouse button that is down right now.
pub fn start_drag(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use objc::runtime::Object;
        use objc::{class, msg_send, sel, sel_impl};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        if let RawWindowHandle::AppKit(appkit) = handle.as_raw() {
            let view = appkit.ns_view.as_ptr() as *mut Object;
            // SAFETY: the view belongs to this live window; the current event is the mouse-down
            // that gpui is handling, which is what performWindowDragWithEvent expects.
            unsafe {
                let ns_window: *mut Object = msg_send![view, window];
                let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
                let event: *mut Object = msg_send![app, currentEvent];
                if !ns_window.is_null() && !event.is_null() {
                    let _: () = msg_send![ns_window, performWindowDragWithEvent: event];
                }
            }
        }
    }
    // Windows: hand the mouse to the system's own "move window" loop. The message is posted,
    // not sent, so the loop starts after gpui has finished handling this click.
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, SC_MOVE, WM_SYSCOMMAND};
        /// SC_MOVE combined with HTCAPTION: move with the mouse, as if dragging the title bar.
        const DRAG_MOVE: usize = SC_MOVE as usize | 0x0002;
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        if let RawWindowHandle::Win32(win) = handle.as_raw() {
            let hwnd = win.hwnd.get() as windows_sys::Win32::Foundation::HWND;
            // SAFETY: `hwnd` is this window's live handle; both calls only queue work for it.
            unsafe {
                ReleaseCapture();
                PostMessageW(hwnd, WM_SYSCOMMAND, DRAG_MOVE, 0);
            }
        }
    }
    // Wayland and X11 ask the compositor.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    window.start_window_move();
}

/// Takes a window out of the maximized state, if it is in it (the mini player must never be).
pub fn unmaximize(window: &mut Window) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IsZoomed, SW_RESTORE, ShowWindowAsync};
        if let Some(hwnd) = hwnd(window) {
            // SAFETY: `hwnd` is this window's live handle.
            unsafe {
                if IsZoomed(hwnd) != 0 {
                    ShowWindowAsync(hwnd, SW_RESTORE);
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    if window.is_maximized() {
        window.zoom_window();
    }
}

/// Maximizes the window, or restores it if it already is (the title bar's middle button and
/// double-click).
pub fn toggle_maximize(window: &mut Window) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IsZoomed, SW_MAXIMIZE, SW_RESTORE, ShowWindowAsync,
        };
        if let Some(hwnd) = hwnd(window) {
            // Asynchronous, as gpui does it: a synchronous ShowWindow resizes the window in the
            // middle of gpui handling the click, and gpui then misses the new size (the page
            // stayed small in a corner of the maximized window).
            // SAFETY: `hwnd` is this window's live handle.
            unsafe {
                let command = if IsZoomed(hwnd) != 0 {
                    SW_RESTORE
                } else {
                    SW_MAXIMIZE
                };
                ShowWindowAsync(hwnd, command);
            }
        }
    }
    #[cfg(target_os = "macos")]
    window.titlebar_double_click();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    window.zoom_window();
}

#[cfg(target_os = "windows")]
fn hwnd(window: &Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win) => Some(win.hwnd.get() as windows_sys::Win32::Foundation::HWND),
        _ => None,
    }
}

/// The part of the window's screen not covered by the taskbar, menu bar or Dock, in the same
/// coordinates gpui uses for windows. `None` where the system cannot tell.
#[cfg(target_os = "windows")]
pub fn work_area(window: &Window) -> Option<gpui::Bounds<gpui::Pixels>> {
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    let hwnd = hwnd(window)?;
    let scale = window.scale_factor();
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: plain queries about this window's monitor into a correctly sized struct.
    let ok = unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info)
    };
    if ok == 0 {
        return None;
    }
    let r = info.rcWork;
    Some(gpui::Bounds::new(
        gpui::point(
            gpui::px(r.left as f32 / scale),
            gpui::px(r.top as f32 / scale),
        ),
        gpui::size(
            gpui::px((r.right - r.left) as f32 / scale),
            gpui::px((r.bottom - r.top) as f32 / scale),
        ),
    ))
}

#[cfg(target_os = "macos")]
pub fn work_area(window: &Window) -> Option<gpui::Bounds<gpui::Pixels>> {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    let ns_window = ns_window(window)?;
    // SAFETY: messages to this live window's screen and to the screen list; NSRect is returned
    // by value.
    let (visible, primary_frame) = unsafe {
        let screen: *mut Object = msg_send![ns_window, screen];
        if screen.is_null() {
            return None;
        }
        let visible: NSRect = msg_send![screen, visibleFrame];
        let screens: *mut Object = msg_send![class!(NSScreen), screens];
        let primary: *mut Object = msg_send![screens, objectAtIndex: 0usize];
        let primary_frame: NSRect = msg_send![primary, frame];
        (visible, primary_frame)
    };
    // Cocoa counts from the bottom of the main screen; gpui from its top.
    let top = primary_frame.h - (visible.y + visible.h);
    Some(gpui::Bounds::new(
        gpui::point(gpui::px(visible.x as f32), gpui::px(top as f32)),
        gpui::size(gpui::px(visible.w as f32), gpui::px(visible.h as f32)),
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn work_area(_window: &Window) -> Option<gpui::Bounds<gpui::Pixels>> {
    None
}

/// Moves the window so its top-left corner is at `origin` (gpui screen coordinates).
#[cfg(target_os = "windows")]
pub fn move_to(window: &Window, origin: gpui::Point<gpui::Pixels>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    };
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    let scale = window.scale_factor();
    let x = (f32::from(origin.x) * scale).round() as i32;
    let y = (f32::from(origin.y) * scale).round() as i32;
    // SAFETY: `hwnd` is this window's live handle.
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
    }
}

#[cfg(target_os = "macos")]
pub fn move_to(window: &Window, origin: gpui::Point<gpui::Pixels>) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    // SAFETY: messages to this live window and to the screen list.
    unsafe {
        let screens: *mut Object = msg_send![class!(NSScreen), screens];
        let primary: *mut Object = msg_send![screens, objectAtIndex: 0usize];
        let primary_frame: NSRect = msg_send![primary, frame];
        let point = NSPoint {
            x: f64::from(f32::from(origin.x)),
            y: primary_frame.h - f64::from(f32::from(origin.y)),
        };
        let _: () = msg_send![ns_window, setFrameTopLeftPoint: point];
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn move_to(_window: &Window, _origin: gpui::Point<gpui::Pixels>) {}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[cfg(target_os = "macos")]
fn ns_window(window: &Window) -> Option<*mut objc::runtime::Object> {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::AppKit(appkit) => {
            let view = appkit.ns_view.as_ptr() as *mut Object;
            // SAFETY: the view belongs to this live window.
            let ns_window: *mut Object = unsafe { msg_send![view, window] };
            (!ns_window.is_null()).then_some(ns_window)
        }
        _ => None,
    }
}
