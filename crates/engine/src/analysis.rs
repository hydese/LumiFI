//! The whole-track waveform drawn by the mini player (view B).

use std::path::Path;

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

#[cfg(test)]
mod tests {
    use super::resample;

    #[test]
    fn resample_keeps_peaks() {
        assert_eq!(resample(&[0.1, 0.9, 0.2, 0.4], 2), vec![0.9, 0.4]);
        assert_eq!(resample(&[], 3), vec![0.0; 3]);
    }
}
