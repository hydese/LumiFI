//! LumiFi colours: black and the red-orange #F0613D, with a light variant.

use gpui::{Rgba, WindowAppearance, rgb, rgba};

/// The accent: same saturation and lightness as LumiPDF's blue, opposite hue.
pub const ACCENT: u32 = 0xF0613D;
/// Icons and text drawn on the accent.
pub const ON_ACCENT: u32 = 0x111111;

#[derive(Clone, Copy)]
pub struct Theme {
    /// Behind the cover and in empty states.
    pub ground: Rgba,
    /// The panel over the cover.
    pub scrim: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub track: Rgba,
    pub unplayed: Rgba,
    pub chip: Rgba,
    pub accent: Rgba,
    pub on_accent: Rgba,
}

impl Theme {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
        }
    }

    pub fn dark() -> Self {
        Self {
            ground: rgb(0x0d0d0e),
            scrim: rgba(0x0a0a0bdb),
            text: rgb(0xf3f2f0),
            muted: rgb(0xb5b5b9),
            track: rgba(0xffffff33),
            unplayed: rgba(0xffffff4d),
            chip: rgba(0x080809a8),
            accent: rgb(ACCENT),
            on_accent: rgb(ON_ACCENT),
        }
    }

    pub fn light() -> Self {
        Self {
            ground: rgb(0xf7f6f4),
            scrim: rgba(0xfaf9f7eb),
            text: rgb(0x1c1c1e),
            muted: rgb(0x55555a),
            track: rgba(0x00000024),
            unplayed: rgba(0x00000038),
            chip: rgba(0x080809a8),
            accent: rgb(ACCENT),
            on_accent: rgb(ON_ACCENT),
        }
    }
}
