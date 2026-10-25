//! PRD 0039 RF10: which build is this, between two releases?
//!
//! The version moves only when a release is prepared, so a build from `main`
//! after `v0.80.0` still says `0.80.0`. [`BUILD_DESCRIBE`] (the output of
//! `git describe --tags --long --dirty --always` at build time, empty without
//! a checkout) lets the About screens and `--version` say
//! `0.80.0 (dev abc1234)` for such a build and the plain `0.80.0` for a build
//! exactly at a release tag.
//!
//! The suffix is display-only (RF10.2): the updater, What's new and
//! `export::v1` keep comparing the plain `X.Y.Z` of `CARGO_PKG_VERSION`.

/// `git describe --tags --long --dirty --always` of the build's checkout; empty
/// when the crate was built without one (crates.io, a source archive).
pub const BUILD_DESCRIBE: &str = env!("CUOTAI_BUILD_DESCRIBE");

/// The suffix of a build that is not exactly a release: `dev abc1234`, or
/// `dev abc1234, modified` when the working tree had uncommitted changes.
/// `None` for an exact tag build, a build without Git and a malformed value.
pub fn dev_suffix(describe: &str) -> Option<String> {
    let describe = describe.trim();
    if describe.is_empty() {
        return None;
    }
    let (describe, dirty) = match describe.strip_suffix("-dirty") {
        Some(rest) => (rest, true),
        None => (describe, false),
    };
    let hash = match describe.rsplit_once("-g") {
        // `<tag>-<commits since the tag>-g<hash>`
        Some((head, hash)) => {
            let (_, commits) = head.rsplit_once('-')?;
            let commits: u32 = commits.parse().ok()?;
            if commits == 0 && !dirty {
                return None; // exactly at a tag: the release itself
            }
            hash
        }
        // No tag to count from: `--always` printed the abbreviated hash.
        None => describe,
    };
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(if dirty {
        format!("dev {hash}, modified")
    } else {
        format!("dev {hash}")
    })
}

/// `0.80.0` for a release build, `0.80.0 (dev abc1234)` for any other.
pub fn display_version_of(version: &str, describe: &str) -> String {
    match dev_suffix(describe) {
        Some(suffix) => format!("{version} ({suffix})"),
        None => version.to_owned(),
    }
}

/// The running build's version for display (never for comparison).
pub fn display_version() -> String {
    display_version_of(env!("CARGO_PKG_VERSION"), BUILD_DESCRIBE)
}

/// The running build's suffix alone (`dev abc1234`), if it has one.
pub fn build_suffix() -> Option<String> {
    dev_suffix(BUILD_DESCRIBE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exact_tag_or_no_git_shows_the_plain_version() {
        assert_eq!(display_version_of("0.80.0", ""), "0.80.0");
        assert_eq!(display_version_of("0.80.0", "v0.80.0-0-g1a2b3c4"), "0.80.0");
        assert_eq!(
            display_version_of("0.80.0", "v0.80.0-rc.1-0-g1a2b3c4"),
            "0.80.0",
            "a release candidate tag is a release build too"
        );
    }

    #[test]
    fn commits_after_the_tag_or_edits_show_a_dev_suffix() {
        assert_eq!(
            display_version_of("0.80.0", "v0.80.0-14-gabc1234"),
            "0.80.0 (dev abc1234)"
        );
        assert_eq!(
            display_version_of("0.80.0", "v0.80.0-0-gabc1234-dirty"),
            "0.80.0 (dev abc1234, modified)"
        );
        assert_eq!(
            display_version_of("0.80.0", "v0.80.0-3-gabc1234-dirty"),
            "0.80.0 (dev abc1234, modified)"
        );
    }

    #[test]
    fn without_a_tag_the_short_hash_is_the_identity() {
        assert_eq!(
            display_version_of("0.80.0", "abc1234"),
            "0.80.0 (dev abc1234)"
        );
        assert_eq!(
            display_version_of("0.80.0", "abc1234-dirty"),
            "0.80.0 (dev abc1234, modified)"
        );
    }

    #[test]
    fn anything_else_is_ignored_rather_than_shown() {
        for odd in ["fatal: not a git repository", "v1-x-gzzz", "v1-1-g", "  "] {
            assert_eq!(dev_suffix(odd), None, "{odd}");
        }
        // A tag name that itself contains `-g`.
        assert_eq!(
            dev_suffix("v0.80.0-go-2-gabc1234").as_deref(),
            Some("dev abc1234")
        );
    }

    #[test]
    fn the_running_build_formats_without_panicking() {
        let shown = display_version();
        assert!(shown.starts_with(env!("CARGO_PKG_VERSION")));
        assert_eq!(build_suffix().is_some(), shown.contains(" (dev "));
    }
}
