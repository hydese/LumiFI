#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! LumiFi: a small, light music player. This is the mini player.

mod actions;
mod assets;
mod mini_player;
mod platform;
mod queue;
mod theme;

use std::path::PathBuf;

use gpui::{
    App, Application, Bounds, Menu, MenuItem, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, prelude::*, px, size,
};

use crate::actions::*;
use crate::mini_player::{MiniPlayer, SIZE};

/// `file:///Users/leo/Music/a%20b.flac` → `/Users/leo/Music/a b.flac`.
fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let mut out = Vec::with_capacity(rest.len());
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == b'%'
            && i + 2 < rest.len()
            && let (Some(high), Some(low)) = (hex(rest[i + 1]), hex(rest[i + 2]))
        {
            out.push((high * 16 + low) as u8);
            i += 3;
        } else {
            out.push(rest[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok().map(PathBuf::from)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    // Files passed on the command line (and by "Open with" on Windows).
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let app = Application::new().with_assets(assets::Assets);
    // macOS "Open with LumiFi" and double-clicked files arrive as file:// URLs, possibly before
    // the window exists; the mini player picks them up on its next tick.
    app.on_open_urls(|urls| {
        let paths = urls.iter().filter_map(|u| file_url_to_path(u)).collect();
        mini_player::queue_opened_paths(paths);
    });
    app.run(move |cx: &mut App| {
        actions::bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![
            Menu {
                name: "LumiFi".into(),
                items: vec![MenuItem::action("Esci da LumiFi", Quit)],
            },
            Menu {
                name: "File".into(),
                items: vec![MenuItem::action("Apri…", Open)],
            },
            Menu {
                name: "Riproduzione".into(),
                items: vec![
                    MenuItem::action("Riproduci / Pausa", TogglePlay),
                    MenuItem::action("Successivo", Next),
                    MenuItem::action("Precedente", Previous),
                    MenuItem::separator(),
                    MenuItem::action("Avanti di 10 secondi", SeekForward),
                    MenuItem::action("Indietro di 10 secondi", SeekBack),
                    MenuItem::separator(),
                    MenuItem::action("Casuale", ToggleShuffle),
                    MenuItem::action("Ripeti", CycleRepeat),
                    MenuItem::separator(),
                    MenuItem::action("Alza il volume", VolumeUp),
                    MenuItem::action("Abbassa il volume", VolumeDown),
                    MenuItem::action("Silenzia", ToggleMute),
                ],
            },
            Menu {
                name: "Vista".into(),
                items: vec![MenuItem::action(
                    "Cambia visualizzazione (A/B/C)",
                    CycleView,
                )],
            },
        ]);

        let bounds = Bounds::centered(None, size(px(SIZE), px(SIZE)), cx);
        // macOS: a panel that floats above other windows. Windows and Linux: a normal
        // window (so it shows in the taskbar), kept on top separately.
        let kind = if cfg!(target_os = "macos") {
            WindowKind::PopUp
        } else {
            WindowKind::Normal
        };
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("LumiFi".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(gpui::point(px(-100.), px(-100.))),
                    }),
                    kind,
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
                },
                |window, cx| {
                    platform::keep_on_top(window, true);
                    cx.new(|cx| MiniPlayer::new(files.clone(), window, cx))
                },
            )
            .expect("could not open the LumiFi window");

        let _ = window;
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::file_url_to_path;
    use std::path::PathBuf;

    #[test]
    fn decodes_file_urls() {
        assert_eq!(
            file_url_to_path("file:///Users/leo/Music/a%20b.flac"),
            Some(PathBuf::from("/Users/leo/Music/a b.flac"))
        );
        assert_eq!(
            file_url_to_path("file:///x/caff%C3%A8.mp3"),
            Some(PathBuf::from("/x/caffè.mp3"))
        );
        assert_eq!(file_url_to_path("https://example.com"), None);
    }
}
