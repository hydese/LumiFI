//! The LumiFi audio engine.
//!
//! The engine is deliberately independent from any user interface: it opens a file, decodes it
//! and sends the samples to an audio device at the file's own sample rate, so the signal reaches
//! the DAC untouched whenever volume is at 100% and no processing is active.
//!
//! - [`Source`] opens and decodes a local file (FLAC, ALAC, WAV, AIFF, MP3, AAC, Vorbis, Opus).
//! - [`Player`] plays a [`Source`] on the default output device.
//! - [`output`] lists devices and chooses a stream configuration that matches the track.

mod error;
pub mod output;
mod player;
mod source;

pub use error::{EngineError, Result};
pub use player::Player;
pub use source::{Source, TrackInfo};

/// Formats a duration as `m:ss`, or `h:mm:ss` from one hour up.
pub fn format_time(duration: std::time::Duration) -> String {
    let total = duration.as_secs();
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_time;
    use std::time::Duration;

    #[test]
    fn formats_minutes_and_hours() {
        assert_eq!(format_time(Duration::from_secs(0)), "0:00");
        assert_eq!(format_time(Duration::from_secs(271)), "4:31");
        assert_eq!(format_time(Duration::from_secs(3_725)), "1:02:05");
    }
}
