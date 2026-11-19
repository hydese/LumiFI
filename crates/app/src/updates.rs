//! New versions, as LumiPDF does it: look at the releases on GitHub, show "Aggiorna" with a dot
//! when there is something newer, and a window with what changed.

use std::time::Duration;

use gpui::{App, AppContext, Context, Entity, Global, Task};
use serde::Deserialize;

const RELEASES_API: &str = "https://api.github.com/repos/hydese/LumiFI/releases?per_page=20";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: Vec<String>,
    pub url: String,
    pub prerelease: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate { checked_at: String },
    Available(Release),
    Failed,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
}

/// "v0.3.0" or "0.3.0-beta.2" → comparable numbers (pre-releases sort before the release).
fn version_key(text: &str) -> Option<(u64, u64, u64, u64)> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next().flatten().unwrap_or(0);
    let patch = parts.next().flatten().unwrap_or(0);
    // A release is newer than any of its pre-releases.
    let pre_rank = match pre {
        None => u64::MAX,
        Some(pre) => pre
            .rsplit('.')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0),
    };
    Some((major, minor, patch, pre_rank))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (version_key(candidate), version_key(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// The bullet points of a release description ("- ..." or "* ..."), or its first lines.
fn notes(body: &str) -> Vec<String> {
    let bullets: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")))
        .map(|l| l.replace("**", ""))
        .take(6)
        .collect();
    if !bullets.is_empty() {
        return bullets;
    }
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .take(3)
        .map(str::to_owned)
        .collect()
}

fn newest(releases: Vec<GithubRelease>, include_beta: bool) -> Option<Release> {
    releases
        .into_iter()
        .filter(|r| !r.draft && (include_beta || !r.prerelease))
        .filter(|r| is_newer(&r.tag_name, CURRENT))
        .max_by_key(|r| version_key(&r.tag_name))
        .map(|r| Release {
            version: r.tag_name.trim_start_matches(['v', 'V']).to_owned(),
            notes: notes(r.body.as_deref().unwrap_or_default()),
            url: r.html_url,
            prerelease: r.prerelease,
        })
}

fn fetch(include_beta: bool) -> anyhow::Result<Option<Release>> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .new_agent();
    let mut response = agent
        .get(RELEASES_API)
        .header("User-Agent", concat!("LumiFi/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()?;
    let releases: Vec<GithubRelease> = response.body_mut().read_json()?;
    Ok(newest(releases, include_beta))
}

pub struct Updates {
    pub state: UpdateState,
    /// The "what's new" window is open.
    pub dialog_open: bool,
    _job: Option<Task<()>>,
}

pub struct GlobalUpdates(pub Entity<Updates>);
impl Global for GlobalUpdates {}

pub fn updates(cx: &App) -> Entity<Updates> {
    cx.global::<GlobalUpdates>().0.clone()
}

impl Updates {
    pub fn install(check_now: bool, include_beta: bool, cx: &mut App) -> Entity<Updates> {
        let entity = cx.new(|cx| {
            let mut this = Updates {
                state: UpdateState::Idle,
                dialog_open: false,
                _job: None,
            };
            if check_now {
                this.check(include_beta, cx);
            }
            this
        });
        cx.set_global(GlobalUpdates(entity.clone()));
        entity
    }

    pub fn available(&self) -> Option<&Release> {
        match &self.state {
            UpdateState::Available(release) => Some(release),
            _ => None,
        }
    }

    pub fn check(&mut self, include_beta: bool, cx: &mut Context<Self>) {
        self.state = UpdateState::Checking;
        cx.notify();
        let job = cx.background_spawn(async move { fetch(include_beta) });
        self._job = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(Some(release)) => UpdateState::Available(release),
                    Ok(None) => UpdateState::UpToDate {
                        checked_at: clock_now(),
                    },
                    Err(error) => {
                        log::info!("update check failed: {error}");
                        UpdateState::Failed
                    }
                };
                cx.notify();
            });
        }));
    }

    pub fn set_dialog(&mut self, open: bool, cx: &mut Context<Self>) {
        self.dialog_open = open;
        cx.notify();
    }
}

/// When the last check happened, for "Hai l'ultima versione · controllato oggi".
fn clock_now() -> String {
    "oggi".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.4.0", "0.3.0"));
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(!is_newer("v0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(is_newer("0.4.0-beta.1", "0.3.0"));
        assert!(
            !is_newer("0.3.0-beta.2", "0.3.0"),
            "a beta of the same version is older"
        );
        assert!(is_newer("0.4.0", "0.4.0-beta.3"));
        assert!(!is_newer("nonsense", "0.3.0"));
    }

    #[test]
    fn picks_the_newest_allowed_release() {
        let r = |tag: &str, pre: bool| GithubRelease {
            tag_name: tag.into(),
            html_url: format!("https://x/{tag}"),
            body: Some("## Novità\n- Prima cosa\n- **Seconda** cosa".into()),
            prerelease: pre,
            draft: false,
        };
        let list = || {
            vec![
                r("v0.1.0", false),
                r("v99.0.0-beta.1", true),
                r("v98.1.0", false),
            ]
        };
        let stable = newest(list(), false).unwrap();
        assert_eq!(stable.version, "98.1.0");
        assert_eq!(stable.notes, vec!["Prima cosa", "Seconda cosa"]);
        assert_eq!(newest(list(), true).unwrap().version, "99.0.0-beta.1");
        assert_eq!(newest(vec![r("v0.0.1", false)], false), None);
    }
}
