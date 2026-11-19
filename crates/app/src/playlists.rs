//! Playlists, kept as plain .m3u8 files in LumiFi's data folder, so other players can open them
//! too. "Preferiti" (the heart) is a playlist like the others, always first.

use std::path::{Path, PathBuf};

use gpui::{App, AppContext, Context, Entity, Global};

use crate::settings::Paths;

pub const FAVORITES: &str = "Preferiti";

#[derive(Debug, Clone, PartialEq)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<PathBuf>,
}

impl Playlist {
    pub fn is_favorites(&self) -> bool {
        self.name == FAVORITES
    }
}

/// Reads an .m3u / .m3u8 file: one path per line, `#` lines are comments. Relative paths are
/// taken from the playlist's folder.
pub fn parse_m3u(text: &str, base: &Path) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let path = PathBuf::from(line.trim_start_matches('\u{feff}'));
            if path.is_absolute() {
                path
            } else {
                base.join(path)
            }
        })
        .collect()
}

pub fn write_m3u(tracks: &[PathBuf]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for track in tracks {
        out.push_str(&track.to_string_lossy());
        out.push('\n');
    }
    out
}

/// A file name that is safe on every system.
fn file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    format!("{}.m3u8", clean.trim())
}

pub struct Playlists {
    dir: PathBuf,
    lists: Vec<Playlist>,
}

pub struct GlobalPlaylists(pub Entity<Playlists>);
impl Global for GlobalPlaylists {}

pub fn playlists(cx: &App) -> Entity<Playlists> {
    cx.global::<GlobalPlaylists>().0.clone()
}

impl Playlists {
    pub fn install(cx: &mut App) -> Entity<Playlists> {
        let entity = cx.new(|_| Playlists::load(Paths::data_dir().join("playlists")));
        cx.set_global(GlobalPlaylists(entity.clone()));
        entity
    }

    pub fn load(dir: PathBuf) -> Self {
        let mut lists = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_list = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    e.eq_ignore_ascii_case("m3u8") || e.eq_ignore_ascii_case("m3u")
                });
                if !is_list {
                    continue;
                }
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                lists.push(Playlist {
                    tracks: parse_m3u(&text, &dir),
                    name,
                });
            }
        }
        if !lists.iter().any(Playlist::is_favorites) {
            lists.push(Playlist {
                name: FAVORITES.to_owned(),
                tracks: Vec::new(),
            });
        }
        let mut this = Self { dir, lists };
        this.sort();
        this
    }

    fn sort(&mut self) {
        self.lists.sort_by(|a, b| {
            (!a.is_favorites(), a.name.to_lowercase())
                .cmp(&(!b.is_favorites(), b.name.to_lowercase()))
        });
    }

    pub fn all(&self) -> &[Playlist] {
        &self.lists
    }

    pub fn get(&self, name: &str) -> Option<&Playlist> {
        self.lists.iter().find(|p| p.name == name)
    }

    fn save(&self, list: &Playlist) {
        let _ = std::fs::create_dir_all(&self.dir);
        let path = self.dir.join(file_name(&list.name));
        if let Err(error) = std::fs::write(&path, write_m3u(&list.tracks)) {
            log::warn!("cannot save playlist {}: {error}", path.display());
        }
    }

    pub fn is_favorite(&self, path: &Path) -> bool {
        self.get(FAVORITES)
            .is_some_and(|p| p.tracks.iter().any(|t| t == path))
    }

    pub fn toggle_favorite(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.is_favorite(path) {
            self.remove_track(FAVORITES, path, cx);
        } else {
            self.add(FAVORITES, vec![path.to_path_buf()], cx);
        }
    }

    /// Creates a playlist with a free name like "Nuova playlist 2" and returns the name.
    pub fn create(&mut self, cx: &mut Context<Self>) -> String {
        let mut n = 1;
        let name = loop {
            let candidate = if n == 1 {
                "Nuova playlist".to_owned()
            } else {
                format!("Nuova playlist {n}")
            };
            if self.get(&candidate).is_none() {
                break candidate;
            }
            n += 1;
        };
        let list = Playlist {
            name: name.clone(),
            tracks: Vec::new(),
        };
        self.save(&list);
        self.lists.push(list);
        self.sort();
        cx.notify();
        name
    }

    /// Renames a playlist; returns the name it ended up with (unchanged if taken or empty).
    pub fn rename(&mut self, from: &str, to: &str, cx: &mut Context<Self>) -> String {
        let to = to.trim();
        if to.is_empty() || from == FAVORITES || to == FAVORITES || self.get(to).is_some() {
            return from.to_owned();
        }
        let Some(list) = self.lists.iter_mut().find(|p| p.name == from) else {
            return from.to_owned();
        };
        let _ = std::fs::remove_file(self.dir.join(file_name(from)));
        list.name = to.to_owned();
        let list = list.clone();
        self.save(&list);
        self.sort();
        cx.notify();
        to.to_owned()
    }

    pub fn delete(&mut self, name: &str, cx: &mut Context<Self>) {
        if name == FAVORITES {
            return;
        }
        self.lists.retain(|p| p.name != name);
        let _ = std::fs::remove_file(self.dir.join(file_name(name)));
        cx.notify();
    }

    pub fn add(&mut self, name: &str, tracks: Vec<PathBuf>, cx: &mut Context<Self>) {
        if let Some(list) = self.lists.iter_mut().find(|p| p.name == name) {
            for track in tracks {
                if !list.tracks.contains(&track) {
                    list.tracks.push(track);
                }
            }
            let list = list.clone();
            self.save(&list);
            cx.notify();
        }
    }

    pub fn remove_track(&mut self, name: &str, path: &Path, cx: &mut Context<Self>) {
        if let Some(list) = self.lists.iter_mut().find(|p| p.name == name) {
            list.tracks.retain(|t| t != path);
            let list = list.clone();
            self.save(&list);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m3u_round_trip_and_relative_paths() {
        let base = Path::new("/music/lists");
        let text = "#EXTM3U\n#EXTINF:123,Artist - Title\n/music/a.flac\n\nsub/b.mp3\n";
        let parsed = parse_m3u(text, base);
        assert_eq!(
            parsed,
            vec![
                PathBuf::from("/music/a.flac"),
                PathBuf::from("/music/lists/sub/b.mp3")
            ]
        );
        let written = write_m3u(&parsed);
        assert_eq!(parse_m3u(&written, base), parsed);
    }

    #[test]
    fn names_become_safe_files() {
        assert_eq!(file_name("Sera / notte?"), "Sera _ notte_.m3u8");
    }

    #[test]
    fn favorites_always_exist_and_come_first() {
        let dir = std::env::temp_dir().join(format!("lumifi-pl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Aaa.m3u8"), "#EXTM3U\n/x.flac\n").unwrap();
        let lists = Playlists::load(dir.clone());
        assert_eq!(lists.all()[0].name, FAVORITES);
        assert_eq!(lists.all()[1].tracks, vec![PathBuf::from("/x.flac")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
