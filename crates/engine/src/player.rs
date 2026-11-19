use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use rtrb::RingBuffer;

use crate::output::{self, MAX_CHANNELS};
use crate::{EngineError, Result, Source, TrackInfo};

/// How much decoded audio is kept ready ahead of the device, in seconds.
const BUFFER_SECONDS: usize = 2;
/// How long the decoder waits when the buffer is full.
const DECODER_IDLE: Duration = Duration::from_millis(5);

/// State shared between the caller, the decoder thread and the audio callback.
struct Shared {
    paused: AtomicBool,
    stop: AtomicBool,
    decoder_done: AtomicBool,
    finished: AtomicBool,
    volume: AtomicU32,
    frames_played: AtomicU64,
}

impl Shared {
    fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
}

/// Plays one local file on the default output device.
///
/// Decoding runs on its own thread and fills a lock-free ring buffer; the audio callback only
/// reads from it, so it never waits on the disk or the decoder.
pub struct Player {
    shared: Arc<Shared>,
    info: TrackInfo,
    device_name: String,
    stream: cpal::Stream,
    decoder: Option<JoinHandle<Result<()>>>,
}

impl Player {
    /// Opens `path` and starts playing it immediately.
    pub fn play(path: &Path) -> Result<Self> {
        let mut source = Source::open(path)?;
        let info = source.info().clone();
        let source_channels = usize::from(info.channels);
        if source_channels == 0 || source_channels > MAX_CHANNELS {
            return Err(EngineError::Unsupported("more than 8 channels"));
        }

        let device = output::default_device()?;
        let device_name = device
            .description()
            .map(|description| description.to_string())
            .unwrap_or_else(|_| "default device".to_owned());
        let config = output::pick_config(&device, info.sample_rate, info.channels)?;
        let device_channels = usize::from(config.device_channels);

        let capacity = info.sample_rate as usize * source_channels * BUFFER_SECONDS;
        let (mut producer, mut consumer) = RingBuffer::<f32>::new(capacity);

        let shared = Arc::new(Shared {
            paused: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            decoder_done: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            volume: AtomicU32::new(1.0_f32.to_bits()),
            frames_played: AtomicU64::new(0),
        });

        let decoder_shared = Arc::clone(&shared);
        let decoder = thread::Builder::new()
            .name("lumifi-decoder".into())
            .spawn(move || {
                let mut block = Vec::new();
                let result = loop {
                    if decoder_shared.stop.load(Ordering::Relaxed) {
                        break Ok(());
                    }
                    match source.decode_next(&mut block) {
                        Ok(true) => {}
                        Ok(false) => break Ok(()),
                        Err(error) => break Err(error),
                    }
                    let mut written = 0;
                    while written < block.len() {
                        if decoder_shared.stop.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        let free = producer.slots().min(block.len() - written);
                        if free == 0 {
                            thread::sleep(DECODER_IDLE);
                            continue;
                        }
                        if let Ok(chunk) = producer.write_chunk_uninit(free) {
                            chunk.fill_from_iter(block[written..written + free].iter().copied());
                            written += free;
                        }
                    }
                };
                decoder_shared.decoder_done.store(true, Ordering::Release);
                result
            })?;

        let callback_shared = Arc::clone(&shared);
        let mut frame = [0.0_f32; MAX_CHANNELS];
        let stream = device.build_output_stream(
            config.stream,
            move |data: &mut [f32], _| {
                let state = &callback_shared;
                if state.paused.load(Ordering::Relaxed) {
                    data.fill(0.0);
                    return;
                }
                let volume = state.volume();
                let mut played = 0_u64;
                for out in data.chunks_mut(device_channels) {
                    if consumer.slots() < source_channels {
                        out.fill(0.0);
                        if state.decoder_done.load(Ordering::Acquire) && consumer.is_empty() {
                            state.finished.store(true, Ordering::Release);
                        }
                        continue;
                    }
                    for slot in frame.iter_mut().take(source_channels) {
                        *slot = consumer.pop().unwrap_or(0.0);
                    }
                    output::map_frame(&frame[..source_channels], out);
                    // At full volume the samples are left exactly as decoded.
                    if volume != 1.0 {
                        out.iter_mut().for_each(|sample| *sample *= volume);
                    }
                    played += 1;
                }
                state.frames_played.fetch_add(played, Ordering::Relaxed);
            },
            |error| log::error!("audio output error: {error}"),
            None,
        )?;
        stream.play()?;

        Ok(Self {
            shared,
            info,
            device_name,
            stream,
            decoder: Some(decoder),
        })
    }

    /// The track being played.
    pub fn info(&self) -> &TrackInfo {
        &self.info
    }

    /// The name of the device the audio goes to.
    pub fn device_name(&self) -> &str {
        &self.device_name
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

    /// Sets the volume from 0.0 (silent) to 1.0 (untouched signal).
    pub fn set_volume(&self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        self.shared
            .volume
            .store(volume.to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        self.shared.volume()
    }

    /// How far playback has got.
    pub fn position(&self) -> Duration {
        let frames = self.shared.frames_played.load(Ordering::Relaxed);
        Duration::from_secs_f64(frames as f64 / f64::from(self.info.sample_rate))
    }

    /// True once every decoded sample has reached the device.
    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Acquire)
    }

    /// Stops playback and reports any error the decoder hit.
    pub fn stop(mut self) -> Result<()> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> Result<()> {
        self.shared.stop.store(true, Ordering::Relaxed);
        let _ = self.stream.pause();
        match self.decoder.take() {
            Some(handle) => handle
                .join()
                .unwrap_or(Err(EngineError::Unsupported("the decoder thread crashed"))),
            None => Ok(()),
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            log::warn!("playback ended with an error: {error}");
        }
    }
}
