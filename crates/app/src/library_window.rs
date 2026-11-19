//! The full app: sidebar, library pages and a Spotify-style player bar along the bottom.

use std::path::Path;

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, PathPromptOptions, Render, ScrollWheelEvent,
    SharedString, StatefulInteractiveElement, Styled, StyledImage, UniformListScrollHandle,
    WeakEntity, Window, div, img, prelude::*, px, uniform_list,
};
use lumifi_engine::format_time;

use crate::actions::*;
use crate::library::{Library, LibraryData, library};
use crate::playback::{Playback, playback};
use crate::queue::Repeat;
use crate::settings::{MiniView, SettingsModel, settings};
use crate::theme::Theme;
use crate::widgets::{SlideEvent, bar, icon, icon_button, slider};

const SIDEBAR_WIDTH: f32 = 220.0;
const CONTENT_PAD: f32 = 28.0;
const CARD_MIN: f32 = 160.0;
const CARD_GAP: f32 = 20.0;
const PLAYER_BAR_HEIGHT: f32 = 84.0;
const ROW_HEIGHT: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Albums,
    Album(usize),
    Artists,
    Artist(usize),
    Tracks,
    Settings,
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
            cx.observe(&playback, |_, _, cx| cx.notify()),
            cx.observe(&library, |_, _, cx| cx.notify()),
            cx.observe(&settings, |_, _, cx| cx.notify()),
        ];
        Self {
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
            _subscriptions: subscriptions,
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
                    this.rescan(cx);
                });
            }
        })
        .detach();
    }

    fn remove_folder(&mut self, index: usize, cx: &mut Context<Self>) {
        self.settings.update(cx, |s, cx| {
            s.update(cx, |v| {
                if index < v.folders.len() {
                    v.folders.remove(index);
                }
            })
        });
        self.rescan(cx);
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        let folders = self.settings.read(cx).value.folders.clone();
        self.library.update(cx, |l, cx| l.rescan(folders, cx));
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

impl Render for LibraryWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window.appearance());
        let content = match self.page {
            Page::Albums => self.render_albums(&theme, window, cx),
            Page::Album(index) => self.render_album(index, &theme, cx),
            Page::Artists => self.render_artists(&theme, cx),
            Page::Artist(index) => self.render_artist(index, &theme, window, cx),
            Page::Tracks => self.render_tracks(&theme, cx),
            Page::Settings => self.render_settings(&theme, cx),
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
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(&theme, cx))
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
            .child(self.render_player_bar(&theme, cx))
    }
}

// Sidebar -----------------------------------------------------------------------------------

impl LibraryWindow {
    fn render_sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let section = |label: &'static str| {
            div()
                .px(px(12.))
                .pt(px(18.))
                .pb(px(6.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted)
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
                .h(px(34.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded_md()
                .cursor_pointer()
                .when(active, |d| d.bg(theme.hover))
                .hover(|d| d.bg(theme.hover))
                .child(icon(
                    icon_name,
                    16.,
                    if active { theme.accent } else { theme.muted },
                ))
                .child(
                    div()
                        .when(active, |d| d.font_weight(FontWeight::SEMIBOLD))
                        .child(label),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.go_root(page, cx)))
        };
        let scan = self.library.read(cx).scan_progress();
        div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .px(px(10.))
            // Room for the macOS window buttons.
            .pt(px(if cfg!(target_os = "macos") { 40. } else { 16. }))
            .pb(px(12.))
            .child(
                div()
                    .px(px(8.))
                    .pb(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .size(px(28.))
                            .rounded(px(7.))
                            .bg(theme.accent)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("waves", 28., theme.on_accent)),
                    )
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .child("LumiFi"),
                    ),
            )
            .child(section("LIBRERIA"))
            .child(item("nav-albums", "album", "Album", Page::Albums))
            .child(item("nav-artists", "artist", "Artisti", Page::Artists))
            .child(item("nav-tracks", "note", "Brani", Page::Tracks))
            .child(section("PLAYLIST"))
            .child(
                div()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_color(theme.muted)
                    .child(icon("playlist", 16., theme.muted))
                    .child("In arrivo"),
            )
            .child(div().flex_1())
            .when_some(scan, |d, (read, found)| {
                d.child(
                    div()
                        .px(px(12.))
                        .pb(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .text_size(px(11.5))
                        .text_color(theme.muted)
                        .child(if found == 0 {
                            "Cerco la musica…".to_string()
                        } else {
                            format!("Leggo la libreria: {read} di {found}")
                        })
                        .child(bar(
                            if found == 0 {
                                0.0
                            } else {
                                read as f32 / found as f32
                            },
                            3.,
                            theme.track,
                            theme.accent,
                        )),
                )
            })
            .child(item(
                "nav-settings",
                "settings",
                "Impostazioni",
                Page::Settings,
            ))
    }
}

// Pages -------------------------------------------------------------------------------------

/// The big title at the top of a page, with a back arrow when there is history.
fn page_header(
    title: impl Into<SharedString>,
    subtitle: Option<String>,
    can_go_back: bool,
    theme: &Theme,
    cx: &mut Context<LibraryWindow>,
) -> AnyElement {
    div()
        .flex_none()
        .px(px(CONTENT_PAD))
        .pt(px(if cfg!(target_os = "macos") { 34. } else { 22. }))
        .pb(px(14.))
        .flex()
        .items_center()
        .gap(px(12.))
        .when(can_go_back, |d| {
            d.child(
                div()
                    .id("back")
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme.hover)
                    .cursor_pointer()
                    .child(icon("back", 16., theme.text))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.back(cx))),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(26.))
                        .font_weight(FontWeight::BOLD)
                        .child(title.into()),
                )
                .when_some(subtitle, |d, s| {
                    d.child(div().text_color(theme.muted).text_size(px(12.5)).child(s))
                }),
        )
        .into_any_element()
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
            .child(icon("waves", size.unwrap_or(120.) * 0.45, theme.muted))
            .into_any_element(),
    }
}

impl LibraryWindow {
    fn grid_columns(window: &Window) -> (usize, f32) {
        let width = f32::from(window.viewport_size().width) - SIDEBAR_WIDTH - 2.0 * CONTENT_PAD;
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
            .child(
                div()
                    .size(px(72.))
                    .rounded(px(18.))
                    .bg(theme.accent)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("waves", 72., theme.on_accent)),
            )
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
                        .child("Scegli la cartella dove tieni la musica."),
                )
                .child(
                    div()
                        .id("empty-add-folder")
                        .mt(px(4.))
                        .h(px(34.))
                        .px(px(16.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded_lg()
                        .bg(theme.accent)
                        .text_color(theme.on_accent)
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .child(icon("folder", 15., theme.on_accent))
                        .child("Aggiungi cartella…")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.add_folder(cx))),
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
        div()
            .id(("album", index))
            .w(px(card))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.))
            .cursor_pointer()
            .child(cover(album.cover.as_deref(), Some(card), 8., theme))
            .child(
                div()
                    .flex()
                    .flex_col()
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let data = self.library.read(cx).data();
        let header = page_header(
            "Album",
            Some(format!(
                "{} album · {} brani",
                data.albums.len(),
                data.tracks.len()
            )),
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
        let (columns, card) = Self::grid_columns(window);
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
                                .pb(px(CARD_GAP))
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

    fn render_album(&self, index: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let data = self.library.read(cx).data();
        let Some(album) = data.albums.get(index).cloned() else {
            return div().into_any_element();
        };
        let playing = self.playback.read(cx).current_path().map(Path::to_path_buf);
        let minutes = album.duration.as_secs().div_ceil(60);
        let summary = format!(
            "{} · {} brani · {} min",
            album.artist,
            album.tracks.len(),
            minutes
        );
        let can_go_back = !self.history.is_empty();
        div()
            .id("album-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .px(px(CONTENT_PAD))
                    .pt(px(if cfg!(target_os = "macos") { 34. } else { 22. }))
                    .when(can_go_back, |d| {
                        d.child(
                            div()
                                .id("back")
                                .mb(px(14.))
                                .size(px(30.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(theme.hover)
                                .cursor_pointer()
                                .child(icon("back", 16., theme.text))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.back(cx))),
                        )
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
                                                div()
                                                    .id("album-play")
                                                    .h(px(36.))
                                                    .px(px(18.))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .rounded_full()
                                                    .bg(theme.accent)
                                                    .text_color(theme.on_accent)
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .cursor_pointer()
                                                    .child(icon("play", 14., theme.on_accent))
                                                    .child("Riproduci")
                                                    .on_click(cx.listener(
                                                        move |this, _: &ClickEvent, _, cx| {
                                                            this.play_album(index, 0, false, cx)
                                                        },
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .id("album-shuffle")
                                                    .h(px(36.))
                                                    .px(px(18.))
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .rounded_full()
                                                    .bg(theme.hover)
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .cursor_pointer()
                                                    .child(icon("shuffle", 14., theme.text))
                                                    .child("Casuale")
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
                    .pt(px(22.))
                    .pb(px(24.))
                    .children(album.tracks.iter().enumerate().map(|(position, &t)| {
                        let track = &data.tracks[t];
                        let is_playing = playing.as_deref() == Some(track.path.as_path());
                        let number = track
                            .track_no
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| (position + 1).to_string());
                        track_row(
                            ("album-track", t),
                            number,
                            track.title.clone(),
                            None,
                            track.duration.map(format_time),
                            is_playing,
                            theme,
                        )
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.play_album(index, position, false, cx);
                            },
                        ))
                    })),
            )
            .into_any_element()
    }

    fn render_artists(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let data = self.library.read(cx).data();
        let header = page_header(
            "Artisti",
            Some(format!("{} artisti", data.artists.len())),
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
                                    .rounded_md()
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let data = self.library.read(cx).data();
        let Some(artist) = data.artists.get(index).cloned() else {
            return div().into_any_element();
        };
        let (_, card) = Self::grid_columns(window);
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

    fn render_tracks(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let data = self.library.read(cx).data();
        let header = page_header(
            "Brani",
            Some(format!("{} brani", data.tracks.len())),
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
                uniform_list("tracks", data.tracks.len(), move |range, _, _| {
                    range
                        .map(|i| {
                            let track = &data.tracks[i];
                            let this = this.clone();
                            let is_playing = playing.as_deref() == Some(track.path.as_path());
                            div().px(px(CONTENT_PAD - 10.)).child(
                                track_row(
                                    ("track", i),
                                    (i + 1).to_string(),
                                    track.title.clone(),
                                    Some(format!("{} · {}", track.artist, track.album)),
                                    track.duration.map(format_time),
                                    is_playing,
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

    fn render_settings(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let value = self.settings.read(cx).value.clone();
        let header = page_header("Impostazioni", None, false, theme, cx);
        let card = || {
            div()
                .p(px(16.))
                .rounded_lg()
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .gap(px(10.))
        };
        let heading = |text: &'static str| {
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(text)
        };
        let hint =
            |text: &'static str| div().text_size(px(12.)).text_color(theme.muted).child(text);
        let button = |id: &'static str, icon_name: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(30.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(6.))
                .rounded_md()
                .bg(theme.hover)
                .cursor_pointer()
                .child(icon(icon_name, 14., theme.text))
                .child(label)
        };
        let segment = |id: &'static str, label: &'static str, view: MiniView| {
            let active = value.mini_view == view;
            div()
                .id(id)
                .h(px(30.))
                .px(px(14.))
                .flex()
                .items_center()
                .rounded_md()
                .cursor_pointer()
                .when(active, |d| {
                    d.bg(theme.accent)
                        .text_color(theme.on_accent)
                        .font_weight(FontWeight::SEMIBOLD)
                })
                .when(!active, |d| d.bg(theme.hover))
                .child(label)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.settings
                        .update(cx, |s, cx| s.update(cx, |v| v.mini_view = view));
                }))
        };
        let on_top = value.always_on_top;

        div()
            .id("settings-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(header)
            .child(
                div()
                    .px(px(CONTENT_PAD))
                    .pb(px(28.))
                    .max_w(px(680.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        card()
                            .child(heading("Cartelle della musica"))
                            .child(hint(
                                "LumiFi legge i brani da queste cartelle e dalle loro sottocartelle.",
                            ))
                            .children(value.folders.iter().enumerate().map(|(i, folder)| {
                                div()
                                    .h(px(34.))
                                    .px(px(10.))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .rounded_md()
                                    .bg(theme.hover)
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
                                            .size(px(24.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_full()
                                            .cursor_pointer()
                                            .hover(|d| d.bg(theme.hover))
                                            .child(icon("close", 12., theme.muted))
                                            .on_click(cx.listener(
                                                move |this, _: &ClickEvent, _, cx| {
                                                    this.remove_folder(i, cx)
                                                },
                                            )),
                                    )
                            }))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(
                                        button("add-folder", "plus", "Aggiungi cartella…").on_click(
                                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                                this.add_folder(cx)
                                            }),
                                        ),
                                    )
                                    .child(button("rescan", "refresh", "Aggiorna libreria").on_click(
                                        cx.listener(|this, _: &ClickEvent, _, cx| this.rescan(cx)),
                                    )),
                            ),
                    )
                    .child(
                        card()
                            .child(heading("Mini player"))
                            .child(hint("Come il mini player mostra l'avanzamento del brano."))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(segment("view-a", "A · Barra", MiniView::A))
                                    .child(segment("view-b", "B · Forma d'onda", MiniView::B)),
                            )
                            .child(
                                div()
                                    .id("on-top")
                                    .mt(px(4.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .cursor_pointer()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .child("Sempre in primo piano")
                                            .child(hint("Il mini player resta sopra le altre finestre.")),
                                    )
                                    .child(switch(on_top, theme))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.settings.update(cx, |s, cx| {
                                            s.update(cx, |v| v.always_on_top = !v.always_on_top)
                                        });
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.muted)
                            .child(format!("LumiFi {}", env!("CARGO_PKG_VERSION"))),
                    ),
            )
            .into_any_element()
    }
}

/// An on/off switch drawn in the accent colour when on.
fn switch(on: bool, theme: &Theme) -> impl IntoElement {
    div()
        .w(px(38.))
        .h(px(22.))
        .flex_none()
        .rounded_full()
        .p(px(3.))
        .flex()
        .when(on, |d| d.bg(theme.accent).justify_end())
        .when(!on, |d| d.bg(theme.track))
        .child(div().size(px(16.)).rounded_full().bg(gpui::rgb(0xffffff)))
}

/// One row of a track list: number, title (and artist), length.
fn track_row(
    id: impl Into<gpui::ElementId>,
    number: String,
    title: SharedString,
    detail: Option<String>,
    duration: Option<String>,
    playing: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(ROW_HEIGHT + if detail.is_some() { 8. } else { 0. }))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(14.))
        .rounded_md()
        .cursor_pointer()
        .hover(|d| d.bg(theme.hover))
        .child(
            div()
                .w(px(28.))
                .flex_none()
                .flex()
                .justify_end()
                .text_color(if playing { theme.accent } else { theme.muted })
                .when(playing, |d| d.child(icon("waves", 18., theme.accent)))
                .when(!playing, |d| d.child(number)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .when(playing, |d| {
                            d.text_color(theme.accent).font_weight(FontWeight::SEMIBOLD)
                        })
                        .child(title),
                )
                .when_some(detail, |d, detail| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted)
                            .truncate()
                            .child(detail),
                    )
                }),
        )
        .child(
            div()
                .flex_none()
                .text_color(theme.muted)
                .text_size(px(12.))
                .child(duration.unwrap_or_default()),
        )
}

// Player bar --------------------------------------------------------------------------------

impl LibraryWindow {
    fn render_player_bar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let playback = self.playback.read(cx);
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
            theme.accent
        } else {
            theme.muted
        };
        let (repeat_icon, repeat_color) = match playback.repeat() {
            Repeat::Off => ("repeat", theme.muted),
            Repeat::All => ("repeat", theme.accent),
            Repeat::One => ("repeat-one", theme.accent),
        };
        let (title, artist, cover_image) = match now {
            Some(n) => (
                n.title.clone(),
                n.artist.clone().unwrap_or_else(|| n.subtitle.clone()),
                n.cover.clone(),
            ),
            None => ("Nessun brano".into(), "".into(), None),
        };
        let error = playback.error();
        let has_track = now.is_some();
        let seek = cx.entity().downgrade();
        let vol = cx.entity().downgrade();

        div()
            .h(px(PLAYER_BAR_HEIGHT))
            .flex_none()
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(16.))
            .bg(theme.sidebar)
            .border_t_1()
            .border_color(theme.border)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                let dy = f32::from(event.delta.pixel_delta(window.line_height()).y);
                this.playback
                    .update(cx, |p, cx| p.set_volume(p.volume() + dy / 400.0, cx));
            }))
            // Now playing.
            .child(
                div()
                    .id("now-playing")
                    .w(gpui::relative(0.28))
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .when(has_track, |d| d.cursor_pointer())
                    .child(match cover_image {
                        Some(image) => div()
                            .size(px(54.))
                            .flex_none()
                            .rounded(px(6.))
                            .overflow_hidden()
                            .child(img(image).size_full().object_fit(ObjectFit::Cover))
                            .into_any_element(),
                        None => cover(None, Some(54.), 6., theme),
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
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
                            ),
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
                            .gap(px(14.))
                            .child(icon_button(
                                "bar-shuffle",
                                "shuffle",
                                16.,
                                30.,
                                shuffle_color,
                                ToggleShuffle,
                            ))
                            .child(icon_button(
                                "bar-previous",
                                "previous",
                                18.,
                                30.,
                                theme.text,
                                Previous,
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
                            .child(icon_button("bar-next", "next", 18., 30., theme.text, Next))
                            .child(icon_button(
                                "bar-repeat",
                                repeat_icon,
                                16.,
                                30.,
                                repeat_color,
                                CycleRepeat,
                            )),
                    )
                    .child(
                        div()
                            .w_full()
                            .max_w(px(620.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .text_size(px(11.))
                            .text_color(theme.muted)
                            .child(
                                div()
                                    .w(px(40.))
                                    .flex()
                                    .justify_end()
                                    .child(format_time(position)),
                            )
                            .child(div().flex_1().child(slider(
                                "bar-progress",
                                matches!(self.drag, Drag::Seek(_)),
                                div().h(px(14.)).flex().items_center().child(bar(
                                    fraction,
                                    4.,
                                    theme.track,
                                    theme.text,
                                )),
                                move |event, _, cx| {
                                    let _ = seek.update(cx, |this, cx| this.slide_seek(event, cx));
                                },
                            )))
                            .child(
                                div()
                                    .w(px(40.))
                                    .child(total.map(format_time).unwrap_or_else(|| "–:––".into())),
                            ),
                    ),
            )
            // Volume and the mini player.
            .child(
                div()
                    .w(gpui::relative(0.28))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(10.))
                    .child(icon_button(
                        "bar-mute",
                        if volume == 0.0 {
                            "volume-off"
                        } else {
                            "volume"
                        },
                        16.,
                        28.,
                        theme.muted,
                        ToggleMute,
                    ))
                    .child(div().w(px(110.)).child(slider(
                        "bar-volume",
                        self.drag == Drag::Volume,
                        div().h(px(14.)).flex().items_center().child(bar(
                            volume,
                            4.,
                            theme.track,
                            theme.text,
                        )),
                        move |event, _, cx| {
                            let _ = vol.update(cx, |this, cx| this.slide_volume(event, cx));
                        },
                    )))
                    .child(icon_button(
                        "bar-mini",
                        "minimize",
                        15.,
                        28.,
                        theme.muted,
                        ToggleLibrary,
                    )),
            )
    }
}
