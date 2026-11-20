//! Signal processing: the equalizer, the resampler and ReplayGain. Nothing here runs unless it
//! is needed: with the equalizer off, no resampling and ReplayGain off, samples pass untouched.

use std::f64::consts::PI;

// Equalizer ---------------------------------------------------------------------------------

/// Centre frequencies of the ten bands, in hertz.
pub const EQ_BANDS: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];
/// How far a band can go up or down, in decibels.
pub const EQ_RANGE_DB: f32 = 12.0;

/// What the user set: on or off, and the gain of each band in decibels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqSettings {
    pub enabled: bool,
    pub gains: [f32; 10],
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            gains: [0.0; 10],
        }
    }
}

impl EqSettings {
    /// True when the equalizer would change the sound.
    pub fn is_active(&self) -> bool {
        self.enabled && self.gains.iter().any(|g| g.abs() > 0.01)
    }

    /// The automatic preamp: as much as the highest boost, so boosted bands cannot clip.
    pub fn preamp_db(&self) -> f32 {
        -self.gains.iter().copied().fold(0.0_f32, f32::max)
    }

    /// The combined response at `frequency`, in decibels (preamp included), for drawing the
    /// curve.
    pub fn response_db(&self, frequency: f32, sample_rate: f32) -> f32 {
        let mut total = self.preamp_db();
        for (band, gain) in EQ_BANDS.iter().zip(self.gains) {
            if gain.abs() < 0.01 || *band >= sample_rate / 2.0 {
                continue;
            }
            let c = Biquad::peaking(*band, gain, EQ_Q, sample_rate);
            total += c.magnitude_db(frequency, sample_rate);
        }
        total
    }
}

/// One octave wide, the usual width for a ten-band graphic equalizer.
const EQ_Q: f32 = 1.41;

/// A second-order filter (RBJ cookbook), transposed direct form II.
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Biquad {
    pub fn peaking(frequency: f32, gain_db: f32, q: f32, sample_rate: f32) -> Self {
        let a = 10f64.powf(f64::from(gain_db) / 40.0);
        let w0 = 2.0 * PI * f64::from(frequency) / f64::from(sample_rate);
        let alpha = w0.sin() / (2.0 * f64::from(q));
        let cos = w0.cos();
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }

    fn magnitude_db(&self, frequency: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * PI * f64::from(frequency) / f64::from(sample_rate);
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        let num = num_re * num_re + num_im * num_im;
        let den = den_re * den_re + den_im * den_im;
        (10.0 * (num / den).log10()) as f32
    }
}

/// The running state of the equalizer for one stream: a chain of filters per channel.
pub struct Equalizer {
    channels: usize,
    filters: Vec<Biquad>,
    /// Two state values per filter and channel.
    state: Vec<[f64; 2]>,
    preamp: f32,
    active: bool,
}

impl Equalizer {
    pub fn new(settings: &EqSettings, channels: usize, sample_rate: u32) -> Self {
        let rate = sample_rate as f32;
        let filters: Vec<Biquad> = if settings.is_active() {
            EQ_BANDS
                .iter()
                .zip(settings.gains)
                .filter(|(band, gain)| gain.abs() > 0.01 && **band < rate / 2.0)
                .map(|(band, gain)| Biquad::peaking(*band, gain, EQ_Q, rate))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            channels,
            state: vec![[0.0; 2]; filters.len() * channels],
            filters,
            preamp: 10f32.powf(settings.preamp_db() / 20.0),
            active: settings.is_active(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Processes one interleaved frame in place.
    pub fn process_frame(&mut self, frame: &mut [f32]) {
        if !self.active {
            return;
        }
        for (channel, sample) in frame.iter_mut().enumerate().take(self.channels) {
            let mut x = f64::from(*sample * self.preamp);
            for (k, f) in self.filters.iter().enumerate() {
                let s = &mut self.state[k * self.channels + channel];
                let y = f.b0 * x + s[0];
                s[0] = f.b1 * x - f.a1 * y + s[1];
                s[1] = f.b2 * x - f.a2 * y;
                x = y;
            }
            *sample = x as f32;
        }
    }
}

// Resampler ---------------------------------------------------------------------------------

/// Taps on each side of the interpolation point.
const HALF_TAPS: usize = 24;
/// How finely the filter is tabulated between two input samples.
const PHASES: usize = 256;

/// A windowed-sinc resampler for a fixed pair of rates. It is used only when the output
/// cannot run at the track's own rate; then it converts with a steep low-pass, far cleaner than
/// the linear interpolation of most system mixers.
pub struct Resampler {
    channels: usize,
    /// Input samples per output sample.
    step: f64,
    /// Position of the next output sample, in input samples from the start of `history`.
    position: f64,
    /// Input not yet fully used, interleaved; starts with `HALF_TAPS` frames of silence.
    history: Vec<f32>,
    /// The filter, tabulated: `PHASES + 1` rows of `2 * HALF_TAPS` taps.
    table: Vec<f32>,
}

impl Resampler {
    pub fn new(from: u32, to: u32, channels: usize) -> Self {
        let step = f64::from(from) / f64::from(to);
        // Below both Nyquist frequencies, with a little room for the transition band.
        let cutoff = 0.95 * (f64::from(to) / f64::from(from)).min(1.0);
        let taps = 2 * HALF_TAPS;
        let mut table = vec![0.0f32; (PHASES + 1) * taps];
        let beta = 8.6;
        let bessel = |x: f64| {
            // Modified Bessel function of the first kind, order zero.
            let mut sum = 1.0;
            let mut term = 1.0;
            let mut k = 1.0;
            while term > 1e-12 * sum {
                term *= (x / (2.0 * k)).powi(2);
                sum += term;
                k += 1.0;
            }
            sum
        };
        let norm = bessel(beta);
        for phase in 0..=PHASES {
            let frac = phase as f64 / PHASES as f64;
            for t in 0..taps {
                // Distance from the interpolation point to tap `t`.
                let x = t as f64 - (HALF_TAPS as f64 - 1.0) - frac;
                let sinc = if x.abs() < 1e-9 {
                    cutoff
                } else {
                    (PI * cutoff * x).sin() / (PI * x)
                };
                let r = x / HALF_TAPS as f64;
                let window = if r.abs() >= 1.0 {
                    0.0
                } else {
                    bessel(beta * (1.0 - r * r).sqrt()) / norm
                };
                table[phase * taps + t] = (sinc * window) as f32;
            }
        }
        Self {
            channels,
            step,
            position: HALF_TAPS as f64,
            history: vec![0.0; HALF_TAPS * channels],
            table,
        }
    }

    /// Converts interleaved `input`, appending to `output`.
    pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        let ch = self.channels;
        self.history.extend_from_slice(input);
        let frames = self.history.len() / ch;
        let taps = 2 * HALF_TAPS;
        // An output sample needs HALF_TAPS frames of input after it.
        while self.position + (HALF_TAPS as f64) < frames as f64 {
            let base = self.position.floor() as usize;
            let frac = self.position - base as f64;
            let phase = frac * PHASES as f64;
            let p0 = phase.floor() as usize;
            let blend = (phase - p0 as f64) as f32;
            let row0 = &self.table[p0 * taps..(p0 + 1) * taps];
            let row1 = &self.table[(p0 + 1) * taps..(p0 + 2) * taps];
            let first = base + 1 - HALF_TAPS;
            for c in 0..ch {
                let mut acc = 0.0f32;
                for t in 0..taps {
                    let coefficient = row0[t] + (row1[t] - row0[t]) * blend;
                    acc += self.history[(first + t) * ch + c] * coefficient;
                }
                output.push(acc);
            }
            self.position += self.step;
        }
        // Keep only what future output samples still need.
        let keep_from = (self.position.floor() as usize).saturating_sub(HALF_TAPS);
        if keep_from > 0 {
            self.history.drain(..keep_from * ch);
            self.position -= keep_from as f64;
        }
    }
}

// ReplayGain --------------------------------------------------------------------------------

/// Which ReplayGain value to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayGain {
    #[default]
    Off,
    Track,
    Album,
}

/// The ReplayGain values written in a file's tags.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GainTags {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

/// "-6.54 dB" → -6.54; "0.987" → 0.987.
pub fn parse_gain_value(text: &str) -> Option<f32> {
    let text = text.trim();
    let number = text
        .strip_suffix("dB")
        .or_else(|| text.strip_suffix("db"))
        .unwrap_or(text)
        .trim()
        .replace(',', ".");
    number.parse().ok()
}

impl GainTags {
    /// The linear gain to apply, never so much that the peak would clip.
    pub fn factor(&self, mode: ReplayGain) -> f32 {
        let (gain, peak) = match mode {
            ReplayGain::Off => return 1.0,
            ReplayGain::Track => (
                self.track_gain.or(self.album_gain),
                self.track_peak.or(self.album_peak),
            ),
            ReplayGain::Album => (
                self.album_gain.or(self.track_gain),
                self.album_peak.or(self.track_peak),
            ),
        };
        let Some(gain) = gain else {
            return 1.0;
        };
        let factor = 10f32.powf(gain / 20.0);
        match peak {
            Some(peak) if peak > 0.0 => factor.min(1.0 / peak),
            _ => factor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn flat_or_disabled_equalizer_leaves_samples_untouched() {
        let mut eq = Equalizer::new(&EqSettings::default(), 2, 44_100);
        let mut frame = [0.25, -0.5];
        eq.process_frame(&mut frame);
        assert_eq!(frame, [0.25, -0.5]);
        let flat = EqSettings {
            enabled: true,
            gains: [0.0; 10],
        };
        assert!(!Equalizer::new(&flat, 2, 44_100).is_active());
    }

    #[test]
    fn a_band_boosts_its_frequency_and_the_preamp_prevents_clipping() {
        let mut settings = EqSettings {
            enabled: true,
            gains: [0.0; 10],
        };
        settings.gains[5] = 6.0; // 1 kHz
        assert_eq!(settings.preamp_db(), -6.0);
        // At the band the boost and the preamp cancel out; far away only the preamp is left.
        assert!(settings.response_db(1_000.0, 48_000.0).abs() < 0.3);
        assert!((settings.response_db(60.0, 48_000.0) + 6.0).abs() < 0.3);
        let mut eq = Equalizer::new(&settings, 1, 48_000);
        let mut tone = sine(48_000, 1_000.0, 48_000);
        for sample in &mut tone {
            let mut frame = [*sample];
            eq.process_frame(&mut frame);
            *sample = frame[0];
        }
        let level = rms(&tone[24_000..]);
        assert!(
            (level - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.03,
            "{level}"
        );
    }

    #[test]
    fn resampler_keeps_a_tone_and_converts_the_length() {
        let input = sine(44_100, 1_000.0, 44_100);
        let mut resampler = Resampler::new(44_100, 48_000, 1);
        let mut out = Vec::new();
        for chunk in input.chunks(1_000) {
            resampler.process(chunk, &mut out);
        }
        let expected = 48_000.0;
        assert!((out.len() as f32 - expected).abs() < 100.0, "{}", out.len());
        let level = rms(&out[4_800..40_000]);
        assert!(
            (level - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "{level}"
        );
        // Same frequency: count zero crossings over one second of output.
        let crossings = out[..48_000 - 100]
            .windows(2)
            .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
            .count();
        assert!((crossings as i32 - 1_000).abs() <= 3, "{crossings}");
    }

    #[test]
    fn replaygain_reads_tags_and_avoids_clipping() {
        assert_eq!(parse_gain_value("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_gain_value(" +2,5 dB"), Some(2.5));
        assert_eq!(parse_gain_value("0.988"), Some(0.988));
        let tags = GainTags {
            track_gain: Some(-6.0),
            track_peak: Some(0.9),
            album_gain: Some(6.0),
            album_peak: Some(0.9),
        };
        assert_eq!(tags.factor(ReplayGain::Off), 1.0);
        assert!((tags.factor(ReplayGain::Track) - 0.501).abs() < 0.01);
        // +6 dB would push a 0.9 peak over 1: limited to 1 / 0.9.
        assert!((tags.factor(ReplayGain::Album) - 1.111).abs() < 0.01);
    }
}
