//! `cargo xtask release tag [--rc]` (PRD 0039 RF5.2/RF5.3): creates the
//! **local** annotated tag of the newest release commit on `main`, after
//! re-running `release prepare --check` and `release-check --tag` against
//! it. Pushing the tag (which starts `release.yml`) stays a separate,
//! human step: this command only prints `git push origin <tag>`.

use std::path::Path;

use super::check_release;
use crate::{err, git, parse_tag, Error, Res};

/// The created tag and the commit it points at.
#[derive(Debug)]
pub struct Tagged {
    pub tag: String,
    pub sha: String,
}

/// Tags the newest release commit reachable from `HEAD`, which must be
/// contained in `main_ref` (normally `origin/main`).
pub fn run(repo: &Path, rc: bool, main_ref: &str) -> Res<Tagged> {
    git::run(
        repo,
        &["rev-parse", "--verify", &format!("{main_ref}^{{commit}}")],
    )
    .map_err(|_| {
        Error::Msg(format!(
            "'{main_ref}' is not available: run `git fetch origin`"
        ))
    })?;
    let log = git::run(repo, &["log", "--format=%H%x1f%s", "HEAD"])?;
    let Some((sha, subject)) = log.lines().find_map(|line| {
        let (sha, subject) = line.split_once('\u{1f}')?;
        crate::commits::release_version(subject).map(|_| (sha.to_string(), subject.to_string()))
    }) else {
        return err("no release commit ('chore(release): X.Y.Z') in the history of HEAD");
    };
    let contained = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["merge-base", "--is-ancestor", &sha, main_ref])
        .status()
        .map_err(|e| Error::Msg(format!("cannot run git merge-base: {e}")))?;
    if contained.code() != Some(0) {
        return err(format!(
            "release commit {} '{subject}' is not on {main_ref}: merge the release PR first",
            &sha[..7]
        ));
    }
    let checked = check_release(repo, &sha)?;
    let version = checked.version;
    if git::run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/tags/v{version}"),
        ],
    )
    .is_ok()
    {
        return err(format!(
            "v{version} is already tagged: a published tag never moves"
        ));
    }
    let tag = if rc {
        let existing = git::run(repo, &["tag", "--list", &format!("v{version}-rc.*")])?;
        let highest = existing
            .lines()
            .filter_map(|name| parse_tag(name.trim()).ok())
            .filter(|t| t.version == version)
            .filter_map(|t| t.rc)
            .max()
            .unwrap_or(0);
        format!("v{version}-rc.{}", highest + 1)
    } else {
        format!("v{version}")
    };
    // The full `release-check` of `release.yml` (root and desktop manifests,
    // tauri.conf.json, both lockfiles, the cuotai-tui requirement and the
    // changelog section), against the selected release commit rather than
    // the caller's working tree; `check_release` above covered the
    // citations and the plan tokens.
    let report = crate::check::run_files(&crate::check::Files::Rev(repo, &sha), Some(&tag))
        .map_err(|e| Error::Msg(format!("release commit {}: {e}", &sha[..7])))?;
    let (date, body) = (report.changelog_date, report.changelog_body);
    let message = format!("{tag}\n\n## [{version}] - {date}\n\n{body}\n");
    let message = crate::commits::neutralize_skip_markers(&message) + "\n";
    super::git_with_message(
        repo,
        "CUOTAI_TAG_MSG",
        &message,
        &["tag", "-a", "--cleanup=verbatim"],
        &[&tag, &sha],
    )?;
    git::check_tag_on_main(repo, &tag, main_ref)?;
    Ok(Tagged { tag, sha })
}
