//! Decoding tests on WAV files generated on the fly, so the repository carries no audio.

use std::path::PathBuf;

use lumifi_engine::{EngineError, Source};

/// A file in the temp directory that is deleted when the test ends.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temp_path(name: &str) -> TempFile {
    TempFile(std::env::temp_dir().join(format!("lumifi-{}-{name}", std::process::id())))
}

/// Writes a sine wave and returns the samples written, as integers.
fn write_wav(path: &PathBuf, rate: u32, channels: u16, bits: u16, frames: u32) -> Vec<i32> {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: bits,
        sample_format: hound::SampleFormat::Int,
    };
    let peak = f64::from((1_i32 << (bits - 1)) - 1) * 0.5;
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    let mut written = Vec::new();
    for frame in 0..frames {
        let phase = f64::from(frame) * 440.0 * std::f64::consts::TAU / f64::from(rate);
        let value = (phase.sin() * peak).round() as i32;
        for _ in 0..channels {
            writer.write_sample(value).unwrap();
            written.push(value);
        }
    }
    writer.finalize().unwrap();
    written
}

fn decode_all(source: &mut Source) -> Vec<f32> {
    let mut all = Vec::new();
    let mut block = Vec::new();
    while source.decode_next(&mut block).unwrap() {
        all.extend_from_slice(&block);
    }
    all
}

#[test]
fn reads_cd_quality_info_and_every_sample() {
    let file = temp_path("cd.wav");
    let written = write_wav(&file.0, 44_100, 2, 16, 22_050);

    let mut source = Source::open(&file.0).unwrap();
    let info = source.info().clone();
    assert_eq!(info.sample_rate, 44_100);
    assert_eq!(info.channels, 2);
    assert_eq!(info.bits_per_sample, Some(16));
    assert_eq!(info.frames, Some(22_050));
    assert!(!info.is_hi_res());
    assert_eq!(info.duration().unwrap().as_millis(), 500);

    let decoded = decode_all(&mut source);
    assert_eq!(decoded.len(), written.len());
    // 16-bit samples survive the trip to 32-bit float exactly.
    for (sample, original) in decoded.iter().zip(&written) {
        assert_eq!((sample * 32_768.0).round() as i32, *original);
    }
}

#[test]
fn recognises_hi_res_files() {
    let file = temp_path("hires.wav");
    write_wav(&file.0, 96_000, 1, 24, 9_600);

    let mut source = Source::open(&file.0).unwrap();
    let info = source.info().clone();
    assert_eq!(info.sample_rate, 96_000);
    assert_eq!(info.channels, 1);
    assert_eq!(info.bits_per_sample, Some(24));
    assert!(info.is_hi_res());
    assert_eq!(decode_all(&mut source).len(), 9_600);
}

#[test]
fn missing_file_is_an_io_error() {
    let missing = std::env::temp_dir().join("lumifi-does-not-exist.flac");
    assert!(matches!(Source::open(&missing), Err(EngineError::Io(_))));
}

#[test]
fn garbage_is_rejected() {
    let file = temp_path("garbage.flac");
    std::fs::write(&file.0, b"this is not audio at all").unwrap();
    assert!(Source::open(&file.0).is_err());
}
