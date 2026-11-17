//! `cargo xtask release edit` (PRD 0039 F-02): a wording fix of the release
//! section, recorded in the release commit as a `Changelog-Entry:` trailer
//! instead of a hand edit of `CHANGELOG.md`.
//!
//! The command amends the release commit at `HEAD` of its release branch:
//! it adds the edit (replacing a recorded edit of the same entries),
//! re-renders the release from the parent commit with the same version and
//! section date, and commits the result with the same message plus the new
//! trailer. The line-level check of [`super::check_release`] and `release
//! prepare --refresh` re-apply the recorded edits, so the wording survives
//! both by construction.

use std::path::Path;

use super::prepare::{self, Options, Replay};
use super::{line_checked, section_date, Tree, LINE_CHECK_AFTER};
use crate::commits::{self, Commit, EntryEdit};
use crate::{err, git, Error, Res};

/// What `release edit` did.
#[derive(Debug)]
pub struct EditOutcome {
    pub version: crate::Version,
    /// Every recorded edit of the amended release commit.
    pub edits: Vec<EntryEdit>,
    /// Recorded edits the new one replaced (they named one of its entries).
    pub replaced: Vec<EntryEdit>,
    /// The re-rendered section.
    pub section: String,
    /// The amended commit.
    pub sha: String,
}

/// Trailer keys `release prepare` writes itself; every other trailer of the
/// release commit (a `Co-Authored-By:`) is kept as it is.
const GENERATED_KEYS: [&str; 3] = ["Changelog", "Version-Override", "Changelog-Entry"];

/// Amends the release commit at `HEAD` with `edit`.
pub fn run(repo: &Path, edit: EntryEdit) -> Res<EditOutcome> {
    let problems = commits::lint_sentence(&edit.text);
    if !problems.is_empty() {
        return err(format!(
            "the new sentence does not pass the changelog sentence check:\n - {}",
            problems.join("\n - ")
        ));
    }
    let sha = git::run(repo, &["rev-parse", "--verify", "HEAD^{commit}"])?
        .trim()
        .to_string();
    let message = git::run(repo, &["log", "-1", "--format=%B", &sha])?;
    let subject = message.lines().next().unwrap_or_default().to_string();
    let release = Commit::parse(&sha, &subject, &message);
    let Some(version) = release.release_version() else {
        return err(format!(
            "HEAD ({} '{subject}') is not a release commit: run release edit on the release branch, after `cargo xtask release prepare --branch`",
            &sha[..7]
        ));
    };
    if !line_checked(&version) {
        return err(format!(
            "{version} keeps the file-level check (releases up to {LINE_CHECK_AFTER}); recorded edits need a later release"
        ));
    }
    let branch = git::run(repo, &["branch", "--show-current"])?
        .trim()
        .to_string();
    let wanted = format!("release/v{version}");
    if branch != wanted {
        return err(format!(
            "release edit amends the release commit on {wanted}, but the current branch is '{}'",
            if branch.is_empty() {
                "(detached HEAD)"
            } else {
                &branch
            }
        ));
    }
    let staged = git::run(repo, &["diff", "--cached", "--name-only"])?;
    if !staged.trim().is_empty() {
        return err(format!(
            "the index already holds staged changes, which the amended release commit would include; unstage them first:\n{}",
            staged.trim_end()
        ));
    }

    let mut edits: Vec<EntryEdit> = Vec::new();
    let mut replaced = Vec::new();
    let mut placed = false;
    for old in &release.trailers.entry_edits {
        if old.ids.iter().any(|id| edit.ids.contains(id)) {
            replaced.push(old.clone());
            if !placed {
                edits.push(edit.clone());
                placed = true;
            }
        } else {
            edits.push(old.clone());
        }
    }
    if !placed {
        edits.push(edit.clone());
    }

    let tree = Tree::Rev(repo, sha.clone());
    let Some(date) = section_date(&tree.read("CHANGELOG.md")?, &version) else {
        return err(format!(
            "CHANGELOG.md at HEAD has no '## [{version}] - YYYY-MM-DD' heading"
        ));
    };
    let parent = format!("{sha}^");
    let opts = Options {
        date,
        dry_run: true,
        edits: Some(edits.clone()),
        trailers: extra_trailers(&release),
        ..Options::default()
    };
    let replay = Replay {
        version: version.clone(),
        overridden: release.trailers.version_override.is_some(),
    };
    let outcome = prepare::compute(
        repo,
        &Tree::Rev(repo, parent.clone()),
        &parent,
        &opts,
        Some(&replay),
    )?
    .ok_or_else(|| {
        Error::Msg(format!(
            "release prepare finds nothing to release on the parent of {}",
            &sha[..7]
        ))
    })?;

    let paths: Vec<&str> = outcome.changes.iter().map(|c| c.path.as_str()).collect();
    let mut status_args = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    status_args.extend(paths.iter().copied());
    let dirty = git::run(repo, &status_args)?;
    if !dirty.trim().is_empty() {
        return err(format!(
            "these release files have uncommitted changes; commit, discard or move them first:\n{}",
            dirty.trim_end()
        ));
    }
    prepare::write_changes(repo, &outcome.changes)?;
    let mut only = vec!["--"];
    only.extend(paths.iter().copied());
    super::git_with_message(
        repo,
        "CUOTAI_RELEASE_MSG",
        &outcome.message,
        &["commit", "--amend", "--cleanup=verbatim"],
        &only,
    )?;
    let amended = git::run(repo, &["rev-parse", "HEAD"])?.trim().to_string();
    super::check_release(repo, &amended).map_err(|e| {
        Error::Msg(format!(
            "the release commit was amended ({}), but it does not pass the release check:\n{e}",
            &amended[..7]
        ))
    })?;
    Ok(EditOutcome {
        version,
        edits,
        replaced,
        section: outcome.section,
        sha: amended,
    })
}

/// The release commit's trailers that `release prepare` does not write,
/// as `Key: value` lines, in order.
pub(crate) fn extra_trailers(release: &Commit) -> Vec<String> {
    release
        .trailers
        .all
        .iter()
        .filter(|(key, _)| !GENERATED_KEYS.contains(&key.as_str()))
        .map(|(key, value)| format!("{key}: {value}"))
        .collect()
}
