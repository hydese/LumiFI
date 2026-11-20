//! Small pieces shared by both windows: icons, icon buttons and sliders.

use std::rc::Rc;

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, ElementId, HitboxBehavior, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Rgba, SharedString, Styled, Window, canvas, div, prelude::*, px, svg,
};

pub fn icon(name: &str, size: f32, color: Rgba) -> impl IntoElement {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// An on/off switch as in the design: accent track and dark knob when on.
pub fn switch(on: bool, theme: &crate::theme::Theme) -> gpui::Div {
    div()
        .w(px(40.))
        .h(px(24.))
        .flex_none()
        .rounded(px(12.))
        .p(px(2.))
        .flex()
        .cursor_pointer()
        .when(on, |d| d.bg(theme.accent).justify_end())
        .when(!on, |d| d.bg(theme.switch_off).justify_start())
        .child(div().size(px(20.)).rounded_full().bg(if on {
            gpui::rgb(crate::theme::ON_ACCENT)
        } else {
            gpui::rgb(0xffffff)
        }))
}

/// A small grey label, for things that are not there yet ("In arrivo").
pub fn badge(text: &'static str, theme: &crate::theme::Theme) -> gpui::Div {
    div()
        .flex_none()
        .px(px(8.))
        .py(px(3.))
        .rounded(px(6.))
        .bg(theme.field)
        .text_color(theme.muted)
        .text_size(px(11.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(text)
}

/// A radio dot, filled with the accent when selected.
pub fn radio(selected: bool, theme: &crate::theme::Theme) -> gpui::Div {
    div()
        .size(px(16.))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .when(selected, |d| {
            d.bg(theme.accent)
                .child(div().size(px(6.)).rounded_full().bg(gpui::rgb(0xffffff)))
        })
        .when(!selected, |d| {
            d.border_1().border_color(theme.hairline_strong)
        })
}

/// One segment of a segmented control.
pub fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    theme: &crate::theme::Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(26.))
        .px(px(11.))
        .flex()
        .items_center()
        .rounded(px(7.))
        .text_size(px(12.))
        .cursor_pointer()
        .when(on, |d| {
            d.bg(theme.segment_on)
                .text_color(theme.text)
                .font_weight(gpui::FontWeight::SEMIBOLD)
        })
        .when(!on, |d| d.text_color(theme.muted))
        .child(label.into())
}

/// The frame around segments.
pub fn segments(theme: &crate::theme::Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .p(px(3.))
        .rounded(px(9.))
        .bg(theme.segment)
}

/// The LumiFi icon: the accent square with the three waves.
pub fn logo(size: f32, theme: &crate::theme::Theme) -> gpui::Div {
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(size * 0.234))
        .bg(theme.accent)
        .flex()
        .items_center()
        .justify_center()
        .child(icon("waves", size, theme.on_accent))
}

/// What happened to a slider.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SlideEvent {
    /// Pressed or moved: the knob is at this fraction (0 to 1).
    Move(f32),
    /// Released at this fraction.
    End(f32),
}

/// Makes `content` behave as a slider: press anywhere on it, drag anywhere in the window,
/// release anywhere. While `dragging` is true the window follows the mouse even outside the
/// slider, so the owner must keep that flag from the first `Move` until `End`.
///
/// The listeners are registered on the window directly (not on the element), which works the
/// same on every platform and is not affected by window drag areas.
pub fn slider(
    id: impl Into<ElementId>,
    dragging: bool,
    content: impl IntoElement,
    on_event: impl Fn(SlideEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    slider_along(id, dragging, false, content, on_event)
}

/// The same, upright: 1 at the top, 0 at the bottom.
pub fn vertical_slider(
    id: impl Into<ElementId>,
    dragging: bool,
    content: impl IntoElement,
    on_event: impl Fn(SlideEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    slider_along(id, dragging, true, content, on_event)
}

fn slider_along(
    id: impl Into<ElementId>,
    dragging: bool,
    vertical: bool,
    content: impl IntoElement,
    on_event: impl Fn(SlideEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_event = Rc::new(on_event);
    div().id(id).relative().child(content).child(
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
                let fraction = move |position: Point<Pixels>| {
                    if vertical {
                        fraction_up(bounds, position)
                    } else {
                        fraction_at(bounds, position)
                    }
                };
                let down = on_event.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        down(SlideEvent::Move(fraction(event.position)), window, cx);
                        cx.stop_propagation();
                    }
                });
                if !dragging {
                    return;
                }
                let moved = on_event.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let at = fraction(event.position);
                    if event.dragging() {
                        moved(SlideEvent::Move(at), window, cx);
                    } else {
                        // The button went up outside the window.
                        moved(SlideEvent::End(at), window, cx);
                    }
                });
                let up = on_event.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                        up(SlideEvent::End(fraction(event.position)), window, cx);
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full(),
    )
}

fn fraction_at(bounds: Bounds<Pixels>, position: Point<Pixels>) -> f32 {
    let width = f32::from(bounds.size.width).max(1.0);
    ((f32::from(position.x) - f32::from(bounds.origin.x)) / width).clamp(0.0, 1.0)
}

fn fraction_up(bounds: Bounds<Pixels>, position: Point<Pixels>) -> f32 {
    let height = f32::from(bounds.size.height).max(1.0);
    (1.0 - (f32::from(position.y) - f32::from(bounds.origin.y)) / height).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    #[test]
    fn fraction_follows_the_mouse_and_stays_in_range() {
        let bounds = Bounds::new(point(px(100.), px(0.)), size(px(200.), px(10.)));
        assert_eq!(fraction_at(bounds, point(px(200.), px(5.))), 0.5);
        assert_eq!(fraction_at(bounds, point(px(50.), px(5.))), 0.0);
        assert_eq!(fraction_at(bounds, point(px(900.), px(5.))), 1.0);
        let upright = Bounds::new(point(px(0.), px(100.)), size(px(10.), px(200.)));
        assert_eq!(fraction_up(upright, point(px(5.), px(100.))), 1.0);
        assert_eq!(fraction_up(upright, point(px(5.), px(250.))), 0.25);
    }
}
