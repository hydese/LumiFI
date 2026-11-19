//! Reads the music folders: tags, length and covers of every new or changed file.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

use lumifi_engine::Source;

use super::db::{Db, Fingerprint, TrackRow};
use crate::queue::collect_audio;

/// Longest side of a cover thumbnail, in pixels: sharp in the grid and on Retina screens.
const THUMB_SIZE: u32 = 320;
/// Names of the pictures that often sit beside the tracks of an album.
const FOLDER_ART: &[&str] = &["cover", "folder", "front", "albumart", "album"];

/// Live counters the window shows while a scan runs.
#[derive(Default)]
pub struct Progress {
    pub found: AtomicUsize,
    pub read: AtomicUsize,
}

/// Brings the database up to date with `folders` and returns every track in it.
pub fn run(
    folders: &[PathBuf],
    db_path: &Path,
    covers_dir: &Path,
    progress: &Arc<Progress>,
) -> anyhow::Result<Vec<TrackRow>> {
    let mut db = Db::open(db_path)?;
    let known = db.fingerprints()?;
    let files = collect_audio(folders);
    progress.found.store(files.len(), Ordering::Relaxed);
    let _ = std::fs::create_dir_all(covers_dir);

    let mut seen = HashSet::with_capacity(files.len());
    let mut batch: Vec<(TrackRow, Fingerprint)> = Vec::new();
    let mut folder_art: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    for path in files {
        let Some(fingerprint) = fingerprint(&path) else {
            continue;
        };
        seen.insert(path.clone());
        if known.get(&path) == Some(&fingerprint) {
            progress.read.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        match read_track(&path, covers_dir, &mut folder_art) {
            Some(row) => batch.push((row, fingerprint)),
            None => log::info!("skipping {}: not playable", path.display()),
        }
        progress.read.fetch_add(1, Ordering::Relaxed);
        if batch.len() >= 200 {
            db.upsert_many(&batch)?;
            batch.clear();
        }
    }
    db.upsert_many(&batch)?;
    db.retain(&seen)?;
    Ok(db.load_all()?)
}

/// Only reads what is already in the database (for a fast start).
pub fn load_cached(db_path: &Path) -> anyhow::Result<Vec<TrackRow>> {
    Ok(Db::open(db_path)?.load_all()?)
}

fn fingerprint(path: &Path) -> Option<Fingerprint> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    Some((mtime, meta.len() as i64))
}

fn read_track(
    path: &Path,
    covers_dir: &Path,
    folder_art: &mut HashMap<PathBuf, Option<PathBuf>>,
) -> Option<TrackRow> {
    let source = Source::open(path).ok()?;
    let tags = source.tags();
    let clean = |value: &Option<String>| value.clone().filter(|v| !v.trim().is_empty());
    let cover = source
        .artwork()
        .and_then(|art| thumbnail(&art.data, covers_dir))
        .or_else(|| {
            let dir = path.parent()?.to_path_buf();
            folder_art
                .entry(dir.clone())
                .or_insert_with(|| folder_cover(&dir, covers_dir))
                .clone()
        });
    Some(TrackRow {
        path: path.to_path_buf(),
        title: clean(&tags.title),
        artist: clean(&tags.artist),
        album: clean(&tags.album),
        album_artist: clean(&tags.album_artist),
        track_no: tags.track_number.and_then(|n| u32::try_from(n).ok()),
        duration_ms: source.info().duration().map(|d| d.as_millis() as u64),
        cover,
    })
}

/// A picture such as `cover.jpg` next to the tracks, made into a thumbnail.
fn folder_cover(dir: &Path, covers_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let picture = entries.flatten().map(|e| e.path()).find(|p| {
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp")
            && FOLDER_ART.iter().any(|name| stem.starts_with(name))
    })?;
    let bytes = std::fs::read(picture).ok()?;
    thumbnail(&bytes, covers_dir)
}

/// Shrinks a cover and saves it once in the cache, named after its content, so the same cover
/// shared by a whole album is stored a single time.
fn thumbnail(bytes: &[u8], covers_dir: &Path) -> Option<PathBuf> {
    let out = covers_dir.join(format!("{:016x}.jpg", fnv1a(bytes)));
    if out.exists() {
        return Some(out);
    }
    let image = image::load_from_memory(bytes).ok()?;
    let small = image.thumbnail(THUMB_SIZE, THUMB_SIZE).to_rgb8();
    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut encoded), 85)
        .encode_image(&small)
        .ok()?;
    std::fs::write(&out, encoded).ok()?;
    Some(out)
}

/// A small, stable hash (FNV-1a), good enough to name cache files.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lumifi-scan-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_wav(path: &Path, seconds: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..8_000 * seconds {
            writer.write_sample((i % 100) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn scans_new_files_then_only_changes() {
        let root = temp_dir("lib");
        let album = root.join("Artista").join("Album");
        std::fs::create_dir_all(&album).unwrap();
        write_wav(&album.join("01 Uno.wav"), 1);
        write_wav(&album.join("02 Due.wav"), 2);
        // A folder cover, written as a tiny PNG.
        let png = image::RgbImage::from_pixel(4, 4, image::Rgb([240, 97, 61]));
        png.save(album.join("cover.png")).unwrap();

        let db = root.join("library.db");
        let covers = root.join("covers");
        let progress = Arc::new(Progress::default());
        let tracks = run(std::slice::from_ref(&root), &db, &covers, &progress).unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(progress.found.load(Ordering::Relaxed), 2);
        let two = tracks
            .iter()
            .find(|t| t.path.ends_with("02 Due.wav"))
            .unwrap();
        assert_eq!(two.duration_ms, Some(2_000));
        assert!(
            two.cover.as_ref().is_some_and(|c| c.exists()),
            "folder cover thumbnail"
        );

        std::fs::remove_file(album.join("01 Uno.wav")).unwrap();
        let tracks = run(std::slice::from_ref(&root), &db, &covers, &progress).unwrap();
        assert_eq!(tracks.len(), 1, "deleted files leave the library");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
