//! Preferences kept between launches, in `settings.json` inside the system's config folder
//! (`%APPDATA%\LumiFi` on Windows, `~/Library/Application Support/LumiFi` on macOS).

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui::{App, AppContext, Context, Entity, Global};
use serde::{Deserialize, Serialize};

pub use crate::theme::ThemeMode;

/// The shape of the mini player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MiniStyle {
    /// 480×72, like a bar on the desktop.
    #[default]
    Strip,
    /// 280×280, cover art behind the controls.
    Square,
}

/// Where the mini player sits on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Dock {
    /// Wherever it was dragged.
    #[default]
    Free,
    /// Centred against the top edge of the screen.
    Top,
    /// Centred against the bottom edge (above the taskbar or the Dock).
    Bottom,
}

/// The mini player's sizes, from the right-click menu. The square can go smaller: at 50% it is
/// only the cover, with a small bar of controls under the mouse.
pub fn mini_scales(style: MiniStyle) -> &'static [f32] {
    match style {
        MiniStyle::Square => &[0.5, 0.75, 1.0, 1.25, 1.5],
        MiniStyle::Strip => &[0.75, 1.0, 1.25, 1.5],
    }
}

/// The nearest size the style offers, for a scale saved with another style.
pub fn fit_scale(style: MiniStyle, scale: f32) -> f32 {
    mini_scales(style)
        .iter()
        .copied()
        .min_by(|a, b| (a - scale).abs().total_cmp(&(b - scale).abs()))
        .unwrap_or(1.0)
}

/// Which ReplayGain value evens out the loudness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GainMode {
    #[default]
    Off,
    Track,
    Album,
}

impl GainMode {
    pub fn engine(self) -> lumifi_engine::dsp::ReplayGain {
        use lumifi_engine::dsp::ReplayGain;
        match self {
            GainMode::Off => ReplayGain::Off,
            GainMode::Track => ReplayGain::Track,
            GainMode::Album => ReplayGain::Album,
        }
    }
}

/// The equalizer for one output device.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EqProfile {
    pub enabled: bool,
    pub gains: [f32; 10],
    /// Bass boost in dB; 0 is off.
    pub bass_boost: f32,
    /// The boost used when bass boost is switched on again.
    pub bass_amount: f32,
    pub gain_db: f32,
}

impl EqProfile {
    pub fn engine(self) -> lumifi_engine::dsp::EqSettings {
        lumifi_engine::dsp::EqSettings {
            enabled: self.enabled,
            gains: self.gains,
            bass_boost: self.bass_boost,
            gain_db: self.gain_db,
        }
    }
}

/// A preset the user saved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EqPreset {
    pub name: String,
    pub gains: [f32; 10],
}

pub use lumifi_engine::dsp::EQ_BANDS;

/// Ready-made curves for the equalizer.
pub const EQ_PRESETS: [(&str, [f32; 10]); 6] = [
    ("Piatto", [0.0; 10]),
    ("Bassi", [5.0, 4.5, 3.5, 2.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0]),
    (
        "Voce",
        [-2.0, -1.5, -0.5, 1.0, 2.5, 3.0, 2.5, 1.0, 0.0, -1.0],
    ),
    (
        "Brillante",
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 1.5, 3.0, 4.0, 4.5],
    ),
    (
        "Morbido",
        [1.0, 1.0, 0.5, 0.0, 0.0, -0.5, -1.5, -2.5, -3.0, -3.5],
    ),
    (
        "Loudness",
        [4.0, 3.0, 1.0, -1.0, -2.0, -1.0, 1.0, 2.5, 3.5, 4.0],
    ),
];

/// Which window LumiFi opens with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StartWith {
    #[default]
    Library,
    Mini,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Folders the library reads.
    pub folders: Vec<PathBuf>,
    pub start_with: StartWith,
    /// Reopen the last track, paused where it was.
    pub resume: bool,
    pub theme: ThemeMode,
    pub mini_style: MiniStyle,
    /// How much bigger than its base size the mini player is drawn (1 = 280×280 or 480×72).
    pub mini_scale: f32,
    pub always_on_top: bool,
    pub volume: f32,
    /// Output device by name; `None` follows the system.
    pub output_device: Option<String>,
    /// Show "FLAC · 24/96" next to the track.
    pub show_quality: bool,
    pub mini_dock: Dock,
    /// Install new versions without asking (otherwise LumiFi asks first). The check itself
    /// always runs at launch.
    pub auto_update: bool,
    /// Also offer beta versions.
    pub beta: bool,
    /// The first-launch welcome has been shown.
    pub welcome_done: bool,
    pub replaygain: GainMode,
    /// The equalizer for each output device, by name ("" for the system default).
    pub eq: BTreeMap<String, EqProfile>,
    /// The user's own equalizer presets.
    pub eq_presets: Vec<EqPreset>,
    /// The library window was maximized when it closed.
    pub library_maximized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            start_with: StartWith::Library,
            resume: true,
            theme: ThemeMode::System,
            mini_style: MiniStyle::Strip,
            mini_scale: 1.0,
            always_on_top: false,
            volume: 0.8,
            output_device: None,
            show_quality: false,
            mini_dock: Dock::Free,
            auto_update: false,
            beta: false,
            welcome_done: false,
            replaygain: GainMode::Off,
            eq: BTreeMap::new(),
            eq_presets: Vec::new(),
            library_maximized: false,
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

    /// What was playing when LumiFi closed.
    pub fn session_file() -> PathBuf {
        Self::data_dir().join("session.json")
    }
}

impl Settings {
    /// The equalizer of the output device in use.
    pub fn eq_profile(&self) -> EqProfile {
        self.eq
            .get(self.output_device.as_deref().unwrap_or(""))
            .copied()
            .unwrap_or_default()
    }

    pub fn eq_profile_mut(&mut self) -> &mut EqProfile {
        let key = self.output_device.clone().unwrap_or_default();
        self.eq.entry(key).or_default()
    }

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
        settings.mini_scale = fit_scale(settings.mini_style, settings.mini_scale);
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

    /// Changes the settings and tells the observers without writing the file: for slider
    /// drags, which end with an `update`.
    pub fn update_quietly(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Settings)) {
        change(&mut self.value);
        cx.notify();
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
        assert_eq!(parsed.replaygain, GainMode::Off);
        assert_eq!(parsed.volume, 0.8);
    }

    #[test]
    fn round_trips_through_json() {
        let settings = Settings {
            folders: vec![PathBuf::from("/music")],
            replaygain: GainMode::Album,
            always_on_top: true,
            volume: 0.4,
            output_device: Some("DAC".into()),
            ..Settings::default()
        };
        let mut settings = settings;
        settings.eq_profile_mut().gains[2] = 3.0;
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), settings);
        assert_eq!(settings.eq_profile().gains[2], 3.0);
    }

    #[test]
    fn sizes_follow_the_style() {
        assert_eq!(fit_scale(MiniStyle::Strip, 0.25), 0.75);
        assert_eq!(fit_scale(MiniStyle::Square, 0.25), 0.5);
        assert_eq!(fit_scale(MiniStyle::Strip, 2.0), 1.5);
    }
}
