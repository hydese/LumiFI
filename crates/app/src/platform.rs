//! The few things gpui 0.2 does not do for a borderless mini player: keeping it above other
//! windows on Windows, and dragging it by its background on macOS. Each is one system call;
//! this is the only module allowed to use `unsafe`.
#![allow(unsafe_code)]
// The objc 0.2 macros test a `feature` cfg that this crate does not declare.
#![allow(unexpected_cfgs)]

use gpui::Window;

/// Keeps the window above all others (Windows). macOS gets this from the window kind.
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

#[cfg(not(target_os = "windows"))]
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
    // Windows moves the window through the drag area (WindowControlArea::Drag); Wayland and X11
    // ask the compositor.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    window.start_window_move();
    #[cfg(target_os = "windows")]
    let _ = window;
}
