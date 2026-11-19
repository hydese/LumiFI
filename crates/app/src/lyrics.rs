//! Lyrics for the playing track. In order: a .lrc file next to the track, the lyrics written in
//! the file's tags, a copy saved earlier, and finally LRCLIB (a free, open lyrics database).

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{App, AppContext, Context, Entity, Global, Task};
use serde::Deserialize;

use crate::playback::{Playback, playback};
use crate::settings::Paths;

#[derive(Debug, Clone, PartialEq)]
pub enum Lyrics {
    /// Lines with the time they start.
    Synced(Vec<(Duration, String)>),
    Plain(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LyricsState {
    Idle,
    Loading,
    Found(Lyrics),
    NotFound,
}

/// Parses LRC text ("[01:23.45] line"). Lines with several time tags repeat; metadata tags
/// like [ar:...] are skipped. Returns `None` when no line has a time.
pub fn parse_lrc(text: &str) -> Option<Vec<(Duration, String)>> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            if let Some(time) = parse_time(tag) {
                times.push(time);
            }
            rest = stripped[end + 1..].trim_start();
        }
        for time in times {
            lines.push((time, rest.trim().to_owned()));
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.sort_by_key(|(t, _)| *t);
    Some(lines)
}

fn parse_time(tag: &str) -> Option<Duration> {
    let (minutes, seconds) = tag.split_once(':')?;
    let minutes: u64 = minutes.trim().parse().ok()?;
    let seconds: f64 = seconds.trim().replace(',', ".").parse().ok()?;
    if !(0.0..60.0).contains(&seconds) {
        return None;
    }
    Some(Duration::from_secs(minutes * 60) + Duration::from_secs_f64(seconds))
}

/// The index of the line being sung at `position`.
pub fn current_line(lines: &[(Duration, String)], position: Duration) -> Option<usize> {
    let after = lines.partition_point(|(t, _)| *t <= position);
    after.checked_sub(1)
}

fn from_text(text: &str) -> Option<Lyrics> {
    if text.trim().is_empty() {
        return None;
    }
    Some(match parse_lrc(text) {
        Some(lines) => Lyrics::Synced(lines),
        None => Lyrics::Plain(text.trim().to_owned()),
    })
}

/// What LRCLIB answers.
#[derive(Deserialize)]
struct LrclibTrack {
    #[serde(rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain: Option<String>,
    #[serde(default)]
    instrumental: bool,
}

fn cache_file(path: &Path) -> PathBuf {
    let key = path
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    Paths::cache_dir()
        .join("lyrics")
        .join(format!("{key:016x}.lrc"))
}

/// Everything that can be done without the network.
fn find_local(path: &Path, embedded: Option<&str>) -> Option<Lyrics> {
    let sidecar = path.with_extension("lrc");
    if let Ok(text) = std::fs::read_to_string(&sidecar)
        && let Some(lyrics) = from_text(&text)
    {
        return Some(lyrics);
    }
    if let Some(lyrics) = embedded.and_then(from_text) {
        return Some(lyrics);
    }
    std::fs::read_to_string(cache_file(path))
        .ok()
        .and_then(|text| from_text(&text))
}

fn fetch_lrclib(
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration: Option<Duration>,
) -> Option<String> {
    let mut url = format!(
        "https://lrclib.net/api/get?track_name={}&artist_name={}",
        encode(title),
        encode(artist)
    );
    if let Some(album) = album {
        url.push_str(&format!("&album_name={}", encode(album)));
    }
    if let Some(duration) = duration {
        url.push_str(&format!("&duration={}", duration.as_secs()));
    }
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .new_agent();
    let mut response = agent
        .get(&url)
        .header(
            "User-Agent",
            concat!(
                "LumiFi/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/hydese/LumiFI)"
            ),
        )
        .call()
        .ok()?;
    let track: LrclibTrack = response.body_mut().read_json().ok()?;
    if track.instrumental {
        return None;
    }
    track
        .synced
        .or(track.plain)
        .filter(|t| !t.trim().is_empty())
}

/// Percent-encoding for a URL query value.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub struct LyricsModel {
    path: Option<PathBuf>,
    pub state: LyricsState,
    _job: Option<Task<()>>,
    _subscription: gpui::Subscription,
}

pub struct GlobalLyrics(pub Entity<LyricsModel>);
impl Global for GlobalLyrics {}

pub fn lyrics(cx: &App) -> Entity<LyricsModel> {
    cx.global::<GlobalLyrics>().0.clone()
}

impl LyricsModel {
    pub fn install(cx: &mut App) -> Entity<LyricsModel> {
        let playback = playback(cx);
        let entity = cx.new(|cx| LyricsModel {
            path: None,
            state: LyricsState::Idle,
            _job: None,
            _subscription: cx.observe(&playback, |this: &mut LyricsModel, playback, cx| {
                this.follow(&playback, cx)
            }),
        });
        cx.set_global(GlobalLyrics(entity.clone()));
        entity
    }

    fn follow(&mut self, playback: &Entity<Playback>, cx: &mut Context<Self>) {
        let playback = playback.read(cx);
        let path = playback.current_path().map(Path::to_path_buf);
        if path == self.path {
            return;
        }
        self.path = path.clone();
        let Some(path) = path else {
            self.state = LyricsState::Idle;
            self._job = None;
            cx.notify();
            return;
        };
        let Some(now) = playback.now() else {
            return;
        };
        let title = now.title.to_string();
        let artist = now.artist.as_ref().map(|a| a.to_string());
        let album = now.album.as_ref().map(|a| a.to_string());
        let duration = now.duration;
        let embedded = now.lyrics.clone();
        self.state = LyricsState::Loading;
        cx.notify();
        let job = cx.background_spawn(async move {
            if let Some(found) = find_local(&path, embedded.as_deref()) {
                return Some(found);
            }
            let artist = artist?;
            let text = fetch_lrclib(&title, &artist, album.as_deref(), duration)?;
            let cache = cache_file(&path);
            if let Some(dir) = cache.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&cache, &text);
            from_text(&text)
        });
        self._job = Some(cx.spawn(async move |this, cx| {
            let found = job.await;
            let _ = this.update(cx, |this, cx| {
                this.state = match found {
                    Some(lyrics) => LyricsState::Found(lyrics),
                    None => LyricsState::NotFound,
                };
                cx.notify();
            });
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc_with_metadata_and_repeated_tags() {
        let text = "[ar:Linkin Park]\n[00:12.50]I am a little bit of loneliness\n[00:20.00][01:05.25]Chorus\n\n[00:30]Plain seconds\n";
        let lines = parse_lrc(text).unwrap();
        assert_eq!(lines.len(), 4);
        assert_eq!(
            lines[0],
            (
                Duration::from_millis(12_500),
                "I am a little bit of loneliness".into()
            )
        );
        assert_eq!(lines[1].1, "Chorus");
        assert_eq!(lines[3].0, Duration::from_millis(65_250));
        assert_eq!(current_line(&lines, Duration::from_secs(5)), None);
        assert_eq!(current_line(&lines, Duration::from_secs(25)), Some(1));
        assert_eq!(current_line(&lines, Duration::from_secs(600)), Some(3));
    }

    #[test]
    fn plain_text_is_not_synced() {
        assert_eq!(
            from_text("Hello\nworld"),
            Some(Lyrics::Plain("Hello\nworld".into()))
        );
        assert_eq!(from_text("   "), None);
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("Faint & co"), "Faint%20%26%20co");
        assert_eq!(encode("Città"), "Citt%C3%A0");
    }
}
