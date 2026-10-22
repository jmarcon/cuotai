//! Draft generator for the changelog's `[Unreleased]` section (PRD 0019
//! RF7.3): reads the commits since the last tag (or a given ref), groups
//! the user-visible ones by type and carries each commit's `Refs:` footer
//! and breaking marker into the draft.
//!
//! The output is a **draft** printed for a person to review and rewrite in
//! the user's voice (RF7.2): commit subjects are developer language and a
//! draft is never written to `CHANGELOG.md` by this crate.

use std::path::Path;

use crate::changelog::Category;
use crate::{err, git, Res};

/// One commit parsed into what a changelog entry needs.
struct Commit {
    sha: String,
    subject: String,
    breaking: bool,
    refs: Vec<String>,
}

impl Commit {
    /// The category the commit belongs to, or `None` when it has no effect
    /// for the user (`docs`, `test`, `style`, `refactor`, plain `chore`).
    fn category(&self) -> Option<Category> {
        let kind = self.subject.split(['(', '!', ':']).next()?;
        let scope = self
            .subject
            .split_once('(')
            .and_then(|(_, rest)| rest.split_once(')').map(|(scope, _)| scope));
        match kind {
            "feat" => Some(Category::Added),
            "fix" => {
                if matches!(scope, Some("deps") | Some("security")) {
                    Some(Category::Security)
                } else {
                    Some(Category::Fixed)
                }
            }
            "chore" if matches!(scope, Some("security")) => Some(Category::Security),
            "revert" => Some(Category::Fixed),
            _ => None,
        }
    }

    fn short_hash(&self) -> &str {
        self.sha.split_at(7.min(self.sha.len())).0
    }

    /// The draft line of this commit: the subject, the `**Breaking**`
    /// marker and the requirement references with the commit hash.
    fn entry(&self) -> String {
        let mut refs = self.refs.clone();
        refs.push(self.short_hash().to_string());
        format!(
            "- {}{} ({})",
            if self.breaking { "**Breaking**: " } else { "" },
            self.subject,
            refs.join(", ")
        )
    }
}

/// Builds the draft for the commits after `since` (default: the last
/// `v*` tag, or the commit that last raised the workspace version while
/// the repository has no tag yet) up to `HEAD`.
pub fn run(repo: &Path, since: Option<&str>) -> Res<String> {
    let since = match since {
        Some(ref_name) => ref_name.to_string(),
        None => last_release_point(repo)?,
    };
    let log = git::run(
        repo,
        &[
            "log",
            "--format=%H%x1f%s%x1f%B%x1e",
            &format!("{since}..HEAD"),
        ],
    )?;
    let mut commits = Vec::new();
    for record in log.split('\u{1e}') {
        let record = record.trim_start_matches(['\n', '\r']);
        if record.trim().is_empty() {
            continue;
        }
        let mut fields = record.split('\u{1f}');
        let sha = fields.next().unwrap_or_default().trim().to_string();
        let subject = fields.next().unwrap_or_default().to_string();
        let body = fields.next().unwrap_or_default().to_string();
        if sha.is_empty() || subject.is_empty() {
            continue;
        }
        let breaking =
            subject.contains('!') || body.lines().any(|line| line.starts_with("BREAKING CHANGE"));
        commits.push(Commit {
            sha,
            subject,
            breaking,
            refs: references(&body),
        });
    }
    commits.reverse(); // chronological, like the changelog's own order.
    Ok(render(&since, &commits))
}

/// The `Refs:` footers of a commit message (`PRD 0019 RF7.3`): every
/// `PRD NNNN RFx` a `Refs:` line names, in order.
fn references(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Refs:") else {
            continue;
        };
        // One footer may name several requirements: `Refs: PRD 0006 RF2,
        // PRD 0009 RF1`.
        for token in rest.split([',', ';']) {
            let token = token.trim_start_matches('-').trim();
            if token.starts_with("PRD ") {
                out.push(token.to_string());
            }
        }
    }
    out
}

/// The reference point of "since the last release": the newest `v*` tag,
/// or — while the repository has no tag (the tags start with PRD 0021's
/// pipeline) — the commit that last changed the workspace version in
/// `Cargo.toml`, whose section is already a dated one.
fn last_release_point(repo: &Path) -> Res<String> {
    let described = git::run(
        repo,
        &[
            "describe",
            "--tags",
            "--abbrev=0",
            "--match",
            "v[0-9]*",
            "HEAD",
        ],
    );
    if let Ok(tag) = described {
        let tag = tag.trim();
        if !tag.is_empty() {
            return Ok(tag.to_string());
        }
    }
    if let Ok(commit) = git::run(
        repo,
        &[
            "log",
            "-1",
            "--format=%H",
            "-G",
            "^version = ",
            "--",
            "Cargo.toml",
        ],
    ) {
        let commit = commit.trim();
        if !commit.is_empty() {
            return Ok(commit.to_string());
        }
    }
    let root = git::run(repo, &["rev-list", "--max-parents=0", "HEAD"])?;
    let root = root.trim();
    if root.is_empty() {
        return err("no commits found: is the repository initialized?");
    }
    Ok(root.to_string())
}

/// Renders the draft section. Categories keep the file's own order, an
/// empty draft says so instead of printing an empty heading.
fn render(since: &str, commits: &[Commit]) -> String {
    let mut out = format!("## [Unreleased]\n\n<!-- draft since {since} — review the wording before editing CHANGELOG.md -->\n");
    for category in Category::ALL {
        let entries: Vec<&Commit> = commits
            .iter()
            .filter(|commit| commit.category() == Some(category))
            .collect();
        if entries.is_empty() {
            continue;
        }
        out.push_str(&format!("\n### {}\n\n", category.title()));
        for commit in entries {
            out.push_str(&commit.entry());
            out.push('\n');
        }
    }
    if commits.iter().all(|commit| commit.category().is_none()) {
        out.push_str("\nNo user-visible changes.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(subject: &str, body: &str) -> Commit {
        Commit {
            sha: "0123456789abcdef".into(),
            subject: subject.into(),
            breaking: subject.contains('!')
                || body.lines().any(|line| line.starts_with("BREAKING CHANGE")),
            refs: references(body),
        }
    }

    #[test]
    fn groups_by_type_and_keeps_refs_and_breaking() {
        let commits = vec![
            commit("feat(cli): add --changelog", "Refs: PRD 0019 RF7.7"),
            commit("fix: wrong date", ""),
            commit("chore(security): update rustls", "Refs: PRD 0019 RF1.3"),
            commit("docs: rewrite readme", ""),
        ];
        let draft = render("v0.68.2", &commits);
        assert!(draft.starts_with("## [Unreleased]"));
        assert!(
            draft.contains("### Added\n\n- feat(cli): add --changelog (PRD 0019 RF7.7, 0123456)")
        );
        assert!(draft.contains("### Fixed\n\n- fix: wrong date (0123456)"));
        assert!(draft.contains(
            "### Security\n\n- chore(security): update rustls (PRD 0019 RF1.3, 0123456)"
        ));
        // A commit with no effect for the user never enters (RF7.2).
        assert!(!draft.contains("docs: rewrite readme"));
        assert!(!draft.contains("### Deprecated"));
    }

    #[test]
    fn breaking_commits_carry_the_marker() {
        let draft = render(
            "main",
            &[commit("fix!: breaking", "BREAKING CHANGE: run --once")],
        );
        assert!(draft.contains("- **Breaking**: fix!: breaking (0123456)"));
    }

    #[test]
    fn multiple_refs_lines_are_collected() {
        let commit = commit(
            "feat: many",
            "Refs: PRD 0006 RF2\nRefs: PRD 0009 RF1\n\nSigned-off-by: x",
        );
        assert_eq!(commit.refs, vec!["PRD 0006 RF2", "PRD 0009 RF1"]);
    }

    #[test]
    fn empty_draft_says_so() {
        let draft = render("v1", &[commit("test: add case", "")]);
        assert!(draft.contains("No user-visible changes."));
    }
}
