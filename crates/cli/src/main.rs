//! `lumifi`: a small command-line front end used to try the engine before the app exists.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lumifi_engine::{Player, Source, TrackInfo, format_time, output};

#[derive(Parser)]
#[command(
    name = "lumifi",
    version,
    about = "LumiFi audio engine, from the command line"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Shows codec, sample rate, bit depth and length of a file
    Info { file: PathBuf },
    /// Plays a file on the default output device
    Play {
        file: PathBuf,
        /// Volume from 0 to 100; 100 leaves the signal untouched
        #[arg(short, long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(0..=100))]
        volume: u8,
    },
    /// Lists the audio output devices
    Devices,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    match Cli::parse().command {
        Command::Info { file } => {
            let source =
                Source::open(&file).with_context(|| format!("cannot open {}", file.display()))?;
            println!("{}", describe(source.info()));
        }
        Command::Play { file, volume } => play(&file, volume)?,
        Command::Devices => {
            for device in output::list_devices()? {
                let marker = if device.is_default { "*" } else { " " };
                println!("{marker} {}", device.name);
            }
        }
    }
    Ok(())
}

fn play(file: &Path, volume: u8) -> Result<()> {
    let player = Player::play(file).with_context(|| format!("cannot play {}", file.display()))?;
    player.set_volume(f32::from(volume) / 100.0);
    println!("{}", describe(player.info()));
    println!("Output: {}", player.device_name());

    let total = player.info().duration().map(format_time);
    while !player.is_finished() {
        let position = format_time(player.position());
        match &total {
            Some(total) => print!("\r  {position} / {total}   "),
            None => print!("\r  {position}   "),
        }
        std::io::stdout().flush().ok();
        thread::sleep(Duration::from_millis(250));
    }
    println!();
    player.stop()?;
    Ok(())
}

fn describe(info: &TrackInfo) -> String {
    let rate = f64::from(info.sample_rate) / 1000.0;
    let bits = info
        .bits_per_sample
        .map(|bits| format!(" / {bits} bit"))
        .unwrap_or_default();
    let channels = match info.channels {
        1 => "mono".to_owned(),
        2 => "stereo".to_owned(),
        n => format!("{n} channels"),
    };
    let length = info
        .duration()
        .map(|d| format!(", {}", format_time(d)))
        .unwrap_or_default();
    let hi_res = if info.is_hi_res() { "  [Hi-Res]" } else { "" };
    format!(
        "{} {rate} kHz{bits}, {channels}{length}{hi_res}",
        info.codec.to_uppercase()
    )
}
