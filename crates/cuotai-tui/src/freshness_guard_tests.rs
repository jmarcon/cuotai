//! PRD 0040 re-validation H-1: no surface ages a reading on its own.
//!
//! PRD 0033's delivery fixed every place that computed a `Freshness` but
//! missed the surfaces that printed an age from the query time
//! (`report.checked_at`) beside the core's label, so an agent feed written
//! 2 h ago read "Up to date · now" or "Stale · now". The age of a reading
//! comes only from `cuotai_core::freshness::present` (`measured_at`, `age`;
//! the desktop's `shownAt`, which reads the core's `measured_at`). This
//! guard scans the TUI and the desktop UI sources for the old forms. The
//! time of a failed attempt is not a reading; it has no pattern here.

use std::path::{Path, PathBuf};

/// Forms that age a reading from the query time (whitespace removed).
const FORBIDDEN: &[&str] = &[
    "age_text(report.checked_at",
    "age_text(stale.checked_at",
    "age_text(reading.checked_at",
    "fmtAge(r.checked_at",
    "fmtAge(r.last_good.checked_at",
    "fmtReset(r.checked_at",
    "fmtReset(r.last_good.checked_at",
];

/// Matches that stay, by file and pattern, each with its reason. An entry
/// that no longer matches fails the test, so the list cannot go stale.
const ALLOWED: &[(&str, &str, &str)] = &[(
    "crates/cuotai-tui/src/dashboard.rs",
    "age_text(report.checked_at",
    "a locale smoke test of the age words themselves, not of a reading",
)];

fn sources(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

#[test]
fn no_surface_ages_a_reading_from_the_query_time() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    sources(&root.join("crates/cuotai-tui/src"), "rs", &mut files);
    sources(&root.join("apps/desktop/ui/js"), "js", &mut files);
    assert!(files.len() > 20, "the scan found the sources");
    let mut found = Vec::new();
    for file in &files {
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if relative.ends_with("freshness_guard_tests.rs") {
            continue;
        }
        let text: String = std::fs::read_to_string(file)
            .unwrap()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        for pattern in FORBIDDEN {
            if text.contains(pattern) {
                found.push((relative.clone(), *pattern));
            }
        }
    }
    for (file, pattern, reason) in ALLOWED {
        let at = found.iter().position(|(f, p)| f == file && p == pattern);
        assert!(
            at.is_some(),
            "allow-list entry no longer needed: {file} {pattern} ({reason})"
        );
        found.remove(at.unwrap());
    }
    assert!(
        found.is_empty(),
        "age a reading with cuotai_core::freshness::present (TUI) or shownAt (desktop), \
         not the query time: {found:?}"
    );
    // The desktop's one instant of a reading reads the core's `measured_at`.
    let lib = std::fs::read_to_string(root.join("apps/desktop/ui/js/lib.js")).unwrap();
    let shown_at = lib
        .split("export const shownAt")
        .nth(1)
        .expect("lib.js defines shownAt");
    assert!(
        shown_at[..shown_at.find("};").unwrap_or(shown_at.len())].contains("measured_at"),
        "shownAt reads measured_at"
    );
}
