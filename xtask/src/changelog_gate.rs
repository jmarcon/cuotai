//! Pull-request changelog gate (PRD 0019 RF7.4): a commit that touches
//! `crates/` or `apps/` with a `feat` or `fix` subject must change
//! `CHANGELOG.md`, unless its body carries the `Changelog: skip` footer
//! with a reason (AGENTS.md).
//!
//! The gate is deliberately dumb about content: it asks whether the file
//! moved in the pull request and whether the commits that deliver user
//! visible work are accounted for. The reviewed wording is the author's
//! job, `cargo xtask changelog-draft` only suggests it.

use std::path::Path;

use crate::{err, git, Res};

/// One commit of the pull request, as read from `git log`.
struct Commit {
    sha: String,
    subject: String,
    body: String,
    files: Vec<String>,
}

impl Commit {
    /// `feat`, `feat(scope)`, `fix!`… — the two user-visible types.
    fn is_user_visible(&self) -> bool {
        let Some(kind) = self.subject.split(['(', '!', ':']).next() else {
            return false;
        };
        matches!(kind, "feat" | "fix")
    }

    fn touches_product(&self) -> bool {
        self.files
            .iter()
            .any(|path| path.starts_with("crates/") || path.starts_with("apps/"))
    }

    fn changes_changelog(&self) -> bool {
        self.files.iter().any(|path| path == "CHANGELOG.md")
    }

    /// `Changelog: skip` plus a reason (never the bare footer).
    fn has_skip_with_reason(&self) -> bool {
        self.body.lines().any(|line| {
            let Some(reason) = line.trim().strip_prefix("Changelog: skip") else {
                return false;
            };
            let reason = reason.trim_start_matches([':', '-', ' ']).trim();
            !reason.is_empty()
        })
    }
}

/// Fails when the pull request `base..head` carries a `feat`/`fix` commit
/// touching `crates/` or `apps/`, changes no changelog content, and
/// justifies no `Changelog: skip` footer with a reason. `base` and `head`
/// are refs (or SHAs) the checkout already has.
pub fn run(repo: &Path, base: &str, head: &str) -> Res<()> {
    let commits = commits(repo, base, head)?;
    // RF7.4 gates the pull request, not each commit: one changelog entry
    // may land in its own commit of the same pull request (RF7.3).
    if commits.iter().any(|commit| commit.changes_changelog()) {
        return Ok(());
    }
    let mut problems: Vec<String> = Vec::new();
    for commit in &commits {
        if !commit.is_user_visible() || !commit.touches_product() {
            continue;
        }
        if commit.has_skip_with_reason() {
            continue;
        }
        problems.push(format!(
            "{} {} touches crates/ or apps/ but the pull request changes no CHANGELOG.md and the commit justifies no 'Changelog: skip <reason>' footer",
            commit.sha, commit.subject
        ));
    }
    if !problems.is_empty() {
        return err(format!(
            "changelog gate failed ({} problem(s)):\n - {}",
            problems.len(),
            problems.join("\n - ")
        ));
    }
    Ok(())
}

/// The commits of `base..head`, oldest first, with their touched files.
fn commits(repo: &Path, base: &str, head: &str) -> Res<Vec<Commit>> {
    let range = format!("{base}..{head}");
    // 0x1f separates records, 0x1e the fields: a message may contain any
    // other character.
    let format = "--format=%H%x1f%s%x1f%B%x1e";
    let log = git::run(repo, &["log", format, &range])?;
    let mut out = Vec::new();
    for record in log.split('\u{1e}') {
        let record = record.trim_start_matches(['\n', '\r']);
        if record.trim().is_empty() {
            continue;
        }
        let mut fields = record.split('\u{1f}');
        let sha = fields.next().unwrap_or_default().trim().to_string();
        let subject = fields.next().unwrap_or_default().to_string();
        let body = fields.next().unwrap_or_default().to_string();
        if sha.is_empty() {
            continue;
        }
        let files = git::run(
            repo,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", &sha],
        )?;
        out.push(Commit {
            sha,
            subject,
            body,
            files: files.lines().map(str::to_string).collect(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(subject: &str, body: &str, files: &[&str]) -> Commit {
        Commit {
            sha: "abc1234".into(),
            subject: subject.into(),
            body: body.into(),
            files: files.iter().map(|f| (*f).to_string()).collect(),
        }
    }

    #[test]
    fn user_visible_types_are_feat_and_fix() {
        assert!(commit("feat(x): a", "", &[]).is_user_visible());
        assert!(commit("fix!: a", "", &[]).is_user_visible());
        assert!(commit("feat(oauth): a", "", &[]).is_user_visible());
        assert!(!commit("docs: a", "", &[]).is_user_visible());
        assert!(!commit("chore(deps): a", "", &[]).is_user_visible());
        assert!(!commit("refactor(core): a", "", &[]).is_user_visible());
    }

    #[test]
    fn skip_needs_a_reason() {
        assert!(commit("feat: a", "Changelog: skip: docs only", &[]).has_skip_with_reason());
        assert!(commit(
            "feat: a",
            "Refs: PRD 0019 RF7\nChangelog: skip - internal only",
            &[]
        )
        .has_skip_with_reason());
        assert!(!commit("feat: a", "Changelog: skip", &[]).has_skip_with_reason());
        assert!(!commit("feat: a", "Changelog: skip\n", &[]).has_skip_with_reason());
        assert!(
            !commit("feat: a", "changelog: skip: lowercase does not count", &[])
                .has_skip_with_reason()
        );
    }

    #[test]
    fn product_paths_and_changelog_are_recognized() {
        let product = commit("feat: a", "", &["crates/cuotai-core/src/lib.rs"]);
        assert!(product.touches_product());
        assert!(!product.changes_changelog());
        let doc = commit("feat: a", "", &["docs/plans/0000_control.md"]);
        assert!(!doc.touches_product());
        let both = commit(
            "fix: a",
            "",
            &["apps/desktop/src-tauri/src/x.rs", "CHANGELOG.md"],
        );
        assert!(both.touches_product() && both.changes_changelog());
    }
}
