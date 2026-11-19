//! Pictures of the sound for the mini player: a whole-track waveform (view B) and a live
//! spectrum (view C).

use std::path::Path;
use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::{Result, Source};

/// The loudness outline of a whole track: `buckets` peaks from 0.0 to 1.0, scaled so the loudest
/// bucket is 1.0. Decodes the whole file, so call it off the main thread.
pub fn waveform(path: &Path, buckets: usize) -> Result<Vec<f32>> {
    let mut source = Source::open(path)?;
    let channels = usize::from(source.info().channels.max(1));
    let total_frames = source.info().frames;
    let mut peaks: Vec<f32> = Vec::new();
    let mut block = Vec::new();
    let mut frame_index: u64 = 0;
    // Without a known length, collect per-second peaks first and resample at the end.
    let frames_per_bucket = match total_frames {
        Some(total) if total > 0 => (total as f64 / buckets.max(1) as f64).max(1.0),
        _ => f64::from(source.info().sample_rate),
    };
    while source.decode_next(&mut block)? {
        for frame in block.chunks(channels) {
            let level = frame.iter().fold(0.0_f32, |max, s| max.max(s.abs()));
            let bucket = (frame_index as f64 / frames_per_bucket) as usize;
            if bucket >= peaks.len() {
                peaks.resize(bucket + 1, 0.0);
            }
            peaks[bucket] = peaks[bucket].max(level);
            frame_index += 1;
        }
    }
    let mut peaks = resample(&peaks, buckets);
    let loudest = peaks.iter().copied().fold(0.0_f32, f32::max);
    if loudest > 0.0 {
        peaks.iter_mut().for_each(|p| *p /= loudest);
    }
    Ok(peaks)
}

fn resample(values: &[f32], len: usize) -> Vec<f32> {
    if values.is_empty() || len == 0 {
        return vec![0.0; len];
    }
    (0..len)
        .map(|i| {
            let from = i * values.len() / len;
            let to = ((i + 1) * values.len() / len)
                .max(from + 1)
                .min(values.len());
            values[from..to].iter().copied().fold(0.0, f32::max)
        })
        .collect()
}

/// Turns recent samples into `bands` bar heights from 0.0 to 1.0, spaced like hearing
/// (logarithmically from 40 Hz to 16 kHz).
pub struct Spectrum {
    size: usize,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buffer: Vec<Complex32>,
    smoothed: Vec<f32>,
}

impl Spectrum {
    /// `size` must be a power of two, such as 2048.
    pub fn new(size: usize, bands: usize) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(size);
        let window = (0..size)
            .map(|i| {
                let x = std::f32::consts::TAU * i as f32 / (size - 1) as f32;
                0.5 - 0.5 * x.cos()
            })
            .collect();
        Self {
            size,
            fft,
            window,
            buffer: vec![Complex32::default(); size],
            smoothed: vec![0.0; bands],
        }
    }

    /// Updates the bars from the newest samples and returns them. Bars fall back slowly, so the
    /// display does not flicker.
    pub fn update(&mut self, samples: &[f32], sample_rate: u32) -> &[f32] {
        let bands = self.smoothed.len();
        if samples.len() < self.size / 2 || bands == 0 {
            self.smoothed.iter_mut().for_each(|v| *v *= 0.85);
            return &self.smoothed;
        }
        let start = samples.len().saturating_sub(self.size);
        let recent = &samples[start..];
        for (i, slot) in self.buffer.iter_mut().enumerate() {
            let s = recent.get(i).copied().unwrap_or(0.0);
            *slot = Complex32::new(s * self.window[i], 0.0);
        }
        self.fft.process(&mut self.buffer);

        let nyquist = sample_rate as f32 / 2.0;
        let bin_hz = nyquist / (self.size / 2) as f32;
        let (low, high) = (40.0_f32, 16_000.0_f32.min(nyquist));
        for band in 0..bands {
            let f0 = low * (high / low).powf(band as f32 / bands as f32);
            let f1 = low * (high / low).powf((band + 1) as f32 / bands as f32);
            let b0 = ((f0 / bin_hz) as usize).max(1);
            let b1 = ((f1 / bin_hz) as usize).max(b0 + 1).min(self.size / 2);
            let power = self.buffer[b0..b1]
                .iter()
                .map(|c| c.norm())
                .fold(0.0_f32, f32::max)
                / (self.size as f32 / 4.0);
            // From about -60 dB to 0 dB onto 0..1.
            let db = 20.0 * power.max(1e-6).log10();
            let level = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            let current = &mut self.smoothed[band];
            *current = if level > *current {
                level
            } else {
                *current * 0.85 + level * 0.15
            };
        }
        &self.smoothed
    }
}

#[cfg(test)]
mod tests {
    use super::{Spectrum, resample};

    #[test]
    fn resample_keeps_peaks() {
        assert_eq!(resample(&[0.1, 0.9, 0.2, 0.4], 2), vec![0.9, 0.4]);
        assert_eq!(resample(&[], 3), vec![0.0; 3]);
    }

    #[test]
    fn a_pure_tone_lights_its_band() {
        let rate = 48_000;
        let tone: Vec<f32> = (0..4096)
            .map(|i| (i as f32 * 1_000.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5)
            .collect();
        let mut spectrum = Spectrum::new(2048, 24);
        let bars = spectrum.update(&tone, rate).to_vec();
        let loudest = bars
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap();
        // 1 kHz sits about halfway between 40 Hz and 16 kHz on a log scale.
        assert!((10..=15).contains(&loudest), "loudest band was {loudest}");
        assert!(bars[0] < bars[loudest]);
    }
}
