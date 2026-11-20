//! The pages of the library: albums, an album, artists, an artist, all tracks, a playlist,
//! search results and "In riproduzione" with the lyrics.

use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, MouseButton,
    MouseUpEvent, ObjectFit, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    StyledImage, WeakEntity, Window, div, img, prelude::*, px, uniform_list,
};
use lumifi_engine::format_time;

use super::parts::{
    TrackRow, cover, label, play_dot, row_actions, table_header, thousands, track_row,
};
use super::{
    CONTENT_PAD, LibraryWindow, Page, QUEUE_WIDTH, SIDEBAR_COMPACT, SIDEBAR_WIDTH, SearchMore,
};
use crate::library::LibraryData;
use crate::lyrics::{Lyrics, LyricsState, current_line};
use crate::playlists::FAVORITES;
use crate::settings_page::Section;
use crate::theme::Theme;
use crate::widgets::{icon, logo};

const CARD_MIN: f32 = 150.0;
const CARD_GAP: f32 = 18.0;

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

fn pill(
    id: &'static str,
    icon_name: &'static str,
    label: &'static str,
    accent: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
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
}

impl LibraryWindow {
    fn content_width(&self, window: &Window, compact: bool) -> f32 {
        let sidebar = if compact {
            SIDEBAR_COMPACT
        } else {
            SIDEBAR_WIDTH
        };
        let queue = if self.queue_open { QUEUE_WIDTH } else { 0.0 };
        f32::from(window.viewport_size().width) - sidebar - queue - 2.0 * CONTENT_PAD
    }

    fn grid_columns(&self, window: &Window, compact: bool) -> (usize, f32) {
        let width = self.content_width(window, compact);
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
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.show_settings(Some(Section::Library), cx)
                        })),
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
        let menu = this.clone();
        let paths = data.album_paths(index);
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
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
            .on_mouse_up(MouseButton::Right, move |event: &MouseUpEvent, _, cx| {
                let paths = paths.clone();
                let position = event.position;
                let _ = menu.update(cx, |this, cx| this.track_menu(paths, position, None, cx));
            })
            .into_any_element()
    }

    pub(super) fn render_albums(
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
        let (columns, card) = self.grid_columns(window, compact);
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

    /// A list of tracks as rows, with clicks (play or pause) and right-clicks (menu). `play`
    /// receives the row's index in `paths`.
    #[allow(clippy::too_many_arguments)]
    fn track_rows(
        &self,
        key: &str,
        paths: &[PathBuf],
        numbers: Option<Vec<String>>,
        with_album: bool,
        with_cover: bool,
        in_playlist: Option<String>,
        compact: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
        play: impl Fn(&mut Self, usize, &mut Context<Self>) + Clone + 'static,
    ) -> Vec<AnyElement> {
        let data = self.library.read(cx).data();
        let entity = cx.entity().downgrade();
        paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let info = label(&data, path);
                let row_key: SharedString = format!("{key}-{i}").into();
                let favorite = self.playlists.read(cx).is_favorite(path);
                let actions = row_actions(
                    &row_key,
                    path.clone(),
                    favorite,
                    in_playlist.clone(),
                    entity.clone(),
                    theme,
                );
                let (current, playing) = self.row_state(path, cx);
                let number = numbers
                    .as_ref()
                    .and_then(|n| n.get(i).cloned())
                    .unwrap_or_else(|| (i + 1).to_string());
                let click_path = path.clone();
                let menu_path = path.clone();
                let in_playlist = in_playlist.clone();
                let play = play.clone();
                track_row(
                    TrackRow {
                        key: row_key,
                        number,
                        title: info.title,
                        artist: info.artist,
                        album: with_album.then_some(info.album),
                        cover: with_cover.then_some(info.cover),
                        duration: info.duration,
                        current,
                        playing,
                        actions: Some(actions),
                    },
                    compact,
                    theme,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let play = play.clone();
                    this.click_track(&click_path, cx, move |this, cx| play(this, i, cx));
                }))
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                        this.track_menu(
                            vec![menu_path.clone()],
                            event.position,
                            in_playlist.clone(),
                            cx,
                        )
                    }),
                )
                .into_any_element()
            })
            .collect()
    }

    pub(super) fn render_album(
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
        let minutes = album.duration.as_secs().div_ceil(60);
        let songs = album.tracks.len();
        let summary = format!(
            "{} · {} {} · {} min",
            album.artist,
            songs,
            if songs == 1 { "brano" } else { "brani" },
            minutes
        );
        let paths: Vec<PathBuf> = album
            .tracks
            .iter()
            .map(|&t| data.tracks[t].path.clone())
            .collect();
        let numbers: Vec<String> = album
            .tracks
            .iter()
            .enumerate()
            .map(|(p, &t)| {
                data.tracks[t]
                    .track_no
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| (p + 1).to_string())
            })
            .collect();
        let rows = self.track_rows(
            "album-track",
            &paths,
            Some(numbers),
            false,
            false,
            None,
            compact,
            theme,
            cx,
            move |this, i, cx| this.play_album(index, i, false, cx),
        );
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
                                                pill(
                                                    "album-play",
                                                    "play",
                                                    "Riproduci",
                                                    true,
                                                    theme,
                                                )
                                                .on_click(cx.listener(
                                                    move |this, _: &ClickEvent, _, cx| {
                                                        this.play_album(index, 0, false, cx)
                                                    },
                                                )),
                                            )
                                            .child(
                                                pill(
                                                    "album-shuffle",
                                                    "shuffle",
                                                    "Casuale",
                                                    false,
                                                    theme,
                                                )
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
                    .children(rows),
            )
            .into_any_element()
    }

    pub(super) fn render_artists(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
                        .map(|i| artist_row(&data, i, &theme, this.clone()))
                        .collect()
                })
                .track_scroll(self.artists_scroll.clone())
                .flex_1(),
            )
            .into_any_element()
    }

    pub(super) fn render_artist(
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
        let (_, card) = self.grid_columns(window, compact);
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

    pub(super) fn render_tracks(
        &self,
        theme: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let data = self.library.read(cx).data();
        let header = page_header(
            "Brani",
            Some(thousands(data.tracks.len())),
            false,
            theme,
            cx,
        );
        let all_paths = |this: &mut Self, cx: &mut Context<Self>| -> Vec<std::path::PathBuf> {
            let data = this.library.read(cx).data();
            data.tracks.iter().map(|t| t.path.clone()).collect()
        };
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .pr(px(CONTENT_PAD))
            .child(header)
            .when(!data.tracks.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(10.))
                        .child(
                            pill("tracks-play", "play", "Riproduci", true, theme).on_click(
                                cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    let paths = all_paths(this, cx);
                                    this.play_paths(paths, 0, false, cx)
                                }),
                            ),
                        )
                        .child(
                            pill("tracks-shuffle", "shuffle", "Casuale", false, theme).on_click(
                                cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    let paths = all_paths(this, cx);
                                    // A different first track each time.
                                    let start = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.subsec_nanos() as usize)
                                        .unwrap_or(0)
                                        % paths.len().max(1);
                                    this.play_paths(paths, start, true, cx)
                                }),
                            ),
                        ),
                )
            });
        if data.tracks.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .child(header)
                .child(self.empty_state(theme, cx))
                .into_any_element();
        }
        let (current_path, playing) = {
            let p = self.playback.read(cx);
            (p.current_path().map(Path::to_path_buf), !p.is_paused())
        };
        let this = cx.entity().downgrade();
        let theme = *theme;
        let favorites: std::collections::HashSet<std::path::PathBuf> = self
            .playlists
            .read(cx)
            .get(crate::playlists::FAVORITES)
            .map(|p| p.tracks.iter().cloned().collect())
            .unwrap_or_default();
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
                            let menu = this.clone();
                            let path = track.path.clone();
                            let menu_path = track.path.clone();
                            let album = &data.albums[track.album_index];
                            let current = current_path.as_deref() == Some(track.path.as_path());
                            let row_key: SharedString = format!("track-{i}").into();
                            let actions = row_actions(
                                &row_key,
                                track.path.clone(),
                                favorites.contains(&track.path),
                                None,
                                this.clone(),
                                &theme,
                            );
                            div().px(px(CONTENT_PAD - 10.)).child(
                                track_row(
                                    TrackRow {
                                        key: row_key,
                                        number: (i + 1).to_string(),
                                        title: track.title.clone(),
                                        artist: track.artist.clone(),
                                        album: Some(track.album.clone()),
                                        cover: Some(album.cover.clone()),
                                        duration: track.duration.map(format_time),
                                        current,
                                        playing: current && playing,
                                        actions: Some(actions),
                                    },
                                    compact,
                                    &theme,
                                )
                                .on_click(move |_, _, cx| {
                                    let _ = this.update(cx, |this, cx| {
                                        this.click_track(&path, cx, |this, cx| {
                                            let data = this.library.read(cx).data();
                                            let paths = data
                                                .tracks
                                                .iter()
                                                .map(|t| t.path.clone())
                                                .collect();
                                            this.play_paths(paths, i, false, cx)
                                        })
                                    });
                                })
                                .on_mouse_up(
                                    MouseButton::Right,
                                    move |event: &MouseUpEvent, _, cx| {
                                        let position = event.position;
                                        let p = menu_path.clone();
                                        let _ = menu.update(cx, |this, cx| {
                                            this.track_menu(vec![p], position, None, cx)
                                        });
                                    },
                                ),
                            )
                        })
                        .collect()
                })
                .track_scroll(self.tracks_scroll.clone())
                .flex_1(),
            )
            .into_any_element()
    }

    pub(super) fn render_playlist(
        &self,
        name: &str,
        theme: &Theme,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(list) = self.playlists.read(cx).get(name).cloned() else {
            return div().into_any_element();
        };
        let favorites = list.is_favorites();
        let count = list.tracks.len();
        let paths = list.tracks.clone();
        let list_name = list.name.clone();
        let renaming = self
            .renaming
            .as_ref()
            .filter(|(n, _)| *n == list.name)
            .map(|(_, input)| input.clone());
        let rows = self.track_rows(
            "playlist-track",
            &paths,
            None,
            true,
            true,
            Some(list_name.clone()),
            compact,
            theme,
            cx,
            {
                let paths = paths.clone();
                move |this, i, cx| this.play_paths(paths.clone(), i, false, cx)
            },
        );
        let play_paths = paths.clone();
        let shuffle_paths = paths.clone();
        div()
            .id("playlist-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .px(px(CONTENT_PAD))
                    .pt(px(22.))
                    .flex()
                    .items_end()
                    .gap(px(22.))
                    .child(
                        div()
                            .size(px(150.))
                            .flex_none()
                            .rounded(px(10.))
                            .bg(if favorites { theme.accent } else { theme.surface })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(
                                if favorites { "heart-filled" } else { "playlist" },
                                56.,
                                if favorites { theme.on_accent } else { theme.faint },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.muted)
                                    .child("PLAYLIST"),
                            )
                            .map(|d| match renaming {
                                Some(input) => d.child(
                                    div()
                                        .h(px(44.))
                                        .px(px(10.))
                                        .flex()
                                        .items_center()
                                        .rounded(px(8.))
                                        .bg(theme.field)
                                        .border_1()
                                        .border_color(theme.accent)
                                        .text_size(px(26.))
                                        .font_weight(FontWeight::BOLD)
                                        .child(input),
                                ),
                                None => d.child(
                                    div()
                                        .id("playlist-title")
                                        .text_size(px(30.))
                                        .font_weight(FontWeight::BOLD)
                                        .truncate()
                                        .when(!favorites, |d| d.cursor_pointer())
                                        .child(list.name.clone())
                                        .on_click(cx.listener(
                                            move |this, event: &ClickEvent, window, cx| {
                                                if !favorites && event.click_count() >= 2 {
                                                    this.start_rename(list_name.clone(), window, cx)
                                                }
                                            },
                                        )),
                                ),
                            })
                            .child(div().text_color(theme.muted).child(format!(
                                "{count} {}",
                                if count == 1 { "brano" } else { "brani" }
                            )))
                            .when(count > 0, |d| {
                                d.child(
                                    div()
                                        .mt(px(10.))
                                        .flex()
                                        .gap(px(10.))
                                        .child(
                                            pill("playlist-play", "play", "Riproduci", true, theme)
                                                .on_click(cx.listener(
                                                    move |this, _: &ClickEvent, _, cx| {
                                                        this.play_paths(play_paths.clone(), 0, false, cx)
                                                    },
                                                )),
                                        )
                                        .child(
                                            pill(
                                                "playlist-shuffle",
                                                "shuffle",
                                                "Casuale",
                                                false,
                                                theme,
                                            )
                                            .on_click(cx.listener(
                                                move |this, _: &ClickEvent, _, cx| {
                                                    this.play_paths(
                                                        shuffle_paths.clone(),
                                                        0,
                                                        true,
                                                        cx,
                                                    )
                                                },
                                            )),
                                        ),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .px(px(CONTENT_PAD - 10.))
                    .pt(px(24.))
                    .pb(px(24.))
                    .when(count > 0, |d| d.child(table_header(true, compact, theme)))
                    .when(count == 0, |d| {
                        d.child(
                            div()
                                .px(px(10.))
                                .py(px(20.))
                                .text_color(theme.muted)
                                .child(if name == FAVORITES {
                                    "Tocca il cuore accanto a un brano, o tasto destro › Aggiungi ai Preferiti."
                                } else {
                                    "Tasto destro su un brano o un album › Aggiungi a questa playlist."
                                }),
                        )
                    })
                    .children(rows),
            )
            .into_any_element()
    }

    pub(super) fn render_search(
        &self,
        theme: &Theme,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        const TOP: usize = 5;
        let query = self.search.read(cx).text().to_owned();
        let data = self.library.read(cx).data();
        let found = data.search(&query);
        let words: Vec<String> = crate::library::fold(&query)
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        let playlists: Vec<(String, usize, Option<PathBuf>)> = self
            .playlists
            .read(cx)
            .all()
            .iter()
            .filter(|p| {
                let name = crate::library::fold(&p.name);
                !words.is_empty() && words.iter().all(|w| name.contains(w.as_str()))
            })
            .map(|p| {
                // The cover of the playlist's first track that has one.
                let cover = p.tracks.iter().find_map(|t| {
                    data.by_path
                        .get(t)
                        .and_then(|&i| data.albums[data.tracks[i].album_index].cover.clone())
                });
                (p.name.clone(), p.tracks.len(), cover)
            })
            .collect();
        let more = self.search_more;
        let header = page_header(
            format!("Risultati per «{}»", query.trim()),
            None,
            more.is_some(),
            theme,
            cx,
        );
        if found.is_empty() && playlists.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .child(header)
                .child(
                    div()
                        .px(px(CONTENT_PAD))
                        .text_color(theme.muted)
                        .child("Nessun brano, artista, album o playlist corrisponde."),
                )
                .into_any_element();
        }
        let (_, card) = self.grid_columns(window, compact);
        let card = card.min(150.);
        let this = cx.entity().downgrade();
        let show = |section: SearchMore, total: usize| -> usize {
            match more {
                Some(m) if m == section => total,
                Some(_) => 0,
                None => total.min(TOP),
            }
        };

        let all_tracks: Vec<PathBuf> = found
            .tracks
            .iter()
            .map(|&i| data.tracks[i].path.clone())
            .collect();
        let shown_tracks = show(SearchMore::Tracks, all_tracks.len());
        let rows = self.track_rows(
            "search-track",
            &all_tracks[..shown_tracks],
            None,
            true,
            true,
            None,
            compact,
            theme,
            cx,
            {
                let paths = all_tracks.clone();
                move |this, i, cx| this.play_paths(paths.clone(), i, false, cx)
            },
        );
        let section = |label: &'static str,
                       id: &'static str,
                       total: usize,
                       target: SearchMore,
                       cx: &mut Context<Self>| {
            div()
                .px(px(CONTENT_PAD))
                .pt(px(8.))
                .pb(px(10.))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::BOLD)
                        .child(label),
                )
                .when(more.is_none() && total > TOP, |d| {
                    d.child(
                        div()
                            .id(id)
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted)
                            .cursor_pointer()
                            .hover(|d| d.text_color(theme.text))
                            .child(format!("Mostra tutti ({total})"))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.search_more = Some(target);
                                cx.notify();
                            })),
                    )
                })
        };

        let shown_artists = show(SearchMore::Artists, found.artists.len());
        let shown_albums = show(SearchMore::Collections, found.albums.len());
        let shown_playlists = show(SearchMore::Collections, playlists.len());
        let collections = found.albums.len() + playlists.len();
        let playlist_card = |(name, count, cover_path): (String, usize, Option<PathBuf>)| {
            let open = this.clone();
            let target = name.clone();
            let favorites = name == crate::playlists::FAVORITES;
            div()
                .id(SharedString::from(format!("search-playlist-{name}")))
                .w(px(card))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(8.))
                .cursor_pointer()
                .child(if favorites || cover_path.is_none() {
                    div()
                        .size(px(card))
                        .rounded(px(8.))
                        .bg(theme.accent_soft)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(
                            if favorites {
                                "heart-filled"
                            } else {
                                "playlist"
                            },
                            card * 0.3,
                            theme.accent_text,
                        ))
                        .into_any_element()
                } else {
                    cover(cover_path.as_deref(), Some(card), 8., theme).into_any_element()
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.muted)
                                .truncate()
                                .child(format!(
                                    "Playlist · {count} {}",
                                    if count == 1 { "brano" } else { "brani" }
                                )),
                        ),
                )
                .on_click(move |_, _, cx| {
                    let target = target.clone();
                    let _ = open.update(cx, |this, cx| this.go(Page::Playlist(target), cx));
                })
        };

        div()
            .id("search-page")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(px(24.))
            .child(header)
            .when(shown_tracks > 0, |d| {
                d.child(section(
                    "Brani",
                    "more-tracks",
                    all_tracks.len(),
                    SearchMore::Tracks,
                    cx,
                ))
                .child(
                    div()
                        .px(px(CONTENT_PAD - 10.))
                        .pb(px(14.))
                        .child(table_header(true, compact, theme))
                        .children(rows),
                )
            })
            .when(shown_artists > 0, |d| {
                d.child(section(
                    "Artisti",
                    "more-artists",
                    found.artists.len(),
                    SearchMore::Artists,
                    cx,
                ))
                .child(
                    div().pb(px(14.)).children(
                        found.artists[..shown_artists]
                            .iter()
                            .map(|&i| artist_row(&data, i, theme, this.clone())),
                    ),
                )
            })
            .when(shown_albums + shown_playlists > 0, |d| {
                d.child(section(
                    "Album e playlist",
                    "more-collections",
                    collections,
                    SearchMore::Collections,
                    cx,
                ))
                .child(
                    div()
                        .px(px(CONTENT_PAD))
                        .pb(px(18.))
                        .flex()
                        .flex_wrap()
                        .gap(px(CARD_GAP))
                        .children(
                            found.albums[..shown_albums]
                                .iter()
                                .map(|&a| Self::album_card(&data, a, card, theme, this.clone())),
                        )
                        .children(
                            playlists
                                .into_iter()
                                .take(shown_playlists)
                                .map(playlist_card),
                        ),
                )
            })
            .into_any_element()
    }

    /// "In riproduzione": the big cover, the title, and the lyrics (or what comes next).
    pub(super) fn render_now_playing(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let playback = self.playback.read(cx);
        let Some(now) = playback.now() else {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.muted)
                .child("Niente in riproduzione.")
                .into_any_element();
        };
        let title = now.title.clone();
        let artist = now.artist.clone().unwrap_or_else(|| now.subtitle.clone());
        let album = now.album.clone();
        let cover_image = now.cover.clone();
        let position = playback.position();
        let (queue, at) = playback.upcoming();
        let kicker = match &album {
            Some(album) => format!(
                "{} · BRANO {} DI {}",
                album.to_uppercase(),
                at + 1,
                queue.len()
            ),
            None => format!("BRANO {} DI {}", at + 1, queue.len()),
        };
        let lyrics_state = self.lyrics.read(cx).state.clone();
        let lyrics_view: AnyElement = match lyrics_state {
            LyricsState::Found(Lyrics::Synced(lines)) => {
                let current = current_line(&lines, position);
                if current != self.last_lyric_line {
                    self.last_lyric_line = current;
                    if let Some(line) = current {
                        // Keep a couple of lines above the current one in view.
                        self.lyrics_scroll.scroll_to_item(line.saturating_sub(2));
                    }
                }
                div()
                    .id("lyrics")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.lyrics_scroll)
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .children(lines.into_iter().enumerate().map(|(i, (time, text))| {
                        let is_current = Some(i) == current;
                        let past = current.is_some_and(|c| i < c);
                        div()
                            .id(("lyric", i))
                            .text_size(px(22.))
                            .line_height(px(30.))
                            .font_weight(FontWeight::BOLD)
                            .cursor_pointer()
                            .text_color(if is_current { theme.text } else { theme.muted })
                            .when(past, |d| d.opacity(0.45))
                            .child(if text.is_empty() {
                                "♪".to_owned()
                            } else {
                                text
                            })
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.playback.update(cx, |p, cx| p.seek_to(time, cx))
                            }))
                    }))
                    .into_any_element()
            }
            LyricsState::Found(Lyrics::Plain(text)) => div()
                .id("lyrics")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .text_size(px(17.))
                .line_height(px(27.))
                .text_color(theme.muted)
                .children(
                    text.lines()
                        .map(|l| div().child(l.to_owned()))
                        .collect::<Vec<_>>(),
                )
                .into_any_element(),
            state => {
                let data = self.library.read(cx).data();
                let message = match state {
                    LyricsState::Loading => "Cerco il testo…",
                    _ => "Testo non disponibile per questo brano.",
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(div().text_color(theme.muted).child(message))
                    .when(queue.len() > at + 1, |d| {
                        d.child(
                            div()
                                .pt(px(10.))
                                .border_t_1()
                                .border_color(theme.hairline)
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .pt(px(6.))
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.muted)
                                        .child("PROSSIMI"),
                                )
                                .children(queue.iter().skip(at + 1).take(5).map(|path| {
                                    let info = label(&data, path);
                                    div()
                                        .py(px(4.))
                                        .flex()
                                        .justify_between()
                                        .gap(px(12.))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .child(format!("{} · {}", info.title, info.artist)),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_color(theme.muted)
                                                .child(info.duration.unwrap_or_default()),
                                        )
                                })),
                        )
                    })
                    .into_any_element()
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .gap(px(48.))
            .px(px(56.))
            .py(px(28.))
            .child(
                div()
                    .size(px(380.))
                    .flex_none()
                    .rounded(px(14.))
                    .overflow_hidden()
                    .shadow_lg()
                    .map(|d| match cover_image {
                        Some(image) => d.child(img(image).size_full().object_fit(ObjectFit::Cover)),
                        None => d.child(cover(None, Some(380.), 14., theme)),
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .max_w(px(620.))
                    .flex()
                    .flex_col()
                    .gap(px(22.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.muted)
                                    .truncate()
                                    .child(kicker),
                            )
                            .child(
                                div()
                                    .text_size(px(36.))
                                    .line_height(px(42.))
                                    .font_weight(FontWeight::BOLD)
                                    .line_clamp(2)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .text_color(theme.muted)
                                    .child(artist),
                            ),
                    )
                    .child(lyrics_view),
            )
            .into_any_element()
    }
}

fn artist_row(
    data: &LibraryData,
    i: usize,
    theme: &Theme,
    this: WeakEntity<LibraryWindow>,
) -> AnyElement {
    let artist = &data.artists[i];
    let albums = artist.albums.len();
    div()
        .px(px(CONTENT_PAD - 10.))
        .child(
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
                .child(cover(artist.cover.as_deref(), Some(44.), 22., theme))
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
                                .child(format!("{albums} album · {} brani", artist.track_count)),
                        ),
                )
                .on_click(move |_, _, cx| {
                    let _ = this.update(cx, |this, cx| this.go(Page::Artist(i), cx));
                }),
        )
        .into_any_element()
}
