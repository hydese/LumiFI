//! The music library: tracks from the chosen folders, grouped into albums and artists.

mod db;
pub mod scan;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use gpui::{App, AppContext, Context, Entity, Global, SharedString, Task};

pub use db::TrackRow;
use scan::Progress;

use crate::settings::Paths;

#[derive(Debug, Clone)]
pub struct Track {
    pub path: PathBuf,
    pub title: SharedString,
    pub artist: SharedString,
    pub album: SharedString,
    pub track_no: Option<u32>,
    pub duration: Option<Duration>,
    /// Index into `LibraryData::albums`.
    pub album_index: usize,
}

#[derive(Debug, Clone)]
pub struct Album {
    pub title: SharedString,
    pub artist: SharedString,
    pub cover: Option<PathBuf>,
    /// Indexes into `LibraryData::tracks`, in track order.
    pub tracks: Vec<usize>,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct Artist {
    pub name: SharedString,
    /// Indexes into `LibraryData::albums`.
    pub albums: Vec<usize>,
    pub track_count: usize,
    pub cover: Option<PathBuf>,
}

/// Everything the library window draws, built once per scan.
#[derive(Debug, Default)]
pub struct LibraryData {
    /// Sorted by artist, album and track number.
    pub tracks: Vec<Track>,
    /// Sorted by artist and title.
    pub albums: Vec<Album>,
    /// Sorted by name.
    pub artists: Vec<Artist>,
}

const UNKNOWN_ARTIST: &str = "Artista sconosciuto";

fn sort_key(text: &str) -> String {
    if text == UNKNOWN_ARTIST {
        // After every real name.
        return "\u{10FFFF}".to_owned();
    }
    let lower = text.to_lowercase();
    // "The Beatles" sorts under B, as in most players.
    lower
        .strip_prefix("the ")
        .map(str::to_owned)
        .unwrap_or(lower)
}

impl LibraryData {
    pub fn build(rows: Vec<TrackRow>) -> Self {
        struct Draft {
            title: String,
            artist: String,
            rows: Vec<TrackRow>,
        }
        let mut drafts: HashMap<(String, String), Draft> = HashMap::new();
        for row in rows {
            let artist = row
                .album_artist
                .clone()
                .or_else(|| row.artist.clone())
                .unwrap_or_else(|| UNKNOWN_ARTIST.to_owned());
            // Without an album tag, the folder is the album.
            let album = row.album.clone().unwrap_or_else(|| {
                row.path
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Senza album".to_owned())
            });
            drafts
                .entry((sort_key(&artist), album.to_lowercase()))
                .or_insert_with(|| Draft {
                    title: album,
                    artist,
                    rows: Vec::new(),
                })
                .rows
                .push(row);
        }

        let mut drafts: Vec<_> = drafts.into_iter().collect();
        drafts.sort_by(|a, b| a.0.cmp(&b.0));

        let mut data = LibraryData::default();
        for (_, mut draft) in drafts {
            draft.rows.sort_by(|a, b| {
                (a.track_no.unwrap_or(u32::MAX), &a.path)
                    .cmp(&(b.track_no.unwrap_or(u32::MAX), &b.path))
            });
            let album_index = data.albums.len();
            let mut album = Album {
                title: draft.title.clone().into(),
                artist: draft.artist.clone().into(),
                cover: draft.rows.iter().find_map(|r| r.cover.clone()),
                tracks: Vec::with_capacity(draft.rows.len()),
                duration: Duration::ZERO,
            };
            for row in draft.rows {
                let duration = row.duration_ms.map(Duration::from_millis);
                album.duration += duration.unwrap_or_default();
                album.tracks.push(data.tracks.len());
                let title = row.title.clone().unwrap_or_else(|| {
                    row.path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default()
                });
                data.tracks.push(Track {
                    title: title.into(),
                    artist: row
                        .artist
                        .clone()
                        .unwrap_or_else(|| draft.artist.clone())
                        .into(),
                    album: draft.title.clone().into(),
                    track_no: row.track_no,
                    duration,
                    album_index,
                    path: row.path,
                });
            }
            data.albums.push(album);
        }

        let mut by_artist: HashMap<String, Artist> = HashMap::new();
        for (index, album) in data.albums.iter().enumerate() {
            let entry = by_artist
                .entry(sort_key(&album.artist))
                .or_insert_with(|| Artist {
                    name: album.artist.clone(),
                    albums: Vec::new(),
                    track_count: 0,
                    cover: None,
                });
            entry.albums.push(index);
            entry.track_count += album.tracks.len();
            if entry.cover.is_none() {
                entry.cover = album.cover.clone();
            }
        }
        let mut artists: Vec<_> = by_artist.into_iter().collect();
        artists.sort_by(|a, b| a.0.cmp(&b.0));
        data.artists = artists.into_iter().map(|(_, artist)| artist).collect();
        data
    }

    pub fn album_paths(&self, album: usize) -> Vec<PathBuf> {
        self.albums
            .get(album)
            .map(|a| {
                a.tracks
                    .iter()
                    .map(|&t| self.tracks[t].path.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// The library as an entity: its data and the state of the scan.
pub struct Library {
    data: Arc<LibraryData>,
    scanning: Option<Arc<Progress>>,
    _scan: Option<Task<()>>,
    _progress_ticker: Option<Task<()>>,
}

pub struct GlobalLibrary(pub Entity<Library>);
impl Global for GlobalLibrary {}

pub fn library(cx: &App) -> Entity<Library> {
    cx.global::<GlobalLibrary>().0.clone()
}

fn db_path() -> PathBuf {
    Paths::data_dir().join("library.db")
}

fn covers_dir() -> PathBuf {
    Paths::cache_dir().join("covers")
}

impl Library {
    /// Shows what was found last time at once, then rescans `folders` in the background.
    pub fn install(folders: Vec<PathBuf>, cx: &mut App) -> Entity<Library> {
        let entity = cx.new(|cx| {
            let mut library = Library {
                data: Arc::new(LibraryData::default()),
                scanning: None,
                _scan: None,
                _progress_ticker: None,
            };
            let cached = cx.background_spawn(async move { scan::load_cached(&db_path()) });
            cx.spawn(async move |this, cx| {
                if let Ok(rows) = cached.await {
                    let data = LibraryData::build(rows);
                    let _ = this.update(cx, |this: &mut Library, cx| {
                        // A scan that already finished wins over the cache.
                        if this.data.tracks.is_empty() {
                            this.data = Arc::new(data);
                            cx.notify();
                        }
                    });
                }
            })
            .detach();
            library.rescan(folders, cx);
            library
        });
        cx.set_global(GlobalLibrary(entity.clone()));
        entity
    }

    pub fn data(&self) -> Arc<LibraryData> {
        self.data.clone()
    }

    /// While scanning: files read so far and files found.
    pub fn scan_progress(&self) -> Option<(usize, usize)> {
        self.scanning.as_ref().map(|p| {
            (
                p.read.load(Ordering::Relaxed),
                p.found.load(Ordering::Relaxed),
            )
        })
    }

    pub fn rescan(&mut self, folders: Vec<PathBuf>, cx: &mut Context<Self>) {
        let progress = Arc::new(Progress::default());
        self.scanning = Some(progress.clone());
        let job_progress = progress.clone();
        let job = cx.background_spawn(async move {
            let rows = scan::run(&folders, &db_path(), &covers_dir(), &job_progress)?;
            anyhow::Ok(LibraryData::build(rows))
        });
        self._scan = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(data) => this.data = Arc::new(data),
                    Err(error) => log::warn!("library scan failed: {error}"),
                }
                this.scanning = None;
                this._progress_ticker = None;
                cx.notify();
            });
        }));
        // Redraw the counter while the scan runs.
        self._progress_ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, artist: Option<&str>, album: Option<&str>, no: Option<u32>) -> TrackRow {
        TrackRow {
            path: PathBuf::from(path),
            title: None,
            artist: artist.map(Into::into),
            album: album.map(Into::into),
            album_artist: None,
            track_no: no,
            duration_ms: Some(60_000),
            cover: None,
        }
    }

    #[test]
    fn groups_tracks_into_albums_and_artists() {
        let data = LibraryData::build(vec![
            row(
                "/m/b/2.flac",
                Some("The Beatles"),
                Some("Abbey Road"),
                Some(2),
            ),
            row(
                "/m/b/1.flac",
                Some("The Beatles"),
                Some("Abbey Road"),
                Some(1),
            ),
            row("/m/a/x.flac", Some("ABBA"), Some("Arrival"), None),
            row("/m/Cartella Senza Tag/y.mp3", None, None, None),
        ]);
        assert_eq!(data.albums.len(), 3);
        assert_eq!(data.artists.len(), 3);
        // "The Beatles" sorts under B; the unknown artist comes last.
        let names: Vec<_> = data.artists.iter().map(|a| a.name.to_string()).collect();
        assert_eq!(names, ["ABBA", "The Beatles", "Artista sconosciuto"]);
        let abbey = data
            .albums
            .iter()
            .find(|a| a.title.as_ref() == "Abbey Road")
            .unwrap();
        let first = &data.tracks[abbey.tracks[0]];
        assert!(first.path.ends_with("1.flac"), "track 1 comes first");
        assert_eq!(first.title.as_ref(), "1", "no title tag: the file name");
        assert_eq!(abbey.duration, Duration::from_secs(120));
        let untagged = data
            .albums
            .iter()
            .find(|a| a.title.as_ref() == "Cartella Senza Tag")
            .unwrap();
        assert_eq!(untagged.artist.as_ref(), "Artista sconosciuto");
    }
}
