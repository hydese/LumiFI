//! The app's windows: the library (where LumiFi opens, settings included) and the mini player as
//! an alternative to it (⌘L / Ctrl+L). They are never open together; the music keeps playing
//! because it lives in `Playback`, not in a window.

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Global, Pixels, Point, Size, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, point, px,
    size,
};

use crate::library_window::LibraryWindow;
use crate::mini_player::{self, MiniPlayer};
use crate::settings::settings;
use crate::settings_page::Section;

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
    let value = settings(cx).read(cx).value.clone();
    let style = value.mini_style;
    let window_size = mini_player::window_size(style, value.mini_scale);
    let bounds = match windows(cx).mini_origin {
        Some(origin) => Bounds::new(origin, window_size),
        None => Bounds::centered(None, window_size, cx),
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("LumiFi".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(-100.), px(-100.))),
        }),
        kind: WindowKind::Normal,
        // Sizes come from the right-click menu: a fixed window cannot be snapped or stretched.
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

fn normal_window(title: &str, bounds: Bounds<Pixels>, min: Size<Pixels>) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.to_owned().into()),
            // No system title bar: the header is the title bar (as in LumiPDF). On macOS the three
            // window buttons sit inside it.
            appears_transparent: true,
            traffic_light_position: Some(point(px(18.), px(19.))),
        }),
        kind: WindowKind::Normal,
        is_resizable: true,
        is_minimizable: true,
        window_min_size: Some(min),
        window_decorations: Some(gpui::WindowDecorations::Client),
        app_id: Some("lumifi".into()),
        ..Default::default()
    }
}

pub fn open_library(cx: &mut App) {
    let saved = windows(cx).library_bounds;
    let bounds = saved.unwrap_or_else(|| Bounds::centered(None, size(px(1180.), px(760.)), cx));
    let options = normal_window("LumiFi", bounds, size(px(720.), px(480.)));
    match cx.open_window(options, |window, cx| {
        cx.new(|cx| LibraryWindow::new(window, cx))
    }) {
        Ok(handle) => windows(cx).library = Some(handle),
        Err(error) => log::error!("cannot open the library: {error}"),
    }
}

/// Shows the settings page of the library, opening the library first if the mini player is
/// showing.
pub fn open_settings(section: Option<Section>, cx: &mut App) {
    if windows(cx).library.is_none() {
        toggle_library(cx);
    }
    if let Some(handle) = windows(cx).library {
        let _ = handle.update(cx, |view, window, cx| {
            view.show_settings(section, cx);
            window.activate_window();
        });
    }
}

/// Puts the keyboard in the library's search field.
pub fn focus_search(cx: &mut App) {
    if let Some(handle) = windows(cx).library {
        let _ = handle.update(cx, |view, window, cx| view.focus_search(window, cx));
    }
}

/// True while the library or the mini player is open. The settings alone do not keep LumiFi
/// running.
pub fn player_is_open(cx: &mut App) -> bool {
    let w = windows(cx);
    let handles: Vec<AnyWindowHandle> = [w.mini.map(Into::into), w.library.map(Into::into)]
        .into_iter()
        .flatten()
        .collect();
    let open = cx.windows();
    handles.iter().any(|h| open.contains(h))
}

/// Mini player ⇄ library. The new window opens before the old one closes, so LumiFi never has
/// no player window (which would quit it).
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
