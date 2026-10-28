//! Checking GitHub for a newer Klaudometer release. Only the desktop app
//! does this: at startup and then once a day, while its setting is on. The
//! request carries nothing but the app's version, in its user agent.

use crate::cache::{read_json, write_json};
use crate::env::Env;
use crate::fetch::request_json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Where releases live. The update link never points anywhere else.
const RELEASES: &str = "https://github.com/ajacks-595/klaudometer/releases";
/// After a failed check, try again after this long rather than a day.
const RETRY_SECONDS: f64 = 3600.0;

/// A release newer than this one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Update {
    pub version: String,
    pub url: String,
}

/// The last check, kept in update.json so restarts don't repeat it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Check {
    pub checked_at: f64,
    /// The latest release's version, without the "v".
    pub latest: Option<String>,
    pub url: Option<String>,
    pub error: Option<String>,
}

/// The last check if it's younger than `max_age` seconds (or an hour, after
/// a failure), otherwise a fresh one.
pub fn check(env: &Env, max_age: f64, now: f64) -> Check {
    let path = env.update_file();
    let last: Check = read_json(&path).unwrap_or_default();
    let limit = if last.error.is_some() { max_age.min(RETRY_SECONDS) } else { max_age };
    if now - last.checked_at < limit {
        return last;
    }
    let fresh = match latest_release(env) {
        Ok((latest, url)) => Check { checked_at: now, latest: Some(latest), url: Some(url), error: None },
        // Keep what the last good check found.
        Err(error) => Check { checked_at: now, error: Some(error), ..last },
    };
    let _ = std::fs::create_dir_all(&env.cache_dir);
    let _ = write_json(&path, &fresh);
    fresh
}

/// The update to offer, if the check found a release newer than `current`.
pub fn available(check: &Check, current: &str) -> Option<Update> {
    let latest = check.latest.as_deref()?;
    (parse(latest)? > parse(current)?).then(|| Update {
        version: latest.to_string(),
        url: check.url.clone().unwrap_or_else(|| format!("{RELEASES}/latest")),
    })
}

/// The latest published release's version and page. GitHub leaves drafts
/// and pre-releases out of "latest".
fn latest_release(env: &Env) -> Result<(String, String), String> {
    let url = format!("{}/repos/ajacks-595/klaudometer/releases/latest", env.github_api);
    let release =
        request_json(&url, &[("X-GitHub-Api-Version", "2022-11-28")], &[]).map_err(|_| "Couldn't reach GitHub.".to_string())?;
    let tag = release.get("tag_name").and_then(Value::as_str).unwrap_or_default();
    let version = tag.strip_prefix('v').unwrap_or(tag);
    if parse(version).is_none() {
        return Err(format!("Unexpected release tag {tag:?}."));
    }
    let page = release
        .get("html_url")
        .and_then(Value::as_str)
        .filter(|u| u.starts_with(&format!("{RELEASES}/")))
        .map_or_else(|| format!("{RELEASES}/tag/v{version}"), str::to_string);
    Ok((version.to_string(), page))
}

/// "1.2.3" as numbers. Anything else, pre-releases included, is None.
fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|p| p.bytes().all(|b| b.is_ascii_digit()).then(|| p.parse().ok()).flatten());
    let parsed = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(latest: &str) -> Check {
        Check { checked_at: 1.0, latest: Some(latest.into()), url: Some(format!("{RELEASES}/tag/v{latest}")), error: None }
    }

    #[test]
    fn compares_versions_as_numbers() {
        assert_eq!(parse("1.10.0"), Some((1, 10, 0)));
        assert_eq!(parse("1.2"), None);
        assert_eq!(parse("1.2.3.4"), None);
        assert_eq!(parse("1.2.3-rc.1"), None);
        assert_eq!(parse("+1.2.3"), None);
        assert_eq!(available(&found("1.10.0"), "1.9.9").map(|u| u.version), Some("1.10.0".into()));
        assert_eq!(available(&found("1.1.0"), "1.1.0"), None);
        assert_eq!(available(&found("1.0.0"), "1.1.0"), None);
        assert_eq!(available(&Check::default(), "1.1.0"), None);
    }
}
