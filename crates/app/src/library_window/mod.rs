//! The library, where LumiFi opens, as in the approved design: a header that is also the window's
//! title bar (logo, search, mini player, settings, window buttons), the sidebar, the pages, an
//! optional queue on the right, and a Spotify-style player bar along the bottom.

mod overlays;
mod pages;
mod parts;
mod queue_panel;

use std::path::{Path, PathBuf};

use gpui::{
    App, ClickEvent, Context, Entity, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ObjectFit, ParentElement, Pixels, Point, Render, ScrollHandle,
    ScrollWheelEvent, SharedString, StatefulInteractiveElement, Styled, StyledImage,
    UniformListScrollHandle, Window, div, img, prelude::*, px,
};
use lumifi_engine::format_time;

use crate::actions::*;
use crate::library::{Library, library};
use crate::lyrics::{LyricsModel, lyrics};
use crate::platform;
use crate::playback::{Playback, playback};
use crate::playlists::{FAVORITES, Playlists, playlists};
use crate::queue::Repeat;
use crate::settings::{SettingsModel, settings};
use crate::settings_page::{BackToLibrary, Section, SettingsPage};
use crate::text_input::{InputEvent, TextInput};
use crate::theme::Theme;
use crate::updates::{Updates, updates};
use crate::widgets::{SlideEvent, icon, logo, slider};
use parts::{Menu, MenuItem, knob_bar, plain_bar, thousands};

const HEADER_HEIGHT: f32 = 52.0;
const SIDEBAR_WIDTH: f32 = 232.0;
/// Below this window width the sidebar shows icons only and the album column goes away.
const COMPACT_WIDTH: f32 = 960.0;
const SIDEBAR_COMPACT: f32 = 64.0;
const CONTENT_PAD: f32 = 28.0;
const PLAYER_BAR_HEIGHT: f32 = 76.0;
const QUEUE_WIDTH: f32 = 320.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Albums,
    Album(usize),
    Artists,
    Artist(usize),
    Tracks,
    Playlist(String),
    Search,
    NowPlaying,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Drag {
    None,
    Volume,
    Seek(f32),
}

pub struct LibraryWindow {
    /// Holds the keyboard for the player keys (space, arrows…) whenever the search field does
    /// not.
    body_focus: FocusHandle,
    playback: Entity<Playback>,
    library: Entity<Library>,
    settings: Entity<SettingsModel>,
    playlists: Entity<Playlists>,
    lyrics: Entity<LyricsModel>,
    updates: Entity<Updates>,
    page: Page,
    /// Pages to go back to.
    history: Vec<Page>,
    drag: Drag,
    albums_scroll: UniformListScrollHandle,
    artists_scroll: UniformListScrollHandle,
    tracks_scroll: UniformListScrollHandle,
    queue_scroll: UniformListScrollHandle,
    lyrics_scroll: ScrollHandle,
    last_lyric_line: Option<usize>,
    /// The title last given to the window (the playing track, for the taskbar).
    window_title: String,
    search: Entity<TextInput>,
    settings_page: Entity<SettingsPage>,
    queue_open: bool,
    menu: Option<Menu>,
    /// A playlist being renamed, with its name field.
    renaming: Option<(String, Entity<TextInput>)>,
    /// The welcome's step, while it shows.
    welcome: Option<usize>,
    _subscriptions: Vec<gpui::Subscription>,
}

impl LibraryWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let body_focus = cx.focus_handle();
        window.focus(&body_focus);
        let playback = playback(cx);
        let library = library(cx);
        let settings = settings(cx);
        let playlists = playlists(cx);
        let lyrics = lyrics(cx);
        let updates = updates(cx);
        let search = cx.new(|cx| TextInput::new("Cerca brani, album, artisti", cx));
        let settings_page = cx.new(|cx| SettingsPage::new(Section::General, window, cx));
        let welcome = (!settings.read(cx).value.welcome_done).then_some(0);
        let subscriptions = vec![
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            cx.observe_in(&playback, window, |this, _, window, cx| {
                this.update_title(window, cx);
                cx.notify();
            }),
            cx.observe(&library, |_, _, cx| cx.notify()),
            cx.observe(&settings, |_, _, cx| cx.notify()),
            cx.observe(&playlists, |_, _, cx| cx.notify()),
            cx.observe(&lyrics, |this, _, cx| {
                this.last_lyric_line = None;
                cx.notify();
            }),
            cx.observe(&updates, |_, _, cx| cx.notify()),
            cx.subscribe_in(&search, window, Self::on_search_event),
            cx.subscribe(&settings_page, |this, _, _: &BackToLibrary, cx| {
                this.back(cx);
            }),
            cx.observe_window_bounds(window, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            body_focus,
            playback,
            library,
            settings,
            playlists,
            lyrics,
            updates,
            page: Page::Albums,
            history: Vec::new(),
            drag: Drag::None,
            albums_scroll: UniformListScrollHandle::new(),
            artists_scroll: UniformListScrollHandle::new(),
            tracks_scroll: UniformListScrollHandle::new(),
            queue_scroll: UniformListScrollHandle::new(),
            lyrics_scroll: ScrollHandle::new(),
            last_lyric_line: None,
            window_title: String::new(),
            search,
            settings_page,
            queue_open: false,
            menu: None,
            renaming: None,
            welcome,
            _subscriptions: subscriptions,
        };
        this.update_title(window, cx);
        this
    }

    /// "Title – Artist" while something is loaded, so the taskbar shows the track.
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

    // Navigation ------------------------------------------------------------------------------

    fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        if page != self.page {
            let previous = std::mem::replace(&mut self.page, page);
            if previous != Page::Search {
                self.history.push(previous);
            }
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
        self.page = self.history.pop().unwrap_or(Page::Albums);
        cx.notify();
    }

    pub fn show_settings(&mut self, section: Option<Section>, cx: &mut Context<Self>) {
        if let Some(section) = section {
            self.settings_page.update(cx, |p, cx| p.show(section, cx));
        }
        self.go(Page::Settings, cx);
    }

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.search.read(cx).focus_handle_ref().clone();
        window.focus(&focus);
        self.search.update(cx, |s, cx| s.select_all_text(cx));
    }

    fn on_search_event(
        &mut self,
        input: &Entity<TextInput>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Changed => {
                let empty = input.read(cx).text().trim().is_empty();
                if empty {
                    if self.page == Page::Search {
                        self.back(cx);
                    }
                } else if self.page != Page::Search {
                    self.go(Page::Search, cx);
                }
                cx.notify();
            }
            InputEvent::Submit => {
                // Enter plays the first track found.
                let query = input.read(cx).text().to_owned();
                let data = self.library.read(cx).data();
                let found = data.search(&query);
                if !found.tracks.is_empty() {
                    let paths = found
                        .tracks
                        .iter()
                        .map(|&i| data.tracks[i].path.clone())
                        .collect();
                    self.playback.update(cx, |p, cx| p.play_list(paths, 0, cx));
                }
            }
            InputEvent::Cancel => {
                input.update(cx, |i, cx| i.set_text("", cx));
                window.focus(&self.body_focus);
            }
        }
    }

    // Playing ---------------------------------------------------------------------------------

    fn play_album(&mut self, album: usize, start: usize, shuffle: bool, cx: &mut Context<Self>) {
        let paths = self.library.read(cx).data().album_paths(album);
        self.play_paths(paths, start, shuffle, cx);
    }

    fn play_paths(
        &mut self,
        paths: Vec<PathBuf>,
        start: usize,
        shuffle: bool,
        cx: &mut Context<Self>,
    ) {
        self.playback.update(cx, |p, cx| {
            if p.is_shuffled() != shuffle {
                p.toggle_shuffle(cx);
            }
            p.play_list(paths, start, cx);
        });
    }

    /// A click on a track: the one already loaded pauses or resumes, any other starts.
    fn click_track(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
        start: impl FnOnce(&mut Self, &mut Context<Self>),
    ) {
        if self.playback.read(cx).current_path() == Some(path) {
            self.playback.update(cx, |p, cx| p.toggle_play(cx));
        } else {
            start(self, cx);
        }
    }

    fn row_state(&self, path: &Path, cx: &App) -> (bool, bool) {
        let playback = self.playback.read(cx);
        let current = playback.current_path() == Some(path);
        (current, current && !playback.is_paused())
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

    /// The album the playing track belongs to.
    fn playing_album(&self, cx: &App) -> Option<usize> {
        let path = self.playback.read(cx).current_path()?.to_path_buf();
        let data = self.library.read(cx).data();
        data.by_path.get(&path).map(|&i| data.tracks[i].album_index)
    }

    // Menus -----------------------------------------------------------------------------------

    fn open_menu(&mut self, position: Point<Pixels>, items: Vec<MenuItem>, cx: &mut Context<Self>) {
        self.menu = Some(Menu { position, items });
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Right-click on one or more tracks. `in_playlist`: the playlist page they are shown on.
    fn track_menu(
        &mut self,
        paths: Vec<PathBuf>,
        position: Point<Pixels>,
        in_playlist: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let mut items = Vec::new();
        let p = paths.clone();
        items.push(MenuItem::new(
            "Riproduci dopo",
            Some("play-next"),
            move |this, _, cx| {
                let p = p.clone();
                this.playback.update(cx, |pb, cx| pb.play_next(p, cx));
            },
        ));
        let p = paths.clone();
        items.push(MenuItem::new(
            "Aggiungi alla coda",
            Some("queue"),
            move |this, _, cx| {
                let p = p.clone();
                this.playback.update(cx, |pb, cx| pb.add_to_queue(p, cx));
            },
        ));
        let lists = self.playlists.read(cx);
        let single = (paths.len() == 1).then(|| paths[0].clone());
        let favorite = single.as_ref().is_some_and(|p| lists.is_favorite(p));
        let p = paths.clone();
        items.push(
            MenuItem::new(
                if favorite {
                    "Togli dai Preferiti"
                } else {
                    "Aggiungi ai Preferiti"
                },
                Some("heart"),
                move |this, _, cx| {
                    let p = p.clone();
                    this.playlists.update(cx, |l, cx| {
                        if favorite {
                            for path in &p {
                                l.remove_track(FAVORITES, path, cx);
                            }
                        } else {
                            l.add(FAVORITES, p, cx);
                        }
                    });
                },
            )
            .separated(),
        );
        for list in lists.all().iter().filter(|l| !l.is_favorites()) {
            let name = list.name.clone();
            let p = paths.clone();
            items.push(MenuItem::new(
                format!("Aggiungi a «{}»", list.name),
                Some("playlist"),
                move |this, _, cx| {
                    let p = p.clone();
                    this.playlists.update(cx, |l, cx| l.add(&name, p, cx));
                },
            ));
        }
        let p = paths.clone();
        items.push(MenuItem::new(
            "Nuova playlist con questi brani",
            Some("plus"),
            move |this, window, cx| {
                let p = p.clone();
                let name = this.playlists.update(cx, |l, cx| {
                    let name = l.create(cx);
                    l.add(&name, p, cx);
                    name
                });
                this.go(Page::Playlist(name.clone()), cx);
                this.start_rename(name, window, cx);
            },
        ));
        if let Some(list) = in_playlist {
            let p = paths.clone();
            items.push(
                MenuItem::new(
                    "Togli da questa playlist",
                    Some("trash"),
                    move |this, _, cx| {
                        let p = p.clone();
                        let list = list.clone();
                        this.playlists.update(cx, |l, cx| {
                            for path in &p {
                                l.remove_track(&list, path, cx);
                            }
                        });
                    },
                )
                .separated()
                .danger(),
            );
        }
        if let Some(path) = single {
            let data = self.library.read(cx).data();
            if let Some(&i) = data.by_path.get(&path) {
                let album = data.tracks[i].album_index;
                items.push(
                    MenuItem::new("Vai all'album", Some("album"), move |this, _, cx| {
                        this.go(Page::Album(album), cx)
                    })
                    .separated(),
                );
            }
        }
        self.open_menu(position, items, cx);
    }

    fn playlist_menu(&mut self, name: String, position: Point<Pixels>, cx: &mut Context<Self>) {
        let mut items = Vec::new();
        let n = name.clone();
        items.push(MenuItem::new(
            "Riproduci",
            Some("play"),
            move |this, _, cx| {
                let tracks = this
                    .playlists
                    .read(cx)
                    .get(&n)
                    .map(|p| p.tracks.clone())
                    .unwrap_or_default();
                this.play_paths(tracks, 0, false, cx);
            },
        ));
        if name != FAVORITES {
            let n = name.clone();
            items.push(
                MenuItem::new("Rinomina", Some("edit"), move |this, window, cx| {
                    this.start_rename(n.clone(), window, cx)
                })
                .separated(),
            );
            let n = name.clone();
            items.push(
                MenuItem::new("Elimina playlist", Some("trash"), move |this, _, cx| {
                    let n = n.clone();
                    this.playlists.update(cx, |l, cx| l.delete(&n, cx));
                    if this.page == Page::Playlist(n) {
                        this.go_root(Page::Albums, cx);
                    }
                })
                .danger(),
            );
        }
        self.open_menu(position, items, cx);
    }

    fn start_rename(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Nome della playlist", cx);
            input.set_text(name.clone(), cx);
            input.select_all_text(cx);
            input
        });
        let subscription =
            cx.subscribe_in(
                &input,
                window,
                |this, input, event, window, cx| match event {
                    InputEvent::Submit => {
                        if let Some((old, _)) = this.renaming.take() {
                            let new_name = input.read(cx).text().to_owned();
                            let name = this
                                .playlists
                                .update(cx, |l, cx| l.rename(&old, &new_name, cx));
                            if this.page == Page::Playlist(old) {
                                this.page = Page::Playlist(name);
                            }
                        }
                        window.focus(&this.body_focus);
                        cx.notify();
                    }
                    InputEvent::Cancel => {
                        this.renaming = None;
                        window.focus(&this.body_focus);
                        cx.notify();
                    }
                    InputEvent::Changed => {}
                },
            );
        self._subscriptions.push(subscription);
        let focus = input.read(cx).focus_handle_ref().clone();
        window.focus(&focus);
        self.renaming = Some((name, input));
        cx.notify();
    }

    fn new_playlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.playlists.update(cx, |l, cx| l.create(cx));
        self.go(Page::Playlist(name.clone()), cx);
        self.start_rename(name, window, cx);
    }
}

impl Render for LibraryWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(window, cx);
        let compact = f32::from(window.viewport_size().width) < COMPACT_WIDTH;
        // The search field follows the theme.
        self.search.update(cx, |input, _| {
            input.placeholder_color = theme.faint.into();
            input.cursor_color = theme.accent.into();
            input.selection_color = theme.accent_soft.into();
        });
        let body = if self.page == Page::Settings {
            let page = self
                .settings_page
                .update(cx, |page, cx| page.render_page(window, cx));
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(page)
                .into_any_element()
        } else {
            let content = match self.page.clone() {
                Page::Albums => self.render_albums(&theme, compact, window, cx),
                Page::Album(index) => self.render_album(index, &theme, compact, cx),
                Page::Artists => self.render_artists(&theme, cx),
                Page::Artist(index) => self.render_artist(index, &theme, compact, window, cx),
                Page::Tracks => self.render_tracks(&theme, compact, cx),
                Page::Playlist(name) => self.render_playlist(&name, &theme, compact, cx),
                Page::Search => self.render_search(&theme, compact, window, cx),
                Page::NowPlaying => self.render_now_playing(&theme, cx),
                Page::Settings => div().into_any_element(),
            };
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(self.render_sidebar(&theme, compact, window, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .child(content),
                )
                .when(self.queue_open, |d| d.child(self.render_queue(&theme, cx)))
                .into_any_element()
        };
        div()
            .id("library-window")
            .size_full()
            .relative()
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
            .child(self.render_header(&theme, window, cx))
            .child(
                div()
                    .id("library-body")
                    .key_context(PLAYER_CONTEXT)
                    .track_focus(&self.body_focus)
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(body),
            )
            .child(self.render_player_bar(&theme, compact, cx))
            .children(self.render_overlays(&theme, cx))
    }
}

// Header (the window's title bar) -------------------------------------------------------------

impl LibraryWindow {
    fn render_header(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let update = self.updates.read(cx).available().map(|r| r.version.clone());
        let maximized = window.is_maximized();
        let stop = |_: &MouseDownEvent, _: &mut Window, cx: &mut App| cx.stop_propagation();
        let window_button = |id: &'static str, icon_name: &'static str, close: bool| {
            let group = SharedString::from(format!("{id}-group"));
            div()
                .id(id)
                .group(group.clone())
                .w(px(46.))
                .h(px(HEADER_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .when(close, |d| d.hover(|d| d.bg(gpui::rgb(0xe81123))))
                .when(!close, |d| d.hover(|d| d.bg(theme.hover)))
                .child(
                    gpui::svg()
                        .path(SharedString::from(format!("icons/{icon_name}.svg")))
                        .size(px(14.))
                        .text_color(theme.muted)
                        .when(close, |s| {
                            s.group_hover(group, |s| s.text_color(gpui::rgb(0xffffff)))
                        }),
                )
                .on_mouse_down(MouseButton::Left, stop)
        };
        div()
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .relative()
            .flex()
            .items_center()
            .gap(px(12.))
            .pl(px(if cfg!(target_os = "macos") { 86. } else { 16. }))
            .bg(theme.bar)
            .border_b_1()
            .border_color(theme.hairline)
            // Behind everything: drag to move the window, double-click to maximize it.
            .child(div().id("title-drag").absolute().inset_0().on_mouse_down(
                MouseButton::Left,
                |event: &MouseDownEvent, window, _| {
                    if event.click_count >= 2 {
                        platform::toggle_maximize(window);
                    } else {
                        platform::start_drag(window);
                    }
                },
            ))
            .child(
                div()
                    .id("logo")
                    .flex()
                    .flex_none()
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
                    .on_mouse_down(MouseButton::Left, stop)
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.show_settings(None, cx)),
                    ),
            )
            .child(
                div().flex_1().flex().justify_center().child(
                    div()
                        .id("search-box")
                        .w_full()
                        .max_w(px(420.))
                        .h(px(32.))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(8.))
                        .bg(theme.field)
                        .text_size(px(13.))
                        .child(icon("search", 14., theme.muted))
                        .child(self.search.clone())
                        .child(
                            div()
                                .flex_none()
                                .px(px(5.))
                                .py(px(1.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(theme.hairline)
                                .text_size(px(11.))
                                .text_color(theme.muted)
                                .child(if cfg!(target_os = "macos") {
                                    "⌘F"
                                } else {
                                    "Ctrl F"
                                }),
                        )
                        .on_mouse_down(MouseButton::Left, stop)
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            this.focus_search(window, cx)
                        })),
                ),
            )
            .when_some(update, |d, _version| {
                d.child(
                    div()
                        .id("update-pill")
                        .relative()
                        .h(px(30.))
                        .px(px(12.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(6.))
                        .rounded_full()
                        .bg(theme.accent)
                        .text_color(theme.on_accent)
                        .text_size(px(12.))
                        .font_weight(FontWeight::BOLD)
                        .cursor_pointer()
                        .child(icon("download", 13., theme.on_accent))
                        .child("Aggiorna")
                        .child(
                            div()
                                .absolute()
                                .top(px(-3.))
                                .right(px(-3.))
                                .size(px(10.))
                                .rounded_full()
                                .bg(gpui::rgb(0xffffff))
                                .border_2()
                                .border_color(theme.bar),
                        )
                        .on_mouse_down(MouseButton::Left, stop)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.updates.update(cx, |u, cx| u.set_dialog(true, cx))
                        })),
                )
            })
            .child(
                div()
                    .id("mini-player-button")
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .flex_none()
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
                    .on_mouse_down(MouseButton::Left, stop)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleLibrary), cx)),
            )
            .child(
                div()
                    .id("settings-button")
                    .size(px(36.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(self.page == Page::Settings, |d| d.bg(theme.accent_soft))
                    .hover(|d| d.bg(theme.hover))
                    .child(icon(
                        "settings",
                        17.,
                        if self.page == Page::Settings {
                            theme.accent_text
                        } else {
                            theme.muted
                        },
                    ))
                    .on_mouse_down(MouseButton::Left, stop)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        if this.page == Page::Settings {
                            this.back(cx)
                        } else {
                            this.show_settings(None, cx)
                        }
                    })),
            )
            // Windows and Linux: our own window buttons (macOS keeps its three dots at left).
            .when(!cfg!(target_os = "macos"), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_none()
                        .ml(px(4.))
                        .child(
                            window_button("win-min", "win-min", false)
                                .on_click(|_, window, _| window.minimize_window()),
                        )
                        .child(
                            window_button(
                                "win-max",
                                if maximized { "win-restore" } else { "win-max" },
                                false,
                            )
                            .on_click(|_, window, _| platform::toggle_maximize(window)),
                        )
                        .child(
                            window_button("win-close", "win-close", true)
                                .on_click(|_, window, _| window.remove_window()),
                        ),
                )
            })
            .when(cfg!(target_os = "macos"), |d| d.pr(px(16.)))
    }

    fn render_sidebar(
        &self,
        theme: &Theme,
        compact: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let _ = window;
        let section = |label: &'static str| {
            div()
                .px(px(10.))
                .pb(px(6.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.faint)
                .child(label)
        };
        let current = match &self.page {
            Page::Album(_) => Page::Albums,
            Page::Artist(_) => Page::Artists,
            page => page.clone(),
        };
        let item = |id: SharedString, icon_name: &'static str, label: SharedString, page: Page| {
            let active = current == page;
            div()
                .id(gpui::ElementId::Name(id))
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
                .when(!compact, |d| {
                    d.child(div().min_w_0().truncate().child(label))
                })
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.go_root(page.clone(), cx)),
                )
        };
        let library = self.library.read(cx);
        let data = library.data();
        let scan = library.scan_progress();
        let lists: Vec<(String, bool)> = self
            .playlists
            .read(cx)
            .all()
            .iter()
            .map(|p| (p.name.clone(), p.is_favorites()))
            .collect();
        let renaming = self.renaming.clone();
        div()
            .id("sidebar")
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
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .when(!compact, |d| d.child(section("LIBRERIA")))
                    .child(item("nav-albums".into(), "grid", "Album".into(), Page::Albums))
                    .child(item("nav-artists".into(), "artist", "Artisti".into(), Page::Artists))
                    .child(item("nav-tracks".into(), "note", "Brani".into(), Page::Tracks)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .when(!compact, |d| d.child(section("PLAYLIST")))
                    .children(lists.into_iter().map(|(name, favorites)| {
                        let editing = renaming
                            .as_ref()
                            .filter(|(n, _)| *n == name)
                            .map(|(_, input)| input.clone());
                        match editing {
                            Some(input) if !compact => div()
                                .h(px(32.))
                                .px(px(10.))
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .rounded(px(7.))
                                .bg(theme.field)
                                .border_1()
                                .border_color(theme.accent)
                                .child(icon("playlist", 15., theme.text))
                                .child(input)
                                .into_any_element(),
                            _ => {
                                let menu_name = name.clone();
                                item(
                                    format!("nav-playlist-{name}").into(),
                                    if favorites { "heart" } else { "playlist" },
                                    name.clone().into(),
                                    Page::Playlist(name.clone()),
                                )
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                        this.playlist_menu(menu_name.clone(), event.position, cx)
                                    }),
                                )
                                .into_any_element()
                            }
                        }
                    }))
                    .child(
                        div()
                            .id("new-playlist")
                            .h(px(32.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .when(compact, |d| d.justify_center())
                            .gap(px(10.))
                            .rounded(px(7.))
                            .text_color(theme.muted)
                            .cursor_pointer()
                            .hover(|d| d.bg(theme.hover))
                            .child(icon("plus", 15., theme.muted))
                            .when(!compact, |d| d.child("Nuova playlist"))
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.new_playlist(window, cx)
                            })),
                    ),
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
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.show_settings(Some(Section::Services), cx)
                                })),
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
        let (title, artist, cover_image, quality, path) = match now {
            Some(n) => (
                n.title.clone(),
                n.artist.clone().unwrap_or_else(|| n.subtitle.clone()),
                n.cover.clone(),
                Some(n.quality.clone()),
                Some(n.path.clone()),
            ),
            None => ("Nessun brano".into(), "".into(), None, None, None),
        };
        let favorite = path
            .as_ref()
            .is_some_and(|p| self.playlists.read(cx).is_favorite(p));
        let error = playback.error();
        let seek = cx.entity().downgrade();
        let vol = cx.entity().downgrade();
        let lyrics_on = self.page == Page::NowPlaying;
        let queue_on = self.queue_open;
        let control = |id: &'static str,
                       name: &'static str,
                       size: f32,
                       color: gpui::Rgba,
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
        let toggle = |id: &'static str, name: &'static str, on: bool| {
            div()
                .id(id)
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(8.))
                .cursor_pointer()
                .when(on, |d| d.bg(theme.accent_soft))
                .hover(|d| d.bg(theme.hover))
                .child(icon(
                    name,
                    15.,
                    if on { theme.accent_text } else { theme.muted },
                ))
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
            // Now playing: the cover opens "In riproduzione", the title the album.
            .child(
                div()
                    .w(gpui::relative(if compact { 0.32 } else { 0.28 }))
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .id("bar-cover")
                            .size(px(50.))
                            .flex_none()
                            .rounded(px(6.))
                            .overflow_hidden()
                            .cursor_pointer()
                            .map(|d| match cover_image {
                                Some(image) => {
                                    d.child(img(image).size_full().object_fit(ObjectFit::Cover))
                                }
                                None => d.child(parts::cover(None, Some(50.), 6., theme)),
                            })
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                if this.page == Page::NowPlaying {
                                    this.back(cx)
                                } else {
                                    this.go(Page::NowPlaying, cx)
                                }
                            })),
                    )
                    .child(
                        div()
                            .id("bar-title")
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .cursor_pointer()
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
                            })
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                if let Some(album) = this.playing_album(cx) {
                                    this.go(Page::Album(album), cx);
                                }
                            })),
                    )
                    .when_some(path, |d, path| {
                        d.child(
                            div()
                                .id("bar-heart")
                                .size(px(32.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|d| d.bg(theme.hover))
                                .child(icon(
                                    if favorite { "heart-filled" } else { "heart" },
                                    16.,
                                    if favorite {
                                        theme.accent_text
                                    } else {
                                        theme.muted
                                    },
                                ))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    let path = path.clone();
                                    this.playlists
                                        .update(cx, |l, cx| l.toggle_favorite(&path, cx));
                                })),
                        )
                    }),
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
            // Lyrics, queue and volume.
            .child(
                div()
                    .w(gpui::relative(if compact { 0.24 } else { 0.28 }))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(6.))
                    .child(toggle("bar-lyrics", "mic", lyrics_on).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            if this.page == Page::NowPlaying {
                                this.back(cx)
                            } else {
                                this.go(Page::NowPlaying, cx)
                            }
                        },
                    )))
                    .child(toggle("bar-queue", "queue", queue_on).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.queue_open = !this.queue_open;
                            cx.notify();
                        },
                    )))
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
                        div().w(px(if compact { 70. } else { 96. })).child(slider(
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
