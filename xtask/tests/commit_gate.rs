//! Temp-repository tests of the commit gate (PRD 0039 RF2 acceptance).

mod support;

use std::path::Path;

use support::*;

fn gate(repo: &Path, base: &str) -> Result<usize, String> {
    xtask::changelog_gate::run(repo, base, "HEAD").map_err(|e| e.to_string())
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
