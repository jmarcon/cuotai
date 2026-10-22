//! Release automation for Cuotai (PRD 0021 RF1–RF5): version/tag checks,
//! artifact manifest + checksums, signing state, release notes, the updater
//! manifest and the package-manager manifests. Library half; `main.rs` is a
//! thin CLI over it.

/// The changelog format itself (PRD 0019 RF7.4): one parser, compiled from
/// the core's file so the repository checks and the product read exactly
/// the same rules. It is dependency-free, which is what makes the `#[path]`
/// include possible (xtask must not drag the core's async tree into the
/// release jobs).
#[path = "../../crates/cuotai-core/src/changelog.rs"]
pub mod changelog;
pub mod changelog_gate;
pub mod check;
pub mod draft;
pub mod git;
pub mod manifest;
pub mod notes;
pub mod plan_links;
pub mod sbom;
pub mod signing;
pub mod updater;

use std::fmt;

/// Result type used across the crate.
pub type Res<T> = Result<T, Error>;

/// One user-facing failure. Messages never carry secrets, only file names,
/// field names and expected/actual values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("JSON serialization failed: {0}")]
    JsonWrite(#[from] serde_json::Error),
    #[error("invalid JSON in {context}: {source}")]
    Json {
        context: String,
        source: serde_json::Error,
    },
    #[error("invalid TOML in {context}: {source}")]
    Toml {
        context: String,
        source: toml::de::Error,
    },
}

/// Builds a plain-message error.
pub fn err<T>(msg: impl Into<String>) -> Res<T> {
    Err(Error::Msg(msg.into()))
}

/// Semantic version `X.Y.Z` (no pre-release part; pre-releases only exist as
/// `-rc.N` tag suffixes, see [`parse_tag`]).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Parses a strict `X.Y.Z` (semver: no leading zeros, no suffixes).
    pub fn parse(text: &str) -> Res<Version> {
        let parts: Vec<&str> = text.split('.').collect();
        if parts.len() != 3 {
            return err(format!(
                "version '{text}' must have exactly three numeric components (X.Y.Z)"
            ));
        }
        let mut nums = [0u64; 3];
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return err(format!(
                    "version '{text}' has a non-numeric component '{part}'"
                ));
            }
            if part.len() > 1 && part.starts_with('0') {
                return err(format!("version '{text}' has a leading zero in '{part}'"));
            }
            nums[i] = part.parse::<u64>().map_err(|_| {
                Error::Msg(format!("version '{text}' component '{part}' is too large"))
            })?;
        }
        Ok(Version {
            major: nums[0],
            minor: nums[1],
            patch: nums[2],
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Release channel: a stable `vX.Y.Z` tag or a `vX.Y.Z-rc.N` release
/// candidate (GitHub pre-release, never package-manager published).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Prerelease,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Prerelease => "prerelease",
        }
    }

    pub fn parse(text: &str) -> Res<Channel> {
        match text {
            "stable" => Ok(Channel::Stable),
            "prerelease" | "rc" => Ok(Channel::Prerelease),
            other => err(format!(
                "channel '{other}' is unknown (expected 'stable' or 'prerelease')"
            )),
        }
    }
}

/// A parsed release tag: `vX.Y.Z` or `vX.Y.Z-rc.N`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub version: Version,
    /// `Some(n)` for `-rc.n`.
    pub rc: Option<u32>,
}

impl Tag {
    pub fn channel(&self) -> Channel {
        match self.rc {
            Some(_) => Channel::Prerelease,
            None => Channel::Stable,
        }
    }
}

/// Parses a release tag. Rejects anything that is not exactly `vX.Y.Z` or
/// `vX.Y.Z-rc.N`; the returned character set is `[0-9.\-a-z]`, safe to pass
/// to `git` as an argument (never through a shell).
pub fn parse_tag(tag: &str) -> Res<Tag> {
    let Some(rest) = tag.strip_prefix('v') else {
        return err(format!(
            "tag '{tag}' must be 'vX.Y.Z' or 'vX.Y.Z-rc.N' (missing leading 'v')"
        ));
    };
    let (version_part, rc) = match rest.split_once("-rc.") {
        Some((v, n)) => (v, Some(n)),
        None => (rest, None),
    };
    let version = Version::parse(version_part)
        .map_err(|e| Error::Msg(format!("invalid tag '{tag}': {e}")))?;
    let rc =
        match rc {
            None => None,
            Some(n) => {
                if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                    return err(format!("tag '{tag}': rc number '{n}' is not numeric"));
                }
                if n.len() > 1 && n.starts_with('0') {
                    return err(format!("tag '{tag}': rc number '{n}' has a leading zero"));
                }
                Some(n.parse::<u32>().map_err(|_| {
                    Error::Msg(format!("tag '{tag}': rc number '{n}' is too large"))
                })?)
            }
        };
    Ok(Tag { version, rc })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stable_and_rc_tags() {
        let stable = parse_tag("v0.68.0").unwrap();
        assert_eq!(stable.version.to_string(), "0.68.0");
        assert_eq!(stable.channel(), Channel::Stable);

        let rc = parse_tag("v1.2.3-rc.4").unwrap();
        assert_eq!(rc.version.to_string(), "1.2.3");
        assert_eq!(rc.rc, Some(4));
        assert_eq!(rc.channel(), Channel::Prerelease);
    }

    #[test]
    fn rejects_bad_tags() {
        for bad in [
            "0.68.0",
            "v0.68",
            "v0.68.0.1",
            "v0.68.0-beta",
            "v0.68.0-rc",
            "v0.68.0-rc.x",
            "v0.68.0-rc.01",
            "v0.68.0 ",
            " v0.68.0",
            "v0.68.0-rc.1;rm -rf /",
            "v1.02.3",
            "",
        ] {
            assert!(parse_tag(bad).is_err(), "accepted bad tag: {bad:?}");
        }
    }

    #[test]
    fn parses_versions() {
        assert_eq!(Version::parse("0.0.0").unwrap().to_string(), "0.0.0");
        assert!(Version::parse("1.2").is_err());
        assert!(Version::parse("1.2.3.4").is_err());
        assert!(Version::parse("1.2.x").is_err());
    }
}
