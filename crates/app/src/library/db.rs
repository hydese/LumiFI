//! The library's memory: one SQLite file with a row per track, so a launch shows the library at
//! once and a rescan only reads files that changed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

/// What the database remembers about a track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRow {
    pub path: PathBuf,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub duration_ms: Option<u64>,
    /// The cover thumbnail in the cache folder, if the track has one.
    pub cover: Option<PathBuf>,
}

/// Size and modification time: when both match, the file has not changed since the last scan.
pub type Fingerprint = (i64, i64);

pub struct Db {
    conn: Connection,
}

const SCHEMA_VERSION: i64 = 1;

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        Self::with_connection(conn)
    }

    #[cfg(test)]
    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS tracks (
                 path TEXT PRIMARY KEY,
                 mtime INTEGER NOT NULL,
                 size INTEGER NOT NULL,
                 title TEXT,
                 artist TEXT,
                 album TEXT,
                 album_artist TEXT,
                 track_no INTEGER,
                 duration_ms INTEGER,
                 cover TEXT
             );",
        )?;
        let version: Option<i64> = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema'", [], |row| {
                row.get(0)
            })
            .ok();
        if version != Some(SCHEMA_VERSION) {
            // An older layout: start over, the next scan refills it.
            conn.execute("DELETE FROM tracks", [])?;
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema', ?1)",
                [SCHEMA_VERSION],
            )?;
        }
        Ok(Self { conn })
    }

    pub fn fingerprints(&self) -> rusqlite::Result<HashMap<PathBuf, Fingerprint>> {
        let mut statement = self.conn.prepare("SELECT path, mtime, size FROM tracks")?;
        let rows = statement.query_map([], |row| {
            Ok((
                PathBuf::from(row.get::<_, String>(0)?),
                (row.get::<_, i64>(1)?, row.get::<_, i64>(2)?),
            ))
        })?;
        rows.collect()
    }

    /// Writes many tracks in one transaction (much faster than one by one).
    pub fn upsert_many(&mut self, rows: &[(TrackRow, Fingerprint)]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut statement = tx.prepare(
                "INSERT OR REPLACE INTO tracks
                 (path, mtime, size, title, artist, album, album_artist, track_no, duration_ms, cover)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for (row, (mtime, size)) in rows {
                statement.execute(params![
                    row.path.to_string_lossy(),
                    mtime,
                    size,
                    row.title,
                    row.artist,
                    row.album,
                    row.album_artist,
                    row.track_no,
                    row.duration_ms.map(|d| d as i64),
                    row.cover.as_ref().map(|c| c.to_string_lossy().into_owned()),
                ])?;
            }
        }
        tx.commit()
    }

    /// Forgets every track whose path is not in `keep`.
    pub fn retain(&mut self, keep: &HashSet<PathBuf>) -> rusqlite::Result<usize> {
        let stale: Vec<PathBuf> = self
            .fingerprints()?
            .into_keys()
            .filter(|path| !keep.contains(path))
            .collect();
        let tx = self.conn.transaction()?;
        for path in &stale {
            tx.execute(
                "DELETE FROM tracks WHERE path = ?1",
                [path.to_string_lossy()],
            )?;
        }
        tx.commit()?;
        Ok(stale.len())
    }

    pub fn load_all(&self) -> rusqlite::Result<Vec<TrackRow>> {
        let mut statement = self.conn.prepare(
            "SELECT path, title, artist, album, album_artist, track_no, duration_ms, cover
             FROM tracks",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(TrackRow {
                path: PathBuf::from(row.get::<_, String>(0)?),
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                album_artist: row.get(4)?,
                track_no: row.get(5)?,
                duration_ms: row.get::<_, Option<i64>>(6)?.map(|d| d.max(0) as u64),
                cover: row.get::<_, Option<String>>(7)?.map(PathBuf::from),
            })
        })?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, title: &str) -> TrackRow {
        TrackRow {
            path: PathBuf::from(path),
            title: Some(title.into()),
            artist: Some("Artista".into()),
            album: Some("Album".into()),
            album_artist: None,
            track_no: Some(1),
            duration_ms: Some(180_000),
            cover: None,
        }
    }

    #[test]
    fn stores_updates_and_forgets_tracks() {
        let mut db = Db::in_memory().unwrap();
        db.upsert_many(&[
            (row("/a.flac", "A"), (1, 10)),
            (row("/b.flac", "B"), (2, 20)),
        ])
        .unwrap();
        assert_eq!(db.load_all().unwrap().len(), 2);
        assert_eq!(db.fingerprints().unwrap()[Path::new("/a.flac")], (1, 10));

        db.upsert_many(&[(row("/a.flac", "A nuova"), (3, 11))])
            .unwrap();
        let all = db.load_all().unwrap();
        let a = all.iter().find(|t| t.path == Path::new("/a.flac")).unwrap();
        assert_eq!(a.title.as_deref(), Some("A nuova"));

        let keep: HashSet<PathBuf> = [PathBuf::from("/a.flac")].into();
        assert_eq!(db.retain(&keep).unwrap(), 1);
        assert_eq!(db.load_all().unwrap().len(), 1);
    }
}
