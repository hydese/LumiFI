//! The mini player, in the two shapes of the approved design: a strip (480×72) and a square
//! (280×280). Both are resizable with their proportions locked, and both always show their
//! controls. It only draws: the music lives in [`Playback`], shared with the library window.

use std::time::Duration;

use gpui::{
    AnyElement, Context, Entity, ExternalPaths, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement, Pixels, Rems, Render, Rgba,
    ScrollWheelEvent, SharedString, Size, StatefulInteractiveElement, Styled, StyledImage, Task,
    Window, div, img, prelude::*, px, rems, size, svg,
};
use lumifi_engine::format_time;

use crate::actions::*;
use crate::platform;
use crate::playback::{Playback, WAVE_BARS, playback};
use crate::queue::Repeat;
use crate::settings::{MiniStyle, MiniView, SettingsModel, settings};
use crate::theme::Theme;
use crate::widgets::{SlideEvent, slider};

/// Bars in the strip's waveform.
const STRIP_BARS: usize = 40;

/// The design's size of each shape, in points at scale 1.
pub fn base_size(style: MiniStyle) -> (f32, f32) {
    match style {
        MiniStyle::Strip => (480.0, 72.0),
        MiniStyle::Square => (280.0, 280.0),
    }
}

/// How far each shape can be shrunk or grown.
pub fn scale_range(style: MiniStyle) -> (f32, f32) {
    match style {
        MiniStyle::Strip => (0.8, 1.7),
        MiniStyle::Square => (0.8, 2.2),
    }
}

pub fn window_size(style: MiniStyle, scale: f32) -> Size<Pixels> {
    let (w, h) = base_size(style);
    let (min, max) = scale_range(style);
    let scale = scale.clamp(min, max);
    size(px((w * scale).round()), px((h * scale).round()))
}

/// A length in the design's points. Every size in the mini player goes through here, and the
/// window's rem size carries the scale, so the whole layout grows and shrinks together.
fn u(points: f32) -> Rems {
    rems(points / 16.0)
}

fn icon(name: &str, points: f32, color: Rgba) -> gpui::Svg {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(u(points))
        .flex_none()
        .text_color(color)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Drag {
    None,
    Volume,
    /// Seeking: where the knob is now, from 0 to 1. The jump happens on release.
    Seek(f32),
}

pub struct MiniPlayer {
    focus: FocusHandle,
    playback: Entity<Playback>,
    settings: Entity<SettingsModel>,
    style: MiniStyle,
    scale: f32,
    drag: Drag,
    _save_scale: Option<Task<()>>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl MiniPlayer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let playback = playback(cx);
        let settings = settings(cx);
        let value = settings.read(cx).value.clone();
        platform::keep_on_top(window, value.always_on_top);
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            cx.observe(&playback, |_, _, cx| cx.notify()),
            cx.observe_in(&settings, window, |this, settings, window, cx| {
                let value = settings.read(cx).value.clone();
                platform::keep_on_top(window, value.always_on_top);
                if value.mini_style != this.style {
                    this.style = value.mini_style;
                    let target = window_size(this.style, this.scale);
                    cx.defer_in(window, move |_, window, _| window.resize(target));
                }
                cx.notify();
            }),
            cx.observe_window_bounds(window, |this, window, cx| this.keep_proportions(window, cx)),
        ];
        let (min, max) = scale_range(value.mini_style);
        Self {
            focus,
            playback,
            settings,
            style: value.mini_style,
            scale: value.mini_scale.clamp(min, max),
            drag: Drag::None,
            _save_scale: None,
            _subscriptions: subscriptions,
        }
    }

    /// After the user resizes the window: follow the side they dragged and fix the other, so the
    /// shape keeps its proportions.
    fn keep_proportions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (base_w, base_h) = base_size(self.style);
        let (min, max) = scale_range(self.style);
        let current = window.bounds().size;
        let (w, h) = (f32::from(current.width), f32::from(current.height));
        let by_width = w / base_w;
        let by_height = h / base_h;
        let scale = if (by_width - self.scale).abs() >= (by_height - self.scale).abs() {
            by_width
        } else {
            by_height
        }
        .clamp(min, max);
        let target = window_size(self.style, scale);
        if (f32::from(target.width) - w).abs() > 1.5 || (f32::from(target.height) - h).abs() > 1.5 {
            // Not from inside the resize callback: the platform is still busy with that one.
            cx.defer_in(window, move |_, window, _| window.resize(target));
        }
        if (scale - self.scale).abs() > 0.004 {
            self.scale = scale;
            cx.notify();
            // Remember the size, once the user has stopped dragging.
            self._save_scale = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(600))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let scale = this.scale;
                    this.settings
                        .update(cx, |s, cx| s.update(cx, |v| v.mini_scale = scale));
                });
            }));
        }
    }

    fn progress(&self, cx: &Context<Self>) -> f32 {
        if let Drag::Seek(fraction) = self.drag {
            return fraction;
        }
        self.playback.read(cx).progress()
    }

    fn slide_volume(&mut self, event: SlideEvent, cx: &mut Context<Self>) {
        let (SlideEvent::Move(at) | SlideEvent::End(at)) = event;
        self.drag = match event {
            SlideEvent::Move(_) => Drag::Volume,
            SlideEvent::End(_) => Drag::None,
        };
        self.playback.update(cx, |p, cx| p.set_volume(at, cx));
        if matches!(event, SlideEvent::End(_)) {
            save_volume(cx);
        }
        cx.notify();
    }

    fn slide_seek(&mut self, event: SlideEvent, cx: &mut Context<Self>) {
        if self
            .playback
            .read(cx)
            .now()
            .and_then(|n| n.duration)
            .is_none()
        {
            return;
        }
        match event {
            SlideEvent::Move(at) => self.drag = Drag::Seek(at),
            SlideEvent::End(at) => {
                self.drag = Drag::None;
                self.playback.update(cx, |p, cx| p.seek_fraction(at, cx));
            }
        }
        cx.notify();
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The rem carries the scale: every length below is in design points through `u`.
        window.set_rem_size(px(16.0 * self.scale));
        let theme = Theme::current(window, cx);
        let content = match self.style {
            MiniStyle::Strip => self.render_strip(&theme, cx),
            MiniStyle::Square => self.render_square(&theme, cx),
        };
        let radius = if self.style == MiniStyle::Strip {
            14.
        } else {
            16.
        };
        div()
            .id("mini-player")
            .key_context(PLAYER_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            // Rounded corners only where the window can be transparent (macOS); elsewhere they
            // would leave dark corners.
            .when(cfg!(target_os = "macos"), |root| root.rounded(u(radius)))
            .bg(match self.style {
                MiniStyle::Strip => theme.surface,
                MiniStyle::Square => theme.ground,
            })
            .text_color(theme.text)
            .font_family(".SystemUIFont")
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                let dy = f32::from(event.delta.pixel_delta(window.line_height()).y);
                this.playback
                    .update(cx, |p, cx| p.set_volume(p.volume() + dy / 400.0, cx));
                save_volume(cx);
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                let paths = paths.paths().to_vec();
                this.playback.update(cx, |p, cx| p.open_paths(paths, cx));
            }))
            // Behind everything: pressing on any empty spot moves the window. Buttons and sliders
            // sit above it and keep their clicks.
            .child(div().id("drag-layer").absolute().inset_0().on_mouse_down(
                MouseButton::Left,
                |_: &MouseDownEvent, window, _| {
                    platform::start_drag(window);
                },
            ))
            .child(content)
    }
}

/// A clickable icon that keeps its click (the window does not start moving under it).
fn button(
    id: &'static str,
    width: f32,
    height: f32,
    child: impl IntoElement,
    action: impl gpui::Action + Clone,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(u(width))
        .h(u(height))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .block_mouse_except_scroll()
        .child(child)
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
}

impl MiniPlayer {
    fn cover(&self, theme: &Theme, cx: &Context<Self>) -> AnyElement {
        match self.playback.read(cx).now().and_then(|n| n.cover.clone()) {
            Some(cover) => img(cover)
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.accent)
                .child(
                    icon("waves", 0., theme.on_accent)
                        .w(gpui::relative(0.7))
                        .h(gpui::relative(0.7)),
                )
                .into_any_element(),
        }
    }

    fn titles(&self, cx: &Context<Self>) -> (SharedString, SharedString) {
        match self.playback.read(cx).now() {
            Some(now) => (
                now.title.clone(),
                now.artist.clone().unwrap_or_else(|| now.subtitle.clone()),
            ),
            None => (
                "Nessun brano".into(),
                "Scegli qualcosa nella libreria".into(),
            ),
        }
    }

    fn play_button(
        &self,
        size: f32,
        icon_size: f32,
        theme: &Theme,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let paused = self.playback.read(cx).is_paused();
        button(
            "play",
            size,
            size,
            icon(
                if paused { "play" } else { "pause" },
                icon_size,
                theme.on_accent,
            ),
            TogglePlay,
        )
        .rounded_full()
        .bg(theme.accent)
    }

    /// The volume slider of the design: a 4-point track with a round knob.
    fn volume_slider(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let volume = self.playback.read(cx).effective_volume();
        let this = cx.entity().downgrade();
        slider(
            "volume",
            self.drag == Drag::Volume,
            div()
                .w_full()
                .h(u(20.))
                .flex()
                .items_center()
                .child(knob_track(volume, theme.track, theme.text)),
            move |event, _, cx| {
                let _ = this.update(cx, |this, cx| this.slide_volume(event, cx));
            },
        )
    }

    fn mute_button(&self, points: f32, theme: &Theme, cx: &Context<Self>) -> impl IntoElement {
        let muted = self.playback.read(cx).effective_volume() == 0.0;
        button(
            "mute",
            points + 8.,
            points + 8.,
            icon(
                if muted { "volume-off" } else { "volume" },
                points,
                theme.muted,
            ),
            ToggleMute,
        )
    }

    fn pin_button(&self, theme: &Theme, color_off: Rgba, cx: &Context<Self>) -> impl IntoElement {
        let on = self.settings.read(cx).value.always_on_top;
        button(
            "pin",
            28.,
            28.,
            icon("pin", 14., if on { theme.accent } else { color_off }),
            ToggleOnTop,
        )
    }

    /// Progress as the design draws it: the waveform (B) or a plain bar (A). Drag to seek.
    fn progress_view(
        &self,
        bars: usize,
        height: f32,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let progress = self.progress(cx);
        let view = self.settings.read(cx).value.mini_view;
        let content = match view {
            MiniView::A => div()
                .h(u(height))
                .flex()
                .items_center()
                .child(plain_track(progress, theme.track, theme.text))
                .into_any_element(),
            MiniView::B => {
                let waveform = self.playback.read(cx).waveform();
                let played = (progress * bars as f32) as usize;
                div()
                    .h(u(height))
                    .flex()
                    .items_center()
                    .gap(u(if bars > 48 { 2. } else { 1.5 }))
                    .children((0..bars).map(|i| {
                        let peak = if waveform.is_empty() {
                            0.15
                        } else {
                            waveform[i * WAVE_BARS / bars]
                        };
                        div()
                            .flex_1()
                            .h(u(3.0 + peak * (height - 3.0)))
                            .rounded(u(1.))
                            .bg(if i < played {
                                theme.accent
                            } else {
                                theme.unplayed
                            })
                    }))
                    .into_any_element()
            }
        };
        let this = cx.entity().downgrade();
        slider(
            "progress",
            matches!(self.drag, Drag::Seek(_)),
            content,
            move |event, _, cx| {
                let _ = this.update(cx, |this, cx| this.slide_seek(event, cx));
            },
        )
    }

    fn times(&self, theme: &Theme, cx: &Context<Self>) -> impl IntoElement {
        let playback = self.playback.read(cx);
        let total = playback.now().and_then(|n| n.duration);
        let position = match (self.drag, total) {
            (Drag::Seek(at), Some(total)) => total.mul_f32(at),
            _ => playback.position(),
        };
        div()
            .flex()
            .justify_between()
            .text_size(u(10.5))
            .text_color(theme.muted)
            .child(format_time(position))
            .child(total.map(format_time).unwrap_or_else(|| "–:––".into()))
    }

    // The strip -------------------------------------------------------------------------------

    fn render_strip(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let (title, artist) = self.titles(cx);
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .when(cfg!(target_os = "macos"), |d| {
                d.border_1().border_color(theme.hairline).rounded(u(14.))
            })
            .child(
                div()
                    .h_full()
                    .w(u(70.))
                    .flex_none()
                    .overflow_hidden()
                    .child(self.cover(theme, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(u(12.))
                    .pr(u(10.))
                    .flex()
                    .flex_col()
                    .gap(u(5.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(u(6.))
                            .min_w_0()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .max_w(gpui::relative(0.7))
                                    .text_size(u(12.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(u(11.))
                                    .text_color(theme.muted)
                                    .truncate()
                                    .child(artist),
                            ),
                    )
                    .child(self.progress_view(STRIP_BARS, 14., theme, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(2.))
                    .pr(u(8.))
                    .child(button(
                        "previous",
                        30.,
                        44.,
                        icon("previous", 16., theme.text),
                        Previous,
                    ))
                    .child(self.play_button(36., 15., theme, cx))
                    .child(button(
                        "next",
                        30.,
                        44.,
                        icon("next", 16., theme.text),
                        Next,
                    ))
                    .child(div().w(u(1.)).h(u(24.)).mx(u(6.)).bg(theme.hairline))
                    .child(self.mute_button(14., theme, cx))
                    .child(
                        div()
                            .w(u(56.))
                            .mr(u(6.))
                            .ml(u(2.))
                            .child(self.volume_slider(theme, cx)),
                    )
                    .child(self.pin_button(theme, theme.muted, cx))
                    .child(button(
                        "library",
                        28.,
                        44.,
                        icon("expand", 14., theme.muted),
                        ToggleLibrary,
                    )),
            )
            .into_any_element()
    }

    // The square ------------------------------------------------------------------------------

    fn render_square(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let (title, artist) = self.titles(cx);
        let playback = self.playback.read(cx);
        let subtitle = match playback.now() {
            Some(now) => now.subtitle.clone(),
            None => artist,
        };
        let shuffle = if playback.is_shuffled() {
            theme.accent
        } else {
            theme.muted
        };
        let (repeat_icon, repeat_color) = match playback.repeat() {
            Repeat::Off => ("repeat", theme.muted),
            Repeat::All => ("repeat", theme.accent),
            Repeat::One => ("repeat-one", theme.accent),
        };
        let volume = (playback.effective_volume() * 100.0).round() as u32;
        let chip = gpui::rgba(0x080809a8);
        let white = gpui::rgb(0xffffff);

        div()
            .absolute()
            .inset_0()
            // The cover fills the window; it takes no clicks, so it moves the window too.
            .child(div().absolute().inset_0().child(self.cover(theme, cx)))
            .child(
                div()
                    .absolute()
                    .top(u(10.))
                    .right(u(10.))
                    .flex()
                    .gap(u(6.))
                    .child(
                        div()
                            .rounded(u(8.))
                            .bg(chip)
                            .child(self.pin_button(theme, white, cx)),
                    )
                    .child(
                        button(
                            "library",
                            28.,
                            28.,
                            icon("expand", 14., white),
                            ToggleLibrary,
                        )
                        .rounded(u(8.))
                        .bg(chip),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .pt(u(11.))
                    .px(u(14.))
                    .pb(u(10.))
                    .bg(theme.scrim)
                    .flex()
                    .flex_col()
                    .gap(u(7.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(u(1.))
                            .child(
                                div()
                                    .text_size(u(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(u(11.5))
                                    .text_color(theme.muted)
                                    .truncate()
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(u(3.))
                            .child(self.progress_view(WAVE_BARS, 22., theme, cx))
                            .child(self.times(theme, cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(button(
                                "shuffle",
                                30.,
                                32.,
                                icon("shuffle", 15., shuffle),
                                ToggleShuffle,
                            ))
                            .child(button(
                                "previous",
                                34.,
                                36.,
                                icon("previous", 19., theme.text),
                                Previous,
                            ))
                            .child(self.play_button(42., 17., theme, cx))
                            .child(button(
                                "next",
                                34.,
                                36.,
                                icon("next", 19., theme.text),
                                Next,
                            ))
                            .child(button(
                                "repeat",
                                30.,
                                32.,
                                icon(repeat_icon, 15., repeat_color),
                                CycleRepeat,
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(8.))
                            .text_color(theme.muted)
                            .child(self.mute_button(14., theme, cx))
                            .child(div().flex_1().child(self.volume_slider(theme, cx)))
                            .child(
                                div()
                                    .w(u(22.))
                                    .flex()
                                    .justify_end()
                                    .text_size(u(10.5))
                                    .child(volume.to_string()),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// A 4-point bar with a 12-point round knob at `fraction`.
fn knob_track(fraction: f32, track: Rgba, fill: Rgba) -> impl IntoElement {
    let fraction = fraction.clamp(0.0, 1.0);
    div()
        .w_full()
        .h(u(4.))
        .rounded(u(2.))
        .bg(track)
        .relative()
        .child(
            div()
                .h_full()
                .w(gpui::relative(fraction))
                .rounded(u(2.))
                .bg(fill),
        )
        .child(
            div()
                .absolute()
                .top(u(-4.))
                .left(gpui::relative(fraction))
                .ml(u(-6.))
                .size(u(12.))
                .rounded_full()
                .bg(fill),
        )
}

fn plain_track(fraction: f32, track: Rgba, fill: Rgba) -> impl IntoElement {
    div().w_full().h(u(4.)).rounded(u(2.)).bg(track).child(
        div()
            .h_full()
            .w(gpui::relative(fraction.clamp(0.0, 1.0)))
            .rounded(u(2.))
            .bg(fill),
    )
}
