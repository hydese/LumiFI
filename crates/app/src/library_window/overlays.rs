//! What opens over the library: the right-click menu, the new-version window and the welcome.

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, PathPromptOptions, SharedString, StatefulInteractiveElement, Styled, anchored,
    deferred, div, prelude::*, px,
};

use super::LibraryWindow;
use super::parts::thousands;
use crate::settings::Settings;
use crate::theme::{Theme, ThemeMode};
use crate::updates::CURRENT;
use crate::widgets::{badge, icon, logo};

const WELCOME_STEPS: usize = 5;

impl LibraryWindow {
    pub(super) fn render_overlays(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut layers = Vec::new();
        if let Some(menu) = self.render_menu(theme, cx) {
            layers.push(menu);
        }
        if self.updates.read(cx).dialog_open {
            layers.push(self.render_update_dialog(theme, cx));
        }
        if let Some(step) = self.welcome {
            layers.push(self.render_welcome(step, theme, cx));
        }
        layers
    }

    fn render_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let items = menu
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let action = item.action.clone();
                let color = if item.danger {
                    gpui::rgb(0xe5484d)
                } else {
                    theme.text
                };
                div()
                    .when(item.separated && i > 0, |d| {
                        d.mt(px(4.))
                            .pt(px(4.))
                            .border_t_1()
                            .border_color(theme.hairline)
                    })
                    .child(
                        div()
                            .id(("menu-item", i))
                            .h(px(32.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .rounded(px(6.))
                            .text_color(color)
                            .cursor_pointer()
                            .hover(|d| d.bg(theme.hover))
                            .child(match item.icon {
                                Some(name) => icon(name, 14., color).into_any_element(),
                                None => div().w(px(14.)).into_any_element(),
                            })
                            .child(div().min_w_0().truncate().child(item.label.clone()))
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                this.menu = None;
                                action(this, window, cx);
                                cx.notify();
                            })),
                    )
            })
            .collect::<Vec<_>>();
        let position = menu.position;
        Some(
            div()
                .id("menu-layer")
                .absolute()
                .inset_0()
                // A click anywhere else closes the menu.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_menu(cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _, _, cx| this.close_menu(cx)),
                )
                .child(deferred(
                    anchored()
                        .position(position)
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .id("menu")
                                .occlude()
                                .min_w(px(220.))
                                .max_w(px(320.))
                                .p(px(4.))
                                .rounded(px(10.))
                                .bg(theme.surface)
                                .border_1()
                                .border_color(theme.hairline_strong)
                                .shadow_lg()
                                .text_size(px(13.))
                                .children(items),
                        ),
                ))
                .into_any_element(),
        )
    }

    fn render_update_dialog(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let release = self.updates.read(cx).available().cloned();
        let Some(release) = release else {
            return div().into_any_element();
        };
        let url = release.url.clone();
        modal_backdrop(theme)
            .child(
                div()
                    .id("update-dialog")
                    .occlude()
                    .w(px(460.))
                    .px(px(28.))
                    .pt(px(26.))
                    .pb(px(22.))
                    .rounded(px(16.))
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.hairline_strong)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(logo(48., theme))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .text_size(px(18.))
                                            .font_weight(FontWeight::BOLD)
                                            .child(format!(
                                                "È disponibile LumiFi {}{}",
                                                release.version,
                                                if release.prerelease { " (beta)" } else { "" }
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .text_color(theme.muted)
                                            .child(format!("Tu hai la {CURRENT}")),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(12.))
                            .rounded(px(10.))
                            .bg(theme.field)
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.muted)
                                    .child("NOVITÀ"),
                            )
                            .children(release.notes.iter().map(|note| {
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .line_height(px(19.))
                                    .child("•")
                                    .child(div().flex_1().child(note.clone()))
                            }))
                            .child(
                                div()
                                    .id("all-notes")
                                    .text_size(px(12.5))
                                    .text_color(theme.accent_text)
                                    .cursor_pointer()
                                    .child("Tutte le note su GitHub")
                                    .on_click({
                                        let url = url.clone();
                                        move |_, _, cx| cx.open_url(&url)
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted)
                            .child("L'installazione automatica arriva con l'installer. Per ora scarichi la nuova versione da GitHub."),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                button("update-later", "Più tardi", false, theme).on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.updates.update(cx, |u, cx| u.set_dialog(false, cx))
                                    }),
                                ),
                            )
                            .child(
                                button("update-download", "Scarica da GitHub", true, theme)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        cx.open_url(&url);
                                        this.updates.update(cx, |u, cx| u.set_dialog(false, cx))
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn add_folder(&mut self, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Aggiungi".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = answer.await {
                let _ = this.update(cx, |this, cx| {
                    this.settings.update(cx, |s, cx| {
                        s.update(cx, |v| {
                            for path in paths {
                                if !v.folders.contains(&path) {
                                    v.folders.push(path);
                                }
                            }
                        })
                    });
                    let folders = this.settings.read(cx).value.folders.clone();
                    this.library.update(cx, |l, cx| l.rescan(folders, cx));
                });
            }
        })
        .detach();
    }

    fn render_welcome(&mut self, step: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let value: Settings = self.settings.read(cx).value.clone();
        let dots = (0..WELCOME_STEPS).map(|i| {
            div()
                .w(px(if i == step { 20. } else { 6. }))
                .h(px(6.))
                .rounded(px(3.))
                .bg(if i == step {
                    theme.accent
                } else {
                    theme.switch_off
                })
        });
        let h1 = |text: &'static str| {
            div()
                .text_size(px(22.))
                .font_weight(FontWeight::BOLD)
                .child(text)
        };
        let p = |text: &'static str| {
            div()
                .text_size(px(13.5))
                .line_height(px(20.))
                .text_color(theme.muted)
                .child(text)
        };
        let body: AnyElement = match step {
            0 => div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(14.))
                .child(logo(84., theme))
                .child(
                    div()
                        .text_size(px(26.))
                        .font_weight(FontWeight::BOLD)
                        .child("Benvenuto in LumiFi"),
                )
                .child(
                    div()
                        .max_w(px(380.))
                        .text_center()
                        .text_size(px(14.))
                        .line_height(px(21.))
                        .text_color(theme.muted)
                        .child("Piccolo, leggero, con audio lossless in alta qualità. Quattro scelte veloci e si parte; puoi cambiarle tutte dopo."),
                )
                .into_any_element(),
            1 => {
                let card = |id: &'static str, label: &'static str, mode: ThemeMode, left: gpui::Rgba, right: gpui::Rgba, bar: gpui::Rgba, cx: &mut Context<Self>| {
                    let on = value.theme == mode;
                    div()
                        .id(id)
                        .flex_1()
                        .p(px(10.))
                        .rounded(px(12.))
                        .border(px(if on { 2. } else { 1. }))
                        .border_color(if on { theme.accent } else { theme.hairline })
                        .cursor_pointer()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .h(px(74.))
                                .rounded(px(8.))
                                .overflow_hidden()
                                .border_1()
                                .border_color(theme.hairline)
                                .relative()
                                .child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .child(div().flex_1().bg(left))
                                        .child(div().flex_1().bg(right)),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(8.))
                                        .right(px(8.))
                                        .bottom(px(8.))
                                        .h(px(14.))
                                        .rounded(px(4.))
                                        .bg(bar)
                                        .flex()
                                        .items_center()
                                        .pl(px(4.))
                                        .child(div().size(px(8.)).rounded_full().bg(theme.accent)),
                                ),
                        )
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.settings.update(cx, |s, cx| s.update(cx, |v| v.theme = mode))
                        }))
                };
                let dark = gpui::rgb(0x0d0d0e);
                let light = gpui::rgb(0xf7f6f4);
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(h1("Chiaro o scuro?"))
                    .child(p("L'aspetto di LumiFi, nel mini player e nell'app."))
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(card("w-dark", "Scuro", ThemeMode::Dark, dark, dark, gpui::rgb(0x18181a), cx))
                            .child(card("w-light", "Chiaro", ThemeMode::Light, light, light, gpui::rgb(0xffffff), cx))
                            .child(card("w-system", "Come il sistema", ThemeMode::System, dark, light, gpui::rgb(0x8b8b90), cx)),
                    )
                    .into_any_element()
            }
            2 => {
                let data = self.library.read(cx).data();
                let scanning = self.library.read(cx).scan_progress();
                let found = match scanning {
                    Some((_, found)) if found > 0 => format!("{} brani trovati finora", thousands(found)),
                    Some(_) => "Sto cercando…".to_owned(),
                    None => format!("{} brani trovati", thousands(data.tracks.len())),
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(h1("Dov'è la tua musica?"))
                    .child(p("LumiFi legge le cartelle che scegli. Vanno bene anche dischi esterni e NAS."))
                    .child(
                        div()
                            .rounded(px(10.))
                            .border_1()
                            .border_color(theme.hairline)
                            .overflow_hidden()
                            .children(value.folders.iter().map(|folder| {
                                let name = folder
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| folder.display().to_string());
                                div()
                                    .px(px(14.))
                                    .py(px(12.))
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .border_b_1()
                                    .border_color(theme.hairline)
                                    .child(icon("folder", 18., theme.accent_text))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(theme.muted)
                                                    .truncate()
                                                    .child(folder.display().to_string()),
                                            ),
                                    )
                                    .child(icon("check", 16., theme.accent_text))
                            }))
                            .child(
                                div()
                                    .id("w-add-folder")
                                    .min_h(px(44.))
                                    .px(px(14.))
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .cursor_pointer()
                                    .hover(|d| d.bg(theme.hover))
                                    .child(icon("plus", 16., theme.muted))
                                    .child(if value.folders.is_empty() {
                                        "Scegli una cartella"
                                    } else {
                                        "Aggiungi un'altra cartella"
                                    })
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.add_folder(cx))),
                            ),
                    )
                    .child(div().text_size(px(12.)).text_color(theme.muted).child(found))
                    .into_any_element()
            }
            3 => div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(h1("Colleghi i tuoi account?"))
                .child(p("Le tue playlist accanto alla musica sul computer. Quello che non colleghi non compare nell'app."))
                .child(
                    div()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme.hairline)
                        .overflow_hidden()
                        .children(
                            [
                                ("Spotify", "Playlist e brani che ti piacciono. Serve Premium."),
                                ("Apple Music", "Le playlist di Music.app. Solo su Mac."),
                                ("YouTube Music", "Allo studio."),
                                ("Last.fm", "Registra quello che ascolti."),
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(i, (name, hint))| {
                                div()
                                    .px(px(14.))
                                    .py(px(8.))
                                    .flex()
                                    .items_center()
                                    .gap(px(12.))
                                    .when(i > 0, |d| d.border_t_1().border_color(theme.hairline))
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                                            .child(div().text_size(px(12.)).text_color(theme.muted).child(hint)),
                                    )
                                    .child(badge("In arrivo", theme))
                            }),
                        ),
                )
                .child(div().text_size(px(12.)).text_color(theme.muted).child("Quando arrivano li trovi in Impostazioni › Servizi."))
                .into_any_element(),
            _ => {
                let mode_card = |id: &'static str, title: &'static str, hint: &'static str, on: bool, soon: bool| {
                    div()
                        .id(id)
                        .px(px(14.))
                        .py(px(12.))
                        .rounded(px(10.))
                        .border(px(if on { 2. } else { 1. }))
                        .border_color(if on { theme.accent } else { theme.hairline })
                        .when(on, |d| d.bg(theme.accent_soft))
                        .when(soon, |d| d.opacity(0.6))
                        .when(!soon, |d| d.cursor_pointer())
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(div().text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).child(title))
                                .child(div().text_size(px(12.5)).line_height(px(18.)).text_color(theme.muted).child(hint)),
                        )
                        .when(soon, |d| d.child(badge("In arrivo", theme)))
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(h1("Come deve aggiornarsi LumiFi?"))
                    .child(p("Le nuove versioni portano correzioni e novità. Musica e playlist restano sempre."))
                    .child(mode_card(
                        "w-auto",
                        "Automaticamente",
                        "Scaricate in silenzio e installate quando chiudi LumiFi. Non devi pensarci.",
                        false,
                        true,
                    ))
                    .child(
                        mode_card(
                            "w-ask",
                            "Chiedimi prima",
                            "LumiFi ti avvisa di una nuova versione con il pulsante Aggiorna.",
                            true,
                            false,
                        ),
                    )
                    .child(div().text_size(px(12.)).text_color(theme.muted).child("Puoi cambiarlo in Impostazioni › Generale."))
                    .into_any_element()
            }
        };
        let next_label: SharedString = match step {
            0 => "Iniziamo".into(),
            s if s + 1 == WELCOME_STEPS => "Inizia ad ascoltare".into(),
            _ => "Avanti".into(),
        };
        modal_backdrop(theme)
            .child(
                div()
                    .id("welcome")
                    .occlude()
                    .w(px(540.))
                    // Fixed, so the buttons stay where they are from one step to the next.
                    .h(px(520.))
                    .px(px(32.))
                    .pt(px(28.))
                    .pb(px(24.))
                    .rounded(px(16.))
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.hairline)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .justify_center()
                            .gap(px(6.))
                            .children(dots),
                    )
                    .child(div().flex_1().min_h_0().flex().flex_col().child(body))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_between()
                            .child(if step > 0 {
                                div()
                                    .id("w-back")
                                    .h(px(36.))
                                    .px(px(14.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(8.))
                                    .text_color(theme.muted)
                                    .cursor_pointer()
                                    .hover(|d| d.bg(theme.hover))
                                    .child("Indietro")
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.welcome = Some(step - 1);
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            })
                            .child(button("w-next", next_label, true, theme).on_click(
                                cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    if step + 1 >= WELCOME_STEPS {
                                        this.welcome = None;
                                        this.settings.update(cx, |s, cx| {
                                            s.update(cx, |v| v.welcome_done = true)
                                        });
                                    } else {
                                        this.welcome = Some(step + 1);
                                    }
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .into_any_element()
    }
}

/// A dark veil over the window with its content centred.
fn modal_backdrop(theme: &Theme) -> gpui::Stateful<gpui::Div> {
    let _ = theme;
    div()
        .id("modal-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .bg(gpui::rgba(0x08080a9e))
        .flex()
        .items_center()
        .justify_center()
}

fn button(
    id: &'static str,
    label: impl Into<SharedString>,
    accent: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(38.))
        .px(px(18.))
        .flex()
        .items_center()
        .rounded(px(9.))
        .cursor_pointer()
        .when(accent, |d| {
            d.bg(theme.accent)
                .text_color(theme.on_accent)
                .font_weight(FontWeight::BOLD)
        })
        .when(!accent, |d| {
            d.border_1()
                .border_color(theme.hairline_strong)
                .hover(|d| d.bg(theme.hover))
        })
        .child(label.into())
}
