//! Small pieces shared by both windows: icons, icon buttons and sliders.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, CursorStyle, DispatchPhase, ElementId, HitboxBehavior,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, Point, Rgba, SharedString, StatefulInteractiveElement, Styled, Window,
    canvas, div, px, svg,
};

pub fn icon(name: &str, size: f32, color: Rgba) -> impl IntoElement {
    svg()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// A round, borderless icon button that runs `action`.
pub fn icon_button(
    id: impl Into<ElementId>,
    name: &str,
    size: f32,
    box_size: f32,
    color: Rgba,
    action: impl gpui::Action + Clone,
) -> impl IntoElement {
    div()
        .id(id)
        .size(px(box_size))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .child(icon(name, size, color))
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
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
    let on_event = Rc::new(on_event);
    div().id(id).relative().child(content).child(
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
                let fraction = move |position: Point<Pixels>| fraction_at(bounds, position);
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

/// A thin rounded bar filled up to `fraction`.
pub fn bar(fraction: f32, height: f32, track: Rgba, fill: Rgba) -> AnyElement {
    div()
        .w_full()
        .h(px(height))
        .rounded_full()
        .bg(track)
        .child(
            div()
                .h_full()
                .w(gpui::relative(fraction.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(fill),
        )
        .into_any_element()
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
    }
}
