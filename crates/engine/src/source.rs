use std::fs::File;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::common::Tier;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, StandardTag, StandardVisualKey};
use symphonia::core::units::Time;

use crate::{EngineError, Result};

/// Every decoder Symphonia ships, plus the pure-Rust Opus decoder.
static CODECS: LazyLock<CodecRegistry> = LazyLock::new(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    // At the fallback tier, so a first-party Symphonia Opus decoder takes over once it ships.
    registry.register_audio_decoder_at_tier::<symphonia_opus::OpusDecoder>(Tier::Fallback);
    registry
});

/// What the engine knows about the track it is about to play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    /// Short codec name, such as `flac` or `alac`.
    pub codec: String,
    /// Sample rate in hertz.
    pub sample_rate: u32,
    /// Number of channels.
    pub channels: u16,
    /// Bit depth of the source, when the container states it (lossy codecs usually don't).
    pub bits_per_sample: Option<u32>,
    /// Number of playable frames, when known.
    pub frames: Option<u64>,
}

impl TrackInfo {
    /// Length of the track, when the container states it.
    pub fn duration(&self) -> Option<Duration> {
        let frames = self.frames?;
        Some(Duration::from_secs_f64(
            frames as f64 / f64::from(self.sample_rate),
        ))
    }

    /// High resolution: above CD quality in sample rate or bit depth.
    pub fn is_hi_res(&self) -> bool {
        self.sample_rate > 48_000 || self.bits_per_sample.is_some_and(|bits| bits > 16)
    }
}

/// What the file's tags say about the track.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<u64>,
    /// Unsynchronised lyrics stored in the file, if any.
    pub lyrics: Option<String>,
}

/// A picture stored in the file, usually the front cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artwork {
    /// MIME type such as `image/jpeg`, when the file states it.
    pub media_type: Option<String>,
    pub data: Vec<u8>,
}

/// A local audio file, opened and ready to decode.
pub struct Source {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    info: TrackInfo,
    tags: TrackTags,
    artwork: Option<Artwork>,
    /// Seconds per timestamp tick of the track, as (numerator, denominator).
    time_base: Option<(u32, u32)>,
    /// Frames still to drop after an accurate seek landed before the asked position.
    skip_frames: u64,
}

impl Source {
    /// Opens a file and prepares the decoder for its first audio track.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());

        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(extension);
        }

        let mut reader = symphonia::default::get_probe().probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )?;

        let track = reader
            .default_track(TrackType::Audio)
            .ok_or(EngineError::NoAudioTrack)?;
        let track_id = track.id;
        let frames = track.num_frames;
        let time_base = track.time_base.map(|tb| (tb.numer.get(), tb.denom.get()));
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or(EngineError::NoAudioTrack)?
            .clone();

        let decoder = CODECS.make_audio_decoder(&params, &AudioDecoderOptions::default())?;

        let sample_rate = params
            .sample_rate
            .ok_or(EngineError::Unsupported("the sample rate is unknown"))?;
        let channels = params
            .channels
            .as_ref()
            .map(|channels| channels.count())
            .ok_or(EngineError::Unsupported("the channel layout is unknown"))?;
        let channels =
            u16::try_from(channels).map_err(|_| EngineError::Unsupported("too many channels"))?;

        let info = TrackInfo {
            codec: decoder.codec_info().short_name.to_string(),
            sample_rate,
            channels,
            bits_per_sample: params.bits_per_sample,
            frames,
        };

        let (tags, artwork) = read_tags(reader.as_mut());

        Ok(Self {
            reader,
            decoder,
            track_id,
            info,
            tags,
            artwork,
            time_base,
            skip_frames: 0,
        })
    }

    /// The track being decoded.
    pub fn info(&self) -> &TrackInfo {
        &self.info
    }

    /// The file's tags.
    pub fn tags(&self) -> &TrackTags {
        &self.tags
    }

    /// The cover stored in the file, if there is one.
    pub fn artwork(&self) -> Option<&Artwork> {
        self.artwork.as_ref()
    }

    /// Moves to `position` from the start of the track. The next block decoded starts there.
    pub fn seek(&mut self, position: Duration) -> Result<()> {
        let time = Time::try_from_secs_f64(position.as_secs_f64())
            .ok_or(EngineError::Unsupported("the position is out of range"))?;
        let seeked = self.reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: Some(self.track_id),
            },
        )?;
        self.decoder.reset();
        // The reader stops at the packet before the position; drop the frames in between.
        let ticks = (seeked.required_ts.get() - seeked.actual_ts.get()).max(0) as u64;
        self.skip_frames = match self.time_base {
            Some((numer, denom)) => {
                ticks * u64::from(numer) * u64::from(self.info.sample_rate) / u64::from(denom)
            }
            None => ticks,
        };
        Ok(())
    }

    /// Decodes the next block of audio into `out` as interleaved 32-bit float samples.
    ///
    /// `out` is cleared and refilled. Returns `false` once the track has ended.
    pub fn decode_next(&mut self, out: &mut Vec<f32>) -> Result<bool> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(false),
                Err(SymphoniaError::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(error) => return Err(error.into()),
            };

            if packet.track_id != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(buffer) => {
                    buffer.copy_to_vec_interleaved(out);
                    if self.skip_frames > 0 {
                        let channels = usize::from(self.info.channels.max(1));
                        let frames = (out.len() / channels) as u64;
                        let drop = self.skip_frames.min(frames);
                        out.drain(..drop as usize * channels);
                        self.skip_frames -= drop;
                    }
                    if !out.is_empty() {
                        return Ok(true);
                    }
                }
                // A damaged packet is skipped rather than ending playback.
                Err(SymphoniaError::DecodeError(reason)) => {
                    log::warn!("skipping a damaged packet: {reason}");
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

/// Reads the newest tags and the front cover (or the first picture) from the container.
fn read_tags(reader: &mut dyn FormatReader) -> (TrackTags, Option<Artwork>) {
    let mut tags = TrackTags::default();
    let mut metadata = reader.metadata();
    let Some(revision) = metadata.skip_to_latest() else {
        return (tags, None);
    };
    let containers =
        std::iter::once(&revision.media).chain(revision.per_track.iter().map(|t| &t.metadata));
    let mut artwork: Option<Artwork> = None;
    for container in containers {
        for tag in &container.tags {
            let text = |value: &std::sync::Arc<String>| Some(value.trim().to_owned());
            match &tag.std {
                Some(StandardTag::TrackTitle(v)) if tags.title.is_none() => tags.title = text(v),
                Some(StandardTag::Artist(v)) if tags.artist.is_none() => tags.artist = text(v),
                Some(StandardTag::Album(v)) if tags.album.is_none() => tags.album = text(v),
                Some(StandardTag::AlbumArtist(v)) if tags.album_artist.is_none() => {
                    tags.album_artist = text(v)
                }
                Some(StandardTag::TrackNumber(n)) if tags.track_number.is_none() => {
                    tags.track_number = Some(*n)
                }
                Some(StandardTag::Lyrics(v)) if tags.lyrics.is_none() => tags.lyrics = text(v),
                _ => {}
            }
        }
        for visual in &container.visuals {
            let is_front = matches!(visual.usage, Some(StandardVisualKey::FrontCover));
            if artwork.is_none() || is_front {
                artwork = Some(Artwork {
                    media_type: visual.media_type.clone(),
                    data: visual.data.to_vec(),
                });
            }
            if is_front {
                break;
            }
        }
    }
    (tags, artwork)
}
