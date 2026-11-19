//! The square mini player: cover art filling the window, controls on hover. It only draws: the
//! music lives in [`Playback`], shared with the library window.

use std::time::Duration;

use gpui::{
    AnyElement, Context, Entity, ExternalPaths, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement, Render, ScrollWheelEvent,
    StatefulInteractiveElement, Styled, StyledImage, Task, Window, WindowControlArea, div, img,
    prelude::*, px,
};
use lumifi_engine::format_time;

use crate::actions::*;
use crate::platform;
use crate::playback::{Playback, WAVE_BARS, playback};
use crate::queue::Repeat;
use crate::settings::{MiniView, SettingsModel, settings};
use crate::theme::Theme;
use crate::widgets::{SlideEvent, bar, icon, icon_button, slider};

/// The window is a fixed square.
pub const SIZE: f32 = 280.0;
/// Inner padding of the control panel; the bars span the rest of the width.
const PAD: f32 = 14.0;
/// Where the window drag strip starts: after the close button and the volume pill.
const DRAG_STRIP_LEFT: f32 = 10.0 + 28.0 + 6.0 + 8.0 + 14.0 + 6.0 + 56.0 + 10.0 + 8.0;

/// A slider being dragged with the mouse.
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
    /// The controls are showing (mouse over the window, paused, or nothing loaded).
    controls: bool,
    /// Hides the controls a moment after the mouse leaves, so edges don't flicker.
    _hide_controls: Option<Task<()>>,
    drag: Drag,
    _subscriptions: Vec<gpui::Subscription>,
}

impl MiniPlayer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let playback = playback(cx);
        let settings = settings(cx);
        let on_top = settings.read(cx).value.always_on_top;
        platform::keep_on_top(window, on_top);
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            // Redraw with the music; at rest only the thin line moves, a few times a second.
            cx.observe(&playback, |_, _, cx| cx.notify()),
            cx.observe_in(&settings, window, |_, settings, window, cx| {
                platform::keep_on_top(window, settings.read(cx).value.always_on_top);
                cx.notify();
            }),
        ];
        Self {
            focus,
            playback,
            settings,
            controls: false,
            _hide_controls: None,
            drag: Drag::None,
            _subscriptions: subscriptions,
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

    pub fn toggle_on_top(&mut self, _: &ToggleOnTop, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.update(cx, |s, cx| {
            s.update(cx, |v| v.always_on_top = !v.always_on_top)
        });
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
        let theme = Theme::for_appearance(window.appearance());
        let playback = self.playback.read(cx);
        let empty = playback.now().is_none();
        let show_controls = self.controls || empty || playback.is_paused();
        let error = playback.error();
        let progress = self.progress(cx);

        div()
            .id("mini-player")
            .key_context(PLAYER_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            // Rounded corners only where the window can be transparent (macOS); elsewhere they
            // would leave dark corners.
            .when(cfg!(target_os = "macos"), |root| root.rounded_xl())
            .bg(theme.ground)
            .text_color(theme.text)
            .font_family(".SystemUIFont")
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_hovered(*hovered, cx);
            }))
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
            .on_action(cx.listener(Self::toggle_on_top))
            .child(self.render_cover(&theme, show_controls, cx))
            // At rest, a thin line along the bottom still shows how far the track has got.
            .when(!show_controls && !empty, |root| {
                root.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(3.))
                        .bg(theme.track)
                        .child(div().h_full().w(gpui::relative(progress)).bg(theme.accent)),
                )
            })
            .child(self.render_drag_strip())
            .when(show_controls, |root| {
                root.child(self.render_top_buttons(&theme, cx))
            })
            .when(show_controls && !empty, |root| {
                root.child(self.render_panel(&theme, cx))
            })
            .when_some(error, |root, error| {
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
    fn render_cover(&self, theme: &Theme, controls: bool, cx: &Context<Self>) -> AnyElement {
        let now = self.playback.read(cx).now();
        match now.and_then(|n| n.cover.clone()) {
            Some(cover) => img(cover)
                .absolute()
                .inset_0()
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None => {
                let empty = now.is_none();
                // With the controls showing, the logo shrinks into the strip above them; at rest
                // it is centred with the title and artist under it.
                let compact = controls && !empty;
                let resting = !controls && !empty;
                let logo = if compact { 52. } else { 84. };
                let (title, subtitle) = now
                    .map(|n| (n.title.clone(), n.subtitle.clone()))
                    .unwrap_or_default();
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .when(!compact, |d| d.justify_center())
                    .when(compact, |d| d.pt(px(60.)))
                    .px(px(PAD))
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
                    .when(resting, |d| {
                        d.child(
                            div()
                                .mt(px(6.))
                                .w_full()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .max_w_full()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .truncate()
                                        .child(title),
                                )
                                .child(
                                    div()
                                        .max_w_full()
                                        .text_size(px(12.))
                                        .text_color(theme.muted)
                                        .truncate()
                                        .child(subtitle),
                                ),
                        )
                    })
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
                        .child(
                            div()
                                .id("open-library-empty")
                                .text_size(px(11.5))
                                .text_color(theme.muted)
                                .cursor_pointer()
                                .child("oppure apri la libreria")
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(ToggleLibrary), cx)
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
            // Starts after the close button and the volume pill, and stops before the library
            // button: on Windows a drag area wins over anything drawn on top of it.
            .left(px(DRAG_STRIP_LEFT))
            .right(px(44.))
            .h(px(44.))
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, window, _| {
                platform::start_drag(window);
            })
    }

    fn render_top_buttons(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let chip = |id: &'static str| {
            div()
                .id(id)
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .bg(theme.chip)
                .cursor_pointer()
        };
        let has_track = self.playback.read(cx).now().is_some();
        div()
            .absolute()
            .top(px(10.))
            .left(px(10.))
            .right(px(10.))
            .flex()
            .justify_between()
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        chip("close")
                            .child(icon("close", 13., gpui::rgb(0xffffff)))
                            .on_click(|_, _, cx| cx.quit()),
                    )
                    .when(has_track, |row| row.child(self.render_volume(cx))),
            )
            .child(
                chip("library")
                    .child(icon("expand", 13., gpui::rgb(0xffffff)))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleLibrary), cx)),
            )
    }

    /// A small pill: speaker (click to mute) and a short slider. The wheel works anywhere.
    fn render_volume(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let volume = self.playback.read(cx).effective_volume();
        let white = gpui::rgb(0xffffff);
        let this = cx.entity().downgrade();
        div()
            .id("volume-pill")
            .h(px(28.))
            .pl(px(8.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_md()
            .bg(Theme::dark().chip)
            .child(
                div()
                    .id("mute")
                    .cursor_pointer()
                    .child(icon(
                        if volume == 0.0 {
                            "volume-off"
                        } else {
                            "volume"
                        },
                        14.,
                        white,
                    ))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleMute), cx)),
            )
            .child(slider(
                "volume",
                self.drag == Drag::Volume,
                div().w(px(56.)).h(px(24.)).flex().items_center().child(bar(
                    volume,
                    3.,
                    gpui::rgba(0xffffff40),
                    white,
                )),
                move |event, _, cx| {
                    let _ = this.update(cx, |this, cx| this.slide_volume(event, cx));
                },
            ))
    }

    fn render_panel(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let playback = self.playback.read(cx);
        let (title, subtitle) = match playback.now() {
            Some(now) => (now.title.clone(), now.subtitle.clone()),
            None => ("LumiFi".into(), "Nessun brano".into()),
        };
        let total = playback.now().and_then(|n| n.duration);
        let progress = self.progress(cx);
        // While seeking, the clock shows where the knob is.
        let position = match (self.drag, total) {
            (Drag::Seek(at), Some(total)) => total.mul_f32(at),
            _ => playback.position(),
        };
        let position = format_time(position);
        let total = total.map(format_time).unwrap_or_else(|| "–:––".into());
        let shuffle_color = if playback.is_shuffled() {
            theme.accent
        } else {
            theme.muted
        };
        let (repeat_icon, repeat_color) = match playback.repeat() {
            Repeat::Off => ("repeat", theme.muted),
            Repeat::All => ("repeat", theme.accent),
            Repeat::One => ("repeat-one", theme.accent),
        };
        let paused = playback.is_paused();
        let viz = self.render_viz(theme, progress, cx);
        let this = cx.entity().downgrade();

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
                    .flex()
                    .flex_col()
                    .child(slider(
                        "progress",
                        matches!(self.drag, Drag::Seek(_)),
                        viz,
                        move |event, _, cx| {
                            let _ = this.update(cx, |this, cx| this.slide_seek(event, cx));
                        },
                    ))
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
                                if paused { "play" } else { "pause" },
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
    }

    fn render_viz(&self, theme: &Theme, progress: f32, cx: &Context<Self>) -> AnyElement {
        match self.settings.read(cx).value.mini_view {
            MiniView::A => div()
                .h(px(22.))
                .flex()
                .items_center()
                .child(bar(progress, 4., theme.track, theme.text))
                .into_any_element(),
            MiniView::B => {
                let played = (progress * WAVE_BARS as f32) as usize;
                let waveform = self.playback.read(cx).waveform();
                let peaks = if waveform.is_empty() {
                    vec![0.15; WAVE_BARS]
                } else {
                    waveform.to_vec()
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
        }
    }
}
