//! Audio devices and stream configuration.
//!
//! The engine never resamples silently: it asks the device for the track's own sample rate. On
//! macOS, CoreAudio then switches the device's nominal rate to match, which keeps the signal
//! bit-perfect. If a device cannot run at that rate the engine reports it instead of playing at
//! the wrong speed.

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, SampleFormat, StreamConfig, SupportedBufferSize};

use crate::{EngineError, Result};

/// The most channels a track may have: 7.1 surround.
pub const MAX_CHANNELS: usize = 8;

/// An output device as shown to the user.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

/// Lists the output devices of the default audio host.
pub fn list_devices() -> Result<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let default_id = host
        .default_output_device()
        .and_then(|device| device.id().ok());
    let mut devices = Vec::new();
    for device in host.output_devices()? {
        let id = device.id().ok();
        let name = device
            .description()
            .map(|description| description.to_string())
            .or_else(|_| device.id().map(|id| id.to_string()))
            .unwrap_or_else(|_| "Unknown device".to_owned());
        devices.push(DeviceInfo {
            name,
            is_default: id.is_some() && id == default_id,
        });
    }
    Ok(devices)
}

/// The default output device.
pub fn default_device() -> Result<cpal::Device> {
    cpal::default_host()
        .default_output_device()
        .ok_or(EngineError::NoOutputDevice)
}

/// The device called `name` (as `list_devices` names it), or the default one when `name` is
/// `None` or that device is gone (unplugged headphones, a DAC switched off).
pub fn device_named(name: Option<&str>) -> Result<cpal::Device> {
    if let Some(name) = name {
        let host = cpal::default_host();
        if let Ok(devices) = host.output_devices() {
            for device in devices {
                let matches = device
                    .description()
                    .map(|d| d.to_string() == name)
                    .unwrap_or(false);
                if matches {
                    return Ok(device);
                }
            }
        }
        log::info!("output device {name:?} not found, using the default one");
    }
    default_device()
}

/// A stream configuration for the device and how many channels it really has.
#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub stream: StreamConfig,
    pub device_channels: u16,
}

/// About 50 ms of audio: big enough not to crackle when the computer is busy, small enough that
/// pause and volume still feel instant.
fn roomy_frames(rate: u32, supported: &SupportedBufferSize) -> Option<u32> {
    let wanted = (rate / 20).max(256);
    match supported {
        SupportedBufferSize::Range { min, max } => Some(wanted.clamp(*min, *max)),
        SupportedBufferSize::Unknown => None,
    }
}

/// Picks a 32-bit float configuration at exactly `rate`, preferring the track's own channel count.
pub fn pick_config(device: &cpal::Device, rate: u32, channels: u16) -> Result<OutputConfig> {
    let unsupported = EngineError::UnsupportedFormat { rate, channels };
    let candidates: Vec<_> = device
        .supported_output_configs()?
        .filter(|range| range.sample_format() == SampleFormat::F32)
        .filter(|range| range.min_sample_rate() <= rate && rate <= range.max_sample_rate())
        .collect();

    // Same channel count first, then the smallest device layout that can hold the track.
    let chosen = candidates
        .iter()
        .find(|range| range.channels() == channels)
        .or_else(|| {
            candidates
                .iter()
                .filter(|range| range.channels() >= channels || channels == 1)
                .min_by_key(|range| range.channels())
        })
        .ok_or(unsupported)?;

    Ok(OutputConfig {
        stream: StreamConfig {
            channels: chosen.channels(),
            sample_rate: rate,
            // A roomier buffer than most systems default to, within what the device allows.
            buffer_size: roomy_frames(rate, chosen.buffer_size())
                .map_or(BufferSize::Default, BufferSize::Fixed),
        },
        device_channels: chosen.channels(),
    })
}

/// Copies one frame of the track into one frame of the device.
///
/// Matching channels are copied as they are. A mono track is sent to every device channel; any
/// extra device channel stays silent.
pub fn map_frame(source: &[f32], device: &mut [f32]) {
    for (index, sample) in device.iter_mut().enumerate() {
        *sample = match source.get(index) {
            Some(value) => *value,
            None if source.len() == 1 => source[0],
            None => 0.0,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{map_frame, roomy_frames};
    use cpal::SupportedBufferSize;

    #[test]
    fn roomy_buffer_is_about_fifty_milliseconds_within_limits() {
        let range = SupportedBufferSize::Range { min: 64, max: 8192 };
        assert_eq!(roomy_frames(48_000, &range), Some(2400));
        let tight = SupportedBufferSize::Range { min: 64, max: 1024 };
        assert_eq!(roomy_frames(48_000, &tight), Some(1024));
        assert_eq!(roomy_frames(48_000, &SupportedBufferSize::Unknown), None);
    }

    #[test]
    fn stereo_to_stereo_is_untouched() {
        let mut out = [0.0; 2];
        map_frame(&[0.25, -0.5], &mut out);
        assert_eq!(out, [0.25, -0.5]);
    }

    #[test]
    fn mono_goes_to_every_channel() {
        let mut out = [0.0; 2];
        map_frame(&[0.75], &mut out);
        assert_eq!(out, [0.75, 0.75]);
    }

    #[test]
    fn extra_device_channels_are_silent() {
        let mut out = [1.0; 4];
        map_frame(&[0.1, 0.2], &mut out);
        assert_eq!(out, [0.1, 0.2, 0.0, 0.0]);
    }
}
