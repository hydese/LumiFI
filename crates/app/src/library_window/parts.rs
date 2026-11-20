//! Small pieces of the library window: covers, track rows, bars and the right-click menu.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ObjectFit, ParentElement,
    Pixels, Point, SharedString, Styled, StyledImage, Window, div, img, prelude::*, px,
};

use super::LibraryWindow;
use crate::library::LibraryData;
use crate::theme::Theme;
use crate::widgets::icon;

pub const ROW_HEIGHT: f32 = 52.0;
pub const NUMBER_WIDTH: f32 = 32.0;
pub const DURATION_WIDTH: f32 = 64.0;

/// 2418 → "2.418", as Italians write it.
pub fn thousands(n: usize) -> String {
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

/// A square cover from the thumbnail cache, or the LumiFi waves when there is none.
pub fn cover(path: Option<&Path>, size: Option<f32>, radius: f32, theme: &Theme) -> AnyElement {
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

/// The round accent button shown over covers under the mouse.
pub fn play_dot(size: f32, theme: &Theme) -> gpui::Div {
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

/// A 4-pixel bar with a round knob, as in the design's player bar.
pub fn knob_bar(fraction: f32, theme: &Theme) -> impl IntoElement {
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

pub fn plain_bar(fraction: f32, height: f32, theme: &Theme) -> impl IntoElement {
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

/// Title, artist, cover and length for any file, from the library when it is there.
pub struct TrackLabel {
    pub title: SharedString,
    pub artist: SharedString,
    pub album: SharedString,
    pub cover: Option<PathBuf>,
    pub duration: Option<String>,
}

pub fn label(data: &LibraryData, path: &Path) -> TrackLabel {
    match data.by_path.get(path) {
        Some(&i) => {
            let t = &data.tracks[i];
            TrackLabel {
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone(),
                cover: data.albums[t.album_index].cover.clone(),
                duration: t.duration.map(lumifi_engine::format_time),
            }
        }
        None => TrackLabel {
            title: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
            artist: path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
            album: SharedString::default(),
            cover: None,
            duration: None,
        },
    }
}

/// What one row of a track list shows.
pub struct TrackRow {
    pub key: SharedString,
    pub number: String,
    pub title: SharedString,
    pub artist: SharedString,
    /// The album column; `None` on an album's own page.
    pub album: Option<SharedString>,
    /// A small cover before the title.
    pub cover: Option<Option<PathBuf>>,
    pub duration: Option<String>,
    /// This row's track is the one loaded.
    pub current: bool,
    /// ...and it is playing (not paused).
    pub playing: bool,
    /// The heart and the ⋯ button, shown under the mouse.
    pub actions: Option<AnyElement>,
}

/// The heart (always visible on a favourite, otherwise under the mouse) and the ⋯ menu button
/// of a track row. Neither click reaches the row.
pub fn row_actions(
    key: &SharedString,
    path: PathBuf,
    favorite: bool,
    in_playlist: Option<String>,
    this: gpui::WeakEntity<LibraryWindow>,
    theme: &Theme,
) -> AnyElement {
    let group = SharedString::from(format!("row-{key}"));
    let stop = |_: &gpui::MouseDownEvent, _: &mut Window, cx: &mut gpui::App| cx.stop_propagation();
    let heart_path = path.clone();
    let heart_this = this.clone();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .child(
            div()
                .id(SharedString::from(format!("{key}-heart")))
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .when(!favorite, |d| {
                    d.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
                })
                .child(if favorite {
                    icon("heart-filled", 15., theme.accent)
                } else {
                    icon("heart", 15., theme.muted)
                })
                .on_mouse_down(gpui::MouseButton::Left, stop)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let path = heart_path.clone();
                    let _ = heart_this.update(cx, |this, cx| {
                        this.playlists
                            .update(cx, |p, cx| p.toggle_favorite(&path, cx))
                    });
                }),
        )
        .child(
            div()
                .id(SharedString::from(format!("{key}-more")))
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|d| d.bg(theme.hover))
                .opacity(0.)
                .group_hover(group, |s| s.opacity(1.))
                .child(icon("more", 15., theme.muted))
                .on_mouse_down(gpui::MouseButton::Left, stop)
                .on_click(move |event, _, cx| {
                    cx.stop_propagation();
                    let position = event.position();
                    let path = path.clone();
                    let in_playlist = in_playlist.clone();
                    let _ = this.update(cx, |this, cx| {
                        this.track_menu(vec![path], position, in_playlist, cx)
                    });
                }),
        )
        .into_any_element()
}

/// "# · Titolo · Album · clock" over a track list, with a line under it.
/// The heart and ⋯ buttons with their gap.
pub const ACTIONS_WIDTH: f32 = 58.0;

pub fn table_header(with_album: bool, compact: bool, theme: &Theme) -> impl IntoElement {
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
        // Room for the heart and the ⋯ of the rows.
        .child(div().w(px(ACTIONS_WIDTH)).flex_none())
        .child(
            div()
                .w(px(DURATION_WIDTH))
                .pr(px(14.))
                .flex_none()
                .flex()
                .justify_end()
                .child(icon("clock", 14., theme.muted)),
        )
}

/// One row of a track list. Under the mouse the number turns into ▶ (or ⏸ on the track that is
/// playing); the loaded track shows the waves in the accent colour.
pub fn track_row(row: TrackRow, compact: bool, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    let group = SharedString::from(format!("row-{}", row.key));
    let hover_icon = if row.playing { "pause" } else { "play" };
    div()
        .id(gpui::ElementId::Name(row.key.clone()))
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
                            if row.current {
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
                        .child(icon(hover_icon, 13., theme.text)),
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
                                .when(row.current, |d| {
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
        .children(row.actions)
        .child(
            div()
                .w(px(DURATION_WIDTH))
                .pr(px(14.))
                .flex_none()
                .flex()
                .justify_end()
                .text_color(theme.muted)
                .text_size(px(12.5))
                .child(row.duration.unwrap_or_default()),
        )
}

// The right-click menu ----------------------------------------------------------------------

pub type MenuAction = Rc<dyn Fn(&mut LibraryWindow, &mut Window, &mut Context<LibraryWindow>)>;

pub struct MenuItem {
    pub label: SharedString,
    pub icon: Option<&'static str>,
    pub action: MenuAction,
    /// A line above this item.
    pub separated: bool,
    pub danger: bool,
}

impl MenuItem {
    pub fn new(
        label: impl Into<SharedString>,
        icon: Option<&'static str>,
        action: impl Fn(&mut LibraryWindow, &mut Window, &mut Context<LibraryWindow>) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            icon,
            action: Rc::new(action),
            separated: false,
            danger: false,
        }
    }

    pub fn separated(mut self) -> Self {
        self.separated = true;
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

pub struct Menu {
    pub position: Point<Pixels>,
    pub items: Vec<MenuItem>,
}
