//! What is playing, shared by every window: the queue, the player, volume and the state of the
//! controls. The mini player and the library window both observe this one entity, so the music
//! never depends on which window is open.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, AppContext, Context, Entity, Global, Image, ImageFormat, SharedString, Task};
use lumifi_engine::{Artwork, Engine, Loaded, Source};

use crate::queue::{Queue, QueueState, Repeat, collect_audio};
use crate::settings::{GlobalSettings, Paths};

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
    pub album: Option<SharedString>,
    /// Lyrics written in the file's tags, if any.
    pub lyrics: Option<String>,
    /// "Artist · Album", or the folder name when the file has no tags.
    pub subtitle: SharedString,
    pub cover: Option<Arc<Image>>,
    pub duration: Option<Duration>,
    /// "FLAC · 24/96", "MP3 · 44,1 kHz".
    pub quality: SharedString,
}

/// What was playing, saved so the next launch can pick it up again.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Session {
    queue: QueueState,
    position_ms: u64,
}

pub struct Playback {
    queue: Queue,
    engine: Engine,
    /// A track is open in the engine.
    loaded: bool,
    /// The track queued in the engine to follow without a pause.
    gapless_next: Option<PathBuf>,
    /// The engine's `track_index` the app has caught up with.
    heard_index: usize,
    /// "Ascolta senza": the equalizer is skipped for a moment, to compare.
    eq_bypass: bool,
    now: Option<NowPlaying>,
    paused: bool,
    volume: f32,
    muted: bool,
    error: Option<SharedString>,
    /// Position at the last notification, so observers redraw only when the clock moved.
    notified_position: Duration,
    /// Position at the last session save.
    saved_position: Duration,
    _ticker: Task<()>,
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
            engine: Engine::new(),
            loaded: false,
            gapless_next: None,
            heard_index: 0,
            eq_bypass: false,
            now: None,
            paused: true,
            volume: volume.clamp(0.0, 1.0),
            muted: false,
            error: None,
            notified_position: Duration::ZERO,
            saved_position: Duration::ZERO,
            _ticker: ticker,
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

    /// The gain that reaches the speakers: 0 when muted. The slider follows the ear, not the
    /// amplitude: a cube gives about −18 dB at half way and −60 dB near the bottom, so every
    /// step of the slider sounds about as big as the last.
    pub fn effective_volume(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            perceptual_gain(self.volume)
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn repeat(&self) -> Repeat {
        self.queue.repeat
    }

    pub fn is_shuffled(&self) -> bool {
        self.queue.is_shuffled()
    }

    pub fn error(&self) -> Option<SharedString> {
        self.error.clone()
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.now.as_ref().map(|n| n.path.as_path())
    }

    pub fn position(&self) -> Duration {
        if self.loaded {
            self.engine.position()
        } else {
            Duration::ZERO
        }
    }

    /// The output device and the rate the stream runs at, while something is loaded.
    pub fn output_description(&self) -> Option<(String, u32)> {
        self.engine.output_description()
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
        self.apply_audio_settings(cx);
        self.engine.set_volume(self.effective_volume());
        match self.engine.load(path, start, play) {
            Ok(loaded) => {
                let same_track = self.now.as_ref().is_some_and(|n| n.path == path);
                if !same_track {
                    self.now = Some(describe(&loaded));
                }
                self.loaded = true;
                self.paused = !play;
                self.error = None;
                self.gapless_next = None;
                self.heard_index = 0;
                self.queue_gapless();
                self.save_session();
            }
            Err(error) => {
                log::warn!("cannot play {}: {error}", path.display());
                self.error = Some(SharedString::from(format!(
                    "Non riesco a suonare {}",
                    file_name(path)
                )));
                self.engine.clear();
                self.loaded = false;
                self.now = None;
                self.paused = true;
            }
        }
        cx.notify();
    }

    /// Device, ReplayGain and equalizer as the settings say.
    fn apply_audio_settings(&mut self, cx: &mut Context<Self>) {
        let Some(settings) = cx
            .try_global::<GlobalSettings>()
            .map(|s| s.0.read(cx).value.clone())
        else {
            return;
        };
        self.engine.set_device(settings.output_device.clone());
        self.engine.set_replaygain(settings.replaygain.engine());
        let mut eq = settings.eq_profile().engine();
        eq.enabled &= !self.eq_bypass;
        self.engine.set_eq(eq);
    }

    pub fn eq_bypassed(&self) -> bool {
        self.eq_bypass
    }

    pub fn toggle_eq_bypass(&mut self, cx: &mut Context<Self>) {
        self.eq_bypass = !self.eq_bypass;
        self.apply_audio_settings(cx);
        cx.notify();
    }

    /// The equalizer or ReplayGain changed: the equalizer follows at once, ReplayGain from the
    /// next track.
    pub fn settings_changed(&mut self, cx: &mut Context<Self>) {
        self.apply_audio_settings(cx);
    }

    /// Tells the engine which track follows, so it can go on without a gap.
    fn queue_gapless(&mut self) {
        let next = self.queue.peek_after_end().map(Path::to_path_buf);
        if next != self.gapless_next {
            self.engine.set_next(next.clone());
            self.gapless_next = next;
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let opened = OPENED
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default();
        if !opened.is_empty() {
            self.open_paths(opened, cx);
        }
        if !self.loaded {
            return;
        }
        // The engine went on to the next track by itself: follow it.
        let index = self.engine.track_index();
        if index > self.heard_index
            && let Some(heard) = self.engine.current_path()
        {
            self.heard_index = index;
            self.queue.advance_on_end();
            self.gapless_next = None;
            self.now = Source::open(&heard).ok().map(|source| {
                describe(&Loaded {
                    path: heard.clone(),
                    info: source.info().clone(),
                    tags: source.tags().clone(),
                    artwork: source.artwork().cloned(),
                })
            });
            self.queue_gapless();
            self.save_session();
            cx.notify();
            return;
        }
        if self.engine.is_finished() {
            match self.queue.advance_on_end().map(Path::to_path_buf) {
                Some(next) => self.load(&next, Duration::ZERO, true, cx),
                None => {
                    // End of the queue: back to the first track, paused.
                    self.paused = true;
                    if let Some(first) = self.queue.rewind().map(Path::to_path_buf) {
                        self.load(&first, Duration::ZERO, false, cx);
                    } else {
                        self.engine.clear();
                        self.loaded = false;
                        cx.notify();
                    }
                }
            }
            return;
        }
        if self.paused {
            return;
        }
        // The clock and the bars: a few redraws a second are enough.
        let position = self.engine.position();
        if position.abs_diff(self.notified_position) >= Duration::from_millis(250) {
            self.notified_position = position;
            cx.notify();
        }
        if position.abs_diff(self.saved_position) >= Duration::from_secs(10) {
            self.save_session();
        }
    }

    // The queue -------------------------------------------------------------------------

    /// The queue in play order, and the position of the playing track in it.
    pub fn upcoming(&self) -> (Vec<PathBuf>, usize) {
        let (list, position) = self.queue.upcoming();
        (list.into_iter().map(Path::to_path_buf).collect(), position)
    }

    pub fn play_queue_position(&mut self, position: usize, cx: &mut Context<Self>) {
        if let Some(path) = self.queue.jump_to(position).map(Path::to_path_buf) {
            self.load(&path, Duration::ZERO, true, cx);
        }
    }

    pub fn remove_from_queue(&mut self, position: usize, cx: &mut Context<Self>) {
        if self.queue.remove_at(position) {
            self.queue_gapless();
            self.save_session();
            cx.notify();
        }
    }

    pub fn move_in_queue(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.queue.move_item(from, to);
        self.queue_gapless();
        self.save_session();
        cx.notify();
    }

    /// "Riproduci dopo": right after the playing track (or now, if nothing is loaded).
    pub fn play_next(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.queue.is_empty() {
            self.play_list(paths, 0, cx);
            return;
        }
        self.queue.insert_next(paths);
        self.queue_gapless();
        self.save_session();
        cx.notify();
    }

    /// "Aggiungi alla coda": at the end.
    pub fn add_to_queue(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.queue.is_empty() {
            self.play_list(paths, 0, cx);
            return;
        }
        self.queue.append(paths);
        self.queue_gapless();
        self.save_session();
        cx.notify();
    }

    // Sessions ----------------------------------------------------------------------------

    /// Saves the queue and position, so the next launch can resume. Cheap: a small JSON file.
    pub fn save_session(&mut self) {
        if self.now.is_none() {
            return;
        }
        let position = self.position();
        self.saved_position = position;
        let session = Session {
            queue: self.queue.state(),
            position_ms: position.as_millis() as u64,
        };
        let path = Paths::session_file();
        let _ = std::fs::create_dir_all(Paths::data_dir());
        if let Ok(bytes) = serde_json::to_vec(&session)
            && let Err(error) = std::fs::write(&path, bytes)
        {
            log::warn!("cannot save the session: {error}");
        }
    }

    /// Reopens the last track, paused where it was.
    pub fn resume_session(&mut self, cx: &mut Context<Self>) {
        let Some(session) = std::fs::read(Paths::session_file())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Session>(&bytes).ok())
        else {
            return;
        };
        if !self.queue.restore(session.queue) {
            return;
        }
        if let Some(path) = self.queue.current().map(Path::to_path_buf)
            && path.exists()
        {
            self.load(&path, Duration::from_millis(session.position_ms), false, cx);
        }
    }

    /// Reopens the current track on the output device now chosen in the settings.
    pub fn reload_output(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.current_path().map(Path::to_path_buf) {
            let position = self.position();
            let play = !self.paused;
            self.load(&path, position, play, cx);
        }
    }

    // Controls ----------------------------------------------------------------------------

    pub fn toggle_play(&mut self, cx: &mut Context<Self>) {
        if !self.loaded {
            if let Some(path) = self.queue.current().map(Path::to_path_buf) {
                self.load(&path, Duration::ZERO, true, cx);
            }
            return;
        }
        if self.paused {
            self.engine.resume();
        } else {
            self.engine.pause();
        }
        self.paused = !self.paused;
        self.save_session();
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
        self.queue_gapless();
        cx.notify();
    }

    pub fn cycle_repeat(&mut self, cx: &mut Context<Self>) {
        self.queue.repeat = self.queue.repeat.next();
        self.queue_gapless();
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
        self.engine.set_volume(self.effective_volume());
        cx.notify();
    }

    pub fn seek_to(&mut self, position: Duration, cx: &mut Context<Self>) {
        let Some(now) = &self.now else {
            return;
        };
        let end = now.duration.unwrap_or(Duration::MAX);
        let position = position.min(end.saturating_sub(Duration::from_millis(200)));
        if !self.loaded {
            let path = now.path.clone();
            self.load(&path, position, false, cx);
            return;
        }
        match self.engine.seek(position) {
            Ok(_) => {
                self.heard_index = 0;
                self.gapless_next = None;
                self.queue_gapless();
                self.notified_position = position;
            }
            Err(error) => log::warn!("seek failed: {error}"),
        }
        cx.notify();
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

/// Amplitude for a slider position: a cube, so the slider moves evenly to the ear.
pub fn perceptual_gain(volume: f32) -> f32 {
    volume.clamp(0.0, 1.0).powi(3)
}

/// Builds the title, subtitle and cover from the tags, falling back to the file name.
fn describe(loaded: &Loaded) -> NowPlaying {
    let path = loaded.path.as_path();
    let tags = &loaded.tags;
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
        album: tags.album.clone().map(Into::into),
        lyrics: tags.lyrics.clone(),
        subtitle: subtitle.into(),
        cover: loaded.artwork.as_ref().and_then(cover_image),
        duration: loaded.info.duration(),
        quality: quality(&loaded.info).into(),
    }
}

/// A short description of the file's format: "FLAC · 24/96", "MP3 · 44,1 kHz".
fn quality(info: &lumifi_engine::TrackInfo) -> String {
    let codec = info.codec.to_uppercase();
    let khz = info.sample_rate as f32 / 1000.0;
    let rate = if khz.fract() == 0.0 {
        format!("{khz:.0}")
    } else {
        format!("{khz:.1}").replace('.', ",")
    };
    match info.bits_per_sample {
        Some(bits) => format!("{codec} · {bits}/{rate}"),
        None => format!("{codec} · {rate} kHz"),
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
