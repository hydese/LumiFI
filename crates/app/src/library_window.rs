//! The library, as in the approved design: a header with the logo, the mini player and the
//! settings; the sidebar; the pages; and a Spotify-style player bar along the bottom.

use std::path::Path;

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, Render, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, UniformListScrollHandle, WeakEntity, Window,
    div, img, prelude::*, px, uniform_list,
};
use lumifi_engine::format_time;

use crate::actions::*;
use crate::library::{Library, LibraryData, library};
use crate::playback::{Playback, playback};
use crate::queue::Repeat;
use crate::settings::{SettingsModel, settings};
use crate::settings_window::Section;
use crate::theme::Theme;
use crate::widgets::{SlideEvent, icon, logo, slider};
use crate::windows;

const HEADER_HEIGHT: f32 = 52.0;
const SIDEBAR_WIDTH: f32 = 232.0;
/// Below this window width the sidebar shows icons only and the album column goes away.
const COMPACT_WIDTH: f32 = 960.0;
const SIDEBAR_COMPACT: f32 = 64.0;
const CONTENT_PAD: f32 = 28.0;
const CARD_MIN: f32 = 150.0;
const CARD_GAP: f32 = 18.0;
const PLAYER_BAR_HEIGHT: f32 = 76.0;
const ROW_HEIGHT: f32 = 52.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Albums,
    Album(usize),
    Artists,
    Artist(usize),
    Tracks,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Drag {
    None,
    Volume,
    Seek(f32),
}

pub struct LibraryWindow {
    focus: FocusHandle,
    playback: Entity<Playback>,
    library: Entity<Library>,
    settings: Entity<SettingsModel>,
    page: Page,
    /// Pages to go back to.
    history: Vec<Page>,
    drag: Drag,
    albums_scroll: UniformListScrollHandle,
    artists_scroll: UniformListScrollHandle,
    tracks_scroll: UniformListScrollHandle,
    /// The title last given to the window (the playing track, for the taskbar).
    window_title: String,
    _subscriptions: Vec<gpui::Subscription>,
}

impl LibraryWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let playback = playback(cx);
        let library = library(cx);
        let settings = settings(cx);
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            cx.observe_in(&playback, window, |this, _, window, cx| {
                this.update_title(window, cx);
                cx.notify();
            }),
            cx.observe(&library, |_, _, cx| cx.notify()),
            cx.observe(&settings, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            focus,
            playback,
            library,
            settings,
            page: Page::Albums,
            history: Vec::new(),
            drag: Drag::None,
            albums_scroll: UniformListScrollHandle::new(),
            artists_scroll: UniformListScrollHandle::new(),
            tracks_scroll: UniformListScrollHandle::new(),
            window_title: String::new(),
            _subscriptions: subscriptions,
        };
        this.update_title(window, cx);
        this
    }

    /// "Title – Artist" while something is loaded, so the Windows taskbar shows the track.
    fn update_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = match self.playback.read(cx).now() {
            Some(now) => match &now.artist {
                Some(artist) => format!("{} – {}", now.title, artist),
                None => now.title.to_string(),
            },
            None => "LumiFi".to_owned(),
        };
        if title != self.window_title {
            window.set_window_title(&title);
            self.window_title = title;
        }
    }

    fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        if page != self.page {
            self.history.push(self.page);
            self.page = page;
            cx.notify();
        }
    }

    /// A top-level page from the sidebar: no way back from there.
    fn go_root(&mut self, page: Page, cx: &mut Context<Self>) {
        self.history.clear();
        self.page = page;
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = self.history.pop() {
            self.page = page;
            cx.notify();
        }
    }

    fn play_album(&mut self, album: usize, start: usize, shuffle: bool, cx: &mut Context<Self>) {
        let paths = self.library.read(cx).data().album_paths(album);
        self.playback.update(cx, |p, cx| {
            if p.is_shuffled() != shuffle {
                p.toggle_shuffle(cx);
            }
            p.play_list(paths, start, cx);
        });
    }

    fn play_all_tracks(&mut self, start: usize, cx: &mut Context<Self>) {
        let data = self.library.read(cx).data();
        let paths = data.tracks.iter().map(|t| t.path.clone()).collect();
        self.playback
            .update(cx, |p, cx| p.play_list(paths, start, cx));
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

    /// The album the playing track belongs to, to jump there from the player bar.
    fn playing_album(&self, cx: &App) -> Option<usize> {
        let path = self.playback.read(cx).current_path()?.to_path_buf();
        let data = self.library.read(cx).data();
        data.tracks
            .iter()
            .find(|t| t.path == path)
            .map(|t| t.album_index)
    }
}

fn open_settings(section: Option<Section>, cx: &mut App) {
    // Deferred: opening a window from inside another window's click is not allowed.
    cx.defer(move |cx| windows::open_settings(section, cx));
}

impl Render for LibraryWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(window, cx);
        let compact = f32::from(window.viewport_size().width) < COMPACT_WIDTH;
        let content = match self.page {
            Page::Albums => self.render_albums(&theme, compact, window, cx),
            Page::Album(index) => self.render_album(index, &theme, compact, cx),
            Page::Artists => self.render_artists(&theme, cx),
            Page::Artist(index) => self.render_artist(index, &theme, compact, window, cx),
            Page::Tracks => self.render_tracks(&theme, compact, cx),
        };
        div()
            .id("library-window")
            .key_context(PLAYER_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.ground)
            .text_color(theme.text)
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .on_drop(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                let paths = paths.paths().to_vec();
                this.playback.update(cx, |p, cx| p.open_paths(paths, cx));
            }))
            .child(self.render_header(&theme))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(&theme, compact, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(content),
                    ),
            )
            .child(self.render_player_bar(&theme, compact, cx))
    }
}

// Header and sidebar ------------------------------------------------------------------------

impl LibraryWindow {
    fn render_header(&self, theme: &Theme) -> impl IntoElement {
        div()
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(16.))
            .bg(theme.bar)
            .border_b_1()
            .border_color(theme.hairline)
            .child(
                div()
                    .id("logo")
                    // Room for the macOS window buttons.
                    .when(cfg!(target_os = "macos"), |d| d.ml(px(70.)))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(6.))
                    .py(px(4.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.hover))
                    .child(logo(22., theme))
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::BOLD)
                            .child("LumiFi"),
                    )
                    .on_click(|_, _, cx| open_settings(None, cx)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("mini-player-button")
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.hairline)
                    .text_size(px(12.))
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.hover))
                    .child(icon("minimize", 13., theme.text))
                    .child("Mini player")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleLibrary), cx)),
            )
            .child(
                div()
                    .id("settings-button")
                    .size(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.hover))
                    .child(icon("settings", 17., theme.muted))
                    .on_click(|_, _, cx| open_settings(None, cx)),
            )
    }

    fn render_sidebar(
        &self,
        theme: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let section = |label: &'static str| {
            div()
                .px(px(10.))
                .pb(px(6.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.faint)
                .child(label)
        };
        let current = match self.page {
            Page::Album(_) => Page::Albums,
            Page::Artist(_) => Page::Artists,
            page => page,
        };
        let item = |id: &'static str, icon_name: &'static str, label: &'static str, page: Page| {
            let active = current == page;
            div()
                .id(id)
                .h(px(32.))
                .px(px(10.))
                .flex()
                .items_center()
                .when(compact, |d| d.justify_center())
                .gap(px(10.))
                .rounded(px(7.))
                .cursor_pointer()
                .when(active, |d| {
                    d.bg(theme.accent_soft)
                        .text_color(theme.accent_text)
                        .font_weight(FontWeight::SEMIBOLD)
                })
                .when(!active, |d| d.hover(|d| d.bg(theme.hover)))
                .child(icon(
                    icon_name,
                    15.,
                    if active {
                        theme.accent_text
                    } else {
                        theme.text
                    },
                ))
                .when(!compact, |d| d.child(label))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.go_root(page, cx)))
        };
        let library = self.library.read(cx);
        let data = library.data();
        let scan = library.scan_progress();
        div()
            .w(px(if compact { SIDEBAR_COMPACT } else { SIDEBAR_WIDTH }))
            .h_full()
            .flex_none()
            .px(px(10.))
            .py(px(16.))
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.hairline)
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .when(!compact, |d| d.child(section("LIBRERIA")))
                    .child(item("nav-albums", "grid", "Album", Page::Albums))
                    .child(item("nav-artists", "artist", "Artisti", Page::Artists))
                    .child(item("nav-tracks", "note", "Brani", Page::Tracks)),
            )
            .when(!compact, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(section("SERVIZI"))
                        .child(
                            div()
                                .id("services-hint")
                                .mx(px(4.))
                                .p(px(10.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(theme.hairline_strong)
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .text_color(theme.muted)
                                .cursor_pointer()
                                .hover(|d| d.bg(theme.hover))
                                .child("Spotify, Apple Music e altri compaiono qui quando li attivi nelle impostazioni.")
                                .on_click(|_, _, cx| open_settings(Some(Section::Services), cx)),
                        ),
                )
            })
            .child(div().flex_1())
            .when(!compact, |d| {
                d.child(
                    div()
                        .px(px(10.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .text_size(px(11.5))
                        .text_color(theme.muted)
                        .child(format!(
                            "{} brani · {} album",
                            thousands(data.tracks.len()),
                            thousands(data.albums.len())
                        ))
                        .when_some(scan, |d, (read, found)| {
                            d.child(if found == 0 {
                                "Cerco la musica…".to_string()
                            } else {
                                format!("Leggo la libreria: {read} di {found}")
                            })
                            .child(plain_bar(
                                if found == 0 {
                                    0.0
                                } else {
                                    read as f32 / found as f32
                                },
                                3.,
                                theme,
                            ))
                        }),
                )
            })
    }
}

/// 2418 → "2.418", as Italians write it.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

fn plain_bar(fraction: f32, height: f32, theme: &Theme) -> impl IntoElement {
    div()
        .w_full()
        .h(px(height))
        .rounded_full()
        .bg(theme.track)
        .child(
            div()
                .h_full()
                .w(gpui::relative(fraction.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(theme.accent),
        )
}

/// A 4-pixel bar with a round knob, as in the design's player bar.
fn knob_bar(fraction: f32, theme: &Theme) -> impl IntoElement {
    let fraction = fraction.clamp(0.0, 1.0);
    div()
        .w_full()
        .h(px(4.))
        .rounded(px(2.))
        .bg(theme.track)
        .relative()
        .child(
            div()
                .h_full()
                .w(gpui::relative(fraction))
                .rounded(px(2.))
                .bg(theme.text),
        )
        .child(
            div()
                .absolute()
                .top(px(-4.))
                .left(gpui::relative(fraction))
                .ml(px(-6.))
                .size(px(12.))
                .rounded_full()
                .bg(theme.text),
        )
}

// Pages -------------------------------------------------------------------------------------

/// The page title with its count beside it, and a back arrow when there is history.
fn page_header(
    title: impl Into<SharedString>,
    count: Option<String>,
    can_go_back: bool,
    theme: &Theme,
    cx: &mut Context<LibraryWindow>,
) -> AnyElement {
    div()
        .flex_none()
        .px(px(CONTENT_PAD))
        .pt(px(22.))
        .pb(px(16.))
        .flex()
        .items_center()
        .gap(px(12.))
        .when(can_go_back, |d| d.child(back_button(theme, cx)))
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(10.))
                .min_w_0()
                .child(
                    div()
                        .text_size(px(26.))
                        .font_weight(FontWeight::BOLD)
                        .truncate()
                        .child(title.into()),
                )
                .when_some(count, |d, count| {
                    d.child(div().text_color(theme.muted).child(count))
                }),
        )
        .into_any_element()
}

fn back_button(theme: &Theme, cx: &mut Context<LibraryWindow>) -> impl IntoElement {
    div()
        .id("back")
        .size(px(30.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme.field)
        .cursor_pointer()
        .hover(|d| d.bg(theme.hover))
        .child(icon("back", 16., theme.text))
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.back(cx)))
}

/// A square cover from the thumbnail cache, or the LumiFi waves when there is none.
fn cover(path: Option<&Path>, size: Option<f32>, radius: f32, theme: &Theme) -> AnyElement {
    let frame = div()
        .flex_none()
        .rounded(px(radius))
        .overflow_hidden()
        .bg(theme.surface)
        .map(|d| match size {
            Some(size) => d.size(px(size)),
            None => d.w_full().h_full(),
        });
    match path {
        Some(path) => frame
            .child(
                img(path.to_path_buf())
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        None => frame
            .flex()
            .items_center()
            .justify_center()
            .child(icon("waves", size.unwrap_or(120.) * 0.45, theme.faint))
            .into_any_element(),
    }
}

/// The round accent ▶ shown over covers and rows under the mouse.
fn play_dot(size: f32, theme: &Theme) -> gpui::Div {
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(theme.accent)
        .flex()
        .items_center()
        .justify_center()
        .shadow_md()
        .child(icon("play", size * 0.42, theme.on_accent))
}

impl LibraryWindow {
    fn grid_columns(window: &Window, compact: bool) -> (usize, f32) {
        let sidebar = if compact {
            SIDEBAR_COMPACT
        } else {
            SIDEBAR_WIDTH
        };
        let width = f32::from(window.viewport_size().width) - sidebar - 2.0 * CONTENT_PAD;
        let columns = ((width + CARD_GAP) / (CARD_MIN + CARD_GAP))
            .floor()
            .max(2.0) as usize;
        let card = (width - CARD_GAP * (columns as f32 - 1.0)) / columns as f32;
        (columns, card.max(80.0))
    }

    fn empty_state(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let scanning = self.library.read(cx).scan_progress().is_some();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .child(logo(72., theme))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if scanning {
                        "Sto leggendo la tua musica…"
                    } else {
                        "La libreria è vuota"
                    }),
            )
            .when(!scanning, |d| {
                d.child(
                    div()
                        .text_color(theme.muted)
                        .child("Scegli nelle impostazioni la cartella dove tieni la musica."),
                )
                .child(
                    div()
                        .id("empty-settings")
                        .mt(px(4.))
                        .h(px(34.))
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(8.))
                        .bg(theme.accent)
                        .text_color(theme.on_accent)
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .child(icon("folder", 15., theme.on_accent))
                        .child("Scegli le cartelle…")
                        .on_click(|_, _, cx| open_settings(Some(Section::Library), cx)),
                )
            })
            .into_any_element()
    }

    fn album_card(
        data: &LibraryData,
        index: usize,
        card: f32,
        theme: &Theme,
        this: WeakEntity<Self>,
    ) -> AnyElement {
        let album = &data.albums[index];
        let group = SharedString::from(format!("album-card-{index}"));
        let play = this.clone();
        div()
            .id(("album", index))
            .group(group.clone())
            .w(px(card))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.))
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .size(px(card))
                    .child(cover(album.cover.as_deref(), Some(card), 8., theme))
                    .child(
                        div()
                            .id(("album-play", index))
                            .absolute()
                            .right(px(8.))
                            .bottom(px(8.))
                            .opacity(0.)
                            .group_hover(group, |s| s.opacity(1.))
                            .child(play_dot(40., theme))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = play
                                    .update(cx, |this, cx| this.play_album(index, 0, false, cx));
                            }),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(album.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted)
                            .truncate()
                            .child(album.artist.clone()),
                    ),
            )
            .on_click(move |_, _, cx| {
                let _ = this.update(cx, |this, cx| this.go(Page::Album(index), cx));
            })
            .into_any_element()
    }

    fn render_albums(
        &self,
        theme: &Theme,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let data = self.library.read(cx).data();
        let header = page_header(
            "Album",
            Some(thousands(data.albums.len())),
            false,
            theme,
            cx,
        );
        if data.albums.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .child(header)
                .child(self.empty_state(theme, cx))
                .into_any_element();
        }
        let (columns, card) = Self::grid_columns(window, compact);
        let rows = data.albums.len().div_ceil(columns);
        let this = cx.entity().downgrade();
        let theme = *theme;
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(
                uniform_list("albums", rows, move |range, _, _| {
                    range
                        .map(|row| {
                            let start = row * columns;
                            let end = (start + columns).min(data.albums.len());
                            div()
                                .px(px(CONTENT_PAD))
                                .pb(px(22.))
                                .flex()
                                .gap(px(CARD_GAP))
                                .children((start..end).map(|i| {
                                    Self::album_card(&data, i, card, &theme, this.clone())
                                }))
                        })
                        .collect()
                })
                .track_scroll(self.albums_scroll.clone())
                .flex_1(),
            )
            .into_any_element()
    }

    fn render_album(
        &self,
        index: usize,
        theme: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let data = self.library.read(cx).data();
        let Some(album) = data.albums.get(index).cloned() else {
            return div().into_any_element();
        };
        let playing = self.playback.read(cx).current_path().map(Path::to_path_buf);
        let minutes = album.duration.as_secs().div_ceil(60);
        let songs = album.tracks.len();
        let summary = format!(
            "{} · {} {} · {} min",
            album.artist,
            songs,
            if songs == 1 { "brano" } else { "brani" },
            minutes
        );
        let pill =
            |id: &'static str, icon_name: &'static str, label: &'static str, accent: bool| {
                div()
                    .id(id)
                    .h(px(36.))
                    .px(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded_full()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .when(accent, |d| d.bg(theme.accent).text_color(theme.on_accent))
                    .when(!accent, |d| d.bg(theme.field).hover(|d| d.bg(theme.hover)))
                    .child(icon(
                        icon_name,
                        14.,
                        if accent { theme.on_accent } else { theme.text },
                    ))
                    .child(label)
            };
        let this = cx.entity().downgrade();
        div()
            .id("album-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .px(px(CONTENT_PAD))
                    .pt(px(22.))
                    .when(!self.history.is_empty(), |d| {
                        d.child(div().mb(px(14.)).child(back_button(theme, cx)))
                    })
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(22.))
                            .child(cover(album.cover.as_deref(), Some(190.), 10., theme))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme.muted)
                                            .child("ALBUM"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(30.))
                                            .font_weight(FontWeight::BOLD)
                                            .line_clamp(2)
                                            .child(album.title.clone()),
                                    )
                                    .child(div().text_color(theme.muted).child(summary))
                                    .child(
                                        div()
                                            .mt(px(10.))
                                            .flex()
                                            .gap(px(10.))
                                            .child(
                                                pill("album-play", "play", "Riproduci", true)
                                                    .on_click(cx.listener(
                                                        move |this, _: &ClickEvent, _, cx| {
                                                            this.play_album(index, 0, false, cx)
                                                        },
                                                    )),
                                            )
                                            .child(
                                                pill("album-shuffle", "shuffle", "Casuale", false)
                                                    .on_click(cx.listener(
                                                        move |this, _: &ClickEvent, _, cx| {
                                                            this.play_album(index, 0, true, cx)
                                                        },
                                                    )),
                                            ),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(CONTENT_PAD - 10.))
                    .pt(px(24.))
                    .pb(px(24.))
                    .child(table_header(false, compact, theme))
                    .children(album.tracks.iter().enumerate().map(|(position, &t)| {
                        let track = &data.tracks[t];
                        let number = track
                            .track_no
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| (position + 1).to_string());
                        let this = this.clone();
                        track_row(
                            TrackRow {
                                key: t,
                                number,
                                title: track.title.clone(),
                                artist: track.artist.clone(),
                                album: None,
                                cover: None,
                                duration: track.duration.map(format_time),
                                playing: playing.as_deref() == Some(track.path.as_path()),
                            },
                            compact,
                            theme,
                        )
                        .on_click(move |_, _, cx| {
                            let _ = this
                                .update(cx, |this, cx| this.play_album(index, position, false, cx));
                        })
                    })),
            )
            .into_any_element()
    }

    fn render_artists(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let data = self.library.read(cx).data();
        let header = page_header(
            "Artisti",
            Some(thousands(data.artists.len())),
            false,
            theme,
            cx,
        );
        if data.artists.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .child(header)
                .child(self.empty_state(theme, cx))
                .into_any_element();
        }
        let this = cx.entity().downgrade();
        let theme = *theme;
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(
                uniform_list("artists", data.artists.len(), move |range, _, _| {
                    range
                        .map(|i| {
                            let artist = &data.artists[i];
                            let this = this.clone();
                            let albums = artist.albums.len();
                            div().px(px(CONTENT_PAD - 10.)).child(
                                div()
                                    .id(("artist", i))
                                    .h(px(60.))
                                    .px(px(10.))
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .hover(|d| d.bg(theme.hover))
                                    .child(cover(artist.cover.as_deref(), Some(44.), 22., &theme))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .truncate()
                                                    .child(artist.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(theme.muted)
                                                    .child(format!(
                                                        "{albums} album · {} brani",
                                                        artist.track_count
                                                    )),
                                            ),
                                    )
                                    .on_click(move |_, _, cx| {
                                        let _ = this
                                            .update(cx, |this, cx| this.go(Page::Artist(i), cx));
                                    }),
                            )
                        })
                        .collect()
                })
                .track_scroll(self.artists_scroll.clone())
                .flex_1(),
            )
            .into_any_element()
    }

    fn render_artist(
        &self,
        index: usize,
        theme: &Theme,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let data = self.library.read(cx).data();
        let Some(artist) = data.artists.get(index).cloned() else {
            return div().into_any_element();
        };
        let (_, card) = Self::grid_columns(window, compact);
        let this = cx.entity().downgrade();
        div()
            .id("artist-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(page_header(
                artist.name.clone(),
                Some(format!(
                    "{} album · {} brani",
                    artist.albums.len(),
                    artist.track_count
                )),
                !self.history.is_empty(),
                theme,
                cx,
            ))
            .child(
                div()
                    .px(px(CONTENT_PAD))
                    .pb(px(24.))
                    .flex()
                    .flex_wrap()
                    .gap(px(CARD_GAP))
                    .children(
                        artist
                            .albums
                            .iter()
                            .map(|&a| Self::album_card(&data, a, card, theme, this.clone())),
                    ),
            )
            .into_any_element()
    }

    fn render_tracks(&self, theme: &Theme, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let data = self.library.read(cx).data();
        let header = page_header(
            "Brani",
            Some(thousands(data.tracks.len())),
            false,
            theme,
            cx,
        );
        if data.tracks.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .child(header)
                .child(self.empty_state(theme, cx))
                .into_any_element();
        }
        let playing = self.playback.read(cx).current_path().map(Path::to_path_buf);
        let this = cx.entity().downgrade();
        let theme = *theme;
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .px(px(CONTENT_PAD - 10.))
                    .child(table_header(true, compact, &theme)),
            )
            .child(
                uniform_list("tracks", data.tracks.len(), move |range, _, _| {
                    range
                        .map(|i| {
                            let track = &data.tracks[i];
                            let this = this.clone();
                            let album = &data.albums[track.album_index];
                            div().px(px(CONTENT_PAD - 10.)).child(
                                track_row(
                                    TrackRow {
                                        key: i,
                                        number: (i + 1).to_string(),
                                        title: track.title.clone(),
                                        artist: track.artist.clone(),
                                        album: Some(track.album.clone()),
                                        cover: Some(album.cover.clone()),
                                        duration: track.duration.map(format_time),
                                        playing: playing.as_deref() == Some(track.path.as_path()),
                                    },
                                    compact,
                                    &theme,
                                )
                                .on_click(move |_, _, cx| {
                                    let _ = this.update(cx, |this, cx| this.play_all_tracks(i, cx));
                                }),
                            )
                        })
                        .collect()
                })
                .track_scroll(self.tracks_scroll.clone())
                .flex_1(),
            )
            .into_any_element()
    }
}

/// What one row of a track list shows.
struct TrackRow {
    key: usize,
    number: String,
    title: SharedString,
    artist: SharedString,
    /// The album column (the full track list); `None` on an album's own page.
    album: Option<SharedString>,
    /// A small cover before the title (the full track list).
    cover: Option<Option<std::path::PathBuf>>,
    duration: Option<String>,
    playing: bool,
}

const NUMBER_WIDTH: f32 = 32.0;
const DURATION_WIDTH: f32 = 56.0;

/// "# · Titolo · Album · ⏱" over a track list, with a line under it.
fn table_header(with_album: bool, compact: bool, theme: &Theme) -> impl IntoElement {
    div()
        .h(px(34.))
        .px(px(10.))
        .mb(px(6.))
        .flex()
        .items_center()
        .gap(px(14.))
        .border_b_1()
        .border_color(theme.hairline)
        .text_size(px(12.))
        .text_color(theme.muted)
        .child(
            div()
                .w(px(NUMBER_WIDTH))
                .flex_none()
                .flex()
                .justify_end()
                .child("#"),
        )
        .child(div().flex_1().min_w_0().child("Titolo"))
        .when(with_album && !compact, |d| {
            d.child(div().flex_1().min_w_0().child("Album"))
        })
        .child(
            div()
                .w(px(DURATION_WIDTH))
                .flex_none()
                .flex()
                .justify_end()
                .child(icon("clock", 14., theme.muted)),
        )
}

/// One row of a track list. Under the mouse the number turns into ▶; the playing track shows
/// the waves in the accent colour.
fn track_row(row: TrackRow, compact: bool, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    let group = SharedString::from(format!("track-row-{}", row.key));
    let accent = row.playing;
    div()
        .id(("track-row", row.key))
        .group(group.clone())
        .h(px(ROW_HEIGHT))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(14.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|d| d.bg(theme.hover))
        .child(
            div()
                .w(px(NUMBER_WIDTH))
                .h_full()
                .flex_none()
                .relative()
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_end()
                        .text_color(theme.muted)
                        .group_hover(group.clone(), |s| s.opacity(0.))
                        .map(|d| {
                            if accent {
                                d.child(icon("waves", 18., theme.accent))
                            } else {
                                d.child(row.number)
                            }
                        }),
                )
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_end()
                        .opacity(0.)
                        .group_hover(group, |s| s.opacity(1.))
                        .child(icon("play", 13., theme.text)),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(12.))
                .when_some(row.cover, |d, path| {
                    d.child(cover(path.as_deref(), Some(36.), 4., theme))
                })
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .truncate()
                                .when(accent, |d| {
                                    d.text_color(theme.accent_text)
                                        .font_weight(FontWeight::SEMIBOLD)
                                })
                                .child(row.title),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.muted)
                                .truncate()
                                .child(row.artist),
                        ),
                ),
        )
        .when_some(row.album.filter(|_| !compact), |d, album| {
            d.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(12.5))
                    .text_color(theme.muted)
                    .truncate()
                    .child(album),
            )
        })
        .child(
            div()
                .w(px(DURATION_WIDTH))
                .flex_none()
                .flex()
                .justify_end()
                .text_color(theme.muted)
                .text_size(px(12.5))
                .child(row.duration.unwrap_or_default()),
        )
}

// Player bar --------------------------------------------------------------------------------

impl LibraryWindow {
    fn render_player_bar(
        &self,
        theme: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let playback = self.playback.read(cx);
        let show_quality = self.settings.read(cx).value.show_quality;
        let now = playback.now();
        let total = now.and_then(|n| n.duration);
        let fraction = match self.drag {
            Drag::Seek(at) => at,
            _ => playback.progress(),
        };
        let position = match (self.drag, total) {
            (Drag::Seek(at), Some(total)) => total.mul_f32(at),
            _ => playback.position(),
        };
        let paused = playback.is_paused();
        let volume = playback.effective_volume();
        let shuffle_color = if playback.is_shuffled() {
            theme.accent_text
        } else {
            theme.muted
        };
        let (repeat_icon, repeat_color) = match playback.repeat() {
            Repeat::Off => ("repeat", theme.muted),
            Repeat::All => ("repeat", theme.accent_text),
            Repeat::One => ("repeat-one", theme.accent_text),
        };
        let (title, artist, cover_image, quality) = match now {
            Some(n) => (
                n.title.clone(),
                n.artist.clone().unwrap_or_else(|| n.subtitle.clone()),
                n.cover.clone(),
                Some(n.quality.clone()),
            ),
            None => ("Nessun brano".into(), "".into(), None, None),
        };
        let error = playback.error();
        let has_track = now.is_some();
        let seek = cx.entity().downgrade();
        let vol = cx.entity().downgrade();
        let control = |id: &'static str,
                       name: &'static str,
                       size: f32,
                       color,
                       action: Box<dyn gpui::Action>| {
            div()
                .id(id)
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .child(icon(name, size, color))
                .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
        };

        div()
            .h(px(PLAYER_BAR_HEIGHT))
            .flex_none()
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(20.))
            .bg(theme.bar)
            .border_t_1()
            .border_color(theme.hairline)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                let dy = f32::from(event.delta.pixel_delta(window.line_height()).y);
                this.playback
                    .update(cx, |p, cx| p.set_volume(p.volume() + dy / 400.0, cx));
            }))
            // Now playing.
            .child(
                div()
                    .id("now-playing")
                    .w(gpui::relative(if compact { 0.32 } else { 0.28 }))
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .when(has_track, |d| d.cursor_pointer())
                    .child(match cover_image {
                        Some(image) => div()
                            .size(px(50.))
                            .flex_none()
                            .rounded(px(6.))
                            .overflow_hidden()
                            .child(img(image).size_full().object_fit(ObjectFit::Cover))
                            .into_any_element(),
                        None => cover(None, Some(50.), 6., theme),
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.muted)
                                    .truncate()
                                    .child(match error {
                                        Some(error) => error,
                                        None => artist,
                                    }),
                            )
                            .when_some(quality.filter(|_| show_quality), |d, q| {
                                d.child(
                                    div()
                                        .text_size(px(10.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.accent_text)
                                        .child(q),
                                )
                            }),
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        if let Some(album) = this.playing_album(cx) {
                            this.go(Page::Album(album), cx);
                        }
                    })),
            )
            // Controls and progress.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(control(
                                "bar-shuffle",
                                "shuffle",
                                15.,
                                shuffle_color,
                                Box::new(ToggleShuffle),
                            ))
                            .child(control(
                                "bar-previous",
                                "previous",
                                17.,
                                theme.text,
                                Box::new(Previous),
                            ))
                            .child(
                                div()
                                    .id("bar-play")
                                    .size(px(36.))
                                    .rounded_full()
                                    .bg(theme.accent)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .child(icon(
                                        if paused { "play" } else { "pause" },
                                        15.,
                                        theme.on_accent,
                                    ))
                                    .on_click(|_, window, cx| {
                                        window.dispatch_action(Box::new(TogglePlay), cx)
                                    }),
                            )
                            .child(control("bar-next", "next", 17., theme.text, Box::new(Next)))
                            .child(control(
                                "bar-repeat",
                                repeat_icon,
                                15.,
                                repeat_color,
                                Box::new(CycleRepeat),
                            )),
                    )
                    .child(
                        div()
                            .w_full()
                            .max_w(px(520.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .text_size(px(11.))
                            .text_color(theme.muted)
                            .child(
                                div()
                                    .w(px(36.))
                                    .flex()
                                    .justify_end()
                                    .child(format_time(position)),
                            )
                            .child(
                                div().flex_1().child(slider(
                                    "bar-progress",
                                    matches!(self.drag, Drag::Seek(_)),
                                    div()
                                        .h(px(16.))
                                        .flex()
                                        .items_center()
                                        .child(knob_bar(fraction, theme)),
                                    move |event, _, cx| {
                                        let _ =
                                            seek.update(cx, |this, cx| this.slide_seek(event, cx));
                                    },
                                )),
                            )
                            .child(
                                div()
                                    .w(px(36.))
                                    .child(total.map(format_time).unwrap_or_else(|| "–:––".into())),
                            ),
                    ),
            )
            // Volume.
            .child(
                div()
                    .w(gpui::relative(if compact { 0.2 } else { 0.28 }))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(6.))
                    .child(control(
                        "bar-mute",
                        if volume == 0.0 {
                            "volume-off"
                        } else {
                            "volume"
                        },
                        15.,
                        theme.muted,
                        Box::new(ToggleMute),
                    ))
                    .child(
                        div().w(px(96.)).child(slider(
                            "bar-volume",
                            self.drag == Drag::Volume,
                            div()
                                .h(px(16.))
                                .flex()
                                .items_center()
                                .child(knob_bar(volume, theme)),
                            move |event, _, cx| {
                                let _ = vol.update(cx, |this, cx| this.slide_volume(event, cx));
                            },
                        )),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::thousands;

    #[test]
    fn thousands_use_dots() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(646), "646");
        assert_eq!(thousands(2418), "2.418");
        assert_eq!(thousands(1_234_567), "1.234.567");
    }
}
