//! Temp-repository tests of the commit gate (PRD 0039 RF2 acceptance).

mod support;

use std::path::Path;

use support::*;

fn gate(repo: &Path, base: &str) -> Result<usize, String> {
    xtask::changelog_gate::run(repo, base, "HEAD").map_err(|e| e.to_string())
}

#[test]
fn a_push_without_a_recoverable_previous_tip_fails_closed() {
    let (_d, repo) = support::repo();
    let tip = head(&repo);
    let error = xtask::changelog_gate::push_base(&repo, &"0".repeat(40))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no previous tip"), "{error}");
    // Gone from the history and no origin to fetch it from.
    let unknown = "1234567".repeat(6)[..40].to_string();
    let error = xtask::changelog_gate::push_base(&repo, &unknown)
        .unwrap_err()
        .to_string();
    assert!(error.contains("fails closed"), "{error}");
    assert_eq!(xtask::changelog_gate::push_base(&repo, &tip).unwrap(), tip);
}

#[test]
fn a_previous_tip_missing_locally_is_fetched_from_origin() {
    let (_d, origin) = support::repo();
    let dir = tempfile::tempdir().unwrap();
    let clone = dir.path().join("clone");
    let status = std::process::Command::new("git")
        .args(["clone", "-q"])
        .arg(&origin)
        .arg(&clone)
        .status()
        .unwrap();
    assert!(status.success());
    // The commit exists only in origin until the gate fetches it.
    let missing = commit(&origin, "docs: later", "docs/later.md", "Later.\n");
    let present = std::process::Command::new("git")
        .arg("-C")
        .arg(&clone)
        .args(["cat-file", "-e", &format!("{missing}^{{commit}}")])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(!present.success());
    assert_eq!(
        xtask::changelog_gate::push_base(&clone, &missing).unwrap(),
        missing
    );
}

#[test]
fn feature_with_a_trailer_passes() {
    let (_d, repo) = support::repo();
    let (_feat, _fix) = feature_and_fix(&repo);
    assert_eq!(gate(&repo, "HEAD~2").unwrap(), 2);
}

#[test]
fn feature_without_a_trailer_fails() {
    let (_d, repo) = support::repo();
    commit(
        &repo,
        "feat(core): add a thing",
        "crates/cuotai-core/src/lib.rs",
        "// v2\n",
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("feat(core): add a thing: rule 2"), "{error}");
    assert!(error.contains("Changelog: skip <reason>"), "{error}");
}

#[test]
fn documentation_feature_without_a_trailer_passes() {
    let (_d, repo) = support::repo();
    commit(
        &repo,
        "feat(docs): document a thing",
        "README.md",
        "# docs\n",
    );
    gate(&repo, "HEAD~1").unwrap();
}

#[test]
fn skip_with_a_reason_passes_and_without_fails() {
    let (_d, repo) = support::repo();
    commit(
        &repo,
        "fix(core): internal\n\nChangelog: skip no user-visible effect",
        "crates/cuotai-core/src/lib.rs",
        "// v2\n",
    );
    gate(&repo, "HEAD~1").unwrap();
    commit(
        &repo,
        "fix(core): internal again\n\nChangelog: skip",
        "crates/cuotai-core/src/lib.rs",
        "// v3\n",
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("needs a reason"), "{error}");
}

#[test]
fn a_version_bump_in_a_feature_branch_fails() {
    let (_d, repo) = support::repo();
    let cargo = read(&repo, "Cargo.toml").replace("0.79.0", "0.80.0");
    commit(
        &repo,
        "feat(core): bump\n\nChangelog: Something new.",
        "Cargo.toml",
        &cargo,
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(
        error.contains("rule 4: changes the version in Cargo.toml"),
        "{error}"
    );
    // A Cargo.toml edit that keeps the version is fine.
    let (_d, repo) = support::repo();
    let cargo = read(&repo, "Cargo.toml") + "\n[workspace.dependencies]\n";
    commit(&repo, "chore(deps): add a table", "Cargo.toml", &cargo);
    gate(&repo, "HEAD~1").unwrap();
}

#[test]
fn changelog_edits_need_a_changelog_edit_commit() {
    let (_d, repo) = support::repo();
    let changelog = read(&repo, "CHANGELOG.md");
    commit(
        &repo,
        "feat(core): x\n\nChangelog: Something.",
        "CHANGELOG.md",
        &changelog.replace(
            "## [Unreleased]\n",
            "## [Unreleased]\n\n### Added\n\n- Hand-written (abc1234).\n",
        ),
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("rule 4: changes CHANGELOG.md"), "{error}");

    let (_d, repo) = support::repo();
    commit(
        &repo,
        "docs(changelog): reword the base entry\n\nChangelog-Edit: clearer wording",
        "CHANGELOG.md",
        &changelog.replace("- Base (", "- The base release ("),
    );
    gate(&repo, "HEAD~1").unwrap();
    commit(
        &repo,
        "docs(changelog): add an entry\n\nChangelog-Edit: forgotten",
        "CHANGELOG.md",
        &read(&repo, "CHANGELOG.md")
            .replace("### Added\n\n", "### Added\n\n- Added later (abc1234).\n"),
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("not add or remove"), "{error}");
}

#[test]
fn a_release_commit_from_prepare_passes_and_a_hand_edited_one_fails() {
    let (_d, repo) = support::repo();
    feature_and_fix(&repo);
    xtask::release::prepare::run(&repo, &options(false, true))
        .unwrap()
        .unwrap();
    gate(&repo, "main").unwrap();
    amend(&repo, "crates/cuotai-core/src/lib.rs", "// sneaked in\n");
    let error = gate(&repo, "main").unwrap_err();
    assert!(
        error.contains("rule 5: a release commit changes only the release file set, not crates/cuotai-core/src/lib.rs"),
        "{error}"
    );
}

#[test]
fn breaking_commits_need_the_migration() {
    let (_d, repo) = support::repo();
    commit(
        &repo,
        "feat(core)!: rename\n\nChangelog: Accounts are renamed.",
        "crates/cuotai-core/src/lib.rs",
        "// v2\n",
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("rule 3"), "{error}");
    commit(
        &repo,
        "feat(core)!: rename again\n\nChangelog: Accounts are renamed.\nBREAKING CHANGE: rename the [accounts] table.",
        "crates/cuotai-core/src/lib.rs",
        "// v3\n",
    );
    gate(&repo, "HEAD~1").unwrap();
}

#[test]
fn skip_markers_in_trailers_unknown_refs_and_bad_headers_fail() {
    let (_d, repo) = support::repo();
    let marker = format!("[{} {}]", "skip", "ci");
    commit(
        &repo,
        &format!("fix(core): a\n\nChangelog: A fix {marker}."),
        "crates/cuotai-core/src/lib.rs",
        "// v2\n",
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("rule 6"), "{error}");

    let (_d, repo) = support::repo();
    commit(
        &repo,
        "fix(core): a\n\nChangelog: A fix.\nRefs: PRD 0001 RF9",
        "crates/cuotai-core/src/lib.rs",
        "// v2\n",
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("has no RF9"), "{error}");

    commit(&repo, "Update stuff", "docs/x.md", "x\n");
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("rule 1"), "{error}");

    commit(
        &repo,
        "fix(core): b\n\nChangelog: lower case and no period",
        "crates/cuotai-core/src/lib.rs",
        "// v3\n",
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(
        error.contains("capital letter") && error.contains("end with a period"),
        "{error}"
    );
}

#[test]
fn folding_internal_versions_into_the_pre_release_history_is_a_changelog_edit() {
    let (_d, repo) = support::repo();
    commit(
        &repo,
        "docs(changelog): consolidate internal versions\n\nChangelog-Edit: owner decision\nChangelog: skip internal history only",
        "CHANGELOG.md",
        PRE_RELEASE_CHANGELOG,
    );
    gate(&repo, "HEAD~1").unwrap();
    // Rewording the summary, entry count included, stays an edit.
    commit(
        &repo,
        "docs(changelog): reword the history\n\nChangelog-Edit: clearer summary",
        "CHANGELOG.md",
        &PRE_RELEASE_CHANGELOG.replace(
            "- Base (PRD 0001 RF1).\n",
            "- The base (PRD 0001 RF1).\n- More.\n",
        ),
    );
    gate(&repo, "HEAD~1").unwrap();
    // Re-ranging or removing it is not.
    commit(
        &repo,
        "docs(changelog): widen the history\n\nChangelog-Edit: wrong",
        "CHANGELOG.md",
        &PRE_RELEASE_CHANGELOG.replace("0.1.0–0.79.0", "0.1.0–0.78.0"),
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("pre-release range"), "{error}");
    let (_d, repo) = support::repo();
    commit(
        &repo,
        "docs(changelog): consolidate\n\nChangelog-Edit: owner decision",
        "CHANGELOG.md",
        PRE_RELEASE_CHANGELOG,
    );
    commit(
        &repo,
        "docs(changelog): drop the history\n\nChangelog-Edit: wrong",
        "CHANGELOG.md",
        &support::CHANGELOG.replace("Preface.", "Preface again."),
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(
        error.contains("must not remove the pre-release history"),
        "{error}"
    );
}

#[test]
fn a_published_version_never_folds_into_the_pre_release_history() {
    let (_d, repo) = support::repo();
    let published = support::CHANGELOG.replace(
        "## [0.79.0]",
        "## [0.80.0] - 2026-10-24\n\n### Added\n\n- Published (2222222).\n\n## [0.79.0]",
    );
    commit(
        &repo,
        "docs(changelog): fixture with a release\n\nChangelog-Edit: fixture",
        "CHANGELOG.md",
        &published,
    );
    commit(
        &repo,
        "docs(changelog): fold everything\n\nChangelog-Edit: wrong",
        "CHANGELOG.md",
        &PRE_RELEASE_CHANGELOG.replace("0.1.0–0.79.0", "0.1.0–0.80.0"),
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("[0.80.0] is a published version"), "{error}");
    // Folding only the internal ones keeps the published section intact.
    let kept = PRE_RELEASE_CHANGELOG.replace(
        "## Pre-release history",
        "## [0.80.0] - 2026-10-24\n\n### Added\n\n- Published (2222222).\n\n## Pre-release history",
    );
    git(&repo, &["reset", "-q", "--hard", "HEAD~1"]);
    commit(
        &repo,
        "docs(changelog): fold the internal ones\n\nChangelog-Edit: owner decision",
        "CHANGELOG.md",
        &kept,
    );
    gate(&repo, "HEAD~1").unwrap();
}

fn gate_with(
    repo: &Path,
    base: &str,
    head: &str,
    since: &[(&'static str, &str)],
) -> Result<usize, String> {
    xtask::changelog_gate::run_with(repo, base, head, since).map_err(|e| e.to_string())
}

const UI_PATH: &str = "Changelog: Open Settings > About to see it.";

#[test]
fn a_scoped_check_spares_the_commits_written_before_it() {
    let (_d, repo) = support::repo();
    let base = head(&repo);
    commit(
        &repo,
        &format!("fix(core): old wording\n\n{UI_PATH}"),
        "crates/cuotai-core/src/lib.rs",
        "// v2\n",
    );
    let marker = commit(&repo, "docs: the rule lands", "docs/x.md", "x\n");
    commit(
        &repo,
        &format!("fix(core): new wording\n\n{UI_PATH}"),
        "crates/cuotai-core/src/lib.rs",
        "// v3\n",
    );
    let error = gate_with(&repo, &base, "HEAD", &[("ui-path", &marker)]).unwrap_err();
    assert!(error.contains("fix(core): new wording: rule 7:"), "{error}");
    assert!(!error.contains("old wording"), "{error}");
    assert!(
        error.contains("commit gate failed (1 problem(s))"),
        "{error}"
    );
    // Without the scoping entry both fail.
    let error = gate_with(&repo, &base, "HEAD", &[]).unwrap_err();
    assert!(
        error.contains("old wording") && error.contains("new wording"),
        "{error}"
    );
    // Only the old one: it passes.
    gate_with(&repo, &base, "HEAD~2", &[("ui-path", &marker)]).unwrap();
}

#[test]
fn a_scoping_commit_missing_from_the_clone_applies_the_check() {
    let (_d, origin) = support::repo();
    let base = head(&origin);
    commit(
        &origin,
        &format!("fix(core): old wording\n\n{UI_PATH}"),
        "crates/cuotai-core/src/lib.rs",
        "// v2\n",
    );
    git(&origin, &["branch", "feature"]);
    let marker = commit(&origin, "docs: the rule lands", "docs/x.md", "x\n");
    git(&origin, &["switch", "-q", "feature"]);
    commit(
        &origin,
        &format!("fix(core): new wording\n\n{UI_PATH}"),
        "crates/cuotai-core/src/lib.rs",
        "// v3\n",
    );
    git(&origin, &["switch", "-q", "main"]);
    let since = [("ui-path", marker.as_str())];
    // With the marker present, only the new commit fails.
    let error = gate_with(&origin, &base, "feature", &since).unwrap_err();
    assert!(!error.contains("old wording"), "{error}");
    // A clone of the feature branch alone lacks the marker: fail closed.
    let dir = tempfile::tempdir().unwrap();
    let clone = dir.path().join("clone");
    let status = std::process::Command::new("git")
        .args(["clone", "-q", "--single-branch", "--branch", "feature"])
        .arg(format!("file://{}", origin.display()))
        .arg(&clone)
        .status()
        .unwrap();
    assert!(status.success());
    let error = gate_with(&clone, &base, "HEAD", &since).unwrap_err();
    assert!(
        error.contains("old wording") && error.contains("new wording"),
        "{error}"
    );
    // The real table names commits of the Cuotai history, absent here.
    let error = gate(&clone, &base).unwrap_err();
    assert!(error.contains("old wording"), "{error}");
}

#[test]
fn problems_carry_one_label_each_and_the_footer_fits() {
    let (_d, repo) = support::repo();
    let cargo = read(&repo, "Cargo.toml").replace("0.79.0", "0.80.0");
    commit(
        &repo,
        "feat(core): bump\n\nChangelog: Something new.",
        "Cargo.toml",
        &cargo,
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(error.contains("Rule 4: drop the version"), "{error}");
    assert!(!error.contains("Rules 1-3"), "{error}");

    let (_d, repo) = support::repo();
    commit(
        &repo,
        "fix(core): a\n\nchangelog: A fix.\nRefs: PRD 0001 RF9",
        "crates/cuotai-core/src/lib.rs",
        "// v2\n",
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(
        error.contains("commit gate failed (2 problem(s))"),
        "{error}"
    );
    assert!(error.contains("rule 2: trailer key 'changelog'"), "{error}");
    assert!(error.contains("rule 8: Refs: PRD 0001 RF9"), "{error}");
    assert!(error.contains("Rules 1-3 and 6-8: amend"), "{error}");
    assert!(!error.contains("Rule 4"), "{error}");
}

#[test]
fn the_skip_marker_anywhere_in_the_message_fails() {
    let marker = format!("[{} {}]", "skip", "ci");
    for message in [
        format!("docs: a {marker}"),
        format!("docs: a\n\nBody with {marker} inside."),
        format!("docs: a\n\nBody.\n\nRefs: #1 {marker}"),
    ] {
        let (_d, repo) = support::repo();
        commit(&repo, &message, "docs/x.md", "x\n");
        let error = gate(&repo, "HEAD~1").unwrap_err();
        assert!(
            error.contains("rule 6: the message carries GitHub's skip marker"),
            "{error}"
        );
        assert_eq!(error.matches("rule 6").count(), 1, "{error}");
    }
    let (_d, repo) = support::repo();
    commit(
        &repo,
        "docs: a\n\nWrite the skip marker.",
        "docs/x.md",
        "x\n",
    );
    gate(&repo, "HEAD~1").unwrap();
}

#[test]
fn identities_must_be_noreply_and_are_masked() {
    let (_d, repo) = support::repo();
    commit(
        &repo,
        "docs: a\n\nBody.\n\nCo-Authored-By: Alex <alex@example.com>",
        "docs/x.md",
        "x\n",
    );
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(
        error.contains("rule 9: 'Co-Authored-By' uses a***@example.com"),
        "{error}"
    );
    assert!(!error.contains("alex@"), "{error}");
    assert!(
        error.contains("Rule 9: set the repository's user.email"),
        "{error}"
    );

    let (_d, repo) = support::repo();
    git(&repo, &["config", "user.email", "alex@example.com"]);
    commit(&repo, "docs: b", "docs/x.md", "x\n");
    let error = gate(&repo, "HEAD~1").unwrap_err();
    assert!(
        error.contains("rule 9: the author address a***@example.com"),
        "{error}"
    );
    assert!(
        error.contains("rule 9: the committer address a***@example.com"),
        "{error}"
    );
    assert!(!error.contains("alex@"), "{error}");

    let (_d, repo) = support::repo();
    commit(
        &repo,
        "docs: c\n\nBody.\n\nCo-Authored-By: Bot <noreply@anthropic.com>",
        "docs/x.md",
        "x\n",
    );
    gate(&repo, "HEAD~1").unwrap();
}
