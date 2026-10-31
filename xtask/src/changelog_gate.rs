//! The commit gate (PRD 0039 RF2; replaces the PRD 0019 RF7.4 gate under
//! the same `changelog-gate` command). Every non-merge commit of a pull
//! request (`base..head`) or of a push to `main` (`before..after`) is
//! checked against the commit contract of [`crate::commits`], and every
//! problem is reported together:
//!
//! 1. the header is conventional;
//! 2. a user-visible commit touching product paths says what changed
//!    (`Changelog: <sentence>`) or why not (`Changelog: skip <reason>`);
//! 3. a breaking commit carries its `BREAKING CHANGE:` migration;
//! 4. no commit but a release commit changes a version location, a
//!    workspace lockfile entry or `CHANGELOG.md` (a `docs(changelog)` commit
//!    with `Changelog-Edit:` may correct released prose);
//! 5. a release commit changes only the release file set and passes
//!    `release prepare --check`;
//! 6. no trailer carries a skip-CI marker;
//! 7. each sentence passes a light lint, and every `Refs:` plan reference
//!    exists.

use std::path::Path;

use crate::commits::{self, Commit, Reference};
use crate::release::{self, Tree};
use crate::{git, Res};

/// Paths whose change is user-visible (shipped code, licenses, notices).
fn is_product(path: &str) -> bool {
    path.starts_with("crates/")
        || path.starts_with("apps/")
        || path.starts_with("LICENSE")
        || path == "THIRD_PARTY_NOTICES"
}

/// RF2.2: the base of a push to `main` is its previous tip. When that commit
/// is not in the checkout (a force push), it is fetched from `origin`; when
/// it still cannot be recovered, or the push created the branch, the gate
/// fails closed instead of checking an empty range.
pub fn push_base(repo: &Path, before: &str) -> Res<String> {
    if before.is_empty() || before.bytes().all(|b| b == b'0') {
        return crate::err(
            "the push has no previous tip (it created the branch): the commit gate cannot \
             tell which commits are new; run it with an explicit --base",
        );
    }
    if !before.bytes().all(|b| b.is_ascii_hexdigit()) {
        return crate::err(format!("--before '{before}' is not a commit hash"));
    }
    let exists =
        |repo: &Path| git::run(repo, &["cat-file", "-e", &format!("{before}^{{commit}}")]).is_ok();
    if !exists(repo) {
        let _ = git::run(repo, &["fetch", "--no-tags", "--quiet", "origin", before]);
    }
    if !exists(repo) {
        return crate::err(format!(
            "the previous tip {before} of this push is not available (force push?) and could \
             not be fetched: the commit gate fails closed. Check the pushed commits by hand \
             with `cargo xtask changelog-gate --base <last good commit> --head <new tip>`"
        ));
    }
    Ok(before.to_string())
}

/// Checks `base..head`; returns how many commits were checked.
pub fn run(repo: &Path, base: &str, head: &str) -> Res<usize> {
    let commits = commits::load(repo, &format!("{base}..{head}"))?;
    let plans = crate::plan_links::Plans::read(repo).ok();
    let mut problems: Vec<String> = Vec::new();
    for commit in &commits {
        let files = commits::files(repo, &commit.sha)?;
        for problem in check_commit(repo, commit, &files, plans.as_ref())? {
            problems.push(format!("{} {}: {problem}", commit.short(), commit.subject));
        }
    }
    if !problems.is_empty() {
        return crate::err(format!(
            "commit gate failed ({} problem(s)):\n - {}\n\nAmend the commit message on your branch \
             (`git commit --amend` for the last commit, `git rebase --exec` for older ones) and \
             push again. AGENTS.md (\"How to deliver a change\") has the template.",
            problems.len(),
            problems.join("\n - ")
        ));
    }
    Ok(commits.len())
}

/// Whether `commit` may change `CHANGELOG.md`: a `docs(changelog)` commit with
/// `Changelog-Edit:` (the content of that edit is checked against its tree).
fn may_edit_changelog(commit: &Commit) -> bool {
    commit
        .header
        .as_ref()
        .is_ok_and(|h| h.kind == "docs" && h.scope.as_deref() == Some("changelog"))
        && commit.trailers.changelog_edit.is_some()
}

/// The rules that need only the message and the changed paths, not a commit
/// object: the `commit-msg` hook runs them before the commit exists.
pub fn check_message(
    commit: &Commit,
    files: &[String],
    plans: Option<&crate::plan_links::Plans>,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Err(problem) = &commit.header {
        out.push(format!("rule 1: {problem}"));
    }
    out.extend(commit.problems.iter().map(|p| format!("trailers: {p}")));

    if commit.release_version().is_none() {
        // Rule 2.
        if commit.is_user_visible()
            && files.iter().any(|f| is_product(f))
            && commit.trailers.sentences.is_empty()
            && commit.trailers.skip.is_none()
        {
            out.push(
                "rule 2: user-visible change without a changelog entry: add a \
                 'Changelog: <one sentence for the user>.' trailer, or 'Changelog: skip <reason>'"
                    .to_string(),
            );
        }
        // Rule 3 (an empty footer is already a trailer problem).
        if commit.is_breaking()
            && commit.trailers.breaking.is_none()
            && !commit
                .problems
                .iter()
                .any(|p| p.contains("BREAKING CHANGE"))
        {
            out.push(
                "rule 3: breaking commit without its migration: add 'BREAKING CHANGE: <what users must change>'"
                    .to_string(),
            );
        }
        // Rule 4, the parts visible without a tree.
        if files.iter().any(|f| f == "CHANGELOG.md") && !may_edit_changelog(commit) {
            out.push(
                "rule 4: changes CHANGELOG.md: entries are generated at release time from the \
                 'Changelog:' trailer; drop the CHANGELOG.md hunk (a prose fix of a released \
                 section is a 'docs(changelog)' commit with 'Changelog-Edit: <reason>')"
                    .to_string(),
            );
        }
        if commit.trailers.version_override.is_some() {
            out.push("rule 4: 'Version-Override:' belongs only in a release commit".to_string());
        }
    }
    // Rule 6.
    for (key, value) in &commit.trailers.all {
        if commits::has_skip_marker(&format!("{key}: {value}")) {
            out.push(format!(
                "rule 6: trailer '{key}' carries a skip-CI marker, which would skip CI on the release PR"
            ));
        }
    }
    // Rule 7.
    for sentence in &commit.trailers.sentences {
        out.extend(
            commits::lint_sentence(sentence)
                .into_iter()
                .map(|p| format!("rule 7: {p}")),
        );
    }
    if let Some(plans) = plans {
        for reference in &commit.trailers.refs {
            if let Reference::Plan { prd, rf } = reference {
                if let Some(problem) = plans.missing(prd, rf) {
                    out.push(format!("rule 7: Refs: PRD {prd} {rf}: {problem}"));
                }
            }
        }
    }
    out
}

/// Every problem of one commit, without the commit prefix.
fn check_commit(
    repo: &Path,
    commit: &Commit,
    files: &[String],
    plans: Option<&crate::plan_links::Plans>,
) -> Res<Vec<String>> {
    let mut out = check_message(commit, files, plans);
    if commit.release_version().is_none() {
        // Rule 4.
        for change in version_changes(repo, &commit.sha, files)? {
            out.push(format!(
                "rule 4: {change}; versions move only in the release commit of \
                 `cargo xtask release prepare`: drop this hunk"
            ));
        }
        if files.iter().any(|f| f == "CHANGELOG.md") && may_edit_changelog(commit) {
            if let Some(problem) = changelog_edit_problem(repo, &commit.sha)? {
                out.push(format!("rule 4: {problem}"));
            }
        }
    } else {
        // Rule 5.
        for file in files.iter().filter(|f| !release::in_release_set(f)) {
            out.push(format!(
                "rule 5: a release commit changes only the release file set, not {file}"
            ));
        }
        if let Err(problem) = release::check_release(repo, &commit.sha) {
            out.push(format!("rule 5: {problem}"));
        }
    }
    Ok(out)
}

/// The version values a commit changes (RF1.3), described for the message.
fn version_changes(repo: &Path, sha: &str, files: &[String]) -> Res<Vec<String>> {
    type Extract = fn(&str) -> Vec<String>;
    let watched: [(&str, Extract); 6] = [
        ("Cargo.toml", |t| {
            toml_path(t, &["workspace", "package", "version"])
        }),
        ("apps/desktop/src-tauri/Cargo.toml", |t| {
            toml_path(t, &["package", "version"])
        }),
        ("apps/desktop/src-tauri/tauri.conf.json", |t| {
            serde_json::from_str::<serde_json::Value>(t)
                .ok()
                .and_then(|v| v.get("version")?.as_str().map(str::to_string))
                .into_iter()
                .collect()
        }),
        ("crates/cuotai-tui/Cargo.toml", |t| {
            toml_path(t, &["dependencies", "cuotai-core", "version"])
        }),
        ("Cargo.lock", |t| {
            lock_versions(t, &["cuotai-core", "cuotai-tui"])
        }),
        ("apps/desktop/src-tauri/Cargo.lock", |t| {
            lock_versions(t, &["cuotai-core", "cuotai-desktop"])
        }),
    ];
    let parent = format!("{sha}^");
    if git::run(repo, &["rev-parse", "--verify", "--quiet", &parent]).is_err() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for (path, extract) in watched {
        if !files.iter().any(|f| f == path) {
            continue;
        }
        let before = Tree::Rev(repo, parent.clone())
            .read(path)
            .map(|t| extract(&t))
            .unwrap_or_default();
        let after = Tree::Rev(repo, sha.to_string())
            .read(path)
            .map(|t| extract(&t))
            .unwrap_or_default();
        if before != after {
            out.push(format!(
                "changes the version in {path} ({} -> {})",
                before.join(", "),
                after.join(", ")
            ));
        }
    }
    Ok(out)
}

fn toml_path(text: &str, keys: &[&str]) -> Vec<String> {
    let Ok(doc) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    let mut value = &doc;
    for key in keys {
        let Some(next) = value.get(key) else {
            return Vec::new();
        };
        value = next;
    }
    value.as_str().map(str::to_string).into_iter().collect()
}

fn lock_versions(text: &str, names: &[&str]) -> Vec<String> {
    let Ok(doc) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    let Some(packages) = doc.get("package").and_then(|p| p.as_array()) else {
        return Vec::new();
    };
    names
        .iter()
        .flat_map(|name| {
            packages
                .iter()
                .filter(move |p| p.get("name").and_then(|n| n.as_str()) == Some(*name))
                .filter_map(move |p| {
                    p.get("version")
                        .and_then(|v| v.as_str())
                        .map(|v| format!("{name} {v}"))
                })
        })
        .collect()
}

/// A `Changelog-Edit` commit may reword released sections and the
/// preface/footer, but never touch `[Unreleased]`, add or remove a section
/// or change how many entries a section has. The one structural edit it may
/// make is folding internal versions into the pre-release history.
fn changelog_edit_problem(repo: &Path, sha: &str) -> Res<Option<String>> {
    let parse = |rev: String| -> Option<crate::changelog::Changelog> {
        let text = Tree::Rev(repo, rev).read("CHANGELOG.md").ok()?;
        crate::changelog::parse(&text).ok()
    };
    let (Some(before), Some(after)) = (parse(format!("{sha}^")), parse(sha.to_string())) else {
        return Ok(Some(
            "the CHANGELOG.md edit leaves a file the shared parser rejects".to_string(),
        ));
    };
    if before.unreleased != after.unreleased {
        return Ok(Some(
            "a Changelog-Edit commit must not touch [Unreleased]".to_string(),
        ));
    }
    let shape = |c: &crate::changelog::Changelog| -> Vec<(String, usize)> {
        c.releases
            .iter()
            .map(|r| {
                (
                    r.version.clone(),
                    r.section.categories.iter().map(|b| b.entries.len()).sum(),
                )
            })
            .collect()
    };
    // The pre-release history (PRD 0039 D11) may be introduced, folding the
    // internal versions' sections into it, and reworded freely (it is a
    // summary), but never removed, re-ranged or widened over a published
    // version.
    match (&before.pre_release, &after.pre_release) {
        (Some(_), None) => {
            return Ok(Some(
                "a Changelog-Edit commit must not remove the pre-release history".to_string(),
            ))
        }
        (Some(old), Some(new)) if (&old.first, &old.last) != (&new.first, &new.last) => {
            return Ok(Some(format!(
                "a Changelog-Edit commit must not change the pre-release range ({} -> {})",
                old.range(),
                new.range()
            )))
        }
        _ => {}
    }
    let folded = |version: &str| after.pre_release_of(version).is_some();
    for release in before.releases.iter().filter(|r| folded(&r.version)) {
        if !crate::Version::parse(&release.version)
            .is_ok_and(|v| v <= release::LAST_INTERNAL_VERSION)
        {
            return Ok(Some(format!(
                "[{}] is a published version: only internal versions (up to {}) fold into the pre-release history",
                release.version,
                release::LAST_INTERNAL_VERSION
            )));
        }
    }
    let mut kept = shape(&before);
    kept.retain(|(version, _)| !folded(version));
    if kept != shape(&after) {
        return Ok(Some(
            "a Changelog-Edit commit may reword released entries but not add or remove sections or entries"
                .to_string(),
        ));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_paths() {
        assert!(is_product("crates/cuotai-core/src/lib.rs"));
        assert!(is_product("apps/desktop/ui/index.html"));
        assert!(is_product("LICENSE-MIT"));
        assert!(is_product("THIRD_PARTY_NOTICES"));
        assert!(!is_product("docs/plans/0000_control.md"));
        assert!(!is_product("xtask/src/main.rs"));
    }

    #[test]
    fn lock_and_toml_extraction() {
        let lock = "[[package]]\nname = \"cuotai-core\"\nversion = \"0.79.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n";
        assert_eq!(
            lock_versions(lock, &["cuotai-core", "cuotai-tui"]),
            ["cuotai-core 0.79.0"]
        );
        let tui = "[dependencies]\ncuotai-core = { path = \"x\", version = \"0.79\" }\n";
        assert_eq!(
            toml_path(tui, &["dependencies", "cuotai-core", "version"]),
            ["0.79"]
        );
    }
}
