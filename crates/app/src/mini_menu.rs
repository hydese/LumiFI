//! The mini player's right-click menu. The strip is only 72 points tall, so the menu is a small
//! window of its own, opened where the mouse is. It closes as soon as it loses focus.

use gpui::{
    App, AppContext, Bounds, ClickEvent, Context, FocusHandle, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Pixels, Point, Render, SharedString, StatefulInteractiveElement,
    Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div,
    prelude::*, px, size,
};

use crate::actions::{OpenSettings, Quit, ToggleLibrary};
use crate::settings::{Dock, MiniStyle, Settings, fit_scale, mini_scales, settings};
use crate::theme::Theme;
use crate::widgets::icon;

const WIDTH: f32 = 230.0;
const ROW: f32 = 28.0;
const SEPARATOR: f32 = 9.0;

#[derive(Clone)]
enum Entry {
    Header(&'static str),
    Item {
        label: SharedString,
        checked: bool,
        action: Choice,
    },
    Separator,
}

#[derive(Clone, Copy)]
enum Choice {
    Scale(f32),
    OnTop,
    Dock(Dock),
    Style(MiniStyle),
    Settings,
    Library,
    Quit,
}

fn entries(value: &Settings) -> Vec<Entry> {
    let mut list = vec![Entry::Header("Dimensione")];
    for &scale in mini_scales(value.mini_style) {
        list.push(Entry::Item {
            label: format!("{}%", (scale * 100.0).round()).into(),
            checked: (value.mini_scale - scale).abs() < 0.01,
            action: Choice::Scale(scale),
        });
    }
    list.push(Entry::Separator);
    list.push(Entry::Header("Posizione"));
    for (label, dock) in [
        ("Libera", Dock::Free),
        ("Agganciata in alto", Dock::Top),
        ("Agganciata in basso", Dock::Bottom),
    ] {
        list.push(Entry::Item {
            label: label.into(),
            checked: value.mini_dock == dock,
            action: Choice::Dock(dock),
        });
    }
    list.push(Entry::Item {
        label: "Sempre in primo piano".into(),
        checked: value.always_on_top,
        action: Choice::OnTop,
    });
    list.push(Entry::Separator);
    list.push(Entry::Header("Forma"));
    for (label, style) in [
        ("Striscia", MiniStyle::Strip),
        ("Quadrato", MiniStyle::Square),
    ] {
        list.push(Entry::Item {
            label: label.into(),
            checked: value.mini_style == style,
            action: Choice::Style(style),
        });
    }
    list.push(Entry::Separator);
    for (label, action) in [
        ("Impostazioni…", Choice::Settings),
        ("Torna alla libreria", Choice::Library),
        ("Esci da LumiFi", Choice::Quit),
    ] {
        list.push(Entry::Item {
            label: label.into(),
            checked: false,
            action,
        });
    }
    list
}

fn height(list: &[Entry]) -> f32 {
    12.0 + list
        .iter()
        .map(|e| match e {
            Entry::Separator => SEPARATOR,
            Entry::Header(_) => 24.0,
            Entry::Item { .. } => ROW,
        })
        .sum::<f32>()
}

/// Opens the menu at `at` (screen coordinates), kept inside `area` (the visible screen) when
/// known: near the bottom it opens upwards, near the right edge to the left.
pub fn open(at: Point<Pixels>, area: Option<Bounds<Pixels>>, cx: &mut App) {
    let value = settings(cx).read(cx).value.clone();
    let list = entries(&value);
    let menu = size(px(WIDTH), px(height(&list)));
    let mut origin = at;
    if let Some(area) = area {
        let right = area.origin.x + area.size.width;
        let bottom = area.origin.y + area.size.height;
        if origin.x + menu.width > right {
            origin.x = (at.x - menu.width).max(area.origin.x);
        }
        if origin.y + menu.height > bottom {
            origin.y = (at.y - menu.height).max(area.origin.y);
        }
    }
    let bounds = Bounds::new(origin, menu);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        focus: true,
        show: true,
        window_background: WindowBackgroundAppearance::Opaque,
        window_decorations: Some(gpui::WindowDecorations::Client),
        app_id: Some("lumifi".into()),
        ..Default::default()
    };
    if let Err(error) = cx.open_window(options, |window, cx| cx.new(|cx| MiniMenu::new(window, cx)))
    {
        log::warn!("cannot open the mini player menu: {error}");
    }
}

pub struct MiniMenu {
    focus: FocusHandle,
    _subscriptions: Vec<gpui::Subscription>,
}

impl MiniMenu {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let subscriptions = vec![cx.observe_window_activation(window, |_, window, _| {
            // Clicked elsewhere: go away.
            if !window.is_window_active() {
                window.remove_window();
            }
        })];
        Self {
            focus,
            _subscriptions: subscriptions,
        }
    }

    fn choose(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        window.remove_window();
        let settings = settings(cx);
        match choice {
            Choice::Scale(scale) => {
                settings.update(cx, |s, cx| s.update(cx, |v| v.mini_scale = scale))
            }
            Choice::OnTop => settings.update(cx, |s, cx| {
                s.update(cx, |v| v.always_on_top = !v.always_on_top)
            }),
            Choice::Dock(dock) => settings.update(cx, |s, cx| s.update(cx, |v| v.mini_dock = dock)),
            Choice::Style(style) => settings.update(cx, |s, cx| {
                s.update(cx, |v| {
                    v.mini_style = style;
                    v.mini_scale = fit_scale(style, v.mini_scale);
                })
            }),
            Choice::Settings => cx.dispatch_action(&OpenSettings),
            Choice::Library => cx.dispatch_action(&ToggleLibrary),
            Choice::Quit => cx.dispatch_action(&Quit),
        }
    }
}

impl Render for MiniMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(window, cx);
        let value = settings(cx).read(cx).value.clone();
        let list = entries(&value);
        div()
            .id("mini-menu")
            .track_focus(&self.focus)
            .size_full()
            .p(px(6.))
            .bg(theme.surface)
            .border_1()
            .border_color(theme.hairline_strong)
            .text_color(theme.text)
            .text_size(px(13.))
            .font_family(".SystemUIFont")
            .flex()
            .flex_col()
            .children(list.into_iter().enumerate().map(|(i, entry)| {
                match entry {
                    Entry::Separator => div()
                        .h(px(SEPARATOR))
                        .flex()
                        .items_center()
                        .child(div().w_full().h(px(1.)).bg(theme.hairline))
                        .into_any_element(),
                    Entry::Header(text) => div()
                        .h(px(24.))
                        .px(px(10.))
                        .flex()
                        .items_end()
                        .pb(px(3.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.faint)
                        .child(text)
                        .into_any_element(),
                    Entry::Item {
                        label,
                        checked,
                        action,
                    } => div()
                        .id(("mini-menu-item", i))
                        .h(px(ROW))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|d| d.bg(theme.hover))
                        .child(
                            div()
                                .w(px(14.))
                                .when(checked, |d| d.child(icon("check", 13., theme.accent_text))),
                        )
                        .child(label)
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.choose(action, window, cx)
                        }))
                        .into_any_element(),
                }
            }))
    }
}
