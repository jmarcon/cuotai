//! PRD 0028 RF8.2 / RF8.3 (= PRD 0021 RF8.2 / RF8.3) — the opt-in CLI update
//! notice.
//!
//! Off by default. `CUOTAI_UPDATE_CHECK=1` or `[updates] check = true` in the
//! TOML turns it on. When on, the CLI reads the same static manifest the
//! desktop updater reads (PRD 0018, `latest.json`), at most once per 24
//! hours, with the answer cached in the data directory. The request carries
//! nothing but the HTTP `User-Agent` (`cuotai/<version>`): no account, path,
//! identifier or configuration. Only a newer *stable* `version` of the
//! manifest is used; the upgrade command comes from the install-source
//! detector, never from the manifest. The check never blocks or fails a run.

use crate::install_source::InstallSource;
use crate::text::{params, Localized, ParamValue};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The manifest the desktop updater reads (`tauri.conf.json`).
pub const MANIFEST_URL: &str =
    "https://github.com/jmarcon/cuotai/releases/latest/download/latest.json";
/// Where a package-manager-less install learns about the new version.
pub const RELEASES_URL: &str = "https://github.com/jmarcon/cuotai/releases/latest";
/// A cached answer younger than this is used without a request.
pub const CHECK_EVERY: Duration = Duration::hours(24);
const BODY_LIMIT: usize = 256 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// The `[updates]` table of the TOML.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatesSettings {
    /// Opt in to the CLI update notice (RF8.2).
    #[serde(default)]
    pub check: bool,
}

/// `[updates]` of a TOML document; a document without the table, or one that
/// does not parse, leaves the check off.
pub fn settings_from_toml(text: &str) -> UpdatesSettings {
    text.parse::<toml::Table>()
        .ok()
        .and_then(|table| table.get("updates").cloned())
        .and_then(|value| value.try_into().ok())
        .unwrap_or_default()
}

/// Whether the check runs: `CUOTAI_UPDATE_CHECK` wins when set (`0` or empty
/// turns it off), otherwise `[updates] check`.
pub fn enabled(environment: Option<&str>, setting: bool) -> bool {
    match environment.map(str::trim) {
        Some(value) => !value.is_empty() && value != "0",
        None => setting,
    }
}

/// A plain `X.Y.Z` release version; pre-releases are never offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u32, u32, u32);

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let text = text.split_once('+').map_or(text, |(core, _)| core);
        let mut parts = text.split('.');
        let version = Self(
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        );
        parts.next().is_none().then_some(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// The `version` of a `latest.json` body.
pub fn manifest_version(body: &[u8]) -> Option<Version> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    Version::parse(value.get("version")?.as_str()?)
}

#[derive(Debug, Serialize, Deserialize)]
struct Cached {
    checked_at: DateTime<Utc>,
    latest: Option<String>,
}

/// The cached answer file, `<data dir>/cuotai/update-check.json`.
pub fn cache_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("cuotai").join("update-check.json"))
}

fn read_cache(path: &Path, now: DateTime<Utc>) -> Option<Option<Version>> {
    let cached: Cached = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    // A future date (clock change, edited file) is not trusted.
    let age = now.signed_duration_since(cached.checked_at);
    (age >= Duration::zero() && age < CHECK_EVERY)
        .then(|| cached.latest.as_deref().and_then(Version::parse))
}

fn write_cache(path: &Path, now: DateTime<Utc>, latest: Option<Version>) {
    let cached = Cached {
        checked_at: now,
        latest: latest.map(|version| version.to_string()),
    };
    let Ok(bytes) = serde_json::to_vec(&cached) else {
        return;
    };
    if let Some(dir) = path.parent() {
        if crate::private_fs::ensure_dir(dir).is_err() {
            return;
        }
    }
    let _ = crate::private_fs::write_file(path, &bytes);
}

async fn fetch(url: &str) -> Option<Version> {
    // Only HTTPS, and only to HTTPS, for the manifest and its redirects.
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 4 && attempt.url().scheme() == "https" {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .user_agent(concat!("cuotai/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    let mut response = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() > BODY_LIMIT {
            return None;
        }
    }
    manifest_version(&body)
}

/// The newest stable version when it is newer than `current`. Uses the cache
/// when it is younger than [`CHECK_EVERY`]; otherwise asks `url` once and
/// caches the answer (also an unsuccessful one, so a flaky network is not
/// retried on every run). Any failure is silent.
pub async fn newer_version(
    url: &str,
    cache: Option<&Path>,
    now: DateTime<Utc>,
    current: &str,
) -> Option<Version> {
    let current = Version::parse(current)?;
    let latest = match cache.and_then(|path| read_cache(path, now)) {
        Some(latest) => latest,
        None => {
            let latest = fetch(url).await;
            if let Some(path) = cache {
                write_cache(path, now, latest);
            }
            latest
        }
    };
    latest.filter(|latest| *latest > current)
}

/// RF8.3: "Cuotai 0.81.0 is available: brew upgrade cuotai", with the
/// channel's own command from the install-source detector, or the releases
/// page when the channel has none.
pub fn notice(latest: Version, source: InstallSource) -> Localized {
    let version = ParamValue::from(latest.to_string());
    match source.upgrade_command() {
        Some(command) => Localized::with_params(
            "update.notice",
            params([
                ("version", version),
                ("command", ParamValue::from(command.to_string())),
            ]),
            "Cuotai {version} is available: {command}",
        ),
        None => Localized::with_params(
            "update.notice_download",
            params([
                ("version", version),
                ("url", ParamValue::from(RELEASES_URL.to_string())),
            ]),
            "Cuotai {version} is available: {url}",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("2026-10-{day:02}T12:00:00Z"))
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn it_is_off_unless_the_user_opts_in() {
        assert!(!enabled(None, false));
        assert!(enabled(None, true));
        assert!(enabled(Some("1"), false));
        assert!(!enabled(Some("0"), true), "the variable wins over the TOML");
        assert!(!enabled(Some(""), true));
    }

    #[test]
    fn the_updates_table_is_read_leniently() {
        assert!(!settings_from_toml("refresh_seconds = 60\n").check);
        assert!(settings_from_toml("[updates]\ncheck = true\n").check);
        assert!(!settings_from_toml("[updates]\ncheck = \"yes\"\n").check);
        assert!(!settings_from_toml("not toml [").check);
    }

    #[test]
    fn the_accounts_configuration_tolerates_the_updates_table() {
        let accounts = "[[accounts]]\nname = \"a\"\nprovider = \"open_ai_api\"\n";
        let config = crate::Config::from_toml(&format!("[updates]\ncheck = true\n{accounts}"))
            .expect("[updates] is accepted");
        assert_eq!(config.accounts.len(), 1);
        assert!(crate::Config::from_toml(&format!("[updates]\nbogus = 1\n{accounts}")).is_err());
        assert!(crate::Config::from_toml(&format!("[other]\nx = 1\n{accounts}")).is_err());
    }

    #[test]
    fn only_newer_stable_versions_count() {
        assert_eq!(Version::parse("v0.81.0"), Version::parse("0.81.0"));
        assert!(Version::parse("0.81.0-rc.1").is_none());
        assert!(Version::parse("0.81").is_none());
        assert!(Version::parse("0.81.0.1").is_none());
        assert!(Version::parse("0.81.0").unwrap() > Version::parse("0.9.9").unwrap());
        assert_eq!(
            manifest_version(br#"{"version":"0.81.0","platforms":{}}"#),
            Version::parse("0.81.0")
        );
        assert!(manifest_version(b"[]").is_none());
        assert!(manifest_version(br#"{"version":7}"#).is_none());
    }

    #[test]
    fn the_notice_names_the_channel_command_or_the_releases_page() {
        let latest = Version::parse("0.81.0").unwrap();
        let brew = notice(latest, InstallSource::HomebrewFormula);
        assert_eq!(
            brew.resolve("en-US"),
            "Cuotai 0.81.0 is available: brew upgrade cuotai"
        );
        let direct = notice(latest, InstallSource::Direct);
        assert!(direct.resolve("en-US").contains(RELEASES_URL));
    }

    #[test]
    fn a_fresh_cache_answers_without_a_request_and_a_stale_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai").join("update-check.json");
        write_cache(&path, at(1), Version::parse("0.81.0"));
        assert_eq!(read_cache(&path, at(1)), Some(Version::parse("0.81.0")));
        assert_eq!(
            read_cache(&path, at(1) + Duration::hours(23)),
            Some(Version::parse("0.81.0"))
        );
        assert_eq!(read_cache(&path, at(3)), None, "older than 24 hours");
        assert_eq!(read_cache(&path, at(1) - Duration::hours(1)), None);
        // Only a version and a date are stored.
        let stored = std::fs::read_to_string(&path).unwrap();
        assert!(stored.contains("0.81.0") && !stored.contains("http"));
    }

    #[tokio::test]
    async fn a_newer_manifest_version_is_reported_once_per_day() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/latest.json"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"version": "9.9.9"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("update-check.json");
        let url = format!("{}/latest.json", server.uri());
        let first = newer_version(&url, Some(&cache), at(1), "0.80.0").await;
        assert_eq!(first, Version::parse("9.9.9"));
        // The second run within a day reads the cache: the mock expects one hit.
        let second = newer_version(&url, Some(&cache), at(1) + Duration::hours(2), "0.80.0").await;
        assert_eq!(second, Version::parse("9.9.9"));
        assert_eq!(
            newer_version(&url, Some(&cache), at(1), "9.9.9").await,
            None,
            "not newer than the running version"
        );
    }

    #[tokio::test]
    async fn failures_are_silent_and_cached() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("update-check.json");
        // Nothing listens on port 1.
        let url = "http://127.0.0.1:1/latest.json";
        assert_eq!(
            newer_version(url, Some(&cache), at(1), "0.80.0").await,
            None
        );
        assert_eq!(read_cache(&cache, at(1)), Some(None));
    }
}
