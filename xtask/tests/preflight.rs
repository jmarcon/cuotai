//! The commit-message half of the preflight against a real repository, and
//! the shape of the versioned hooks.

mod support;

use std::path::{Path, PathBuf};

use support::*;
use xtask::preflight::message;

/// A temporary message file, as Git hands it to `commit-msg`.
fn check(repo: &Path, text: &str) -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("COMMIT_EDITMSG");
    std::fs::write(&file, text).unwrap();
    message::check_file(repo, &file).map_err(|e| e.to_string())
}

fn noreply_repo() -> (tempfile::TempDir, PathBuf) {
    let (dir, repo) = support::repo();
    git(
        &repo,
        &["config", "user.email", "octo-dev@users.noreply.github.com"],
    );
    (dir, repo)
}

fn stage(repo: &Path, path: &str) {
    write_file(repo, path, "// staged\n");
    git(repo, &["add", "--", path]);
}

#[test]
fn a_conventional_message_passes() {
    let (_d, repo) = noreply_repo();
    let message = "chore(tooling): add hooks\n\nBody.\n\n\
                   Co-Authored-By: Bot <noreply@anthropic.com>\n";
    check(&repo, message).unwrap();
}

#[test]
fn a_bad_header_reuses_the_rule_of_the_gate() {
    let (_d, repo) = noreply_repo();
    let error = check(&repo, "added some hooks\n").unwrap_err();
    assert!(error.contains("rule 1"), "{error}");
}

#[test]
fn a_user_visible_commit_of_staged_product_files_needs_its_trailer() {
    let (_d, repo) = noreply_repo();
    stage(&repo, "crates/cuotai-core/src/lib.rs");
    let error = check(&repo, "feat(core): add a thing\n").unwrap_err();
    assert!(error.contains("rule 2"), "{error}");
    check(
        &repo,
        "feat(core): add a thing\n\nChangelog: You can do a thing.\n",
    )
    .unwrap();
    check(
        &repo,
        "feat(core): add a thing\n\nChangelog: skip internal only\n",
    )
    .unwrap();
}

#[test]
fn only_a_changelog_edit_may_stage_the_changelog() {
    let (_d, repo) = noreply_repo();
    stage(&repo, "CHANGELOG.md");
    let error = check(&repo, "chore: touch the changelog\n").unwrap_err();
    assert!(error.contains("rule 4"), "{error}");
    check(
        &repo,
        "docs(changelog): reword\n\nChangelog-Edit: typo in a released section\n",
    )
    .unwrap();
}

#[test]
fn a_release_commit_passes() {
    let (_d, repo) = noreply_repo();
    stage(&repo, "CHANGELOG.md");
    check(&repo, "chore(release): 0.80.0\n\nChangelog: skip release\n").unwrap();
}

#[test]
fn the_skip_marker_is_rejected_in_the_body() {
    let (_d, repo) = noreply_repo();
    let marker = format!("[{} {}]", "skip", "ci");
    let error = check(&repo, &format!("chore: a\n\nBody {marker} here.\n")).unwrap_err();
    assert!(error.contains("skip marker"), "{error}");
}

#[test]
fn a_real_address_is_rejected_and_masked() {
    let (_d, repo) = support::repo();
    git(&repo, &["config", "user.email", "alex@example.com"]);
    let error = check(&repo, "chore: a\n").unwrap_err();
    assert!(error.contains("a***@example.com"), "{error}");
    assert!(!error.contains("alex"), "{error}");
}

#[test]
fn comments_and_empty_messages_are_left_to_git() {
    let (_d, repo) = noreply_repo();
    check(&repo, "# only a comment\n").unwrap();
    check(&repo, "chore: a\n# not conventional, but a comment\n").unwrap();
}

#[test]
fn autosquash_and_merge_messages_are_not_checked() {
    let (_d, repo) = noreply_repo();
    check(&repo, "fixup! chore: a\n").unwrap();
    write_file(&repo, ".git/MERGE_HEAD", &format!("{}\n", head(&repo)));
    check(&repo, "Merge branch 'x'\n").unwrap();
}

#[test]
fn the_range_check_reads_the_recorded_identity() {
    let (_d, repo) = noreply_repo();
    commit(&repo, "chore: good", "docs/a.md", "A.\n");
    assert_eq!(message::check_range(&repo, "HEAD~1", "HEAD").unwrap(), 1);
    git(&repo, &["config", "user.email", "alex@example.com"]);
    commit(&repo, "chore: bad", "docs/b.md", "B.\n");
    let error = message::check_range(&repo, "HEAD~2", "HEAD")
        .unwrap_err()
        .to_string();
    assert!(error.contains("chore: bad"), "{error}");
    assert!(!error.contains("chore: good"), "{error}");
}

fn hooks_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../.githooks")
}

#[test]
fn every_hook_calls_the_preflight() {
    for (hook, mode) in [
        ("pre-commit", "preflight --commit"),
        ("commit-msg", "preflight --commit-msg \"$1\""),
        ("pre-push", "preflight --push --stdin-refs"),
    ] {
        let text = std::fs::read_to_string(hooks_dir().join(hook)).unwrap();
        assert!(text.starts_with("#!/bin/sh\n"), "{hook}");
        assert!(text.contains(mode), "{hook}");
    }
}

#[cfg(unix)]
#[test]
fn hooks_are_executable() {
    use std::os::unix::fs::PermissionsExt;
    for hook in ["pre-commit", "commit-msg", "pre-push"] {
        let mode = std::fs::metadata(hooks_dir().join(hook))
            .unwrap()
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0, "{hook} is not executable");
    }
}
