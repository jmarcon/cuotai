//! The commit-message half of the preflight: the `commit-msg` hook checks a
//! message before its commit exists, with the rules of the commit gate
//! ([`crate::changelog_gate::check_message`]), and `--push` applies the two
//! rules the gate leaves to the author (identity and the skip marker in the
//! whole message) to the commits of the range.

use std::path::Path;

use crate::commits::{self, Commit};
use crate::{changelog_gate, git, plan_links, Res};

/// The placeholder sha of a commit that does not exist yet.
const NO_SHA: &str = "staged";

/// What `git commit` keeps of a message file: everything before the
/// scissors line of `--verbose`, without comment lines.
pub fn clean_message(raw: &str, comment_char: char) -> String {
    let scissors = format!("{comment_char} ------------------------ >8 ------------------------");
    raw.lines()
        .take_while(|line| *line != scissors)
        .filter(|line| !line.starts_with(comment_char))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A message Git wrote that is folded away before anything is pushed.
fn is_autosquash(subject: &str) -> bool {
    ["fixup! ", "squash! ", "amend! "]
        .iter()
        .any(|prefix| subject.starts_with(prefix))
}

/// An address that reaches nobody by mistake: the GitHub noreply form of a
/// person or a bot, or a `noreply@` service address.
pub fn is_noreply(email: &str) -> bool {
    let email = email.trim().to_ascii_lowercase();
    email.ends_with("@users.noreply.github.com") || email.starts_with("noreply@")
}

/// `n***@example.com`: a failure must not reprint the address.
fn mask(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) => {
            let first = local.chars().next().unwrap_or('?');
            format!("{first}***@{domain}")
        }
        None => "an address without @".to_string(),
    }
}

/// The address of `Name <address>`.
fn bracketed(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once('<')?;
    let (email, _) = rest.split_once('>')?;
    Some(email)
}

/// Problems of a message as a whole. GitHub honors the skip marker anywhere
/// in a commit message, and a co-author is an identity too.
pub fn message_problems(commit: &Commit) -> Vec<String> {
    let mut out = Vec::new();
    if commits::has_skip_marker(&commit.message) {
        out.push(
            "the message carries GitHub's skip marker, which skips every workflow: write \
             \"the skip marker\" instead"
                .to_string(),
        );
    }
    for (key, value) in &commit.trailers.all {
        if key.eq_ignore_ascii_case("Co-Authored-By") {
            match bracketed(value) {
                Some(email) if is_noreply(email) => {}
                Some(email) => out.push(format!(
                    "'{key}' uses {}: co-authors use a noreply address",
                    mask(email)
                )),
                None => out.push(format!("'{key}' has no <address>")),
            }
        }
    }
    out
}

/// Problems of the author and committer addresses.
pub fn identity_problems(author: &str, committer: &str) -> Vec<String> {
    [("author", author), ("committer", committer)]
        .into_iter()
        .filter(|(_, email)| !is_noreply(email))
        .map(|(role, email)| {
            format!(
                "the {role} address {} is not a noreply address: set the repository's \
                 user.email to the GitHub noreply form (AGENTS.md, \"Commit identity\")",
                mask(email)
            )
        })
        .collect()
}

/// The address `git commit` is about to record for `GIT_AUTHOR_IDENT` or
/// `GIT_COMMITTER_IDENT`. Inside a hook it reflects `--author` too.
fn pending_email(repo: &Path, variable: &str) -> Res<String> {
    let ident = git::run(repo, &["var", variable])?;
    Ok(bracketed(&ident).unwrap_or_default().to_string())
}

/// The files of the commit being created (`-z`, so no path is quoted).
pub fn staged_files(repo: &Path) -> Res<Vec<String>> {
    let out = git::run(
        repo,
        &["diff", "--cached", "--name-only", "--no-renames", "-z"],
    )?;
    Ok(split_paths(&out))
}

/// Splits the output of a `-z` listing.
pub fn split_paths(listing: &str) -> Vec<String> {
    listing
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

/// The `commit-msg` check of the message at `file`.
pub fn check_file(repo: &Path, file: &Path) -> Res<()> {
    let raw = std::fs::read_to_string(file)?;
    let comment_char = git::run(repo, &["config", "--get", "core.commentChar"])
        .ok()
        .and_then(|value| value.trim().chars().next())
        .unwrap_or('#');
    let message = clean_message(&raw, comment_char);
    let Some(subject) = message.lines().find(|line| !line.trim().is_empty()) else {
        // Git itself aborts an empty message.
        return Ok(());
    };
    let merging = git::run(repo, &["rev-parse", "--quiet", "--verify", "MERGE_HEAD"]).is_ok();
    if merging || is_autosquash(subject) {
        return Ok(());
    }
    let commit = Commit::parse(NO_SHA, subject, &message);
    let files = staged_files(repo)?;
    let plans = plan_links::Plans::read(repo).ok();
    let mut problems = changelog_gate::check_message(&commit, &files, plans.as_ref());
    problems.extend(message_problems(&commit));
    problems.extend(identity_problems(
        &pending_email(repo, "GIT_AUTHOR_IDENT")?,
        &pending_email(repo, "GIT_COMMITTER_IDENT")?,
    ));
    if problems.is_empty() {
        return Ok(());
    }
    crate::err(format!(
        "commit message rejected ({} problem(s)):\n - {}\n\nAGENTS.md (\"How to deliver a \
         change\") has the template.",
        problems.len(),
        problems.join("\n - "),
    ))
}

/// Identity and skip marker of every non-merge commit of `base..head`.
pub fn check_range(repo: &Path, base: &str, head: &str) -> Res<usize> {
    let commits = commits::load(repo, &format!("{base}..{head}"))?;
    let mut problems = Vec::new();
    for commit in &commits {
        let identity = git::run(
            repo,
            &["log", "-1", "--format=%ae%x1f%ce", commit.sha.as_str()],
        )?;
        let (author, committer) = identity.trim().split_once('\u{1f}').unwrap_or(("", ""));
        let found = message_problems(commit)
            .into_iter()
            .chain(identity_problems(author, committer));
        problems.extend(found.map(|p| format!("{} {}: {p}", commit.short(), commit.subject)));
    }
    if problems.is_empty() {
        return Ok(commits.len());
    }
    crate::err(format!(
        "commit policy failed ({} problem(s)):\n - {}",
        problems.len(),
        problems.join("\n - ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(message: &str) -> Commit {
        let subject = message.lines().next().unwrap_or_default();
        Commit::parse(NO_SHA, subject, message)
    }

    #[test]
    fn comments_and_the_verbose_diff_are_not_the_message() {
        let raw = "chore: a\n\nBody.\n# Please enter the message\n\
                   # ------------------------ >8 ------------------------\n\
                   diff --git a/x b/x\n+Changelog: not a trailer\n";
        assert_eq!(clean_message(raw, '#'), "chore: a\n\nBody.");
        assert_eq!(clean_message("chore: a\n; note\n", ';'), "chore: a");
    }

    #[test]
    fn noreply_forms() {
        for good in [
            "1767705+octo-dev@users.noreply.github.com",
            "octo-dev@users.noreply.github.com",
            "49699333+dependabot[bot]@users.noreply.github.com",
            "noreply@github.com",
            "noreply@anthropic.com",
            "NoReply@Example.com",
        ] {
            assert!(is_noreply(good), "{good}");
        }
        let personal = format!("someone@{}", "gmail.com");
        for bad in [
            personal.as_str(),
            "alex@example.com",
            "noreply.alex@example.com",
            "",
        ] {
            assert!(!is_noreply(bad), "{bad}");
        }
    }

    #[test]
    fn a_problem_never_reprints_the_address() {
        let problems = identity_problems("alex@example.com", "noreply@github.com");
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("a***@example.com"), "{problems:?}");
        assert!(!problems[0].contains("alex"), "{problems:?}");
    }

    #[test]
    fn the_skip_marker_is_rejected_anywhere_in_the_message() {
        let marker = format!("[{} {}]", "skip", "ci");
        for message in [
            format!("chore: a {marker}"),
            format!("chore: a\n\nBody with {marker} inside."),
        ] {
            let problems = message_problems(&commit(&message));
            assert_eq!(problems.len(), 1, "{message}");
            assert!(problems[0].contains("skip marker"), "{problems:?}");
        }
        assert!(message_problems(&commit("chore: a\n\nWrite the skip marker.")).is_empty());
    }

    #[test]
    fn co_authors_need_a_noreply_address() {
        let good = "chore: a\n\nCo-Authored-By: Bot <noreply@anthropic.com>";
        assert!(message_problems(&commit(good)).is_empty());
        let bad = "chore: a\n\nCo-Authored-By: Alex <alex@example.com>";
        let problems = message_problems(&commit(bad));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("a***@example.com"), "{problems:?}");
        let none = "chore: a\n\nCo-Authored-By: Alex";
        assert!(message_problems(&commit(none))[0].contains("no <address>"));
    }

    #[test]
    fn autosquash_subjects_are_recognized() {
        assert!(is_autosquash("fixup! feat: a"));
        assert!(is_autosquash("squash! feat: a"));
        assert!(!is_autosquash("fix: a"));
    }
}
