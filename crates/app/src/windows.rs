//! Opening the two windows and switching between them (⌘L / Ctrl+L). Only one is open at a
//! time; the music keeps playing because it lives in `Playback`, not in a window.

use gpui::{
    App, AppContext, Bounds, Global, Pixels, Point, Size, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, point, px,
    size,
};

use crate::library_window::LibraryWindow;
use crate::mini_player::{MiniPlayer, SIZE};

#[derive(Default)]
struct Windows {
    mini: Option<WindowHandle<MiniPlayer>>,
    library: Option<WindowHandle<LibraryWindow>>,
    /// Where each window was last, so switching back puts it in the same place.
    mini_origin: Option<Point<Pixels>>,
    library_bounds: Option<Bounds<Pixels>>,
}

impl Global for Windows {}

fn windows(cx: &mut App) -> &mut Windows {
    if !cx.has_global::<Windows>() {
        cx.set_global(Windows::default());
    }
    cx.global_mut::<Windows>()
}

pub fn open_mini(cx: &mut App) {
    let origin = windows(cx).mini_origin;
    let bounds = match origin {
        Some(origin) => Bounds::new(origin, size(px(SIZE), px(SIZE))),
        None => Bounds::centered(None, size(px(SIZE), px(SIZE)), cx),
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("LumiFi".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(-100.), px(-100.))),
        }),
        kind: WindowKind::Normal,
        is_resizable: false,
        is_minimizable: true,
        window_background: if cfg!(target_os = "linux") {
            WindowBackgroundAppearance::Opaque
        } else {
            WindowBackgroundAppearance::Transparent
        },
        window_decorations: Some(gpui::WindowDecorations::Client),
        app_id: Some("lumifi".into()),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        cx.new(|cx| MiniPlayer::new(window, cx))
    }) {
        Ok(handle) => windows(cx).mini = Some(handle),
        Err(error) => log::error!("cannot open the mini player: {error}"),
    }
}

pub fn open_library(cx: &mut App) {
    let saved = windows(cx).library_bounds;
    let bounds = saved.unwrap_or_else(|| Bounds::centered(None, size(px(1100.), px(720.)), cx));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("LumiFi".into()),
            // On macOS the sidebar runs under the window buttons; elsewhere the system draws a
            // normal title bar.
            appears_transparent: cfg!(target_os = "macos"),
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        kind: WindowKind::Normal,
        is_resizable: true,
        is_minimizable: true,
        window_min_size: Some(Size {
            width: px(760.),
            height: px(480.),
        }),
        app_id: Some("lumifi".into()),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        cx.new(|cx| LibraryWindow::new(window, cx))
    }) {
        Ok(handle) => windows(cx).library = Some(handle),
        Err(error) => log::error!("cannot open the library: {error}"),
    }
}

/// Mini player ⇄ library. The new window opens before the old one closes, so the app never has
/// zero windows (which would quit it).
pub fn toggle_library(cx: &mut App) {
    let library = windows(cx).library.take();
    if let Some(handle) = library
        && let Ok(bounds) = handle.update(cx, |_, window, _| window.bounds())
    {
        windows(cx).library_bounds = Some(bounds);
        open_mini(cx);
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        return;
    }
    let mini = windows(cx).mini.take();
    if let Some(handle) = mini
        && let Ok(origin) = handle.update(cx, |_, window, _| window.bounds().origin)
    {
        windows(cx).mini_origin = Some(origin);
        open_library(cx);
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        return;
    }
    open_library(cx);
}
