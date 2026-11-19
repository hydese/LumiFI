//! Icons compiled into the binary, so LumiFi is a single file.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(
            (concat!("icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg"))),
        )*]
    };
}

static ICONS: &[(&str, &[u8])] = icons![
    "album",
    "artist",
    "back",
    "close",
    "expand",
    "folder",
    "minimize",
    "next",
    "note",
    "pause",
    "play",
    "playlist",
    "plus",
    "previous",
    "refresh",
    "repeat",
    "repeat-one",
    "settings",
    "shuffle",
    "volume",
    "volume-off",
    "waves",
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_an_svg() {
        for (name, bytes) in ICONS {
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(text.starts_with("<svg"), "{name} is not an SVG");
        }
        assert!(Assets.load("icons/play.svg").unwrap().is_some());
        assert!(Assets.load("icons/missing.svg").unwrap().is_none());
    }
}
