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
        "feat(core): add a thing\n\nChangelog: skip internal only, no visible effect\n",
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

/// The pre-push hook stamps its start before building xtask, so the
/// preflight can stop a run that outlived git's connection (SIGPIPE, 141).
#[test]
fn the_pre_push_hook_stamps_its_start_before_the_preflight() {
    let text = std::fs::read_to_string(hooks_dir().join("pre-push")).unwrap();
    let stamp = text
        .find("export CUOTAI_HOOK_STARTED")
        .expect("exported start");
    let source = text.find(". \"$(dirname \"$0\")/lib.sh\"").expect("lib.sh");
    let run = text.find("preflight --push --stdin-refs").unwrap();
    assert!(stamp < source && source < run, "{text}");
    assert!(text.contains("CUOTAI_HOOK_STARTED=$(date +%s)"), "{text}");
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

/// `preflight --push --dry-run --base <base>` in `repo`: its standard output.
fn push_dry_run(repo: &Path, base: &str) -> String {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["preflight", "--push", "--dry-run", "--base", base])
        .current_dir(repo)
        .env_remove("CUOTAI_SKIP")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

/// T-02: a lone release commit verified line by line runs only the release
/// checks; a range with more than the release commit, or a hand-edited
/// release commit, falls back to the full selection.
#[test]
fn push_of_a_verified_release_commit_runs_only_the_release_checks() {
    let (_d, repo) = repo_at("0.86.7");
    feature_and_fix(&repo);
    xtask::release::prepare::run(&repo, &options(false, true))
        .unwrap()
        .unwrap();
    // The release commit is the only commit of main..HEAD.
    let stdout = push_dry_run(&repo, "main");
    assert!(
        stdout.contains(
            "release commit 0.87.0 verified line by line: release checks only (clippy, tests and deny skipped: only version lines, the section and plan tokens changed)"
        ),
        "{stdout}"
    );
    assert!(
        stdout.contains(
            "preflight --push: 5 step(s) for 9 file(s)\n  commit-policy\n  changelog-gate\n  changelog-check\n  release-check\n  release-prepare-check\ndry run: --push ran nothing"
        ),
        "{stdout}"
    );

    // Two commits in the range: the full selection, with no claim.
    let stdout = push_dry_run(&repo, "main~1");
    assert!(!stdout.contains("verified line by line"), "{stdout}");
    assert!(!stdout.contains("release-prepare-check"), "{stdout}");
    assert!(stdout.contains("  test"), "{stdout}");

    // A hand edit of the section: the full selection again, and why.
    let changelog = read(&repo, "CHANGELOG.md").replace("The order survives", "Order survives");
    amend(&repo, "CHANGELOG.md", &changelog);
    let stdout = push_dry_run(&repo, "main");
    assert!(
        stdout.contains("the release commit is not verified line by line (CHANGELOG.md:"),
        "{stdout}"
    );
    assert!(!stdout.contains("release-prepare-check"), "{stdout}");
    assert!(stdout.contains("  deny"), "{stdout}");
}
