//! Update notice and auto-update tip.
//!
//! At most once a day, a session start launches a detached process that records the latest
//! GitHub release, so the session never waits on the network. Session starts return a one-line
//! notice when the recorded release is newer than the installed version. The check is skipped
//! when the user turned off Claude Code's own update traffic, and any failure is silent. Right after
//! a project is enabled, a one-time tip explains how to turn on automatic updates.

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use wizard_core::{VERSION, atomic};

use crate::install;

const LATEST: &str = "https://api.github.com/repos/Anthony14FR/Context-Wizard/releases/latest";
const RECENT: &str = "https://api.github.com/repos/Anthony14FR/Context-Wizard/releases?per_page=20";
const BETA_SUFFIX: &str = "-beta";
const CHECK_INTERVAL: Duration = Duration::from_hours(24);
const TIMEOUT: Duration = Duration::from_secs(3);
const OPT_OUT: [&str; 3] = [
    "DISABLE_UPDATES",
    "DISABLE_AUTOUPDATER",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
];
const STAMP: &str = "update-check.json";
const TIP_MARKER: &str = "auto-update-tip-shown";

/// Returns a notice when a newer release exists, checking GitHub at most once a day.
pub(crate) fn notice() -> Option<String> {
    if opted_out() {
        return None;
    }
    let data = install::data_dir()?;
    let latest = latest_version(&data)?;
    let plugin_id = install::plugin_id()
        .unwrap_or_else(|| format!("{}@{}", install::PLUGIN_NAME, install::PLUGIN_NAME));
    is_newer(&latest, VERSION).then(|| {
        format!(
            "Context Wizard {latest} is available (installed: {VERSION}). Update with `claude plugin update {plugin_id}`, then run /reload-plugins, or enable auto-update in /plugin, Marketplaces."
        )
    })
}

/// Returns the auto-update tip the first time it is asked for, then never again.
pub(crate) fn auto_update_tip() -> Option<String> {
    let marker = install::data_dir()?.join(TIP_MARKER);
    if marker.exists() {
        return None;
    }
    atomic::write(&marker, b"shown\n").ok()?;
    Some(
        "Tip for the user: to receive Context Wizard updates automatically, open /plugin, go to Marketplaces and choose Enable auto-update for this marketplace."
            .to_owned(),
    )
}

fn opted_out() -> bool {
    OPT_OUT.iter().any(|name| {
        env::var(name).is_ok_and(|value| !value.is_empty() && value != "0" && value != "false")
    })
}

/// Fetches the latest release and records it with the time of the check.
pub(crate) fn check() -> ExitCode {
    if let Some(data) = install::data_dir() {
        let latest = fetch_latest();
        write_stamp(&data.join(STAMP), now(), latest.as_deref());
    }
    ExitCode::SUCCESS
}

fn latest_version(data: &Path) -> Option<String> {
    let stamp_path = data.join(STAMP);
    let now = now()?;
    let (checked, latest) = read_stamp(&stamp_path).unwrap_or((0, None));
    if now.saturating_sub(checked) >= CHECK_INTERVAL.as_secs() {
        write_stamp(&stamp_path, Some(now), latest.as_deref());
        spawn_check();
    }
    latest
}

fn now() -> Option<u64> {
    Some(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs())
}

fn write_stamp(path: &Path, checked: Option<u64>, latest: Option<&str>) {
    let stamp = json!({ "checked": checked.unwrap_or(0), "latest": latest });
    let _ = atomic::write(path, format!("{stamp}\n").as_bytes());
}

fn spawn_check() {
    let Ok(binary) = env::current_exe() else {
        return;
    };
    let mut command = Command::new(binary);
    command
        .arg("update-check")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    let _ = command.spawn();
}

fn read_stamp(path: &Path) -> Option<(u64, Option<String>)> {
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    let checked = value.get("checked")?.as_u64()?;
    let latest = value
        .get("latest")
        .and_then(|latest| latest.as_str().map(str::to_owned));
    Some((checked, latest))
}

fn fetch_latest() -> Option<String> {
    let beta = install::plugin_id().is_some_and(|id| id.ends_with(BETA_SUFFIX));
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .proxy(ureq::Proxy::try_from_env())
        .build()
        .new_agent();
    let body = agent
        .get(if beta { RECENT } else { LATEST })
        .header("User-Agent", "context-wizard")
        .header("Accept", "application/vnd.github+json")
        .call()
        .ok()?
        .into_body()
        .read_to_string()
        .ok()?;
    let answer: serde_json::Value = serde_json::from_str(&body).ok()?;
    let releases = answer.as_array().cloned().unwrap_or_else(|| vec![answer]);
    newest(releases.iter().filter_map(|release| {
        let tag = release.get("tag_name")?.as_str()?;
        Some(tag.strip_prefix('v').unwrap_or(tag).to_owned())
    }))
}

/// The newest of `versions`, prereleases included.
fn newest(versions: impl Iterator<Item = String>) -> Option<String> {
    versions.reduce(|best, candidate| {
        if is_newer(&candidate, &best) {
            candidate
        } else {
            best
        }
    })
}

/// Whether `candidate` is a later version than `current`, following semantic versioning: a
/// prerelease such as `0.2.0-beta.2` comes before `0.2.0` and after `0.2.0-beta.1`.
fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier {
    Number(u64),
    Text(String),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    numbers: Vec<u64>,
    release: bool,
    prerelease: Vec<Identifier>,
}

fn parse_version(version: &str) -> Option<Version> {
    let version = version.split('+').next()?;
    let (core, prerelease) = version
        .split_once('-')
        .map_or((version, None), |(core, pre)| (core, Some(pre)));
    let numbers = core
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<Vec<u64>>>()?;
    let prerelease = prerelease
        .map(|pre| {
            pre.split('.')
                .map(|part| {
                    part.parse()
                        .map_or_else(|_| Identifier::Text(part.to_owned()), Identifier::Number)
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Version {
        numbers,
        release: version.find('-').is_none(),
        prerelease,
    })
}

#[cfg(test)]
mod tests {
    use super::{is_newer, newest};

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("1.0.0", "0.99.0"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("not a version", "0.1.0"));
    }

    #[test]
    fn orders_prereleases_before_their_release() {
        assert!(is_newer("0.2.0-beta.1", "0.1.0"));
        assert!(is_newer("0.2.0", "0.2.0-beta.3"));
        assert!(is_newer("0.2.0-beta.10", "0.2.0-beta.9"));
        assert!(is_newer("0.2.0-rc.1", "0.2.0-beta.2"));
        assert!(!is_newer("0.2.0-beta.1", "0.2.0"));
        let versions = ["0.1.0", "0.2.0-beta.2", "0.2.0-beta.1"].map(str::to_owned);
        assert_eq!(
            newest(versions.into_iter()).as_deref(),
            Some("0.2.0-beta.2")
        );
    }
}
