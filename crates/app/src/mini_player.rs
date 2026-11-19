//! The mini player, in the two shapes of the approved design: a strip (480×72) and a square
//! (280×280). Sizes come from the right-click menu (75–200%), never from dragging the border,
//! so the system cannot snap or stretch it. It only draws: the music lives in [`Playback`].

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, Bounds, Context, Entity, ExternalPaths, FocusHandle,
    FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ObjectFit,
    ParentElement, Pixels, Point, Rems, Render, Rgba, ScrollWheelEvent, SharedString, Size,
    StatefulInteractiveElement, Styled, StyledImage, Task, Window, div, img, point, prelude::*, px,
    rems, size, svg,
};
use lumifi_engine::format_time;

use crate::actions::*;
use crate::mini_menu;
use crate::platform;
use crate::playback::{Playback, WAVE_BARS, playback};
use crate::queue::Repeat;
use crate::settings::{Dock, MiniStyle, MiniView, Settings, SettingsModel, settings};
use crate::theme::Theme;
use crate::widgets::{SlideEvent, slider};

/// Bars in the strip's waveform: fewer and wider than in the square.
const STRIP_BARS: usize = 26;
/// Height of the square's extra controls (times, buttons, volume) when they are up.
const SQUARE_EXTRA: f32 = 102.0;

/// The design's size of each shape, in points at 100%.
pub fn base_size(style: MiniStyle) -> (f32, f32) {
    match style {
        MiniStyle::Strip => (480.0, 72.0),
        MiniStyle::Square => (280.0, 280.0),
    }
}

pub fn window_size(style: MiniStyle, scale: f32) -> Size<Pixels> {
    let (w, h) = base_size(style);
    let scale = scale.clamp(0.5, 3.0);
    size(px((w * scale).round()), px((h * scale).round()))
}

/// Where a docked mini player goes inside `area` (the screen minus taskbar and menu bar).
pub fn docked_origin(
    area: Bounds<Pixels>,
    window: Size<Pixels>,
    dock: Dock,
) -> Option<Point<Pixels>> {
    let x = area.origin.x + (area.size.width - window.width) / 2.0;
    match dock {
        Dock::Free => None,
        Dock::Top => Some(point(x, area.origin.y + px(8.))),
        Dock::Bottom => Some(point(
            x,
            area.origin.y + area.size.height - window.height - px(8.),
        )),
    }
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

/// The strip's waveform: the analysed peaks averaged into fewer bars, never too flat.
pub fn strip_peaks(waveform: &[f32], bars: usize) -> Vec<f32> {
    (0..bars)
        .map(|i| {
            if waveform.is_empty() {
                return 0.35;
            }
            let from = i * waveform.len() / bars;
            let to = ((i + 1) * waveform.len() / bars)
                .max(from + 1)
                .min(waveform.len());
            let slice = &waveform[from..to];
            let mean = slice.iter().sum::<f32>() / slice.len() as f32;
            0.25 + 0.75 * mean.clamp(0.0, 1.0)
        })
        .collect()
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
    dock: Dock,
    drag: Drag,
    /// The mouse is over the window (the square raises its controls).
    hovered: bool,
    /// The square's controls are up; `epoch` restarts the animation at each change.
    expanded: bool,
    epoch: usize,
    _hide: Option<Task<()>>,
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
                this.apply(&value, window, cx);
                cx.notify();
            }),
            cx.observe_window_bounds(window, |this, window, cx| {
                this.leave_dock_if_moved(window, cx)
            }),
        ];
        let paused = playback.read(cx).is_paused();
        if value.mini_dock != Dock::Free {
            cx.defer_in(window, |this: &mut Self, window, cx| this.place(window, cx));
        }
        Self {
            focus,
            playback,
            settings,
            style: value.mini_style,
            scale: value.mini_scale,
            dock: value.mini_dock,
            drag: Drag::None,
            hovered: false,
            expanded: paused,
            epoch: 0,
            _hide: None,
            _subscriptions: subscriptions,
        }
    }

    /// Follows a change in the settings: shape, size, dock, always on top.
    fn apply(&mut self, value: &Settings, window: &mut Window, cx: &mut Context<Self>) {
        platform::keep_on_top(window, value.always_on_top);
        let resized =
            value.mini_style != self.style || (value.mini_scale - self.scale).abs() > 0.001;
        let redocked = value.mini_dock != self.dock;
        self.style = value.mini_style;
        self.scale = value.mini_scale;
        self.dock = value.mini_dock;
        if resized {
            let target = window_size(self.style, self.scale);
            cx.defer_in(window, move |this, window, cx| {
                window.resize(target);
                this.place(window, cx);
            });
        } else if redocked {
            cx.defer_in(window, |this, window, cx| this.place(window, cx));
        }
    }

    /// Moves the window to its dock, if it has one.
    fn place(&self, window: &mut Window, cx: &App) {
        let size = window_size(self.style, self.scale);
        let area = platform::work_area(window).or_else(|| window.display(cx).map(|d| d.bounds()));
        if let Some(area) = area
            && let Some(origin) = docked_origin(area, size, self.dock)
        {
            platform::move_to(window, origin);
        }
    }

    /// Dragging a docked player away frees it.
    fn leave_dock_if_moved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dock == Dock::Free {
            return;
        }
        let Some(area) = platform::work_area(window) else {
            return;
        };
        let bounds = window.bounds();
        if let Some(origin) = docked_origin(area, bounds.size, self.dock) {
            let dx = (f32::from(bounds.origin.x) - f32::from(origin.x)).abs();
            let dy = (f32::from(bounds.origin.y) - f32::from(origin.y)).abs();
            if dx > 24.0 || dy > 24.0 {
                self.dock = Dock::Free;
                self.settings
                    .update(cx, |s, cx| s.update(cx, |v| v.mini_dock = Dock::Free));
            }
        }
    }

    fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.hovered = hovered;
        if hovered {
            self._hide = None;
            self.set_expanded(true, cx);
            return;
        }
        // A moment before lowering, so brushing the edge does not make it jump.
        self._hide = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let _ = this.update(cx, |this, cx| {
                let paused = this.playback.read(cx).is_paused();
                if !this.hovered && !paused {
                    this.set_expanded(false, cx);
                }
            });
        }));
    }

    fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.expanded != expanded {
            self.expanded = expanded;
            self.epoch += 1;
            cx.notify();
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

    fn open_menu(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let at = window.bounds().origin + event.position;
        let area = platform::work_area(window).or_else(|| window.display(cx).map(|d| d.bounds()));
        cx.defer(move |cx| mini_menu::open(at, area, cx));
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The rem carries the scale: every length below is in design points through `u`.
        window.set_rem_size(px(16.0 * self.scale));
        let theme = Theme::current(window, cx);
        // Paused, the square keeps its controls up.
        if self.playback.read(cx).is_paused() && !self.expanded {
            self.expanded = true;
            self.epoch += 1;
        }
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
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.set_hovered(*hovered, cx)))
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
            .on_mouse_down(MouseButton::Right, cx.listener(Self::open_menu))
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
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
                let strip = bars == STRIP_BARS;
                let peaks: Vec<f32> = if strip {
                    strip_peaks(waveform, bars)
                } else {
                    (0..bars)
                        .map(|i| {
                            if waveform.is_empty() {
                                0.15
                            } else {
                                waveform[i * WAVE_BARS / bars]
                            }
                        })
                        .collect()
                };
                div()
                    .h(u(height))
                    .flex()
                    .items_center()
                    .gap(u(if strip { 2.5 } else { 2. }))
                    .children(peaks.into_iter().enumerate().map(|(i, peak)| {
                        let min = if strip { 4. } else { 3. };
                        div()
                            .flex_1()
                            .h(u(min + peak * (height - min)))
                            .rounded(u(if strip { 2. } else { 1. }))
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
                    .gap(u(3.))
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
                    .child(self.progress_view(STRIP_BARS, 22., theme, cx)),
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
                            .w(u(50.))
                            .mr(u(4.))
                            .ml(u(2.))
                            .child(self.volume_slider(theme, cx)),
                    )
                    .child(self.pin_button(theme, theme.muted, cx))
                    .child(button(
                        "settings",
                        26.,
                        44.,
                        icon("settings", 14., theme.muted),
                        OpenSettings,
                    ))
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
        let expanded = self.expanded;

        // Times, controls and volume: they rise with the mouse and sink a moment after it leaves.
        let extra = div()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap(u(7.))
            .child(self.times(theme, cx))
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
            )
            .with_animation(
                ("square-extra", self.epoch),
                Animation::new(Duration::from_millis(200)).with_easing(gpui::ease_in_out),
                move |el, t| {
                    let t = if expanded { t } else { 1.0 - t };
                    el.h(u(SQUARE_EXTRA * t)).opacity(t)
                },
            );

        div()
            .absolute()
            .inset_0()
            // The cover fills the window; it takes no clicks, so it moves the window too.
            .child(div().absolute().inset_0().child(self.cover(theme, cx)))
            .when(expanded, |d| {
                d.child(
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
                                "settings",
                                28.,
                                28.,
                                icon("settings", 14., white),
                                OpenSettings,
                            )
                            .rounded(u(8.))
                            .bg(chip),
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
            })
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .pt(u(11.))
                    .px(u(14.))
                    .pb(u(12.))
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
                    .child(self.progress_view(WAVE_BARS, 22., theme, cx))
                    .child(extra),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_peaks_average_and_never_go_flat() {
        let peaks = strip_peaks(&[0.0, 1.0, 0.0, 1.0], 2);
        assert_eq!(peaks, vec![0.625, 0.625]);
        assert!(strip_peaks(&[], 26).iter().all(|&p| p > 0.3));
        assert_eq!(strip_peaks(&[0.5; 56], 26).len(), 26);
    }

    #[test]
    fn docks_centre_on_the_work_area() {
        let area = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1040.)));
        let window = size(px(480.), px(72.));
        assert_eq!(
            docked_origin(area, window, Dock::Top),
            Some(point(px(720.), px(8.)))
        );
        assert_eq!(
            docked_origin(area, window, Dock::Bottom),
            Some(point(px(720.), px(960.)))
        );
        assert_eq!(docked_origin(area, window, Dock::Free), None);
    }
}
