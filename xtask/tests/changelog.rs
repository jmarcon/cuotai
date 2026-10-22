//! Behavior-level tests for the changelog and plan-control checks (PRD 0019
//! RF7.4/RF7.8) and for the draft generator (RF7.3): the format, the
//! references, the divergence with the plan tables, the pull-request gate
//! and the grouped draft over real commits.

use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::changelog::parse;

const CHANGELOG: &str = "\
# Changelog

All notable changes to Cuotai are recorded here.

## [Unreleased]

## [0.2.0] - 2026-10-21

### Added

- A new thing you can do (PRD 0019 RF7.7, abc1234).

## [0.1.0] - 2026-10-20

### Fixed

- An old thing fixed (PRD 0001 RF1).

[Unreleased]: https://example/compare/v0.2.0...HEAD
[0.2.0]: https://example/compare/v0.1.0...v0.2.0
";

const PLAN: &str = "\
# PRD 0019 — Packaging, license, changelog and distribution

## Implementation status (2026-10-21)

| RF | Version | Delivery |
|---|---|---|
| RF7.7 | 0.2.0 | The What's new screen and `--changelog`. |
| RF1 | 0.1.0 | License. |

- **RF1.1** License text.
- **RF7.7** Display in the product.
";

const PLAN_0001: &str = "\
# PRD 0001 — Core support for graphical front ends

## Implementation status (2026-10-20)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.1.0 | The core contract. |

- **RF1** Core support for graphical front ends.
";

const CONTROL: &str = "\
# Plan control (PRDs)

## 3. Delivered by version

| Version | Plan | Requirements delivered |
|---|---|---|
| 0.2.0 | 0019 | RF7.7 (What's new). |
| 0.1.0 | 0001 | RF1 (license). |

## 4. Dependency graph
";

fn write_file(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A repository with a changelog, one plan and the control index.
fn fixture(changelog: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_file(root, "CHANGELOG.md", changelog);
    write_file(root, "docs/plans/0019_packaging.md", PLAN);
    write_file(root, "docs/plans/0001_core.md", PLAN_0001);
    write_file(root, "docs/plans/0000_control.md", CONTROL);
    dir
}

fn check(root: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
    xtask::plan_links::run(root, &text).map_err(|e| e.to_string())
}

#[test]
fn consistent_repository_passes() {
    let dir = fixture(CHANGELOG);
    check(dir.path()).unwrap();
}

#[test]
fn reference_to_unknown_requirement_fails() {
    let dir = fixture(&CHANGELOG.replace("(PRD 0019 RF7.7,", "(PRD 0019 RF77,"));
    let error = check(dir.path()).unwrap_err();
    assert!(error.contains("PRD 0019 RF77"), "{error}");
    assert!(error.contains("has no RF77"), "{error}");
}

#[test]
fn reference_to_undotted_requirement_does_not_match_its_sub_requirement() {
    // `RF7` must not be satisfied by `RF7.1`: both are separate ids.
    let dir = fixture(&CHANGELOG.replace("(PRD 0019 RF7.7,", "(PRD 0019 RF7,"));
    let error = check(dir.path()).unwrap_err();
    assert!(error.contains("has no RF7"), "{error}");
}

#[test]
fn reference_to_unknown_plan_fails() {
    let dir = fixture(&CHANGELOG.replace("PRD 0019 RF7.7", "PRD 0031 RF1"));
    let error = check(dir.path()).unwrap_err();
    assert!(
        error.contains("there is no docs/plans/0031_*.md plan"),
        "{error}"
    );
}

#[test]
fn delivered_version_without_a_changelog_section_fails() {
    let dir = fixture(CHANGELOG);
    let plan = dir.path().join("docs/plans/0019_packaging.md");
    let text = std::fs::read_to_string(&plan)
        .unwrap()
        .replace("0.2.0 |", "0.2.9 |");
    std::fs::write(&plan, text).unwrap();
    let error = check(dir.path()).unwrap_err();
    assert!(error.contains("marks 0.2.9 as delivered"), "{error}");
}

#[test]
fn control_row_without_a_changelog_section_fails() {
    let dir = fixture(CHANGELOG);
    let control = dir.path().join("docs/plans/0000_control.md");
    let text = std::fs::read_to_string(&control)
        .unwrap()
        .replace("| 0.2.0 |", "| 0.2.9 |");
    std::fs::write(&control, text).unwrap();
    let error = check(dir.path()).unwrap_err();
    assert!(error.contains("0000_control.md"), "{error}");
    assert!(error.contains("0.2.9"), "{error}");
}

#[test]
fn format_violations_fail_the_check() {
    for (broken, expected) in [
        (
            CHANGELOG.replace("## [0.1.0] - 2026-10-20", "## [0.1.0]"),
            "no date",
        ),
        (
            CHANGELOG.replace("### Fixed", "### Improved"),
            "invalid category",
        ),
        (
            CHANGELOG.replace("## [0.1.0] - 2026-10-20", "## [0.3.0] - 2026-10-20"),
            "out of order",
        ),
    ] {
        let dir = fixture(&broken);
        let error = check(dir.path()).unwrap_err();
        assert!(error.contains(expected), "unexpected error: {error}");
    }
}

// --------------------------------------------------------------------------
// The pull-request gate and the draft generator need real commits.
// --------------------------------------------------------------------------

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_DATE", "2026-10-21T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-10-21T00:00:00Z")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A git repository whose first commit holds the fixture, so every later
/// commit is a pull-request candidate.
fn repo_fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().to_path_buf();
    let root = dir.path();
    write_file(root, "CHANGELOG.md", CHANGELOG);
    write_file(root, "docs/plans/0019_packaging.md", PLAN);
    write_file(root, "docs/plans/0001_core.md", PLAN_0001);
    write_file(root, "docs/plans/0000_control.md", CONTROL);
    write_file(root, "crates/core/lib.rs", "// v1\n");
    write_file(
        root,
        "Cargo.toml",
        "[workspace.package]\nversion = \"0.2.0\"\n",
    );
    git(&repo, &["init", "-b", "main", "-q"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "chore: fixture"]);
    (dir, repo)
}

fn commit(repo: &Path, message: &str, file: &str, content: &str) {
    let path = repo.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
}

#[test]
fn feature_without_changelog_fails_the_gate() {
    let (_d, repo) = repo_fixture();
    commit(
        &repo,
        "feat(core): add a thing",
        "crates/core/lib.rs",
        "// v2\n",
    );
    let error = xtask::changelog_gate::run(&repo, "HEAD~1", "HEAD")
        .unwrap_err()
        .to_string();
    assert!(error.contains("changelog gate failed"), "{error}");
    assert!(error.contains("feat(core): add a thing"), "{error}");
}

#[test]
fn feature_with_a_skip_footer_and_a_reason_passes() {
    let (_d, repo) = repo_fixture();
    commit(
        &repo,
        "fix(core): internal tweak\n\nChangelog: skip: no user-visible change",
        "crates/core/lib.rs",
        "// v2\n",
    );
    xtask::changelog_gate::run(&repo, "HEAD~1", "HEAD").unwrap();
}

#[test]
fn one_unjustified_commit_fails_the_whole_pull_request() {
    let (_d, repo) = repo_fixture();
    commit(
        &repo,
        "feat(core): justified\n\nChangelog: skip: internal only",
        "crates/core/lib.rs",
        "// v2\n",
    );
    commit(
        &repo,
        "fix(desktop): unjustified",
        "apps/desktop/src/main.rs",
        "// v1\n",
    );
    let error = xtask::changelog_gate::run(&repo, "HEAD~2", "HEAD")
        .unwrap_err()
        .to_string();
    assert!(error.contains("fix(desktop): unjustified"), "{error}");
    assert!(!error.contains("feat(core): justified"), "{error}");
}

#[test]
fn skip_without_a_reason_fails_the_gate() {
    let (_d, repo) = repo_fixture();
    commit(
        &repo,
        "fix(core): internal tweak\n\nChangelog: skip",
        "crates/core/lib.rs",
        "// v2\n",
    );
    let error = xtask::changelog_gate::run(&repo, "HEAD~1", "HEAD")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Changelog: skip"), "{error}");
}

#[test]
fn feature_that_changes_the_changelog_passes() {
    let (_d, repo) = repo_fixture();
    commit(
        &repo,
        "feat(core): add a thing\n\nRefs: PRD 0019 RF7.7",
        "crates/core/lib.rs",
        "// v2\n",
    );
    commit(
        &repo,
        "docs: changelog entry\n",
        "CHANGELOG.md",
        &CHANGELOG.replace(
            "## [Unreleased]\n",
            "## [Unreleased]\n\n### Added\n\n- A new thing you can do (PRD 0019 RF7.7, abc1234).\n",
        ),
    );
    xtask::changelog_gate::run(&repo, "HEAD~2", "HEAD").unwrap();
}

#[test]
fn commits_outside_the_product_are_never_gated() {
    let (_d, repo) = repo_fixture();
    commit(
        &repo,
        "feat(docs): document a thing",
        "README.md",
        "# docs\n",
    );
    xtask::changelog_gate::run(&repo, "HEAD~1", "HEAD").unwrap();
}

#[test]
fn draft_groups_the_commits_since_the_given_ref() {
    let (_d, repo) = repo_fixture();
    commit(
        &repo,
        "feat(cli): add --changelog\n\nRefs: PRD 0019 RF7.7",
        "crates/core/lib.rs",
        "// v2\n",
    );
    commit(
        &repo,
        "fix(core): wrong date\n",
        "crates/core/lib.rs",
        "// v3\n",
    );
    commit(
        &repo,
        "test(core): add a case",
        "crates/core/lib.rs",
        "// v4\n",
    );
    let draft = xtask::draft::run(&repo, Some("HEAD~3")).unwrap();
    assert!(draft.contains("### Added"), "{draft}");
    assert!(
        draft.contains("- feat(cli): add --changelog (PRD 0019 RF7.7,"),
        "{draft}"
    );
    assert!(draft.contains("### Fixed"), "{draft}");
    assert!(draft.contains("- fix(core): wrong date ("), "{draft}");
    assert!(!draft.contains("test(core): add a case"), "{draft}");
}

#[test]
fn draft_without_a_tag_falls_back_to_the_last_version_bump() {
    let (_d, repo) = repo_fixture();
    // The bump of 0.3.0 is the last release point while no tag exists, so
    // the draft covers only the commits after it.
    write_file(
        &repo,
        "Cargo.toml",
        "[workspace.package]\nversion = \"0.3.0\"\n",
    );
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "chore: bump"]);
    commit(
        &repo,
        "feat(core): after the bump\n\nRefs: PRD 0019 RF1",
        "crates/core/lib.rs",
        "// v2\n",
    );
    let draft = xtask::draft::run(&repo, None).unwrap();
    assert!(
        draft.contains("feat(core): after the bump (PRD 0019 RF1,"),
        "{draft}"
    );
    assert!(!draft.contains("chore: bump"), "{draft}");
}

#[test]
fn parsed_entries_expose_refs_hashes_and_breaking() {
    let changelog = parse(
        "# C\n\n## [Unreleased]\n\n## [0.1.0] - 2026-10-20\n\n### Added\n\n- **Breaking**: gone (PRD 0001 RF5/RF8, 3855c72).\n",
    )
    .unwrap();
    let entry = &changelog.releases[0].section.categories[0].entries[0];
    assert!(entry.breaking);
    assert_eq!(entry.hashes, vec!["3855c72"]);
    assert_eq!(
        changelog.references().len(),
        2,
        "RF5/RF8 is two requirements"
    );
}
