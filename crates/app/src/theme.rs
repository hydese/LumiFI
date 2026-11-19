//! LumiFi colours, taken from the approved design: black and the red-orange #F0613D, with a
//! light variant. The user picks system, light or dark in Settings › Aspetto.

use gpui::{App, Rgba, Window, WindowAppearance, rgb, rgba};
use serde::{Deserialize, Serialize};

/// The accent: same saturation and lightness as LumiPDF's blue, opposite hue.
pub const ACCENT: u32 = 0xF0613D;
/// Icons and text drawn on the accent.
pub const ON_ACCENT: u32 = 0x111111;

/// Which colours the app uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    /// Follow the system's light or dark setting.
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy)]
pub struct Theme {
    /// Window background.
    pub ground: Rgba,
    /// Header and player bar.
    pub bar: Rgba,
    pub sidebar: Rgba,
    /// Cards, the mini player strip.
    pub surface: Rgba,
    /// The panel over the cover in the square mini player.
    pub scrim: Rgba,
    pub hairline: Rgba,
    pub hairline_strong: Rgba,
    /// Inputs, chips, secondary buttons.
    pub field: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub faint: Rgba,
    pub track: Rgba,
    pub unplayed: Rgba,
    pub accent: Rgba,
    pub on_accent: Rgba,
    /// Accent-coloured text, readable on the background.
    pub accent_text: Rgba,
    /// A light wash of the accent, behind the selected item.
    pub accent_soft: Rgba,
    /// Rows under the mouse.
    pub hover: Rgba,
    /// Background of a segmented control, and its selected segment.
    pub segment: Rgba,
    pub segment_on: Rgba,
    /// Switch off.
    pub switch_off: Rgba,
}

impl Theme {
    /// The theme for `window`, following the user's choice.
    pub fn current(window: &Window, cx: &App) -> Self {
        let mode = crate::settings::settings(cx).read(cx).value.theme;
        Self::resolve(mode, window.appearance())
    }

    pub fn resolve(mode: ThemeMode, appearance: WindowAppearance) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
            ThemeMode::System => match appearance {
                WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            },
        }
    }

    pub fn dark() -> Self {
        Self {
            ground: rgb(0x0d0d0e),
            bar: rgb(0x131314),
            sidebar: rgb(0x101011),
            surface: rgb(0x18181a),
            scrim: rgba(0x0a0a0bc2),
            hairline: rgba(0xffffff14),
            hairline_strong: rgba(0xffffff29),
            field: rgba(0xffffff0f),
            text: rgb(0xf3f2f0),
            muted: rgb(0xa3a3a7),
            faint: rgb(0x8b8b90),
            track: rgba(0xffffff29),
            unplayed: rgba(0xffffff4d),
            accent: rgb(ACCENT),
            on_accent: rgb(ON_ACCENT),
            accent_text: rgb(0xff9b80),
            accent_soft: rgba(0xf0613d29),
            hover: rgba(0xffffff0d),
            segment: rgba(0xffffff0f),
            segment_on: rgba(0xffffff1f),
            switch_off: rgba(0xffffff2e),
        }
    }

    pub fn light() -> Self {
        Self {
            ground: rgb(0xf7f6f4),
            bar: rgb(0xfaf9f7),
            sidebar: rgb(0xf0efec),
            surface: rgb(0xffffff),
            scrim: rgba(0xfaf9f7d6),
            hairline: rgba(0x00000014),
            hairline_strong: rgba(0x0000002e),
            field: rgba(0x0000000d),
            text: rgb(0x1c1c1e),
            muted: rgb(0x6e6e73),
            faint: rgb(0x6e6e73),
            track: rgba(0x0000001f),
            unplayed: rgba(0x00000038),
            accent: rgb(ACCENT),
            on_accent: rgb(ON_ACCENT),
            accent_text: rgb(0xc2401f),
            accent_soft: rgba(0xf0613d1c),
            hover: rgba(0x0000000a),
            segment: rgba(0x0000000d),
            segment_on: rgb(0xffffff),
            switch_off: rgba(0x00000029),
        }
    }
}
