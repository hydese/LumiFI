//! Everything LumiFi can be told to do, from buttons, keys or the menu bar. Playback actions are
//! handled once for the whole app, so they work the same in the mini player and the library.

use gpui::{App, KeyBinding, PathPromptOptions, actions};

use crate::playback::playback;
use crate::settings::{MiniView, settings};

actions!(
    lumifi,
    [
        TogglePlay,
        Next,
        Previous,
        ToggleShuffle,
        CycleRepeat,
        CycleView,
        ToggleMute,
        VolumeUp,
        VolumeDown,
        SeekForward,
        SeekBack,
        Open,
        ToggleOnTop,
        ToggleLibrary,
        OpenSettings,
        Quit,
    ]
);

/// The key context both windows set on their root.
pub const PLAYER_CONTEXT: &str = "Player";

pub fn bind_keys(cx: &mut App) {
    let ctx = Some(PLAYER_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("space", TogglePlay, ctx),
        KeyBinding::new("right", Next, ctx),
        KeyBinding::new("left", Previous, ctx),
        KeyBinding::new("shift-right", SeekForward, ctx),
        KeyBinding::new("shift-left", SeekBack, ctx),
        KeyBinding::new("up", VolumeUp, ctx),
        KeyBinding::new("down", VolumeDown, ctx),
        KeyBinding::new("m", ToggleMute, ctx),
        KeyBinding::new("s", ToggleShuffle, ctx),
        KeyBinding::new("r", CycleRepeat, ctx),
        KeyBinding::new("v", CycleView, ctx),
        KeyBinding::new("t", ToggleOnTop, ctx),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-l", ToggleLibrary, None),
        KeyBinding::new("secondary-o", Open, None),
        KeyBinding::new("secondary-q", Quit, None),
    ]);
}

/// Playback and settings actions that do not depend on a window.
pub fn register_global(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| {
        playback(cx).update(cx, |p, _| p.save_session());
        cx.quit();
    });
    cx.on_action(|_: &ToggleOnTop, cx| {
        settings(cx).update(cx, |s, cx| {
            s.update(cx, |v| v.always_on_top = !v.always_on_top)
        });
    });
    cx.on_action(|_: &TogglePlay, cx| playback(cx).update(cx, |p, cx| p.toggle_play(cx)));
    cx.on_action(|_: &Next, cx| playback(cx).update(cx, |p, cx| p.next(cx)));
    cx.on_action(|_: &Previous, cx| playback(cx).update(cx, |p, cx| p.previous(cx)));
    cx.on_action(|_: &ToggleShuffle, cx| playback(cx).update(cx, |p, cx| p.toggle_shuffle(cx)));
    cx.on_action(|_: &CycleRepeat, cx| playback(cx).update(cx, |p, cx| p.cycle_repeat(cx)));
    cx.on_action(|_: &ToggleMute, cx| playback(cx).update(cx, |p, cx| p.toggle_mute(cx)));
    cx.on_action(|_: &SeekForward, cx| playback(cx).update(cx, |p, cx| p.seek_by(10, cx)));
    cx.on_action(|_: &SeekBack, cx| playback(cx).update(cx, |p, cx| p.seek_by(-10, cx)));
    cx.on_action(|_: &VolumeUp, cx| {
        playback(cx).update(cx, |p, cx| p.set_volume(p.volume() + 0.05, cx));
        save_volume(cx);
    });
    cx.on_action(|_: &VolumeDown, cx| {
        playback(cx).update(cx, |p, cx| p.set_volume(p.volume() - 0.05, cx));
        save_volume(cx);
    });
    cx.on_action(|_: &CycleView, cx| {
        settings(cx).update(cx, |s, cx| {
            s.update(cx, |v| {
                v.mini_view = match v.mini_view {
                    MiniView::A => MiniView::B,
                    MiniView::B => MiniView::A,
                }
            })
        });
    });
    cx.on_action(|_: &Open, cx| {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Apri".into()),
        });
        cx.spawn(async move |cx| {
            if let Ok(Ok(Some(paths))) = answer.await {
                let _ = cx.update(|cx| playback(cx).update(cx, |p, cx| p.open_paths(paths, cx)));
            }
        })
        .detach();
    });
}

/// Remembers the volume for the next launch.
pub fn save_volume(cx: &mut App) {
    let volume = playback(cx).read(cx).volume();
    settings(cx).update(cx, |s, cx| {
        if (s.value.volume - volume).abs() > f32::EPSILON {
            s.update(cx, |v| v.volume = volume);
        }
    });
}
