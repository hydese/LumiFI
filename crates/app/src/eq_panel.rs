//! The equalizer, as in the design: on/off, presets, the response curve, ten bands, bass boost
//! and output gain. The same panel is shown in Impostazioni › Equalizzatore and in its own small
//! window, opened from the EQ button of the player bar.

use gpui::{
    AnyElement, ClickEvent, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    anchored, deferred, div, prelude::*, px,
};
use lumifi_engine::dsp::{BASS_BOOST_MAX_DB, EqSettings, GAIN_RANGE_DB};

use crate::playback::playback;
use crate::settings::{EQ_BANDS, EQ_PRESETS, EqPreset, Settings, SettingsModel, settings};
use crate::text_input::{InputEvent, TextInput};
use crate::theme::Theme;
use crate::widgets::{SlideEvent, badge, icon, segment, segments, slider, switch, vertical_slider};

/// "+4,5 dB", "−2 dB", "0 dB".
pub fn format_db(db: f32) -> String {
    let rounded = (db * 10.0).round() / 10.0;
    let sign = if rounded > 0.0 {
        "+"
    } else if rounded < 0.0 {
        "−"
    } else {
        ""
    };
    let number = format!("{}", rounded.abs()).replace('.', ",");
    format!("{sign}{number} dB")
}

fn band_label(frequency: f32) -> String {
    if frequency >= 1000.0 {
        format!("{}k", frequency / 1000.0)
    } else {
        format!("{frequency}")
    }
}

/// Where a frequency sits across the graph, 0 to 1 (20 Hz to 20 kHz, logarithmic).
fn graph_x(frequency: f32) -> f32 {
    (frequency.log10() - 20f32.log10()) / 3.0
}

/// Where a gain sits down the graph, 0 to 1 (+12 dB at the top, −12 dB at the bottom).
fn graph_y(db: f32) -> f32 {
    (12.0 - db.clamp(-12.0, 12.0)) / 24.0
}

/// Rounds to half a decibel, the step of every control here.
fn half_db(db: f32) -> f32 {
    (db * 2.0).round() / 2.0
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Drag {
    None,
    Band(usize),
    Bass,
    Gain,
}

pub struct EqPanel {
    settings: Entity<SettingsModel>,
    /// Shown in its own window (otherwise inside the settings, with a button to open it).
    in_window: bool,
    drag: Drag,
    /// The preset list is open.
    menu_open: bool,
    /// Naming a new preset.
    naming: Option<Entity<TextInput>>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EqPanel {
    pub fn new(in_window: bool, cx: &mut Context<Self>) -> Self {
        let settings = settings(cx);
        let subscriptions = vec![
            cx.observe(&settings, |_, _, cx| cx.notify()),
            cx.observe(&playback(cx), |_, _, cx| cx.notify()),
        ];
        Self {
            settings,
            in_window,
            drag: Drag::None,
            menu_open: false,
            naming: None,
            _subscriptions: subscriptions,
        }
    }

    fn change(
        &mut self,
        cx: &mut Context<Self>,
        quietly: bool,
        change: impl FnOnce(&mut Settings),
    ) {
        self.settings.update(cx, |s, cx| {
            if quietly {
                s.update_quietly(cx, change)
            } else {
                s.update(cx, change)
            }
        });
    }

    fn slide(&mut self, target: Drag, event: SlideEvent, cx: &mut Context<Self>) {
        let (fraction, done) = match event {
            SlideEvent::Move(f) => (f, false),
            SlideEvent::End(f) => (f, true),
        };
        self.drag = if done { Drag::None } else { target };
        self.change(cx, !done, move |v| {
            let profile = v.eq_profile_mut();
            profile.enabled = true;
            match target {
                Drag::Band(band) => profile.gains[band] = half_db((fraction * 2.0 - 1.0) * 12.0),
                Drag::Bass => {
                    profile.bass_boost = half_db(fraction * BASS_BOOST_MAX_DB);
                    if profile.bass_boost > 0.0 {
                        profile.bass_amount = profile.bass_boost;
                    }
                }
                Drag::Gain => {
                    let (low, high) = GAIN_RANGE_DB;
                    profile.gain_db = half_db(low + fraction * (high - low));
                }
                Drag::None => {}
            }
        });
    }

    fn start_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_open = false;
        let count = self.settings.read(cx).value.eq_presets.len();
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Nome del preset", cx);
            input.set_text(format!("Il mio preset {}", count + 1), cx);
            input.select_all_text(cx);
            input
        });
        let subscription = cx.subscribe(&input, |this, input, event, cx| match event {
            InputEvent::Submit => this.save_preset(&input, cx),
            InputEvent::Cancel => {
                this.naming = None;
                cx.notify();
            }
            InputEvent::Changed => {}
        });
        self._subscriptions.push(subscription);
        let focus = input.read(cx).focus_handle_ref().clone();
        window.focus(&focus);
        self.naming = Some(input);
        cx.notify();
    }

    fn save_preset(&mut self, input: &Entity<TextInput>, cx: &mut Context<Self>) {
        let name = input.read(cx).text().trim().to_owned();
        self.naming = None;
        if name.is_empty() {
            cx.notify();
            return;
        }
        self.change(cx, false, move |v| {
            let gains = v.eq_profile().gains;
            match v.eq_presets.iter_mut().find(|p| p.name == name) {
                Some(existing) => existing.gains = gains,
                None => v.eq_presets.push(EqPreset { name, gains }),
            }
        });
    }

    pub fn render_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(window, cx);
        let value = self.settings.read(cx).value.clone();
        let profile = value.eq_profile();
        let enabled = profile.enabled;
        let gains = profile.gains;
        let bypass = playback(cx).read(cx).eq_bypassed();
        let device: SharedString = value
            .output_device
            .clone()
            .unwrap_or_else(|| "uscita di sistema".into())
            .into();
        let curve_settings = EqSettings {
            enabled: true,
            ..profile.engine()
        };
        let preamp = curve_settings.preamp_db();
        let level = curve_settings.level_db();
        let same = |curve: &[f32; 10]| curve.iter().zip(gains).all(|(a, b)| (a - b).abs() < 0.01);
        let preset_name: SharedString = EQ_PRESETS
            .iter()
            .find(|(_, curve)| same(curve))
            .map(|(name, _)| SharedString::from(*name))
            .or_else(|| {
                value
                    .eq_presets
                    .iter()
                    .find(|p| same(&p.gains))
                    .map(|p| p.name.clone().into())
            })
            .unwrap_or_else(|| "Personalizzato".into());
        let line = if enabled { theme.accent } else { theme.muted };
        let grid = theme.hairline;
        let zero = theme.hairline_strong;
        let area = gpui::Rgba {
            a: if enabled { 0.16 } else { 0.08 },
            ..line
        };

        // The response curve, drawn as in the design (without preamp and gain: the shape).
        let graph = gpui::canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let left = f32::from(bounds.origin.x);
                let top = f32::from(bounds.origin.y);
                let width = f32::from(bounds.size.width);
                let height = f32::from(bounds.size.height);
                let at =
                    |fx: f32, fy: f32| gpui::point(px(left + fx * width), px(top + fy * height));
                for f in [
                    20., 50., 100., 200., 500., 1000., 2000., 5000., 10000., 20000.,
                ] {
                    let mut b = gpui::PathBuilder::stroke(px(1.));
                    b.move_to(at(graph_x(f), 0.0));
                    b.line_to(at(graph_x(f), 1.0));
                    if let Ok(path) = b.build() {
                        window.paint_path(path, grid);
                    }
                }
                for db in [12., 6., 0., -6., -12.] {
                    let mut b = gpui::PathBuilder::stroke(px(1.));
                    b.move_to(at(0.0, graph_y(db)));
                    b.line_to(at(1.0, graph_y(db)));
                    if let Ok(path) = b.build() {
                        window.paint_path(path, if db == 0.0 { zero } else { grid });
                    }
                }
                let points: Vec<_> = (0..=140)
                    .map(|i| {
                        let f = 20.0 * 1000f32.powf(i as f32 / 140.0);
                        at(graph_x(f), graph_y(curve_settings.curve_db(f, 48_000.0)))
                    })
                    .collect();
                let mut fill = gpui::PathBuilder::fill();
                fill.move_to(at(0.0, graph_y(0.0)));
                for p in &points {
                    fill.line_to(*p);
                }
                fill.line_to(at(1.0, graph_y(0.0)));
                fill.close();
                if let Ok(path) = fill.build() {
                    window.paint_path(path, area);
                }
                let mut stroke = gpui::PathBuilder::stroke(px(2.5));
                stroke.move_to(points[0]);
                for p in &points[1..] {
                    stroke.line_to(*p);
                }
                if let Ok(path) = stroke.build() {
                    window.paint_path(path, line);
                }
            },
        )
        .absolute()
        .top(px(8.))
        .bottom(px(24.))
        .left(px(34.))
        .right(px(14.));

        let freq_labels = [
            20., 50., 100., 200., 500., 1000., 2000., 5000., 10000., 20000.,
        ]
        .into_iter()
        .map(|f| {
            div()
                .absolute()
                .bottom(px(5.))
                .left(gpui::relative(graph_x(f)))
                .w(px(30.))
                .ml(px(-15.))
                .flex()
                .justify_center()
                .text_size(px(10.))
                .text_color(theme.muted)
                .child(band_label(f))
        });
        let db_labels = [12., 6., 0., -6., -12.].into_iter().map(|db: f32| {
            div()
                .absolute()
                .left(px(-30.))
                .w(px(24.))
                .top(gpui::relative(graph_y(db)))
                .mt(px(-7.))
                .flex()
                .justify_end()
                .text_size(px(10.))
                .text_color(theme.muted)
                .child(if db > 0.0 {
                    format!("+{db}")
                } else if db < 0.0 {
                    format!("−{}", -db)
                } else {
                    "0".into()
                })
        });
        let nodes = EQ_BANDS
            .iter()
            .zip(gains)
            .enumerate()
            .filter(|(_, (_, gain))| gain.abs() > 0.01)
            .map(|(i, (f, _))| {
                let db = curve_settings.curve_db(*f, 48_000.0);
                div()
                    .absolute()
                    .left(gpui::relative(graph_x(*f)))
                    .top(gpui::relative(graph_y(db)))
                    .ml(px(-9.))
                    .mt(px(-9.))
                    .size(px(18.))
                    .rounded_full()
                    .bg(theme.surface)
                    .border_2()
                    .border_color(line)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(9.5))
                    .font_weight(FontWeight::BOLD)
                    .child((i + 1).to_string())
            });

        let entity = cx.entity().downgrade();
        let drag = self.drag;
        let knob_color = if enabled { theme.accent } else { theme.muted };
        let bands: Vec<_> = EQ_BANDS
            .iter()
            .zip(gains)
            .enumerate()
            .map(|(i, (f, gain))| {
                let fraction = (gain + 12.0) / 24.0;
                let this = entity.clone();
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(if gain.abs() > 0.01 {
                                theme.accent_text
                            } else {
                                theme.muted
                            })
                            .child(format_db(gain)),
                    )
                    .child(vertical_slider(
                        ("eq-band", i),
                        drag == Drag::Band(i),
                        div().w(px(28.)).h(px(120.)).flex().justify_center().child(
                            div()
                                .w(px(4.))
                                .h_full()
                                .rounded(px(2.))
                                .bg(theme.track)
                                .relative()
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(-1.))
                                        .w(px(6.))
                                        .h(px(1.))
                                        .top(gpui::relative(0.5))
                                        .bg(theme.muted),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(-6.))
                                        .size(px(16.))
                                        .mt(px(-8.))
                                        .top(gpui::relative(1.0 - fraction))
                                        .rounded_full()
                                        .bg(knob_color)
                                        .border_2()
                                        .border_color(theme.surface),
                                ),
                        ),
                        move |event, _, cx| {
                            let _ =
                                this.update(cx, |this, cx| this.slide(Drag::Band(i), event, cx));
                        },
                    ))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(band_label(*f)),
                    )
            })
            .collect();

        // A horizontal slider with its value, for bass boost and gain.
        let hslider = |id: &'static str, fraction: f32, target: Drag, active: bool| {
            let this = entity.clone();
            slider(
                id,
                drag == target,
                div().w_full().h(px(20.)).flex().items_center().child(
                    div()
                        .w_full()
                        .h(px(4.))
                        .rounded(px(2.))
                        .bg(theme.track)
                        .relative()
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(fraction.clamp(0.0, 1.0)))
                                .rounded(px(2.))
                                .bg(if active { theme.accent } else { theme.muted }),
                        )
                        .child(
                            div()
                                .absolute()
                                .top(px(-6.))
                                .left(gpui::relative(fraction.clamp(0.0, 1.0)))
                                .ml(px(-8.))
                                .size(px(16.))
                                .rounded_full()
                                .bg(if active { theme.accent } else { theme.muted })
                                .border_2()
                                .border_color(theme.surface),
                        ),
                ),
                move |event, _, cx| {
                    let _ = this.update(cx, |this, cx| this.slide(target, event, cx));
                },
            )
        };
        let bass_on = profile.bass_boost > 0.01;
        let (gain_low, gain_high) = GAIN_RANGE_DB;
        let gain_fraction = (profile.gain_db - gain_low) / (gain_high - gain_low);
        let may_clip = level > 0.05;
        let extra_card = |label: &'static str, hint: &'static str| {
            div()
                .flex_1()
                .min_w_0()
                .px(px(14.))
                .py(px(12.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme.hairline)
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
                        .child(div().text_size(px(12.)).text_color(theme.muted).child(hint)),
                )
        };
        let bass_card = extra_card("Bass boost", "Bassi più pieni sotto i 100 Hz.").child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(switch(bass_on, &theme).id("bass-on").on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.change(cx, false, |v| {
                            let p = v.eq_profile_mut();
                            if p.bass_boost > 0.01 {
                                p.bass_amount = p.bass_boost;
                                p.bass_boost = 0.0;
                            } else {
                                p.bass_boost = if p.bass_amount > 0.01 {
                                    p.bass_amount
                                } else {
                                    6.0
                                };
                                p.enabled = true;
                            }
                        })
                    },
                )))
                .child(div().flex_1().min_w_0().child(hslider(
                    "bass-amount",
                    profile.bass_boost / BASS_BOOST_MAX_DB,
                    Drag::Bass,
                    enabled && bass_on,
                )))
                .child(
                    div()
                        .w(px(56.))
                        .flex()
                        .justify_end()
                        .text_size(px(12.))
                        .text_color(if bass_on {
                            theme.accent_text
                        } else {
                            theme.muted
                        })
                        .child(format_db(profile.bass_boost)),
                ),
        );
        let gain_card = extra_card("Gain", "Alza o abbassa tutto, dopo il preamp automatico.")
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().flex_1().min_w_0().child(hslider(
                        "gain",
                        gain_fraction,
                        Drag::Gain,
                        enabled,
                    )))
                    .child(
                        div()
                            .w(px(56.))
                            .flex()
                            .justify_end()
                            .text_size(px(12.))
                            .child(format_db(profile.gain_db)),
                    ),
            )
            .when(may_clip, |d| {
                d.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.accent_text)
                        .child("Sopra 0 dB può distorcere."),
                )
            });

        // The preset selector and its list.
        let mut menu_items: Vec<AnyElement> = Vec::new();
        if self.menu_open {
            let item = |id: SharedString, label: SharedString, checked: bool| {
                div()
                    .id(id)
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.hover))
                    .child(
                        div()
                            .w(px(14.))
                            .when(checked, |d| d.child(icon("check", 13., theme.accent_text))),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(label))
            };
            let heading = |text: &'static str| {
                div()
                    .px(px(10.))
                    .pt(px(6.))
                    .pb(px(2.))
                    .text_size(px(11.))
                    .text_color(theme.faint)
                    .child(text)
            };
            menu_items.push(heading("Pronti").into_any_element());
            for (name, curve) in EQ_PRESETS {
                menu_items.push(
                    item(
                        format!("eq-preset-{name}").into(),
                        name.into(),
                        preset_name.as_ref() == name,
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.menu_open = false;
                        this.change(cx, false, |v| {
                            let p = v.eq_profile_mut();
                            p.gains = curve;
                            p.enabled = true;
                        })
                    }))
                    .into_any_element(),
                );
            }
            if !value.eq_presets.is_empty() {
                menu_items.push(heading("I tuoi").into_any_element());
                for (index, preset) in value.eq_presets.iter().enumerate() {
                    let gains = preset.gains;
                    let checked = preset_name.as_ref() == preset.name.as_str();
                    menu_items.push(
                        item(
                            format!("eq-user-{index}").into(),
                            preset.name.clone().into(),
                            checked,
                        )
                        .child(
                            div()
                                .id(("eq-user-delete", index))
                                .size(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(5.))
                                .hover(|d| d.bg(theme.hover))
                                .child(icon("trash", 12., theme.muted))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.change(cx, false, |v| {
                                        if index < v.eq_presets.len() {
                                            v.eq_presets.remove(index);
                                        }
                                    })
                                })),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.menu_open = false;
                            this.change(cx, false, |v| {
                                let p = v.eq_profile_mut();
                                p.gains = gains;
                                p.enabled = true;
                            })
                        }))
                        .into_any_element(),
                    );
                }
            }
            menu_items.push(
                div()
                    .my(px(4.))
                    .h(px(1.))
                    .bg(theme.hairline)
                    .into_any_element(),
            );
            menu_items.push(
                item("eq-save".into(), "Salva come preset…".into(), false)
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.start_naming(window, cx)
                        }),
                    )
                    .into_any_element(),
            );
        }
        let selector = div()
            .relative()
            .child(
                div()
                    .id("eq-preset")
                    .h(px(32.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.hairline)
                    .bg(theme.field)
                    .text_size(px(12.5))
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.hover))
                    .child(div().text_color(theme.muted).child("Preset"))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(preset_name.clone()),
                    )
                    .child(icon("chevron-down", 12., theme.muted))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.menu_open = !this.menu_open;
                        cx.notify();
                    })),
            )
            .when(self.menu_open, |d| {
                d.child(deferred(
                    anchored().snap_to_window_with_margin(px(8.)).child(
                        div()
                            .id("eq-preset-menu")
                            .occlude()
                            .mt(px(36.))
                            .w(px(240.))
                            .p(px(4.))
                            .rounded(px(10.))
                            .bg(theme.surface)
                            .border_1()
                            .border_color(theme.hairline)
                            .shadow_lg()
                            .text_size(px(12.5))
                            .children(menu_items)
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.menu_open = false;
                                cx.notify();
                            })),
                    ),
                ))
            });

        let naming_row = self.naming.clone().map(|input| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .h(px(32.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(theme.accent)
                        .bg(theme.field)
                        .child(input.clone()),
                )
                .child(
                    div()
                        .id("eq-save-confirm")
                        .h(px(32.))
                        .px(px(14.))
                        .flex()
                        .items_center()
                        .rounded(px(8.))
                        .bg(theme.accent)
                        .text_color(theme.on_accent)
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .child("Salva")
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.save_preset(&input, cx)
                        })),
                )
                .child(
                    outline_button("eq-save-cancel", "Annulla", &theme).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.naming = None;
                            cx.notify();
                        },
                    )),
                )
        });

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .font_family(crate::theme::display_font(cx))
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Equalizzatore"),
                            )
                            .child(switch(enabled, &theme).id("eq-on").on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    this.change(cx, false, |v| {
                                        let p = v.eq_profile_mut();
                                        p.enabled = !p.enabled;
                                    })
                                },
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                segments(&theme)
                                    .child(segment("eq-simple", "Semplice", true, &theme))
                                    .child(
                                        segment("eq-advanced", "Avanzata", false, &theme)
                                            .opacity(0.5)
                                            .cursor_default(),
                                    ),
                            )
                            .when(!self.in_window, |d| {
                                d.child(
                                    outline_button("eq-window", "Apri in una finestra", &theme)
                                        .on_click(|_, _, cx| {
                                            cx.defer(crate::windows::open_equalizer)
                                        }),
                                )
                            })
                            .child(
                                outline_button("eq-ab", "Ascolta senza (A/B)", &theme)
                                    .when(bypass, |d| {
                                        d.bg(theme.accent_soft)
                                            .border_color(theme.accent)
                                            .text_color(theme.accent_text)
                                    })
                                    .on_click(|_, _, cx| {
                                        playback(cx).update(cx, |p, cx| p.toggle_eq_bypass(cx))
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(selector)
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(theme.muted)
                            .child(SharedString::from(format!("Salvato per: {device}"))),
                    ),
            )
            .children(naming_row)
            .child(
                div()
                    .relative()
                    .h(px(220.))
                    .rounded(px(10.))
                    .bg(theme.ground)
                    .border_1()
                    .border_color(theme.hairline)
                    .child(graph)
                    .child(
                        div()
                            .absolute()
                            .top(px(8.))
                            .bottom(px(24.))
                            .left(px(34.))
                            .right(px(14.))
                            .children(db_labels)
                            .children(nodes),
                    )
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .h(px(24.))
                            .left(px(34.))
                            .right(px(14.))
                            .children(freq_labels),
                    ),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme.hairline)
                    .flex()
                    .when(!enabled, |d| d.opacity(0.6))
                    .children(bands),
            )
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .when(!enabled, |d| d.opacity(0.6))
                    .child(bass_card)
                    .child(gain_card),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .text_size(px(12.))
                            .text_color(theme.muted)
                            .child("Preamp automatico")
                            .child(
                                div()
                                    .text_color(theme.text)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format_db(preamp)),
                            )
                            .child("· niente clipping"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .opacity(0.55)
                                    .child(div().text_size(px(12.)).child("Importa profilo AutoEq"))
                                    .child(badge("In arrivo", &theme)),
                            )
                            .child(outline_button("eq-reset", "Azzera", &theme).on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.change(cx, false, |v| {
                                        let p = v.eq_profile_mut();
                                        p.gains = [0.0; 10];
                                        p.bass_boost = 0.0;
                                        p.gain_db = 0.0;
                                    })
                                }),
                            )),
                    ),
            )
            .into_any_element()
    }
}

fn outline_button(
    id: &'static str,
    label: &'static str,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(px(32.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.hairline)
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|d| d.bg(theme.hover))
        .child(label)
}

impl Render for EqPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_panel(window, cx)
    }
}

/// The equalizer's own window: the panel on the window's background, with room around it.
pub struct EqWindow {
    panel: Entity<EqPanel>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EqWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let panel = cx.new(|cx| EqPanel::new(true, cx));
        Self {
            _subscriptions: vec![
                cx.observe(&panel, |_, _, cx| cx.notify()),
                cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            ],
            panel,
        }
    }
}

impl Render for EqWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(window, cx);
        let content = self.panel.update(cx, |p, cx| p.render_panel(window, cx));
        div()
            .id("eq-window")
            .key_context(crate::actions::PLAYER_CONTEXT)
            .size_full()
            .overflow_y_scroll()
            .bg(theme.ground)
            .text_color(theme.text)
            .text_size(px(13.))
            .font_family(crate::theme::ui_font(cx))
            .on_mouse_down(MouseButton::Left, |_, _, _| {})
            .child(div().px(px(24.)).pt(px(20.)).pb(px(24.)).child(content))
    }
}
