//! The square mini player: cover art filling the window, controls on hover.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, Context, ExternalPaths, FocusHandle, FontWeight, Image, ImageFormat,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement,
    PathPromptOptions, Pixels, Render, ScrollWheelEvent, SharedString, StatefulInteractiveElement,
    Styled, StyledImage, Task, Window, WindowControlArea, div, img, prelude::*, px, svg,
};
use lumifi_engine::analysis::{self, Spectrum};
use lumifi_engine::{Artwork, Player, format_time};

use crate::actions::*;
use crate::platform;
use crate::queue::{Queue, Repeat, collect_audio};
use crate::theme::Theme;

/// The window is a fixed square.
pub const SIZE: f32 = 280.0;
/// Inner padding of the control panel; the bars span the rest of the width.
const PAD: f32 = 14.0;
const WAVE_BARS: usize = 56;
const SPECTRUM_BARS: usize = 28;
const SPECTRUM_FFT: usize = 2048;

/// How the progress is drawn: plain bar, waveform or live spectrum.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Viz {
    A,
    B,
    C,
}

impl Viz {
    fn label(self) -> &'static str {
        match self {
            Viz::A => "A",
            Viz::B => "B",
            Viz::C => "C",
        }
    }
    fn next(self) -> Self {
        match self {
            Viz::A => Viz::B,
            Viz::B => Viz::C,
            Viz::C => Viz::A,
        }
    }
}

/// Paths handed over by the system ("Open with LumiFi") until the mini player takes them.
static OPENED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

pub fn queue_opened_paths(paths: Vec<PathBuf>) {
    if let Ok(mut pending) = OPENED.lock() {
        pending.extend(paths);
    }
}

/// What the window shows about the track that is loaded.
struct NowPlaying {
    path: PathBuf,
    title: SharedString,
    subtitle: SharedString,
    cover: Option<Arc<Image>>,
    duration: Option<Duration>,
    sample_rate: u32,
}

pub struct MiniPlayer {
    focus: FocusHandle,
    queue: Queue,
    player: Option<Player>,
    now: Option<NowPlaying>,
    paused: bool,
    volume: f32,
    muted: bool,
    viz: Viz,
    /// The controls are showing (mouse over the window, paused, or nothing loaded).
    controls: bool,
    /// Hides the controls a moment after the mouse leaves, so edges don't flicker.
    _hide_controls: Option<Task<()>>,
    always_on_top: bool,
    /// Position shown at the last redraw, to redraw only when something visible moved.
    drawn_position: Duration,
    waveform: Vec<f32>,
    spectrum: Spectrum,
    bars: Vec<f32>,
    scope: Vec<f32>,
    error: Option<SharedString>,
    _ticker: Task<()>,
    _waveform_job: Option<Task<()>>,
}

impl MiniPlayer {
    pub fn new(files: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        cx.observe_window_appearance(window, |_, _, cx| cx.notify())
            .detach();

        // One loop drives everything that moves: the clock, the spectrum and the next track.
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
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
        let mut this = Self {
            focus,
            queue: Queue::new(seed),
            player: None,
            now: None,
            paused: true,
            volume: 0.8,
            muted: false,
            viz: Viz::B,
            controls: false,
            _hide_controls: None,
            always_on_top: false,
            drawn_position: Duration::ZERO,
            waveform: Vec::new(),
            spectrum: Spectrum::new(SPECTRUM_FFT, SPECTRUM_BARS),
            bars: vec![0.0; SPECTRUM_BARS],
            scope: Vec::with_capacity(SPECTRUM_FFT),
            error: None,
            _ticker: ticker,
            _waveform_job: None,
        };
        this.open_paths(files, cx);
        this
    }

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
        self.queue.replace(tracks);
        if let Some(path) = self.queue.current().map(Path::to_path_buf) {
            self.load(&path, Duration::ZERO, true, cx);
        }
    }

    fn effective_volume(&self) -> f32 {
        if self.muted { 0.0 } else { self.volume }
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
        if self.paused || !self.controls {
            return;
        }
        if self.viz == Viz::C {
            player.take_scope(&mut self.scope, SPECTRUM_FFT);
            let rate = self.now.as_ref().map_or(44_100, |n| n.sample_rate);
            self.bars = self.spectrum.update(&self.scope, rate).to_vec();
            cx.notify();
            return;
        }
        // Bar and waveform: a few redraws a second are enough.
        let position = player.position();
        if position.abs_diff(self.drawn_position) >= Duration::from_millis(250) {
            self.drawn_position = position;
            cx.notify();
        }
    }

    fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self._hide_controls = None;
            if !self.controls {
                self.controls = true;
                cx.notify();
            }
            return;
        }
        self._hide_controls = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(900))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.controls = false;
                cx.notify();
            });
        }));
    }

    pub fn toggle_on_top(&mut self, _: &ToggleOnTop, window: &mut Window, cx: &mut Context<Self>) {
        self.always_on_top = !self.always_on_top;
        platform::keep_on_top(window, self.always_on_top);
        cx.notify();
    }

    fn position(&self) -> Duration {
        self.player
            .as_ref()
            .map_or(Duration::ZERO, |p| p.position())
    }

    fn progress(&self) -> f32 {
        match self.now.as_ref().and_then(|n| n.duration) {
            Some(total) if !total.is_zero() => {
                (self.position().as_secs_f32() / total.as_secs_f32()).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    // Actions -----------------------------------------------------------------------------

    pub fn toggle_play(&mut self, _: &TogglePlay, _: &mut Window, cx: &mut Context<Self>) {
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

    pub fn next(&mut self, _: &Next, _: &mut Window, cx: &mut Context<Self>) {
        let play = !self.paused;
        if let Some(path) = self.queue.skip_forward().map(Path::to_path_buf) {
            self.load(&path, Duration::ZERO, play, cx);
        }
    }

    pub fn previous(&mut self, _: &Previous, _: &mut Window, cx: &mut Context<Self>) {
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

    pub fn toggle_shuffle(&mut self, _: &ToggleShuffle, _: &mut Window, cx: &mut Context<Self>) {
        let on = !self.queue.is_shuffled();
        self.queue.set_shuffle(on);
        cx.notify();
    }

    pub fn cycle_repeat(&mut self, _: &CycleRepeat, _: &mut Window, cx: &mut Context<Self>) {
        self.queue.repeat = self.queue.repeat.next();
        cx.notify();
    }

    pub fn cycle_view(&mut self, _: &CycleView, _: &mut Window, cx: &mut Context<Self>) {
        self.viz = self.viz.next();
        cx.notify();
    }

    pub fn toggle_mute(&mut self, _: &ToggleMute, _: &mut Window, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        self.apply_volume(cx);
    }

    pub fn volume_up(&mut self, _: &VolumeUp, _: &mut Window, cx: &mut Context<Self>) {
        self.set_volume(self.volume + 0.05, cx);
    }

    pub fn volume_down(&mut self, _: &VolumeDown, _: &mut Window, cx: &mut Context<Self>) {
        self.set_volume(self.volume - 0.05, cx);
    }

    pub fn seek_forward(&mut self, _: &SeekForward, _: &mut Window, cx: &mut Context<Self>) {
        self.seek_to(self.position() + Duration::from_secs(10), cx);
    }

    pub fn seek_back(&mut self, _: &SeekBack, _: &mut Window, cx: &mut Context<Self>) {
        self.seek_to(self.position().saturating_sub(Duration::from_secs(10)), cx);
    }

    pub fn open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Apri".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await {
                let _ = this.update(cx, |this, cx| this.open_paths(paths, cx));
            }
        })
        .detach();
    }

    fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
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

    fn seek_to(&mut self, position: Duration, cx: &mut Context<Self>) {
        let Some(now) = &self.now else {
            return;
        };
        let end = now.duration.unwrap_or(Duration::MAX);
        let position = position.min(end.saturating_sub(Duration::from_millis(200)));
        let path = now.path.clone();
        let play = !self.paused;
        self.load(&path, position, play, cx);
    }

    /// A click on the progress area: `x` is the window coordinate of the click.
    fn seek_at(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(total) = self.now.as_ref().and_then(|n| n.duration) else {
            return;
        };
        let fraction = ((f32::from(x) - PAD) / (SIZE - 2.0 * PAD)).clamp(0.0, 1.0);
        self.seek_to(total.mul_f32(fraction), cx);
    }

    fn volume_at(&mut self, x: Pixels, cx: &mut Context<Self>) {
        // The slider starts after the speaker button and stops before the number.
        let (start, end) = (PAD + 30.0, SIZE - PAD - 30.0);
        self.set_volume((f32::from(x) - start) / (end - start), cx);
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
        subtitle: subtitle.into(),
        cover: player.artwork().and_then(cover_image),
        duration: player.info().duration(),
        sample_rate: player.info().sample_rate,
    }
}

fn cover_image(artwork: &Artwork) -> Option<Arc<Image>> {
    let data = &artwork.data;
    let format = match artwork.media_type.as_deref() {
        Some("image/png") => ImageFormat::Png,
        Some("image/jpeg" | "image/jpg") => ImageFormat::Jpeg,
        Some("image/webp") => ImageFormat::Webp,
        Some("image/gif") => ImageFormat::Gif,
        Some("image/bmp") => ImageFormat::Bmp,
        _ if data.starts_with(&[0x89, b'P', b'N', b'G']) => ImageFormat::Png,
        _ if data.starts_with(&[0xFF, 0xD8]) => ImageFormat::Jpeg,
        _ if data.len() > 12 && &data[8..12] == b"WEBP" => ImageFormat::Webp,
        _ => return None,
    };
    Some(Arc::new(Image::from_bytes(format, data.clone())))
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

// Drawing ----------------------------------------------------------------------------------

fn icon(name: &'static str, size: f32, color: gpui::Rgba) -> impl IntoElement {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .text_color(color)
}

/// A round, borderless icon button that runs `action`.
fn icon_button(
    id: &'static str,
    name: &'static str,
    size: f32,
    box_size: f32,
    color: gpui::Rgba,
    action: impl gpui::Action + Clone,
) -> impl IntoElement {
    div()
        .id(id)
        .size(px(box_size))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .child(icon(name, size, color))
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
}

impl Render for MiniPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window.appearance());
        let empty = self.now.is_none();
        let show_controls = self.controls || empty || self.paused;

        div()
            .id("mini-player")
            .key_context("MiniPlayer")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .rounded_xl()
            .bg(theme.ground)
            .text_color(theme.text)
            .font_family(".SystemUIFont")
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_hovered(*hovered, cx);
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                let dy = f32::from(event.delta.pixel_delta(window.line_height()).y);
                this.set_volume(this.volume + dy / 400.0, cx);
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.open_paths(paths.paths().to_vec(), cx);
            }))
            .on_action(cx.listener(Self::toggle_play))
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::toggle_shuffle))
            .on_action(cx.listener(Self::cycle_repeat))
            .on_action(cx.listener(Self::cycle_view))
            .on_action(cx.listener(Self::toggle_mute))
            .on_action(cx.listener(Self::volume_up))
            .on_action(cx.listener(Self::volume_down))
            .on_action(cx.listener(Self::seek_forward))
            .on_action(cx.listener(Self::seek_back))
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::toggle_on_top))
            .child(self.render_cover(&theme))
            .child(self.render_drag_strip())
            .when(show_controls, |root| {
                root.child(self.render_top_buttons(&theme))
            })
            .when(show_controls && !empty, |root| {
                root.child(self.render_panel(&theme, cx))
            })
            .when_some(self.error.clone(), |root, error| {
                root.child(
                    div()
                        .absolute()
                        .top(px(46.))
                        .left(px(12.))
                        .right(px(12.))
                        .p(px(8.))
                        .rounded_lg()
                        .bg(theme.chip)
                        .text_color(gpui::rgb(0xffffff))
                        .text_size(px(11.5))
                        .child(error),
                )
            })
    }
}

impl MiniPlayer {
    fn render_cover(&self, theme: &Theme) -> AnyElement {
        match self.now.as_ref().and_then(|n| n.cover.clone()) {
            Some(cover) => img(cover)
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => {
                let empty = self.now.is_none();
                // Without a cover the logo sits in the strip above the controls, smaller.
                let logo = if empty { 84. } else { 52. };
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .when(empty, |d| d.justify_center())
                    .when(!empty, |d| d.pt(px(28.)))
                    .gap(px(10.))
                    .child(
                        div()
                            .size(px(logo))
                            .rounded(px(logo * 0.24))
                            .bg(theme.accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("waves", logo, theme.on_accent)),
                    )
                    .when(empty, |d| {
                        d.child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Trascina qui la tua musica"),
                        )
                        .child(
                            div()
                                .id("open-empty")
                                .mt(px(4.))
                                .h(px(32.))
                                .px(px(14.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .rounded_lg()
                                .bg(theme.accent)
                                .text_color(theme.on_accent)
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .child(icon("folder", 14., theme.on_accent))
                                .child("Apri musica…")
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(Open), cx)
                                }),
                        )
                    })
                    .into_any_element()
            }
        }
    }

    /// A strip across the top that moves the window, since it has no title bar.
    fn render_drag_strip(&self) -> impl IntoElement {
        div()
            .id("drag-strip")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(44.))
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, window, _| {
                platform::start_drag(window);
            })
    }

    fn render_top_buttons(&self, theme: &Theme) -> impl IntoElement {
        let chip = |id: &'static str| {
            div()
                .id(id)
                .h(px(28.))
                .min_w(px(28.))
                .px(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .bg(theme.chip)
                .text_color(gpui::rgb(0xffffff))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
        };
        div()
            .absolute()
            .top(px(10.))
            .left(px(10.))
            .right(px(10.))
            .flex()
            .justify_between()
            .child(
                chip("close")
                    .px(px(0.))
                    .child(icon("close", 13., gpui::rgb(0xffffff)))
                    .on_click(|_, _, cx| cx.quit()),
            )
            .child(
                chip("view")
                    .child(format!("Vista {}", self.viz.label()))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(CycleView), cx)),
            )
    }

    fn render_panel(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, subtitle) = match &self.now {
            Some(now) => (now.title.clone(), now.subtitle.clone()),
            None => ("LumiFi".into(), "Nessun brano".into()),
        };
        let position = format_time(self.position());
        let total = self
            .now
            .as_ref()
            .and_then(|n| n.duration)
            .map(format_time)
            .unwrap_or_else(|| "–:––".into());
        let progress = self.progress();
        let shuffle_color = if self.queue.is_shuffled() {
            theme.accent
        } else {
            theme.muted
        };
        let (repeat_icon, repeat_color) = match self.queue.repeat {
            Repeat::Off => ("repeat", theme.muted),
            Repeat::All => ("repeat", theme.accent),
            Repeat::One => ("repeat-one", theme.accent),
        };
        let volume = self.effective_volume();

        div()
            .id("panel")
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .px(px(PAD))
            .pt(px(11.))
            .pb(px(10.))
            .bg(theme.scrim)
            .flex()
            .flex_col()
            .gap(px(7.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.muted)
                            .truncate()
                            .child(subtitle),
                    ),
            )
            .child(
                div()
                    .id("progress")
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.seek_at(event.position.x, cx)
                        }),
                    )
                    .child(self.render_viz(theme, progress))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(10.5))
                            .text_color(theme.muted)
                            .child(position)
                            .child(total),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(icon_button(
                        "shuffle",
                        "shuffle",
                        15.,
                        30.,
                        shuffle_color,
                        ToggleShuffle,
                    ))
                    .child(icon_button(
                        "previous", "previous", 19., 34., theme.text, Previous,
                    ))
                    .child(
                        div()
                            .id("play")
                            .size(px(42.))
                            .rounded_full()
                            .bg(theme.accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(icon(
                                if self.paused { "play" } else { "pause" },
                                17.,
                                theme.on_accent,
                            ))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(TogglePlay), cx)
                            }),
                    )
                    .child(icon_button("next", "next", 19., 34., theme.text, Next))
                    .child(icon_button(
                        "repeat",
                        repeat_icon,
                        15.,
                        30.,
                        repeat_color,
                        CycleRepeat,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon_button(
                        "mute",
                        if volume == 0.0 {
                            "volume-off"
                        } else {
                            "volume"
                        },
                        14.,
                        22.,
                        theme.muted,
                        ToggleMute,
                    ))
                    .child(
                        div()
                            .id("volume")
                            .flex_1()
                            .h(px(14.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    this.volume_at(event.position.x, cx)
                                }),
                            )
                            .child(bar(volume, theme, 4.)),
                    )
                    .child(
                        div()
                            .w(px(22.))
                            .flex()
                            .justify_end()
                            .text_size(px(10.5))
                            .text_color(theme.muted)
                            .child(format!("{}", (volume * 100.0).round() as u32)),
                    ),
            )
    }

    fn render_viz(&self, theme: &Theme, progress: f32) -> AnyElement {
        match self.viz {
            Viz::A => div()
                .h(px(22.))
                .flex()
                .items_center()
                .child(bar(progress, theme, 4.))
                .into_any_element(),
            Viz::B => {
                let played = (progress * WAVE_BARS as f32) as usize;
                let peaks = if self.waveform.is_empty() {
                    vec![0.15; WAVE_BARS]
                } else {
                    self.waveform.clone()
                };
                div()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .children(peaks.into_iter().enumerate().map(|(i, peak)| {
                        div()
                            .flex_1()
                            .h(px(3.0 + peak * 19.0))
                            .rounded(px(1.))
                            .bg(if i < played {
                                theme.accent
                            } else {
                                theme.unplayed
                            })
                    }))
                    .into_any_element()
            }
            Viz::C => div()
                .h(px(22.))
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(div().h(px(16.)).flex().items_end().gap(px(2.)).children(
                    self.bars.iter().map(|level| {
                        div()
                            .flex_1()
                            .h(px(1.0 + level * 15.0))
                            .rounded_t(px(2.))
                            .bg(theme.accent)
                    }),
                ))
                .child(bar(progress, theme, 3.))
                .into_any_element(),
        }
    }
}

/// A thin track with its filled part and a knob.
fn bar(fraction: f32, theme: &Theme, height: f32) -> impl IntoElement {
    let fraction = fraction.clamp(0.0, 1.0);
    div()
        .w_full()
        .h(px(height))
        .rounded_full()
        .bg(theme.track)
        .child(
            div()
                .h_full()
                .w(gpui::relative(fraction))
                .rounded_full()
                .bg(theme.text),
        )
}
