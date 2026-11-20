//! Plays two short files through the real output and checks that the engine goes from one to
//! the other by itself. Skipped where the machine has no audio output.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use lumifi_engine::Engine;

fn tone(path: &Path, rate: u32, seconds: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(rate as f32 * seconds) as u32 {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16;
        writer.write_sample(v).unwrap();
        writer.write_sample(v).unwrap();
    }
    writer.finalize().unwrap();
}

#[test]
fn goes_on_to_the_next_track_without_a_reload() {
    if lumifi_engine::output::default_device().is_err() {
        eprintln!("no audio output: skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("lumifi-gapless-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let first: PathBuf = dir.join("a.wav");
    let second: PathBuf = dir.join("b.wav");
    tone(&first, 44_100, 0.6);
    // Another rate: the stream stays as it is and the engine resamples.
    tone(&second, 48_000, 0.6);

    let mut engine = Engine::new();
    let Ok(_) = engine.load(&first, Duration::ZERO, true) else {
        eprintln!("cannot open the output: skipped");
        return;
    };
    engine.set_next(Some(second.clone()));
    let started = Instant::now();
    while engine.track_index() == 0 && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(engine.track_index(), 1);
    assert_eq!(engine.current_path(), Some(second.clone()));
    while !engine.is_finished() && started.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(engine.is_finished());
    let _ = std::fs::remove_dir_all(dir);
}
