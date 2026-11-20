//! The queue, on the right of the library: what is playing and what comes next. Click to jump,
//! arrows to move, × to remove.

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, prelude::*, px,
    uniform_list,
};

use super::parts::{cover, label};
use super::{LibraryWindow, QUEUE_WIDTH};
use crate::theme::Theme;
use crate::widgets::icon;

impl LibraryWindow {
    pub(super) fn render_queue(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let (queue, at) = self.playback.read(cx).upcoming();
        let data = self.library.read(cx).data();
        let playing = !self.playback.read(cx).is_paused();
        let count = queue.len();
        let this = cx.entity().downgrade();
        let theme = *theme;
        let next = count.saturating_sub(at + 1);
        // The playing track and the next 50; the rest is counted below.
        const SHOWN_NEXT: usize = 50;
        let shown = (count.saturating_sub(at)).min(SHOWN_NEXT + 1);
        let hidden = next.saturating_sub(SHOWN_NEXT);
        div()
            .w(px(QUEUE_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .border_l_1()
            .border_color(theme.hairline)
            .child(
                div()
                    .h(px(52.))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::BOLD)
                            .child("Coda"),
                    )
                    .child(
                        div()
                            .id("queue-close")
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|d| d.bg(theme.hover))
                            .child(icon("close", 12., theme.muted))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.queue_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .when(count == 0, |d| {
                d.child(
                    div()
                        .px(px(16.))
                        .text_color(theme.muted)
                        .child("La coda è vuota. Fai partire un album o una playlist."),
                )
            })
            .when(count > 0, |d| {
                d.child(
                    div()
                        .px(px(16.))
                        .pb(px(6.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.faint)
                        .child(if next == 0 {
                            "NIENTE DOPO QUESTO BRANO".to_owned()
                        } else {
                            format!("{next} DOPO QUESTO")
                        }),
                )
                .child(
                    uniform_list("queue", shown, move |range, _, _| {
                        range
                            .map(|row| {
                                let position = at + row;
                                let info = label(&data, &queue[position]);
                                let current = position == at;
                                let past = position < at;
                                let group = SharedString::from(format!("queue-{position}"));
                                let jump = this.clone();
                                let up = this.clone();
                                let down = this.clone();
                                let remove = this.clone();
                                let small = |id: &'static str, name: &'static str| {
                                    div()
                                        .id((id, position))
                                        .size(px(24.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(6.))
                                        .cursor_pointer()
                                        .hover(|d| d.bg(theme.hover))
                                        .child(icon(name, 13., theme.muted))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                };
                                div()
                                    .id(("queue-row", position))
                                    .group(group.clone())
                                    .mx(px(8.))
                                    .h(px(52.))
                                    .px(px(8.))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .when(current, |d| d.bg(theme.accent_soft))
                                    .when(!current, |d| d.hover(|d| d.bg(theme.hover)))
                                    .when(past, |d| d.opacity(0.5))
                                    .child(cover(info.cover.as_deref(), Some(36.), 4., &theme))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .truncate()
                                                    .when(current, |d| {
                                                        d.text_color(theme.accent_text)
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                    })
                                                    .child(info.title),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(theme.muted)
                                                    .truncate()
                                                    .child(info.artist),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_none()
                                            .gap(px(2.))
                                            .opacity(0.)
                                            .group_hover(group, |s| s.opacity(1.))
                                            .when(!current && position > 0, |d| {
                                                d.child(small("queue-up", "chevron-up").on_click(
                                                    move |_, _, cx| {
                                                        let _ = up.update(cx, |this, cx| {
                                                            this.playback.update(cx, |p, cx| {
                                                                p.move_in_queue(
                                                                    position,
                                                                    position - 1,
                                                                    cx,
                                                                )
                                                            })
                                                        });
                                                    },
                                                ))
                                            })
                                            .when(!current && position + 1 < count, |d| {
                                                d.child(
                                                    small("queue-down", "chevron-down").on_click(
                                                        move |_, _, cx| {
                                                            let _ = down.update(cx, |this, cx| {
                                                                this.playback.update(cx, |p, cx| {
                                                                    p.move_in_queue(
                                                                        position,
                                                                        position + 1,
                                                                        cx,
                                                                    )
                                                                })
                                                            });
                                                        },
                                                    ),
                                                )
                                            })
                                            .when(!current, |d| {
                                                d.child(small("queue-remove", "close").on_click(
                                                    move |_, _, cx| {
                                                        let _ = remove.update(cx, |this, cx| {
                                                            this.playback.update(cx, |p, cx| {
                                                                p.remove_from_queue(position, cx)
                                                            })
                                                        });
                                                    },
                                                ))
                                            }),
                                    )
                                    .when(current, |d| {
                                        d.child(icon(
                                            if playing { "waves" } else { "pause" },
                                            16.,
                                            theme.accent,
                                        ))
                                    })
                                    .on_click(move |_, _, cx| {
                                        let _ = jump.update(cx, |this, cx| {
                                            if position == at {
                                                this.playback.update(cx, |p, cx| p.toggle_play(cx));
                                            } else {
                                                this.playback.update(cx, |p, cx| {
                                                    p.play_queue_position(position, cx)
                                                });
                                            }
                                        });
                                    })
                            })
                            .collect()
                    })
                    .track_scroll(self.queue_scroll.clone())
                    .flex_1(),
                )
                .when(hidden > 0, |d| {
                    d.child(
                        div()
                            .flex_none()
                            .px(px(16.))
                            .py(px(10.))
                            .border_t_1()
                            .border_color(theme.hairline)
                            .text_size(px(12.))
                            .text_color(theme.muted)
                            .child(format!(
                                "… e altri {hidden} {}",
                                if hidden == 1 { "brano" } else { "brani" }
                            )),
                    )
                })
            })
            .into_any_element()
    }
}
