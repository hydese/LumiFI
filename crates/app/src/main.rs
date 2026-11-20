#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

//! LumiFi: a small, light music player. It opens on the library; the mini player (a strip or a
//! square) is one shortcut away (⌘L / Ctrl+L).

mod actions;
mod assets;
mod eq_panel;
mod library;
mod library_window;
mod lyrics;
mod mini_menu;
mod mini_player;
mod platform;
mod playback;
mod playlists;
mod queue;
mod settings;
mod settings_page;
mod text_input;
mod theme;
mod updates;
mod widgets;
mod windows;

use std::path::PathBuf;

use gpui::{App, Application, Menu, MenuItem};

use crate::actions::*;
use crate::library::Library;
use crate::playback::Playback;
use crate::settings::{Settings, SettingsModel, StartWith};

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
    // the window exists; playback picks them up on its next tick.
    app.on_open_urls(|urls| {
        let paths = urls.iter().filter_map(|u| file_url_to_path(u)).collect();
        playback::queue_opened_paths(paths);
    });
    app.run(move |cx: &mut App| {
        theme::install_fonts(cx);
        let settings = Settings::load();
        let start_with_mini = settings.start_with == StartWith::Mini;
        let resume = settings.resume;
        let volume = settings.volume;
        let folders = settings.folders.clone();
        let beta = settings.beta;
        let settings_model = SettingsModel::install(settings, cx);
        let playback = Playback::install(volume, cx);
        // The equalizer and ReplayGain follow the settings while the music plays.
        let audio = playback.clone();
        cx.observe(&settings_model, move |_, cx| {
            audio.update(cx, |p, cx| p.settings_changed(cx));
        })
        .detach();
        Library::install(folders, cx);
        playlists::Playlists::install(cx);
        lyrics::LyricsModel::install(cx);
        // Always look for a new version; the settings only decide whether to ask first.
        updates::Updates::install(true, beta, cx);

        actions::bind_keys(cx);
        text_input::bind_keys(cx);
        actions::register_global(cx);
        // Deferred: these actions arrive while a window is being updated, and windows can only
        // be opened, read and closed once that update is over.
        cx.on_action(|_: &ToggleLibrary, cx| cx.defer(windows::toggle_library));
        cx.on_action(|_: &OpenSettings, cx| cx.defer(|cx| windows::open_settings(None, cx)));
        cx.on_action(|_: &FocusSearch, cx| cx.defer(windows::focus_search));
        // Closing the player (the library's close button) quits, like any music app; the
        // settings window alone does not keep LumiFi open.
        cx.on_window_closed(|cx| {
            if !windows::player_is_open(cx) {
                playback::playback(cx).update(cx, |p, _| p.save_session());
                cx.quit();
            }
        })
        .detach();
        cx.set_menus(vec![
            Menu {
                name: "LumiFi".into(),
                items: vec![
                    MenuItem::action("Impostazioni…", OpenSettings),
                    MenuItem::separator(),
                    MenuItem::action("Esci da LumiFi", Quit),
                ],
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
                items: vec![
                    MenuItem::action("Libreria / Mini player", ToggleLibrary),
                    MenuItem::action("Cerca", FocusSearch),
                    MenuItem::action("Sempre in primo piano", ToggleOnTop),
                ],
            },
        ]);

        if start_with_mini {
            windows::open_mini(cx);
        } else {
            windows::open_library(cx);
        }
        if !files.is_empty() {
            playback.update(cx, |p, cx| p.open_paths(files.clone(), cx));
        } else if resume {
            playback.update(cx, |p, cx| p.resume_session(cx));
        }
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
