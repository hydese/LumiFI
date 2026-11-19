//! Preferences kept between launches, in `settings.json` inside the system's config folder
//! (`%APPDATA%\LumiFi` on Windows, `~/Library/Application Support/LumiFi` on macOS).

use std::path::PathBuf;

use gpui::{App, AppContext, Context, Entity, Global};
use serde::{Deserialize, Serialize};

/// How the mini player draws progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MiniView {
    /// A plain bar.
    A,
    /// The track's waveform.
    #[default]
    B,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Folders the library reads.
    pub folders: Vec<PathBuf>,
    pub mini_view: MiniView,
    pub always_on_top: bool,
    pub volume: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            mini_view: MiniView::B,
            always_on_top: false,
            volume: 0.8,
        }
    }
}

/// Where LumiFi keeps its files.
pub struct Paths;

impl Paths {
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("LumiFi")
    }

    pub fn data_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("LumiFi")
    }

    pub fn cache_dir() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("LumiFi")
    }

    fn settings_file() -> PathBuf {
        Self::config_dir().join("settings.json")
    }
}

impl Settings {
    /// Reads the saved settings, or the defaults when there are none (or they are unreadable).
    pub fn load() -> Self {
        let mut settings: Settings = std::fs::read(Paths::settings_file())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if settings.folders.is_empty()
            && let Some(music) = dirs::audio_dir()
        {
            // First launch: start from the system's Music folder.
            settings.folders.push(music);
        }
        settings
    }

    pub fn save(&self) {
        let path = Paths::settings_file();
        let result = std::fs::create_dir_all(Paths::config_dir())
            .and_then(|_| {
                serde_json::to_vec_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))
            })
            .and_then(|bytes| std::fs::write(&path, bytes));
        if let Err(error) = result {
            log::warn!("cannot save settings to {}: {error}", path.display());
        }
    }
}

/// The settings as an entity every window can read and observe.
pub struct GlobalSettings(pub Entity<SettingsModel>);
impl Global for GlobalSettings {}

pub struct SettingsModel {
    pub value: Settings,
}

impl SettingsModel {
    pub fn install(value: Settings, cx: &mut App) -> Entity<SettingsModel> {
        let entity = cx.new(|_| SettingsModel { value });
        cx.set_global(GlobalSettings(entity.clone()));
        entity
    }

    /// Changes the settings, saves them and tells the observers.
    pub fn update(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Settings)) {
        change(&mut self.value);
        self.value.save();
        cx.notify();
    }
}

pub fn settings(cx: &App) -> Entity<SettingsModel> {
    cx.global::<GlobalSettings>().0.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_their_defaults() {
        let parsed: Settings = serde_json::from_str(r#"{"always_on_top": true}"#).unwrap();
        assert!(parsed.always_on_top);
        assert_eq!(parsed.mini_view, MiniView::B);
        assert_eq!(parsed.volume, 0.8);
    }

    #[test]
    fn round_trips_through_json() {
        let settings = Settings {
            folders: vec![PathBuf::from("/music")],
            mini_view: MiniView::A,
            always_on_top: true,
            volume: 0.4,
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
    }
}
