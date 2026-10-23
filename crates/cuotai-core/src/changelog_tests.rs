//! Parser tests for `changelog` (PRD 0019 RF7.4 acceptance: valid
//! format, a version with no date, an invalid category, wrong order).
//! They live beside the module like every other core test file; the
//! module itself is compiled by `xtask` through `#[path]`, which must
//! not drag the crate's own assertions along.

use super::*;

fn parse_ok(text: &str) -> Changelog {
    parse(text).unwrap_or_else(|e| panic!("should parse: {e}"))
}

#[test]
fn embedded_changelog_parses() {
    let changelog = entries();
    assert!(!changelog.releases.is_empty() || changelog.pre_release.is_some());
    // Descending order and dated sections. That the top released section
    // is the current version is the release gate's rule
    // (`xtask release-check`, PRD 0021 RF1.2), not the parser's.
    for pair in changelog.releases.windows(2) {
        assert_eq!(
            compare_versions(&pair[0].version, &pair[1].version),
            std::cmp::Ordering::Greater
        );
    }
    assert!(changelog.releases.iter().all(|r| valid_date(&r.date)));
    // Every entry keeps its traceability data and no released section is
    // empty (the release notes come from them).
    for release in &changelog.releases {
        assert!(!release.section.is_empty(), "[{}]", release.version);
        assert!(!release.body.trim().is_empty(), "[{}]", release.version);
    }
    // The first version exists and its entries keep working: its own
    // section, or the start of the pre-release history that summarizes
    // the internal builds (PRD 0039 D11).
    match &changelog.pre_release {
        Some(history) => {
            assert_eq!(history.first, "0.1.0");
            assert!(history.covers("0.1.0") && history.covers(&history.last));
            assert!(!history.section.categories.is_empty());
            assert!(!history.body.trim().is_empty());
            assert!(changelog.knows("0.1.0"));
        }
        None => {
            let first = changelog.releases.last().expect("0.1.0");
            assert_eq!(first.version, "0.1.0");
            assert!(!first.section.categories.is_empty());
        }
    }
}

const WITH_HISTORY: &str = "# C\n\nPreface.\n\n## [Unreleased]\n\n## [0.80.0] - 2026-10-24\n\n### Added\n\n- New (PRD 0039 RF4, abc1234).\n\n## Pre-release history (0.1.0–0.79.0, internal builds, never published)\n\nThese builds were never published.\n\n### Added\n\n- The dashboard (PRD 0001 RF2).\n\n### Security\n\n- HTTPS only.\n\n[Unreleased]: https://example/compare/v0.80.0...HEAD\n[0.80.0]: https://example/compare/abc1234...v0.80.0\n";

#[test]
fn parses_the_pre_release_history_as_a_distinct_last_section() {
    let changelog = parse_ok(WITH_HISTORY);
    assert_eq!(changelog.releases.len(), 1);
    assert_eq!(changelog.releases[0].version, "0.80.0");
    let history = changelog.pre_release.as_ref().expect("history");
    assert_eq!(
        history.title,
        "Pre-release history (0.1.0–0.79.0, internal builds, never published)"
    );
    assert_eq!(
        (history.first.as_str(), history.last.as_str()),
        ("0.1.0", "0.79.0")
    );
    assert_eq!(history.range(), "0.1.0–0.79.0");
    assert_eq!(
        history.section.paragraphs,
        ["These builds were never published."]
    );
    assert_eq!(history.section.categories.len(), 2);
    assert!(!history.body.contains("[Unreleased]:"), "{}", history.body);
    // Covered versions are known without a section of their own.
    for version in ["0.1.0", "0.5.0", "0.14.3", "0.79.0"] {
        assert!(changelog.knows(version), "{version}");
        assert!(changelog.find(version).is_none(), "{version}");
        assert!(changelog.pre_release_of(version).is_some(), "{version}");
    }
    assert!(changelog.knows("0.80.0"));
    assert!(changelog.pre_release_of("0.80.0").is_none());
    assert!(!changelog.knows("0.79.1"));
    assert!(!changelog.knows("0.5"));
    // Its references count like any other section's.
    let refs: Vec<String> = changelog
        .references()
        .iter()
        .map(|r| r.to_string())
        .collect();
    assert_eq!(refs, ["PRD 0039 RF4", "PRD 0001 RF2"]);
    // Parsing is stable: the same text gives the same model.
    assert_eq!(parse_ok(WITH_HISTORY), changelog);
    // A changelog with only the history (before the first release) parses.
    let only = WITH_HISTORY
        .replace(
            "## [0.80.0] - 2026-10-24\n\n### Added\n\n- New (PRD 0039 RF4, abc1234).\n\n",
            "",
        )
        .replace("[0.80.0]: https://example/compare/abc1234...v0.80.0\n", "");
    let only = parse_ok(&only);
    assert!(only.releases.is_empty());
    assert!(only.knows("0.79.0"));
}

#[test]
fn rejects_a_misplaced_or_malformed_pre_release_history() {
    // Not the last section.
    let after = WITH_HISTORY.replace(
        "[Unreleased]: ",
        "## [0.0.1] - 2026-09-01\n\n### Added\n\n- Old.\n\n[Unreleased]: ",
    );
    let error = parse(&after).unwrap_err();
    assert!(error.message.contains("is the last section"), "{error}");
    // Overlapping a released version.
    let overlap = WITH_HISTORY.replace("0.1.0–0.79.0", "0.1.0–0.80.0");
    let error = parse(&overlap).unwrap_err();
    assert!(error.message.contains("is not newer"), "{error}");
    // Before [Unreleased].
    let error =
        parse("# C\n\n## Pre-release history (0.1.0–0.2.0)\n\n### Added\n\n- X.\n").unwrap_err();
    assert!(
        error.message.contains("unreleased section comes first"),
        "{error}"
    );
    // Hyphen instead of en dash, missing parenthesis, bad version, reversed.
    for heading in [
        "Pre-release history (0.1.0-0.79.0)",
        "Pre-release history (0.1.0–0.79.0",
        "Pre-release history (0.1–0.79.0)",
        "Pre-release history (0.79.0–0.1.0)",
        "Pre-release history",
    ] {
        let text = WITH_HISTORY.replace(
            "Pre-release history (0.1.0–0.79.0, internal builds, never published)",
            heading,
        );
        assert!(parse(&text).is_err(), "{heading}");
    }
    // A note-less heading is fine.
    let bare = WITH_HISTORY.replace(", internal builds, never published)", ")");
    assert_eq!(parse_ok(&bare).pre_release.unwrap().last, "0.79.0");
    // Empty, or with an empty category.
    let empty = "# C\n\n## [Unreleased]\n\n## Pre-release history (0.1.0–0.2.0)\n\n";
    assert!(parse(empty).unwrap_err().message.contains("is empty"));
    let empty_category =
        "# C\n\n## [Unreleased]\n\n## Pre-release history (0.1.0–0.2.0)\n\n### Added\n";
    assert!(parse(empty_category)
        .unwrap_err()
        .message
        .contains("of the pre-release history"));
    // Twice.
    let twice = WITH_HISTORY.replace(
        "[Unreleased]: ",
        "## Pre-release history (0.0.1–0.0.2)\n\n### Added\n\n- X.\n\n[Unreleased]: ",
    );
    assert!(parse(&twice).is_err());
}

#[test]
fn parses_sections_entries_and_traceability() {
    let changelog = parse_ok(
        "# Changelog\n\
         \n\
         ## [Unreleased]\n\
         \n\
         ### Added\n\
         \n\
         - Pending thing (PRD 0019 RF7.7).\n\
         \n\
         ## [0.2.0] - 2026-10-18\n\
         \n\
         ### Fixed\n\
         \n\
         - A fix (PRD 0001 RF5/RF8, 6caf668).\n\
         - **Breaking**: a break (PRD 0021 RF1–RF3).\n\
         \n\
         ## [0.1.0] - 2026-10-01\n\
         \n\
         No user-visible changes (docs only, 089d964).\n\
         \n\
         [Unreleased]: https://example/compare/v0.2.0...HEAD\n\
         [0.2.0]: https://example/compare/v0.1.0...v0.2.0\n",
    );
    assert_eq!(changelog.unreleased.categories.len(), 1);
    assert_eq!(changelog.releases.len(), 2);
    let release = &changelog.releases[0];
    assert_eq!(release.version, "0.2.0");
    assert_eq!(release.date, "2026-10-18");
    assert!(release.body.contains("### Fixed"));
    assert!(!release.body.contains("[0.2.0]:"));
    let entries = &release.section.categories[0].entries;
    assert_eq!(
        entries[0].refs,
        vec![
            PlanRef {
                prd: "0001".into(),
                rf: "RF5".into()
            },
            PlanRef {
                prd: "0001".into(),
                rf: "RF8".into()
            },
        ]
    );
    assert_eq!(entries[0].hashes, vec!["6caf668"]);
    assert!(!entries[0].breaking);
    assert!(entries[1].breaking);
    assert_eq!(
        entries[1].refs,
        vec![
            PlanRef {
                prd: "0021".into(),
                rf: "RF1".into()
            },
            PlanRef {
                prd: "0021".into(),
                rf: "RF2".into()
            },
            PlanRef {
                prd: "0021".into(),
                rf: "RF3".into()
            },
        ]
    );
    assert_eq!(changelog.releases[1].section.paragraphs.len(), 1);
    assert_eq!(
        changelog.releases[1].section.paragraphs[0],
        "No user-visible changes (docs only, 089d964)."
    );
}

#[test]
fn rejects_missing_date() {
    let error = parse("# C\n\n## [Unreleased]\n\n## [0.1.0]\n\n### Added\n\n- X.\n").unwrap_err();
    assert!(error.message.contains("no date"), "{error}");
}

#[test]
fn rejects_invalid_date() {
    let error = parse("# C\n\n## [Unreleased]\n\n## [0.1.0] - 2026-13-40\n\n### Added\n\n- X.\n")
        .unwrap_err();
    assert!(error.message.contains("YYYY-MM-DD"), "{error}");
}

#[test]
fn rejects_invalid_category() {
    let error =
        parse("# C\n\n## [Unreleased]\n\n## [0.1.0] - 2026-10-01\n\n### New stuff\n\n- X.\n")
            .unwrap_err();
    assert!(error.message.contains("invalid category"), "{error}");
}

#[test]
fn rejects_wrong_order() {
    let error = parse(
        "# C\n\n## [Unreleased]\n\n## [0.1.0] - 2026-10-01\n\n### Added\n\n- X.\n\n## [0.2.0] - 2026-10-02\n\n### Added\n\n- Y.\n",
    )
    .unwrap_err();
    assert!(error.message.contains("out of order"), "{error}");
}

#[test]
fn rejects_duplicate_version_and_unreleased() {
    let error = parse(
        "# C\n\n## [Unreleased]\n\n## [0.1.0] - 2026-10-01\n\n### Added\n\n- X.\n\n## [0.1.0] - 2026-10-02\n\n### Added\n\n- Y.\n",
    )
    .unwrap_err();
    assert!(error.message.contains("duplicated"), "{error}");
    let error = parse("# C\n\n## [Unreleased]\n\n## [Unreleased]\n").unwrap_err();
    assert!(error.message.contains("only once"), "{error}");
}

#[test]
fn rejects_released_section_before_unreleased() {
    let error = parse("# C\n\n## [0.1.0] - 2026-10-01\n\n### Added\n\n- X.\n").unwrap_err();
    assert!(
        error.message.contains("before '## [Unreleased]'"),
        "{error}"
    );
}

#[test]
fn rejects_empty_category() {
    let error =
        parse("# C\n\n## [Unreleased]\n\n## [0.1.0] - 2026-10-01\n\n### Added\n").unwrap_err();
    assert!(error.message.contains("is empty"), "{error}");
}

#[test]
fn rejects_entry_outside_category() {
    let error = parse("# C\n\n## [Unreleased]\n\n## [0.1.0] - 2026-10-01\n\n- X.\n").unwrap_err();
    assert!(error.message.contains("outside a category"), "{error}");
}

#[test]
fn rejects_content_after_footer_links() {
    let error = parse("# C\n\n## [Unreleased]\n\n[Unreleased]: https://example\n\nstray text\n")
        .unwrap_err();
    assert!(error.message.contains("footer link"), "{error}");
}

#[test]
fn rejects_missing_unreleased() {
    let error = parse("# C\n\nno sections\n").unwrap_err();
    assert!(error.message.contains("no '## [Unreleased]'"), "{error}");
}

#[test]
fn hashes_need_a_digit_and_hex() {
    let entry = parse_entry("Something (facade), really (deedeed).");
    assert!(entry.hashes.is_empty());
    let entry = parse_entry("Something (3855c72, d407d0d).");
    assert_eq!(entry.hashes, vec!["3855c72", "d407d0d"]);
}

#[test]
fn grouped_requirements_continue_their_plan() {
    // PRD 0039 RF4.3: `release prepare` groups requirements by plan.
    let changelog = parse_ok(
        "# C\n\n## [Unreleased]\n\n## [0.80.0] - 2026-10-24\n\n### Added\n\n- A thing (PRD 0018 RF1.2, RF1.3; PRD 0021 RF2, RF5/RF6; issue #2; abc1234).\n",
    );
    let refs: Vec<String> = changelog
        .references()
        .iter()
        .map(|r| r.to_string())
        .collect();
    assert_eq!(
        refs,
        [
            "PRD 0018 RF1.2",
            "PRD 0018 RF1.3",
            "PRD 0021 RF2",
            "PRD 0021 RF5",
            "PRD 0021 RF6"
        ]
    );
    let entry = &changelog.releases[0].section.categories[0].entries[0];
    assert_eq!(entry.hashes, ["abc1234"]);
    // Review ids group like RF ids (FIX-01, INT-04, GEM-01, SES-01…).
    let review = parse_ok(
        "# C\n\n## [Unreleased]\n\n## [0.80.0] - 2026-10-24\n\n### Security\n\n- Fixed (PRD 0013 FIX-01, FIX-02, RF3; PRD 0016 SES-01; PRD 0012 INT-04; abc1234).\n",
    );
    let refs: Vec<String> = review.references().iter().map(|r| r.to_string()).collect();
    assert_eq!(
        refs,
        [
            "PRD 0013 FIX-01",
            "PRD 0013 FIX-02",
            "PRD 0013 RF3",
            "PRD 0016 SES-01",
            "PRD 0012 INT-04"
        ]
    );
    // A bare requirement never borrows a plan across a `;` or without one.
    let loose = parse_ok(
        "# C\n\n## [Unreleased]\n\n## [0.1.0] - 2026-10-20\n\n### Fixed\n\n- A fix (RF3; PRD 0001 RF1; RF4).\n",
    );
    assert_eq!(loose.references().len(), 1);
}
