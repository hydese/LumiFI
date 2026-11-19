//! Everything the mini player can be told to do, from buttons, keys or the menu bar.

use gpui::{App, KeyBinding, actions};

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
        Quit,
    ]
);

pub fn bind_keys(cx: &mut App) {
    let ctx = Some("MiniPlayer");
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
        KeyBinding::new("secondary-o", Open, None),
        KeyBinding::new("secondary-q", Quit, None),
    ]);
}
