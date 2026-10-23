//! Behavior-level tests for the changelog and plan-control checks (PRD 0019
//! RF7.4/RF7.8, PRD 0039 RF9.5): the format, the references, the
//! divergence with the plan tables and the footer links. The commit gate
//! and `release prepare` have their own temp-repository tests
//! (`commit_gate.rs`, `release_prepare.rs`).

use std::path::Path;

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

[Unreleased]: https://example/compare/abc1234...HEAD
[0.2.0]: https://example/compare/def5678...abc1234
[0.1.0]: https://example/commit/def5678
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

#[test]
fn footer_links_of_internal_versions_compare_commits() {
    for (broken, expected) in [
        (
            CHANGELOG.replace(
                "[0.2.0]: https://example/compare/def5678...abc1234",
                "[0.2.0]: https://example/compare/v0.1.0...v0.2.0",
            ),
            "[0.2.0] is an internal version",
        ),
        (
            CHANGELOG.replace(
                "[0.2.0]: https://example/compare/def5678...abc1234",
                "[0.2.0]: https://example/compare/def5678...main",
            ),
            "[0.2.0] is an internal version",
        ),
        (
            CHANGELOG.replace(
                "[Unreleased]: https://example/compare/abc1234...HEAD",
                "[Unreleased]: https://example/compare/v0.2.0...HEAD",
            ),
            "[Unreleased] compares a tag",
        ),
        (
            CHANGELOG.replace("[0.1.0]: https://example/commit/def5678\n", ""),
            "[0.1.0] has no footer link",
        ),
    ] {
        let dir = fixture(&broken);
        let error = check(dir.path()).unwrap_err();
        assert!(error.contains(expected), "unexpected error: {error}");
    }
}

#[test]
fn footer_links_of_published_versions_compare_tags() {
    let published = CHANGELOG
        .replace(
            "## [Unreleased]\n",
            "## [Unreleased]\n\n## [0.80.0] - 2026-10-24\n\n### Added\n\n- Published (PRD 0019 RF7.7; 1234abc).\n",
        )
        .replace(
            "[Unreleased]: https://example/compare/abc1234...HEAD\n",
            "[Unreleased]: https://example/compare/v0.80.0...HEAD\n[0.80.0]: https://example/compare/abc1234...v0.80.0\n",
        );
    check(fixture(&published).path()).unwrap();
    let stale = published.replace(
        "[Unreleased]: https://example/compare/v0.80.0...HEAD",
        "[Unreleased]: https://example/compare/abc1234...HEAD",
    );
    let error = check(fixture(&stale).path()).unwrap_err();
    assert!(error.contains("must compare v0.80.0...HEAD"), "{error}");
}

#[test]
fn every_grouped_requirement_is_checked_against_its_plan() {
    let grouped = CHANGELOG.replace(
        "- A new thing you can do (PRD 0019 RF7.7, abc1234).",
        "- A new thing you can do (PRD 0019 RF7.7, RF1.1, RF9; abc1234).",
    );
    let error = check(fixture(&grouped).path()).unwrap_err();
    assert!(error.contains("PRD 0019 RF9"), "{error}");
    assert!(!error.contains("RF1.1"), "RF1.1 exists: {error}");
    // Review ids are checked one by one too.
    let review = CHANGELOG.replace(
        "- A new thing you can do (PRD 0019 RF7.7, abc1234).",
        "- A new thing you can do (PRD 0019 RF7.7, FIX-01; abc1234).",
    );
    let error = check(fixture(&review).path()).unwrap_err();
    assert!(error.contains("PRD 0019 FIX-01"), "{error}");
}

#[test]
fn next_token_in_plans_is_accepted_between_releases() {
    let dir = fixture(CHANGELOG);
    write_file(
        dir.path(),
        "docs/plans/0019_packaging.md",
        &PLAN.replace("| RF1 | 0.1.0 | License. |", "| RF1 | vNEXT | License. |"),
    );
    check(dir.path()).unwrap();
}
