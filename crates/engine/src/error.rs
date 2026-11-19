use thiserror::Error;

/// Everything that can go wrong inside the engine.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("could not read the file")]
    Io(#[from] std::io::Error),

    #[error("could not decode the file")]
    Decode(#[from] symphonia::core::errors::Error),

    #[error("the file contains no playable audio track")]
    NoAudioTrack,

    #[error("unsupported stream: {0}")]
    Unsupported(&'static str),

    #[error("no audio output device is available")]
    NoOutputDevice,

    #[error(
        "the output device does not support {rate} Hz with {channels} channel(s); \
         resampling is not implemented yet"
    )]
    UnsupportedFormat { rate: u32, channels: u16 },

    #[error("audio output error")]
    Output(#[from] cpal::Error),
}

/// Shorthand for results produced by the engine.
pub type Result<T> = std::result::Result<T, EngineError>;
