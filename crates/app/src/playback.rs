//! What is playing, shared by every window: the queue, the player, volume and the state of the
//! controls. The mini player and the library window both observe this one entity, so the music
//! never depends on which window is open.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AppContext, Context, Entity, Global, Image, ImageFormat, SharedString, Task};
use lumifi_engine::{Artwork, Player, analysis};

use crate::queue::{Queue, Repeat, collect_audio};

/// How many peaks the waveform view draws.
pub const WAVE_BARS: usize = 56;

/// Paths handed over by the system ("Open with LumiFi") until playback picks them up.
static OPENED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

pub fn queue_opened_paths(paths: Vec<PathBuf>) {
    if let Ok(mut pending) = OPENED.lock() {
        pending.extend(paths);
    }
}

/// What the windows show about the loaded track.
pub struct NowPlaying {
    pub path: PathBuf,
    pub title: SharedString,
    pub artist: Option<SharedString>,
    /// "Artist · Album", or the folder name when the file has no tags.
    pub subtitle: SharedString,
    pub cover: Option<Arc<Image>>,
    pub duration: Option<Duration>,
}

pub struct Playback {
    queue: Queue,
    player: Option<Player>,
    now: Option<NowPlaying>,
    paused: bool,
    volume: f32,
    muted: bool,
    waveform: Vec<f32>,
    error: Option<SharedString>,
    /// Position at the last notification, so observers redraw only when the clock moved.
    notified_position: Duration,
    _ticker: Task<()>,
    _waveform_job: Option<Task<()>>,
}

/// Lets any part of the app reach the one `Playback`.
pub struct GlobalPlayback(pub Entity<Playback>);
impl Global for GlobalPlayback {}

pub fn playback(cx: &App) -> Entity<Playback> {
    cx.global::<GlobalPlayback>().0.clone()
}

impl Playback {
    pub fn install(volume: f32, cx: &mut App) -> Entity<Playback> {
        let entity = cx.new(|cx| Playback::new(volume, cx));
        cx.set_global(GlobalPlayback(entity.clone()));
        entity
    }

    fn new(volume: f32, cx: &mut Context<Self>) -> Self {
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        });
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9);
        Self {
            queue: Queue::new(seed),
            player: None,
            now: None,
            paused: true,
            volume: volume.clamp(0.0, 1.0),
            muted: false,
            waveform: Vec::new(),
            error: None,
            notified_position: Duration::ZERO,
            _ticker: ticker,
            _waveform_job: None,
        }
    }

    // Reading -----------------------------------------------------------------------------

    pub fn now(&self) -> Option<&NowPlaying> {
        self.now.as_ref()
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// The volume the user set (0 to 1), ignoring mute.
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// The volume that reaches the speakers: 0 when muted.
    pub fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
    }

    pub fn repeat(&self) -> Repeat {
        self.queue.repeat
    }

    pub fn is_shuffled(&self) -> bool {
        self.queue.is_shuffled()
    }

    pub fn waveform(&self) -> &[f32] {
        &self.waveform
    }

    pub fn error(&self) -> Option<SharedString> {
        self.error.clone()
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.now.as_ref().map(|n| n.path.as_path())
    }

    pub fn position(&self) -> Duration {
        self.player
            .as_ref()
            .map_or(Duration::ZERO, |p| p.position())
    }

    /// How far through the track, from 0 to 1.
    pub fn progress(&self) -> f32 {
        match self.now.as_ref().and_then(|n| n.duration) {
            Some(total) if !total.is_zero() => {
                (self.position().as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    // Starting music ----------------------------------------------------------------------

    /// Replaces the queue with `paths` (files or folders) and starts playing.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let tracks = collect_audio(&paths);
        if tracks.is_empty() {
            if !paths.is_empty() {
                self.error = Some("Nessun file audio qui dentro".into());
                cx.notify();
            }
            return;
        }
        self.play_list(tracks, 0, cx);
    }

    /// Plays `tracks` starting from the one at `start`.
    pub fn play_list(&mut self, tracks: Vec<PathBuf>, start: usize, cx: &mut Context<Self>) {
        if tracks.is_empty() {
            return;
        }
        self.queue.replace_at(tracks, start);
        if let Some(path) = self.queue.current().map(Path::to_path_buf) {
            self.load(&path, Duration::ZERO, true, cx);
        }
    }

    /// Opens `path` at `start`. Keeps the player paused when `play` is false.
    fn load(&mut self, path: &Path, start: Duration, play: bool, cx: &mut Context<Self>) {
        log::info!("load {} at {:?} (play: {play})", path.display(), start);
        self.player = None;
        match Player::play_from(path, start, self.effective_volume()) {
            Ok(player) => {
                if !play {
                    player.pause();
                }
                let same_track = self.now.as_ref().is_some_and(|n| n.path == path);
                if !same_track {
                    self.now = Some(describe(path, &player));
                    self.waveform.clear();
                    self.start_waveform(path.to_path_buf(), cx);
                }
                self.paused = !play;
                self.player = Some(player);
                self.error = None;
            }
            Err(error) => {
                log::warn!("cannot play {}: {error}", path.display());
                self.error = Some(SharedString::from(format!(
                    "Non riesco a suonare {}",
                    file_name(path)
                )));
                self.now = None;
                self.paused = true;
            }
        }
        cx.notify();
    }

    fn start_waveform(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let job_path = path.clone();
        let job = cx.background_spawn(async move { analysis::waveform(&job_path, WAVE_BARS) });
        self._waveform_job = Some(cx.spawn(async move |this, cx| {
            if let Ok(peaks) = job.await {
                let _ = this.update(cx, |this, cx| {
                    if this.now.as_ref().is_some_and(|n| n.path == path) {
                        this.waveform = peaks;
                        cx.notify();
                    }
                });
            }
        }));
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let opened = OPENED
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default();
        if !opened.is_empty() {
            self.open_paths(opened, cx);
        }
        let Some(player) = &self.player else {
            return;
        };
        if player.is_finished() {
            match self.queue.advance_on_end().map(Path::to_path_buf) {
                Some(next) => self.load(&next, Duration::ZERO, true, cx),
                None => {
                    // End of the queue: back to the first track, paused.
                    self.player = None;
                    self.paused = true;
                    if let Some(first) = self.queue.rewind().map(Path::to_path_buf) {
                        self.load(&first, Duration::ZERO, false, cx);
                    }
                }
            }
            return;
        }
        if self.paused {
            return;
        }
        // The clock and the bars: a few redraws a second are enough.
        let position = player.position();
        if position.abs_diff(self.notified_position) >= Duration::from_millis(250) {
            self.notified_position = position;
            cx.notify();
        }
    }

    // Controls ----------------------------------------------------------------------------

    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if self.player.is_none() {
            if let Some(path) = self.queue.current().map(Path::to_path_buf) {
                self.load(&path, Duration::ZERO, true, cx);
            }
            return;
        }
        if let Some(player) = &self.player {
            if self.paused {
                player.resume();
            } else {
                player.pause();
            }
            self.paused = !self.paused;
        }
        cx.notify();
    }

    pub fn next(&mut self, cx: &mut Context<Self>) {
        let play = !self.paused;
        if let Some(path) = self.queue.skip_forward().map(Path::to_path_buf) {
            self.load(&path, Duration::ZERO, play, cx);
        }
    }

    pub fn previous(&mut self, cx: &mut Context<Self>) {
        let play = !self.paused;
        // Like every player: past the first seconds, Previous restarts the track.
        if self.position() > Duration::from_secs(3) {
            if let Some(path) = self.queue.current().map(Path::to_path_buf) {
                self.load(&path, Duration::ZERO, play, cx);
            }
            return;
        }
        if let Some(path) = self.queue.skip_back().map(Path::to_path_buf) {
            self.load(&path, Duration::ZERO, play, cx);
        }
    }

    pub fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        let on = !self.queue.is_shuffled();
        self.queue.set_shuffle(on);
        cx.notify();
    }

    pub fn cycle_repeat(&mut self, cx: &mut Context<Self>) {
        self.queue.repeat = self.queue.repeat.next();
        cx.notify();
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        self.apply_volume(cx);
    }

    pub fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        self.volume = volume.clamp(0.0, 1.0);
        self.muted = false;
        self.apply_volume(cx);
    }

    fn apply_volume(&mut self, cx: &mut Context<Self>) {
        if let Some(player) = &self.player {
            player.set_volume(self.effective_volume());
        }
        cx.notify();
    }

    pub fn seek_to(&mut self, position: Duration, cx: &mut Context<Self>) {
        let Some(now) = &self.now else {
            return;
        };
        let end = now.duration.unwrap_or(Duration::MAX);
        let position = position.min(end.saturating_sub(Duration::from_millis(200)));
        let path = now.path.clone();
        let play = !self.paused;
        self.load(&path, position, play, cx);
    }

    /// Seeks to `fraction` (0 to 1) of the track.
    pub fn seek_fraction(&mut self, fraction: f32, cx: &mut Context<Self>) {
        if let Some(total) = self.now.as_ref().and_then(|n| n.duration) {
            self.seek_to(total.mul_f32(fraction.clamp(0.0, 1.0)), cx);
        }
    }

    pub fn seek_by(&mut self, seconds: i64, cx: &mut Context<Self>) {
        let now = self.position();
        let target = if seconds >= 0 {
            now + Duration::from_secs(seconds as u64)
        } else {
            now.saturating_sub(Duration::from_secs(seconds.unsigned_abs()))
        };
        self.seek_to(target, cx);
    }
}

/// Builds the title, subtitle and cover from the tags, falling back to the file name.
fn describe(path: &Path, player: &Player) -> NowPlaying {
    let tags = player.tags();
    let title = tags
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| file_stem(path));
    let subtitle = match (&tags.artist, &tags.album) {
        (Some(artist), Some(album)) => format!("{artist} · {album}"),
        (Some(artist), None) => artist.clone(),
        (None, Some(album)) => album.clone(),
        (None, None) => path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    NowPlaying {
        path: path.to_path_buf(),
        title: title.into(),
        artist: tags.artist.clone().map(Into::into),
        subtitle: subtitle.into(),
        cover: player.artwork().and_then(cover_image),
        duration: player.info().duration(),
    }
}

pub fn cover_image(artwork: &Artwork) -> Option<Arc<Image>> {
    let data = &artwork.data;
    let format = image_format(artwork.media_type.as_deref(), data)?;
    Some(Arc::new(Image::from_bytes(format, data.clone())))
}

/// Recognises an image by its MIME type or, failing that, by its first bytes.
pub fn image_format(media_type: Option<&str>, data: &[u8]) -> Option<ImageFormat> {
    Some(match media_type {
        Some("image/png") => ImageFormat::Png,
        Some("image/jpeg" | "image/jpg") => ImageFormat::Jpeg,
        Some("image/webp") => ImageFormat::Webp,
        Some("image/gif") => ImageFormat::Gif,
        Some("image/bmp") => ImageFormat::Bmp,
        _ if data.starts_with(&[0x89, b'P', b'N', b'G']) => ImageFormat::Png,
        _ if data.starts_with(&[0xFF, 0xD8]) => ImageFormat::Jpeg,
        _ if data.len() > 12 && &data[8..12] == b"WEBP" => ImageFormat::Webp,
        _ => return None,
    })
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
