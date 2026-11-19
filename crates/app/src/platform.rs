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
