//! The settings, as in the approved design (and like LumiPDF), shown as a page of the library
//! window: sections on the left, grouped rows on the right. Things that do not exist yet are shown with an "In arrivo" label
//! and do nothing, instead of pretending to work.

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, PathPromptOptions, Render, SharedString, StatefulInteractiveElement, Styled,
    Window, div, prelude::*, px,
};
use lumifi_engine::output::{DeviceInfo, list_devices};

use crate::library::{Library, library};
use crate::playback::playback;
use crate::settings::{GainMode, MiniStyle, Settings, SettingsModel, StartWith, settings};
use crate::theme::{Theme, ThemeMode};
use crate::updates::{UpdateState, updates};
use crate::widgets::{badge, icon, logo, radio, segment, segments, switch};

const REPOSITORY: &str = "https://github.com/hydese/LumiFI";
const RELEASES: &str = "https://github.com/hydese/LumiFI/releases";
const AUTHOR: &str = "https://github.com/hydese";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    General,
    Audio,
    Library,
    Equalizer,
    Services,
    Appearance,
    About,
}

impl Section {
    const ALL: [Section; 7] = [
        Section::General,
        Section::Appearance,
        Section::Audio,
        Section::Library,
        Section::Equalizer,
        Section::Services,
        Section::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Section::General => "Generale",
            Section::Audio => "Audio",
            Section::Library => "Libreria",
            Section::Equalizer => "Equalizzatore",
            Section::Services => "Servizi",
            Section::Appearance => "Aspetto",
            Section::About => "Informazioni",
        }
    }
}

/// Asks the library window to leave the settings.
pub struct BackToLibrary;

impl gpui::EventEmitter<BackToLibrary> for SettingsPage {}

pub struct SettingsPage {
    section: Section,
    settings: Entity<SettingsModel>,
    library: Entity<Library>,
    /// Output devices, read once when the window opens (and on "Aggiorna").
    devices: Option<Vec<DeviceInfo>>,
    /// The equalizer, the same panel as in its own window.
    eq: Entity<crate::eq_panel::EqPanel>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl SettingsPage {
    pub fn new(section: Section, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = settings(cx);
        let library = library(cx);
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            cx.observe(&settings, |_, _, cx| cx.notify()),
            cx.observe(&library, |_, _, cx| cx.notify()),
            cx.observe(&playback(cx), |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            section,
            settings,
            library,
            devices: None,
            eq: cx.new(|cx| crate::eq_panel::EqPanel::new(false, cx)),
            _subscriptions: subscriptions,
        };
        this.refresh_devices(cx);
        this
    }

    pub fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        cx.notify();
    }

    fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        let job = cx.background_spawn(async { list_devices().unwrap_or_default() });
        cx.spawn(async move |this, cx| {
            let devices = job.await;
            let _ = this.update(cx, |this, cx| {
                this.devices = Some(devices);
                cx.notify();
            });
        })
        .detach();
    }

    fn change(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Settings)) {
        self.settings.update(cx, |s, cx| s.update(cx, change));
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        let folders = self.settings.read(cx).value.folders.clone();
        self.library.update(cx, |l, cx| l.rescan(folders, cx));
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
                    this.change(cx, |v| {
                        for path in paths {
                            if !v.folders.contains(&path) {
                                v.folders.push(path);
                            }
                        }
                    });
                    this.rescan(cx);
                });
            }
        })
        .detach();
    }

    fn set_output(&mut self, device: Option<String>, cx: &mut Context<Self>) {
        if self.settings.read(cx).value.output_device == device {
            return;
        }
        self.change(cx, |v| v.output_device = device);
        playback(cx).update(cx, |p, cx| p.reload_output(cx));
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_page(window, cx)
    }
}

impl SettingsPage {
    /// The page as an element, for the library window to place directly in its layout. As in
    /// LumiPDF: the list of sections and the content form one block, at most 1040 points wide,
    /// centred in the window; the list stays put while the content scrolls.
    pub fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(window, cx);
        let available = f32::from(window.viewport_size().width);
        let block = available.min(1040.0);
        // Explicit widths: inside a scroll area a "max width" alone lets rows shrink to their
        // text.
        let content_width = (block - 64.0 - NAV_WIDTH - 32.0).max(360.0);
        let value = self.settings.read(cx).value.clone();
        let page = match self.section {
            Section::General => self.general(&value, &theme, cx),
            Section::Audio => self.audio(&value, &theme, cx),
            Section::Library => self.library_page(&value, &theme, cx),
            Section::Equalizer => self.eq.update(cx, |p, cx| p.render_panel(window, cx)),
            Section::Services => services(&theme),
            Section::Appearance => self.appearance(&value, &theme, cx),
            Section::About => about(&theme, cx),
        };
        div()
            .size_full()
            .flex()
            .justify_center()
            .bg(theme.ground)
            .text_color(theme.text)
            .text_size(px(13.))
            .child(
                div()
                    .w(px(block))
                    .h_full()
                    .px(px(32.))
                    .flex()
                    .gap(px(32.))
                    .child(self.nav(&theme, cx))
                    .child(
                        div()
                            .id("settings-page")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .child(
                                div()
                                    .w(px(content_width))
                                    .pt(px(32.))
                                    .pb(px(48.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(20.))
                                    .child(page),
                            ),
                    ),
            )
            .into_any_element()
    }
}

const NAV_WIDTH: f32 = 220.0;

impl Section {
    /// The icon and the colour of its square, as in LumiPDF.
    fn icon(self) -> (&'static str, u32) {
        match self {
            Section::General => ("settings", 0x3d6bf0),
            Section::Appearance => ("sun", 0x7a5af5),
            Section::Audio => ("volume", 0xF0613D),
            Section::Library => ("folder", 0x2f9be8),
            Section::Equalizer => ("eq", 0xf0a020),
            Section::Services => ("plug", 0x34a853),
            Section::About => ("info", 0x8e8e93),
        }
    }
}

// Building blocks ---------------------------------------------------------------------------

fn title(text: &'static str) -> impl IntoElement {
    div()
        .mb(px(4.))
        .font_family(crate::theme::title_font())
        .text_size(px(22.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// A labelled group of rows.
fn group(label: &'static str, theme: &Theme, rows: Vec<AnyElement>) -> AnyElement {
    let mut frame = div()
        .w_full()
        .rounded(px(12.))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.hairline)
        .when(!theme.dark, |d| d.shadow_sm())
        .overflow_hidden()
        .flex()
        .flex_col();
    for (i, row) in rows.into_iter().enumerate() {
        frame = frame.child(
            div()
                .w_full()
                .when(i > 0, |d| d.border_t_1().border_color(theme.hairline))
                .child(row),
        );
    }
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(8.))
        .when(!label.is_empty(), |d| {
            d.child(
                div()
                    .px(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.muted)
                    .child(label),
            )
        })
        .child(frame)
        .into_any_element()
}

/// A row with a title, a hint under it and a control on the right.
fn row(
    title: impl Into<SharedString>,
    hint: Option<&'static str>,
    control: impl IntoElement,
    theme: &Theme,
) -> gpui::Div {
    div()
        .w_full()
        .px(px(18.))
        .py(px(12.))
        .min_h(px(48.))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().child(title.into()))
                .when_some(hint, |d, hint| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted)
                            .line_height(px(17.))
                            .child(hint),
                    )
                }),
        )
        .child(div().flex_none().child(control))
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
        .gap(px(6.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme.hairline)
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|d| d.bg(theme.hover))
        .child(label)
}

impl SettingsPage {
    fn nav(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(NAV_WIDTH))
            .flex_none()
            .pt(px(32.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(Section::ALL.iter().map(|&section| {
                let active = section == self.section;
                let (icon_name, color) = section.icon();
                div()
                    .id(section.label())
                    .h(px(34.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_color(if active { theme.text } else { theme.muted })
                    .when(active, |d| {
                        d.bg(theme.pressed).font_weight(FontWeight::SEMIBOLD)
                    })
                    .when(!active, |d| d.hover(|d| d.bg(theme.hover)))
                    .child(
                        div()
                            .size(px(22.))
                            .flex_none()
                            .rounded(px(6.))
                            .bg(gpui::rgb(color))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(icon_name, 13., gpui::rgb(0xffffff))),
                    )
                    .child(section.label())
                    .on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.show(section, cx)),
                    )
            }))
    }

    // Generale --------------------------------------------------------------------------------

    fn general(&self, value: &Settings, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let version = format!("LumiFi {}", env!("CARGO_PKG_VERSION"));
        let start = value.start_with;
        let resume = value.resume;
        let beta = value.beta;
        let update_state = updates(cx).read(cx).state.clone();
        let update_status: SharedString = match &update_state {
            UpdateState::Idle => "Non ancora controllato".into(),
            UpdateState::Checking => "Controllo in corso…".into(),
            UpdateState::UpToDate { checked_at } => {
                format!("Hai l'ultima versione · controllato {checked_at}").into()
            }
            UpdateState::Available(release) => {
                format!("È disponibile LumiFi {}", release.version).into()
            }
            UpdateState::Failed => "Non riesco a raggiungere GitHub. Riprova più tardi.".into(),
        };
        let update_button = if matches!(update_state, UpdateState::Available(_)) {
            outline_button("update-now", "Aggiorna", theme)
                .bg(theme.accent)
                .text_color(theme.on_accent)
                .font_weight(FontWeight::SEMIBOLD)
                .on_click(|_, _, cx| updates(cx).update(cx, |u, cx| u.set_dialog(true, cx)))
        } else {
            outline_button("check-now", "Cerca ora", theme).on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    let beta = this.settings.read(cx).value.beta;
                    updates(cx).update(cx, |u, cx| u.check(beta, cx));
                },
            ))
        };
        let install_card = |name: &'static str, hint: &'static str, selected: bool| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .gap(px(10.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(9.))
                .border_1()
                .border_color(theme.hairline)
                .items_start()
                .child(radio(selected, theme).mt(px(2.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                        .child(div().text_size(px(12.)).text_color(theme.muted).child(hint)),
                )
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(title("Generale"))
            .child(group(
                "Aggiornamenti",
                theme,
                vec![
                    div()
                        .px(px(18.))
                        .py(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .child(logo(36., theme))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(div().font_weight(FontWeight::SEMIBOLD).child(version))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.muted)
                                        .child(update_status),
                                ),
                        )
                        .child(update_button)
                        .into_any_element(),
                    row(
                        "Versioni beta",
                        Some("Ricevi le novità prima degli altri. Possono avere qualche difetto; torni alla versione stabile quando vuoi."),
                        switch(beta, theme).id("beta").on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.change(cx, |v| v.beta = !v.beta);
                                let beta = this.settings.read(cx).value.beta;
                                updates(cx).update(cx, |u, cx| u.check(beta, cx));
                            },
                        )),
                        theme,
                    )
                    .into_any_element(),
                    div()
                        .px(px(18.))
                        .py(px(12.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .child("Come installare le nuove versioni")
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.muted)
                                        .child("LumiFi controlla a ogni avvio se c'è una versione nuova."),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(
                                    install_card(
                                        "Automaticamente",
                                        "Scaricate in silenzio e installate quando chiudi LumiFi.",
                                        false,
                                    )
                                    .opacity(0.55)
                                    .child(badge("In arrivo", theme)),
                                )
                                .child(install_card(
                                    "Chiedimi prima",
                                    "LumiFi ti avvisa e installa quando fai clic.",
                                    true,
                                )),
                        )
                        .into_any_element(),
                ],
            ))
            .child(group(
                "Avvio",
                theme,
                vec![
                    row(
                        "All'apertura mostra",
                        None,
                        segments(theme)
                            .child(
                                segment(
                                    "start-library",
                                    "Libreria",
                                    start == StartWith::Library,
                                    theme,
                                )
                                .on_click(cx.listener(
                                    |this, _: &ClickEvent, _, cx| {
                                        this.change(cx, |v| v.start_with = StartWith::Library)
                                    },
                                )),
                            )
                            .child(
                                segment(
                                    "start-mini",
                                    "Mini player",
                                    start == StartWith::Mini,
                                    theme,
                                )
                                .on_click(cx.listener(
                                    |this, _: &ClickEvent, _, cx| {
                                        this.change(cx, |v| v.start_with = StartWith::Mini)
                                    },
                                )),
                            ),
                        theme,
                    )
                    .into_any_element(),
                    row(
                        "Riprendi da dove eri",
                        Some("All'avvio ritrovi l'ultimo brano e la coda, in pausa."),
                        switch(resume, theme).id("resume").on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| this.change(cx, |v| v.resume = !v.resume),
                        )),
                        theme,
                    )
                    .into_any_element(),
                ],
            ))
            .into_any_element()
    }

    // Audio -----------------------------------------------------------------------------------

    fn audio(&self, value: &Settings, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let chosen = value.output_device.clone();
        let devices = self.devices.clone().unwrap_or_default();
        let default_name = devices
            .iter()
            .find(|d| d.is_default)
            .map(|d| format!("ora: {}", d.name))
            .unwrap_or_default();
        let device_row = |id: SharedString,
                          name: SharedString,
                          note: String,
                          selected: bool,
                          device: Option<String>,
                          cx: &mut Context<Self>| {
            div()
                .id(gpui::ElementId::Name(id))
                .px(px(18.))
                .min_h(px(44.))
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .child(radio(selected, theme))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted)
                        .truncate()
                        .child(note),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.set_output(device.clone(), cx)
                }))
                .into_any_element()
        };
        let mut outputs = vec![device_row(
            "out-system".into(),
            "Segui il sistema".into(),
            default_name,
            chosen.is_none(),
            None,
            cx,
        )];
        for (i, device) in devices.iter().enumerate() {
            let selected = chosen.as_deref() == Some(device.name.as_str());
            outputs.push(device_row(
                format!("out-{i}").into(),
                device.name.clone().into(),
                String::new(),
                selected,
                Some(device.name.clone()),
                cx,
            ));
        }
        if self.devices.is_none() {
            outputs.push(row("Cerco i dispositivi…", None, div(), theme).into_any_element());
        }
        let show_quality = value.show_quality;
        let gain = value.replaygain;
        let gain_segment =
            |id: &'static str, label: &'static str, mode: GainMode, cx: &mut Context<Self>| {
                segment(id, label, gain == mode, theme).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| this.change(cx, |v| v.replaygain = mode),
                ))
            };
        let stream_note: Option<SharedString> =
            playback(cx)
                .read(cx)
                .output_description()
                .map(|(device, rate)| {
                    let khz = rate as f32 / 1000.0;
                    let khz = if khz.fract() == 0.0 {
                        format!("{khz:.0}")
                    } else {
                        format!("{khz:.1}").replace('.', ",")
                    };
                    format!("Ora: {device} a {khz} kHz.").into()
                });

        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(title("Audio"))
                    .child(
                        outline_button("refresh-devices", "Aggiorna dispositivi", theme).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.refresh_devices(cx)),
                        ),
                    ),
            )
            .child(group("Uscita", theme, outputs))
            .children(stream_note.map(|note| {
                div()
                    .mt(px(-10.))
                    .text_size(px(12.))
                    .text_color(theme.muted)
                    .child(note)
            }))
            .child(group(
                "Riproduzione",
                theme,
                vec![
                    row(
                        "Adatta la frequenza al brano",
                        Some("Il dispositivo passa alla frequenza di ogni file, che arriva così com'è. Se non può, LumiFi converte con un ricampionatore di alta qualità."),
                        badge("Sempre attivo", theme),
                        theme,
                    )
                    .into_any_element(),
                    row(
                        "Modalità esclusiva",
                        Some("Mentre LumiFi suona, le altre app non usano l'uscita. Utile con un DAC."),
                        badge("In arrivo", theme),
                        theme,
                    )
                    .into_any_element(),
                    row(
                        "Riproduzione senza pause",
                        Some("Niente silenzio tra i brani di un album dal vivo o di un concept."),
                        badge("Sempre attivo", theme),
                        theme,
                    )
                    .into_any_element(),
                    row(
                        "Mostra formato e qualità",
                        Some("Indica nel player il formato del file, ad esempio FLAC 24/96."),
                        switch(show_quality, theme).id("quality").on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.change(cx, |v| v.show_quality = !v.show_quality)
                            },
                        )),
                        theme,
                    )
                    .into_any_element(),
                    row(
                        "Normalizzazione del volume (ReplayGain)",
                        Some("Brani e album suonano allo stesso volume, senza schiacciare la dinamica. Usa i valori scritti nei file; vale dal brano successivo."),
                        segments(theme)
                            .child(gain_segment("rg-off", "Spenta", GainMode::Off, cx))
                            .child(gain_segment("rg-track", "Brano", GainMode::Track, cx))
                            .child(gain_segment("rg-album", "Album", GainMode::Album, cx)),
                        theme,
                    )
                    .into_any_element(),
                ],
            ))
            .into_any_element()
    }

    // Libreria --------------------------------------------------------------------------------

    fn library_page(&self, value: &Settings, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let data = library.data();
        let scan = library.scan_progress();
        let mut rows: Vec<AnyElement> = value
            .folders
            .iter()
            .enumerate()
            .map(|(i, folder)| {
                div()
                    .px(px(18.))
                    .min_h(px(44.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(icon("folder", 15., theme.muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(folder.display().to_string()),
                    )
                    .child(
                        div()
                            .id(("remove-folder", i))
                            .size(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|d| d.bg(theme.hover))
                            .child(icon("close", 12., theme.muted))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.change(cx, |v| {
                                    if i < v.folders.len() {
                                        v.folders.remove(i);
                                    }
                                });
                                this.rescan(cx);
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        if rows.is_empty() {
            rows.push(
                row(
                    "Nessuna cartella",
                    Some("Aggiungi la cartella dove tieni la musica."),
                    div(),
                    theme,
                )
                .into_any_element(),
            );
        }
        let status = match scan {
            Some((read, found)) if found > 0 => {
                format!("Sto leggendo la libreria: {read} di {found}")
            }
            Some(_) => "Cerco la musica…".to_owned(),
            None => format!(
                "{} brani · {} album · {} artisti",
                data.tracks.len(),
                data.albums.len(),
                data.artists.len()
            ),
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(title("Libreria"))
            .child(group("Cartelle della musica", theme, rows))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        outline_button("add-folder", "Aggiungi cartella…", theme)
                            .child(icon("plus", 13., theme.text))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.add_folder(cx))),
                    )
                    .child(
                        outline_button("rescan", "Aggiorna la libreria", theme)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.rescan(cx))),
                    )
                    .child(div().flex_1())
                    .child(div().text_size(px(12.)).text_color(theme.muted).child(status)),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted)
                    .child("Le cartelle possono stare sul disco interno, su un disco esterno o su un NAS. I brani doppi (stesso titolo, artista e durata) compaiono una volta sola."),
            )
            .into_any_element()
    }

    // Aspetto ---------------------------------------------------------------------------------

    fn appearance(&self, value: &Settings, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mode = value.theme;
        let style = value.mini_style;
        let on_top = value.always_on_top;
        let theme_segment = |id: &'static str,
                             label: &'static str,
                             m: ThemeMode,
                             cx: &mut Context<Self>| {
            segment(id, label, mode == m, theme).on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.change(cx, |v| v.theme = m)),
            )
        };
        let mini_card =
            |id: &'static str, label: &'static str, s: MiniStyle, cx: &mut Context<Self>| {
                let selected = style == s;
                div()
                    .id(id)
                    .flex_1()
                    .p(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .when(selected, |d| {
                        d.border_color(theme.accent).bg(theme.accent_soft)
                    })
                    .when(!selected, |d| {
                        d.border_color(theme.hairline).hover(|d| d.bg(theme.hover))
                    })
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .h(px(118.))
                            .rounded(px(8.))
                            .bg(theme.ground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(mini_preview(s, theme)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(radio(selected, theme))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(label)),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.change(cx, |v| {
                            v.mini_style = s;
                            v.mini_scale = crate::settings::fit_scale(s, v.mini_scale);
                        })
                    }))
            };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(title("Aspetto"))
            .child(group(
                "",
                theme,
                vec![
                    row(
                        "Tema",
                        None,
                        segments(theme)
                            .child(theme_segment("theme-system", "Sistema", ThemeMode::System, cx))
                            .child(theme_segment("theme-light", "Chiaro", ThemeMode::Light, cx))
                            .child(theme_segment("theme-dark", "Scuro", ThemeMode::Dark, cx)),
                        theme,
                    )
                    .into_any_element(),
                ],
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted)
                            .child("Mini player"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(mini_card("mini-strip", "Striscia", MiniStyle::Strip, cx))
                            .child(mini_card("mini-square", "Quadrato", MiniStyle::Square, cx)),
                    ),
            )
            .child(group(
                "",
                theme,
                vec![
                    row(
                        "Sempre in primo piano",
                        Some("Il mini player resta sopra le altre finestre. Anche con la puntina sul player o il tasto T. La dimensione si sceglie col tasto destro sul mini player."),
                        switch(on_top, theme).id("on-top").on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.change(cx, |v| v.always_on_top = !v.always_on_top)
                            },
                        )),
                        theme,
                    )
                    .into_any_element(),
                ],
            ))
            .into_any_element()
    }
}

/// A tiny drawing of each mini player shape, for the choice in Aspetto. The square is shown at
/// rest (only the title and the bar).
fn mini_preview(style: MiniStyle, theme: &Theme) -> AnyElement {
    let logo_box = |size: f32| {
        div()
            .size_full()
            .bg(theme.accent)
            .flex()
            .items_center()
            .justify_center()
            .child(icon("waves", size, theme.on_accent))
    };
    let progress = |_bars: usize, h: f32| {
        div().h(px(h)).flex().items_center().child(
            div()
                .w_full()
                .h(px(3.))
                .rounded(px(1.5))
                .bg(theme.track)
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(0.4))
                        .rounded(px(1.5))
                        .bg(theme.accent),
                ),
        )
    };
    match style {
        MiniStyle::Strip => div()
            .w(px(240.))
            .h(px(36.))
            .rounded(px(7.))
            .overflow_hidden()
            .bg(theme.surface)
            .border_1()
            .border_color(theme.hairline)
            .flex()
            .items_center()
            .child(div().size(px(36.)).flex_none().child(logo_box(26.)))
            .child(
                div()
                    .flex_1()
                    .px(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(div().h(px(4.)).w(px(46.)).rounded(px(2.)).bg(theme.muted))
                    .child(progress(13, 9.)),
            )
            .child(
                div()
                    .size(px(18.))
                    .mr(px(8.))
                    .rounded_full()
                    .bg(theme.accent),
            )
            .into_any_element(),
        MiniStyle::Square => div()
            .size(px(100.))
            .rounded(px(8.))
            .overflow_hidden()
            .relative()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(70.))
                    .child(logo_box(44.)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(30.))
                    .px(px(7.))
                    .bg(theme.scrim)
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(4.))
                    .child(div().h(px(4.)).w(px(40.)).rounded(px(2.)).bg(theme.muted))
                    .child(progress(16, 7.)),
            )
            .into_any_element(),
    }
}

fn services(theme: &Theme) -> AnyElement {
    let list = [
        (
            "Spotify",
            "Le tue playlist e la libreria nel player. Serve Premium; client non ufficiale.",
        ),
        (
            "Apple Music",
            "Comanda Music.app: playlist, play, pausa e salto. Solo su Mac.",
        ),
        (
            "Last.fm",
            "Registra ogni brano che ascolti sul tuo profilo.",
        ),
        ("YouTube Music", "Allo studio."),
    ];
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(18.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(title("Servizi"))
                .child(
                    div()
                        .text_color(theme.muted)
                        .child("Un servizio spento non compare da nessuna parte nell'app."),
                ),
        )
        .child(group(
            "",
            theme,
            list.iter()
                .map(|(name, hint)| {
                    div()
                        .px(px(18.))
                        .py(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(14.))
                        .child(
                            div()
                                .size(px(36.))
                                .flex_none()
                                .rounded(px(9.))
                                .bg(theme.field)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("note", 17., theme.muted)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(div().font_weight(FontWeight::SEMIBOLD).child(*name))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.muted)
                                        .child(*hint),
                                ),
                        )
                        .child(badge("In arrivo", theme))
                        .into_any_element()
                })
                .collect(),
        ))
        .into_any_element()
}

fn about(theme: &Theme, cx: &mut Context<SettingsPage>) -> AnyElement {
    let version = format!("LumiFi {}", env!("CARGO_PKG_VERSION"));
    let link = |id: &'static str, label: &'static str, url: &'static str| {
        div()
            .id(id)
            .text_color(theme.accent_text)
            .cursor_pointer()
            .hover(|d| d.underline())
            .child(label)
            .on_click(move |_, _, cx: &mut App| cx.open_url(url))
    };
    let _ = cx;
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(18.))
        .child(title("Informazioni"))
        .child(
            div()
                .rounded(px(12.))
                .border_1()
                .border_color(theme.hairline)
                .bg(theme.surface)
                .px(px(24.))
                .py(px(32.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .child(logo(56., theme))
                .child(
                    div()
                        .px(px(18.))
                        .py(px(5.))
                        .rounded_full()
                        .border_1()
                        .border_color(theme.hairline)
                        .bg(theme.field)
                        .text_size(px(15.))
                        .font_weight(FontWeight::BOLD)
                        .child(version.clone()),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .text_size(px(12.5))
                        .text_color(theme.muted)
                        .child("Fatto da")
                        .child(link("author", "hydese", AUTHOR))
                        .child("·")
                        .child(link("handle", "@hydesee", AUTHOR)),
                )
                .child(
                    div()
                        .max_w(px(400.))
                        .text_color(theme.muted)
                        .text_center()
                        .line_height(px(20.))
                        .child("Un music player piccolo e leggero, con audio lossless in alta qualità."),
                )
                .child(
                    outline_button("whats-new", "Mostra le novità di questa versione", theme)
                        .mt(px(6.))
                        .on_click(|_, _, cx| cx.open_url(RELEASES)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .text_size(px(12.))
                .text_color(theme.muted)
                .child(version)
                .child("·")
                .child(link("github", "GitHub", REPOSITORY))
                .child("·")
                .child(link("notes", "Note di rilascio", RELEASES))
                .child("· GNU GPL v3 o successiva"),
        )
        .into_any_element()
}
