//! The playback engine LumiFi uses: one output stream that stays open across tracks.
//!
//! A decoder thread reads the current track and, when it ends, goes straight on with the next
//! one (gapless), writing into a lock-free ring buffer. The audio callback only copies from the
//! ring, applies the equalizer and the volume, and counts frames. The stream runs at the track's
//! own sample rate whenever the device allows it; a track at another rate that follows without a
//! pause is resampled so the music does not stop.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::dsp::{EqSettings, Equalizer, ReplayGain, Resampler};
use crate::output::{self, MAX_CHANNELS};
use crate::{Artwork, EngineError, Result, Source, TrackInfo, TrackTags};

/// Decoded audio kept ready ahead of the device, in seconds.
const BUFFER_SECONDS: usize = 2;
/// How long the decoder waits when the ring is full.
const DECODER_IDLE: Duration = Duration::from_millis(5);

/// What `load` found in the file.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub path: PathBuf,
    pub info: TrackInfo,
    pub tags: TrackTags,
    pub artwork: Option<Artwork>,
}

/// State shared with the audio callback and the decoder.
struct Shared {
    paused: AtomicBool,
    volume: AtomicU32,
    /// Stream frames sent to the device since the last flush.
    played: AtomicU64,
    /// Stream frames the decoder has queued since the last flush.
    written: AtomicU64,
    /// Where the callback reads from; swapped on every flush.
    consumer: Mutex<Option<Consumer<f32>>>,
    eq: Mutex<EqSettings>,
    eq_version: AtomicU64,
}

/// One track in the stream, from the frame it starts at.
#[derive(Debug, Clone)]
struct Entry {
    path: PathBuf,
    start_frame: u64,
    start_pos: Duration,
}

#[derive(Default)]
struct Timeline {
    entries: Vec<Entry>,
    /// The track to go on with when the current one ends.
    next: Option<PathBuf>,
    /// The decoder has nothing more to write.
    done: bool,
}

struct Output {
    stream: cpal::Stream,
    rate: u32,
    channels: u16,
    device_name: String,
    /// The device asked for (`None`: the system default) when this stream was built.
    requested: Option<String>,
}

struct Decoder {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

pub struct Engine {
    device: Option<String>,
    output: Option<Output>,
    shared: Arc<Shared>,
    timeline: Arc<Mutex<Timeline>>,
    decoder: Option<Decoder>,
    replaygain: ReplayGain,
    gapless: bool,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self {
            device: None,
            output: None,
            shared: Arc::new(Shared {
                paused: AtomicBool::new(true),
                volume: AtomicU32::new(1.0f32.to_bits()),
                played: AtomicU64::new(0),
                written: AtomicU64::new(0),
                consumer: Mutex::new(None),
                eq: Mutex::new(EqSettings::default()),
                eq_version: AtomicU64::new(0),
            }),
            timeline: Arc::new(Mutex::new(Timeline::default())),
            decoder: None,
            replaygain: ReplayGain::Off,
            gapless: true,
        }
    }

    // Settings --------------------------------------------------------------------------------

    /// Chooses the output device by name (`None`: follow the system). Takes effect at the next
    /// `load` or `seek`.
    pub fn set_device(&mut self, device: Option<String>) {
        if self.device != device {
            self.device = device;
        }
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_eq(&self, settings: EqSettings) {
        if let Ok(mut eq) = self.shared.eq.lock() {
            *eq = settings;
        }
        self.shared.eq_version.fetch_add(1, Ordering::Release);
    }

    /// Takes effect from the next track that starts.
    pub fn set_replaygain(&mut self, mode: ReplayGain) {
        self.replaygain = mode;
    }

    pub fn set_gapless(&mut self, on: bool) {
        self.gapless = on;
    }

    // Playing ---------------------------------------------------------------------------------

    /// Opens `path` at `start`: the music already queued is dropped and this track begins
    /// (paused unless `play`).
    pub fn load(&mut self, path: &Path, start: Duration, play: bool) -> Result<Loaded> {
        self.stop_decoder();
        let mut source = Source::open(path)?;
        if !start.is_zero() {
            source.seek(start)?;
        }
        let info = source.info().clone();
        let tags = source.tags().clone();
        let artwork = source.artwork().cloned();
        let channels = usize::from(info.channels);
        if channels == 0 || channels > MAX_CHANNELS {
            return Err(EngineError::Unsupported("more than 8 channels"));
        }
        self.ensure_output(info.sample_rate, info.channels)?;
        let (rate, out_channels) = {
            let output = self.output.as_ref().expect("ensured above");
            (output.rate, usize::from(output.channels))
        };

        // Flush: the callback stops reading the old ring before the new one goes in.
        let capacity = rate as usize * out_channels * BUFFER_SECONDS;
        let (producer, consumer) = RingBuffer::<f32>::new(capacity);
        self.shared.paused.store(!play, Ordering::Relaxed);
        if let Ok(mut slot) = self.shared.consumer.lock() {
            *slot = None;
            self.shared.played.store(0, Ordering::Relaxed);
            self.shared.written.store(0, Ordering::Relaxed);
            *slot = Some(consumer);
        }
        {
            // What comes next is decided again by the caller after a load.
            let mut timeline = self.timeline.lock().unwrap_or_else(|e| e.into_inner());
            *timeline = Timeline {
                entries: vec![Entry {
                    path: path.to_path_buf(),
                    start_frame: 0,
                    start_pos: start,
                }],
                next: None,
                done: false,
            };
        }

        let stop = Arc::new(AtomicBool::new(false));
        let job = DecodeJob {
            source,
            rate,
            channels: out_channels,
            producer,
            shared: Arc::clone(&self.shared),
            timeline: Arc::clone(&self.timeline),
            stop: Arc::clone(&stop),
            replaygain: self.replaygain,
            gapless: self.gapless,
        };
        let handle = thread::Builder::new()
            .name("lumifi-decoder".into())
            .spawn(move || job.run())?;
        self.decoder = Some(Decoder { stop, handle });
        Ok(Loaded {
            path: path.to_path_buf(),
            info,
            tags,
            artwork,
        })
    }

    /// The track to continue with, without a pause, when the current one ends.
    pub fn set_next(&self, next: Option<PathBuf>) {
        if let Ok(mut timeline) = self.timeline.lock() {
            timeline.next = next;
        }
    }

    /// Jumps within the current track, keeping what comes next.
    pub fn seek(&mut self, position: Duration) -> Result<Loaded> {
        let Some(path) = self.current_path() else {
            return Err(EngineError::Unsupported("nothing is loaded"));
        };
        let next = self.timeline.lock().ok().and_then(|t| t.next.clone());
        let play = !self.is_paused();
        let loaded = self.load(&path, position, play)?;
        self.set_next(next);
        Ok(loaded)
    }

    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.shared.paused.store(false, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    /// Stops and forgets the current track.
    pub fn clear(&mut self) {
        self.stop_decoder();
        if let Ok(mut slot) = self.shared.consumer.lock() {
            *slot = None;
        }
        if let Ok(mut timeline) = self.timeline.lock() {
            *timeline = Timeline::default();
        }
        self.shared.paused.store(true, Ordering::Relaxed);
    }

    // Reading ---------------------------------------------------------------------------------

    /// How many tracks the engine has gone on to by itself since the last `load`.
    pub fn track_index(&self) -> usize {
        let played = self.shared.played.load(Ordering::Relaxed);
        self.timeline
            .lock()
            .map(|t| {
                t.entries
                    .iter()
                    .rposition(|e| e.start_frame <= played)
                    .unwrap_or(0)
            })
            .unwrap_or(0)
    }

    /// The track the listener hears now (after a gapless change, the new one).
    pub fn current_path(&self) -> Option<PathBuf> {
        let played = self.shared.played.load(Ordering::Relaxed);
        let timeline = self.timeline.lock().ok()?;
        current_entry(&timeline.entries, played).map(|e| e.path.clone())
    }

    /// How far into the current track the listener is.
    pub fn position(&self) -> Duration {
        let Some(rate) = self.output.as_ref().map(|o| o.rate) else {
            return Duration::ZERO;
        };
        let played = self.shared.played.load(Ordering::Relaxed);
        let Ok(timeline) = self.timeline.lock() else {
            return Duration::ZERO;
        };
        match current_entry(&timeline.entries, played) {
            Some(entry) => {
                entry.start_pos
                    + Duration::from_secs_f64((played - entry.start_frame) as f64 / f64::from(rate))
            }
            None => Duration::ZERO,
        }
    }

    /// True once everything decoded has been heard and nothing comes next.
    pub fn is_finished(&self) -> bool {
        let done = self.timeline.lock().map(|t| t.done).unwrap_or(false);
        done && self.shared.played.load(Ordering::Relaxed)
            >= self.shared.written.load(Ordering::Relaxed)
    }

    /// The device the sound goes to, and the rate the stream runs at.
    pub fn output_description(&self) -> Option<(String, u32)> {
        self.output
            .as_ref()
            .map(|o| (o.device_name.clone(), o.rate))
    }

    // Inside ----------------------------------------------------------------------------------

    fn stop_decoder(&mut self) {
        if let Some(decoder) = self.decoder.take() {
            decoder.stop.store(true, Ordering::Relaxed);
            let _ = decoder.handle.join();
        }
    }

    /// Opens (or reopens) the stream when there is none, the device changed, or the track's
    /// own rate is possible and the stream runs at another one.
    fn ensure_output(&mut self, rate: u32, channels: u16) -> Result<()> {
        let device_changed = self
            .output
            .as_ref()
            .is_none_or(|o| o.requested != self.device);
        let wants_more_channels = self
            .output
            .as_ref()
            .is_some_and(|o| o.channels < channels.max(2));
        let rate_differs = self.output.as_ref().is_some_and(|o| o.rate != rate);
        if !device_changed && !wants_more_channels && !rate_differs {
            return Ok(());
        }
        let device = output::device_named(self.device.as_deref())?;
        let config = output::pick_lasting_config(&device, rate, channels)?;
        if !device_changed
            && !wants_more_channels
            && self
                .output
                .as_ref()
                .is_some_and(|o| o.rate == config.stream.sample_rate)
        {
            // The device cannot run at the track's rate either way: keep the stream.
            return Ok(());
        }
        if let Some(old) = self.output.take() {
            let _ = old.stream.pause();
        }
        let device_name = device
            .description()
            .map(|d| d.to_string())
            .unwrap_or_else(|_| "default device".to_owned());
        let stream_rate = config.stream.sample_rate;
        let stream_channels = config.device_channels;
        let stream = build_stream(&device, &config, Arc::clone(&self.shared))?;
        stream.play()?;
        log::info!("output: {device_name} at {stream_rate} Hz, {stream_channels} channels");
        self.output = Some(Output {
            stream,
            rate: stream_rate,
            channels: stream_channels,
            device_name,
            requested: self.device.clone(),
        });
        Ok(())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop_decoder();
        if let Some(output) = self.output.take() {
            let _ = output.stream.pause();
        }
    }
}

fn current_entry(entries: &[Entry], played: u64) -> Option<&Entry> {
    entries.iter().rev().find(|e| e.start_frame <= played)
}

fn build_stream(
    device: &cpal::Device,
    config: &output::OutputConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream> {
    let channels = usize::from(config.device_channels);
    let rate = config.stream.sample_rate;
    let mut eq_version = u64::MAX;
    let mut equalizer = Equalizer::new(&EqSettings::default(), channels, rate);
    let stream = device.build_output_stream(
        config.stream,
        move |data: &mut [f32], _| {
            let state = &shared;
            if state.paused.load(Ordering::Relaxed) {
                data.fill(0.0);
                return;
            }
            // A new equalizer setting: rebuild the filters (without ever waiting on a lock).
            let version = state.eq_version.load(Ordering::Acquire);
            if version != eq_version
                && let Ok(settings) = state.eq.try_lock()
            {
                equalizer = Equalizer::new(&settings, channels, rate);
                eq_version = version;
            }
            let Ok(mut slot) = state.consumer.try_lock() else {
                data.fill(0.0);
                return;
            };
            let Some(consumer) = slot.as_mut() else {
                data.fill(0.0);
                return;
            };
            let volume = f32::from_bits(state.volume.load(Ordering::Relaxed));
            let mut played = 0_u64;
            for frame in data.chunks_mut(channels) {
                if consumer.slots() < channels {
                    frame.fill(0.0);
                    continue;
                }
                for sample in frame.iter_mut() {
                    *sample = consumer.pop().unwrap_or(0.0);
                }
                equalizer.process_frame(frame);
                // At full volume the samples are left exactly as decoded.
                if volume != 1.0 {
                    frame.iter_mut().for_each(|sample| *sample *= volume);
                }
                played += 1;
            }
            state.played.fetch_add(played, Ordering::Relaxed);
        },
        |error| log::error!("audio output error: {error}"),
        None,
    )?;
    Ok(stream)
}

/// Everything the decoder thread needs.
struct DecodeJob {
    source: Source,
    rate: u32,
    channels: usize,
    producer: Producer<f32>,
    shared: Arc<Shared>,
    timeline: Arc<Mutex<Timeline>>,
    stop: Arc<AtomicBool>,
    replaygain: ReplayGain,
    gapless: bool,
}

impl DecodeJob {
    fn run(mut self) {
        let mut block = Vec::new();
        let mut mapped = Vec::new();
        let mut resampled = Vec::new();
        let mut written: u64 = 0;
        'tracks: loop {
            let info = self.source.info().clone();
            let in_channels = usize::from(info.channels).max(1);
            let gain = self.source.tags().gain.factor(self.replaygain);
            let mut resampler = (info.sample_rate != self.rate)
                .then(|| Resampler::new(info.sample_rate, self.rate, self.channels));
            loop {
                if self.stop.load(Ordering::Relaxed) {
                    return;
                }
                match self.source.decode_next(&mut block) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(error) => {
                        log::warn!("decoding stopped: {error}");
                        break;
                    }
                }
                // To the stream's channel layout.
                mapped.clear();
                let mut out_frame = [0.0f32; MAX_CHANNELS];
                for frame in block.chunks(in_channels) {
                    output::map_frame(frame, &mut out_frame[..self.channels]);
                    mapped.extend_from_slice(&out_frame[..self.channels]);
                }
                if gain != 1.0 {
                    mapped.iter_mut().for_each(|s| *s *= gain);
                }
                let samples: &[f32] = match &mut resampler {
                    Some(r) => {
                        resampled.clear();
                        r.process(&mapped, &mut resampled);
                        &resampled
                    }
                    None => &mapped,
                };
                if !self.push(samples) {
                    return;
                }
                written += (samples.len() / self.channels) as u64;
                self.shared.written.store(written, Ordering::Relaxed);
            }
            if !self.gapless {
                break 'tracks;
            }
            let next = self.timeline.lock().ok().and_then(|mut t| t.next.take());
            let Some(path) = next else {
                break 'tracks;
            };
            match Source::open(&path) {
                Ok(source) => {
                    self.source = source;
                    if let Ok(mut timeline) = self.timeline.lock() {
                        timeline.entries.push(Entry {
                            path,
                            start_frame: written,
                            start_pos: Duration::ZERO,
                        });
                    }
                }
                Err(error) => {
                    log::warn!("cannot continue with {}: {error}", path.display());
                    break 'tracks;
                }
            }
        }
        if let Ok(mut timeline) = self.timeline.lock() {
            timeline.done = true;
        }
    }

    /// Writes all of `samples`, waiting while the ring is full. False if told to stop.
    fn push(&mut self, samples: &[f32]) -> bool {
        let mut done = 0;
        while done < samples.len() {
            if self.stop.load(Ordering::Relaxed) {
                return false;
            }
            let free = self.producer.slots().min(samples.len() - done);
            if free == 0 {
                thread::sleep(DECODER_IDLE);
                continue;
            }
            if let Ok(chunk) = self.producer.write_chunk_uninit(free) {
                chunk.fill_from_iter(samples[done..done + free].iter().copied());
                done += free;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_track_follows_the_frames_played() {
        let entries = vec![
            Entry {
                path: "a".into(),
                start_frame: 0,
                start_pos: Duration::from_secs(10),
            },
            Entry {
                path: "b".into(),
                start_frame: 1_000,
                start_pos: Duration::ZERO,
            },
        ];
        assert_eq!(
            current_entry(&entries, 999).unwrap().path,
            PathBuf::from("a")
        );
        assert_eq!(
            current_entry(&entries, 1_000).unwrap().path,
            PathBuf::from("b")
        );
        assert!(current_entry(&[], 5).is_none());
    }
}
