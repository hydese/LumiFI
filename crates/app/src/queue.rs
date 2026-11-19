//! The play queue: which files, in which order, and what comes next.

use std::path::{Path, PathBuf};

/// File extensions LumiFi plays.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "alac", "m4a", "mp4", "aac", "mp3", "wav", "wave", "aif", "aiff", "aifc", "caf", "ogg",
    "oga", "opus", "mka", "webm",
];

/// True for a file LumiFi can play, judged by its extension.
pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Turns dropped or chosen paths into a playable list: files as given, folders walked
/// recursively and sorted by path, so albums play in track order.
pub fn collect_audio(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut found = Vec::new();
            walk(path, &mut found, 0);
            found.sort_by_key(|p| p.to_string_lossy().to_lowercase());
            out.extend(found);
        } else if is_audio(path) {
            out.push(path.clone());
        }
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    // Deep enough for Artist/Album/Disc layouts, shallow enough to stop symlink loops.
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let hidden = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            walk(&path, out, depth + 1);
        } else if is_audio(&path) {
            out.push(path);
        }
    }
}

/// What happens at the end of the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn next(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}

/// The list being played, its order and the current position.
#[derive(Debug, Default)]
pub struct Queue {
    tracks: Vec<PathBuf>,
    /// Indexes into `tracks` in play order (shuffled or not).
    order: Vec<usize>,
    /// Position in `order`.
    position: usize,
    pub repeat: Repeat,
    shuffle: bool,
    seed: u64,
}

impl Queue {
    pub fn new(seed: u64) -> Self {
        Self {
            seed: seed | 1,
            ..Default::default()
        }
    }

    /// Replaces the queue and starts from its first track.
    #[cfg(test)]
    pub fn replace(&mut self, tracks: Vec<PathBuf>) {
        self.replace_at(tracks, 0);
    }

    /// Replaces the queue and starts from `start` (an index into `tracks`). With shuffle on,
    /// that track plays first and the others follow in random order.
    pub fn replace_at(&mut self, tracks: Vec<PathBuf>, start: usize) {
        self.tracks = tracks;
        let start = start.min(self.tracks.len().saturating_sub(1));
        self.order = (0..self.tracks.len()).collect();
        self.position = start;
        if self.shuffle && !self.tracks.is_empty() {
            self.order.swap(0, start);
            self.position = 0;
            self.shuffle_after(0);
        }
    }

    /// The tracks in play order, and which one is playing.
    #[cfg(test)]
    pub fn upcoming(&self) -> (Vec<&Path>, usize) {
        let list = self
            .order
            .iter()
            .map(|&i| self.tracks[i].as_path())
            .collect();
        (list, self.position)
    }

    pub fn current(&self) -> Option<&Path> {
        self.order
            .get(self.position)
            .map(|&i| self.tracks[i].as_path())
    }

    pub fn is_shuffled(&self) -> bool {
        self.shuffle
    }

    /// Turns shuffle on or off. The current track keeps playing either way.
    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle || self.tracks.is_empty() {
            self.shuffle = on;
            return;
        }
        self.shuffle = on;
        let current = self.order[self.position];
        if on {
            self.shuffle_after(self.position);
        } else {
            self.order = (0..self.tracks.len()).collect();
            self.position = current;
        }
    }

    /// Back to the first track of the play order (after the queue has ended).
    pub fn rewind(&mut self) -> Option<&Path> {
        self.position = 0;
        self.current()
    }

    /// Moves to the next track when the current one ends. `None` means stop.
    pub fn advance_on_end(&mut self) -> Option<&Path> {
        if self.repeat == Repeat::One {
            return self.current();
        }
        self.skip_forward()
    }

    /// The Next button: always moves on, wrapping only with Repeat All.
    pub fn skip_forward(&mut self) -> Option<&Path> {
        if self.tracks.is_empty() {
            return None;
        }
        if self.position + 1 < self.order.len() {
            self.position += 1;
        } else if self.repeat != Repeat::Off {
            self.position = 0;
            if self.shuffle {
                self.shuffle_after(0);
            }
        } else {
            return None;
        }
        self.current()
    }

    /// The Previous button: back one track, or to the last one with Repeat All.
    pub fn skip_back(&mut self) -> Option<&Path> {
        if self.tracks.is_empty() {
            return None;
        }
        if self.position > 0 {
            self.position -= 1;
        } else if self.repeat != Repeat::Off {
            self.position = self.order.len() - 1;
        }
        self.current()
    }

    /// Shuffles everything after `keep` (Fisher–Yates with a small xorshift generator).
    fn shuffle_after(&mut self, keep: usize) {
        let start = (keep + 1).min(self.order.len());
        let rest = &mut self.order[start..];
        for i in (1..rest.len()).rev() {
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 7;
            self.seed ^= self.seed << 17;
            let j = (self.seed % (i as u64 + 1)) as usize;
            rest.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks(n: usize) -> Vec<PathBuf> {
        (0..n)
            .map(|i| PathBuf::from(format!("{i:02}.flac")))
            .collect()
    }

    #[test]
    fn plays_in_order_and_stops_at_the_end() {
        let mut q = Queue::new(7);
        q.replace(tracks(3));
        assert_eq!(q.current(), Some(Path::new("00.flac")));
        assert_eq!(q.advance_on_end(), Some(Path::new("01.flac")));
        assert_eq!(q.advance_on_end(), Some(Path::new("02.flac")));
        assert_eq!(q.advance_on_end(), None);
        assert_eq!(q.rewind(), Some(Path::new("00.flac")));
    }

    #[test]
    fn repeat_all_wraps_and_repeat_one_stays() {
        let mut q = Queue::new(7);
        q.replace(tracks(2));
        q.repeat = Repeat::All;
        q.skip_forward();
        assert_eq!(q.skip_forward(), Some(Path::new("00.flac")));
        q.repeat = Repeat::One;
        assert_eq!(q.advance_on_end(), Some(Path::new("00.flac")));
        // Next still moves on with Repeat One.
        assert_eq!(q.skip_forward(), Some(Path::new("01.flac")));
    }

    #[test]
    fn shuffle_keeps_the_current_track_and_every_track_once() {
        let mut q = Queue::new(42);
        q.replace(tracks(20));
        q.skip_forward();
        q.set_shuffle(true);
        assert_eq!(q.current(), Some(Path::new("01.flac")));
        let mut seen = vec![q.current().unwrap().to_path_buf()];
        while let Some(p) = q.skip_forward() {
            seen.push(p.to_path_buf());
        }
        assert_eq!(
            seen.len(),
            19,
            "the tracks after the current one, each once"
        );
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 19);
        q.set_shuffle(false);
    }

    #[test]
    fn replace_at_starts_from_the_chosen_track() {
        let mut q = Queue::new(3);
        q.replace_at(tracks(5), 3);
        assert_eq!(q.current(), Some(Path::new("03.flac")));
        assert_eq!(q.skip_forward(), Some(Path::new("04.flac")));
        q.set_shuffle(true);
        q.replace_at(tracks(5), 2);
        assert_eq!(q.current(), Some(Path::new("02.flac")));
        let (order, position) = q.upcoming();
        assert_eq!(position, 0);
        assert_eq!(order.len(), 5);
    }

    #[test]
    fn recognises_audio_files() {
        assert!(is_audio(Path::new("a/B.FLAC")));
        assert!(is_audio(Path::new("x.opus")));
        assert!(!is_audio(Path::new("cover.jpg")));
        assert!(!is_audio(Path::new("README")));
    }

    #[test]
    fn collects_folders_sorted_and_skips_other_files() {
        let dir = std::env::temp_dir().join(format!("lumifi-queue-{}", std::process::id()));
        let album = dir.join("Album");
        std::fs::create_dir_all(&album).unwrap();
        for name in ["02 b.flac", "01 a.flac", "cover.jpg", ".hidden.flac"] {
            std::fs::write(album.join(name), b"").unwrap();
        }
        let found = collect_audio(std::slice::from_ref(&dir));
        let names: Vec<_> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["01 a.flac", "02 b.flac"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
