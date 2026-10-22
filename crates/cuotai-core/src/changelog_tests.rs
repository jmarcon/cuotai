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
    assert!(!changelog.releases.is_empty());
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
    // The first version exists and its entries keep working.
    let first = changelog.releases.last().expect("0.1.0");
    assert_eq!(first.version, "0.1.0");
    assert!(!first.section.categories.is_empty());
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
