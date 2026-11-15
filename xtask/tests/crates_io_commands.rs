//! crates.io guard (PRD 0021 re-validation S-01): while `cuotai-core` and
//! `cuotai-tui` are not published on crates.io, nobody owns those names for
//! the project, so no user-facing text may tell people to install them from
//! the registry. The only Cargo route is the pinned
//! `cargo install --git https://github.com/jmarcon/cuotai --tag vX.Y.Z cuotai-tui --locked`.
//!
//! The switch is `CRATES_IO_PUBLISHED` in
//! `crates/cuotai-core/src/install_source.rs`; this test reads that line, so
//! the product and the docs flip together after the first publication.

use std::path::Path;
use std::process::Command;

const SOURCE_OF_TRUTH: &str = "crates/cuotai-core/src/install_source.rs";

/// User-facing trees scanned by the guard. Plans are history and requirements,
/// not instructions, so `docs/plans/` is left out.
const SCANNED: &[&str] = &[
    "README.md",
    "docs/",
    "site/",
    "crates/",
    "apps/",
    "packaging/",
];

/// A bare registry install of any Cuotai crate, or any `cargo binstall`
/// (which resolves crates through crates.io).
fn registry_commands(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("cargo install cuotai") || lower.contains("cargo binstall")
}

fn crates_io_published(root: &Path) -> bool {
    let source = std::fs::read_to_string(root.join(SOURCE_OF_TRUTH)).expect("read install_source");
    let line = source
        .lines()
        .find(|line| line.starts_with("pub const CRATES_IO_PUBLISHED: bool ="))
        .expect("CRATES_IO_PUBLISHED is declared");
    match line.trim_end_matches(';').rsplit('=').next().map(str::trim) {
        Some("true") => true,
        Some("false") => false,
        other => panic!("CRATES_IO_PUBLISHED must be a literal bool, found {other:?}"),
    }
}

#[test]
fn the_scanner_flags_registry_commands_only() {
    assert!(registry_commands("`cargo install cuotai-tui --locked`"));
    assert!(registry_commands("cargo install cuotai"));
    assert!(registry_commands("or `cargo binstall cuotai-tui`"));
    assert!(!registry_commands(
        "cargo install --git https://github.com/jmarcon/cuotai --tag v1.2.3 cuotai-tui --locked"
    ));
    assert!(!registry_commands(
        "cargo install --path crates/cuotai-tui --locked"
    ));
    assert!(!registry_commands("[package.metadata.binstall]"));
}

#[test]
fn no_bare_crates_io_command_while_the_crates_are_unpublished() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    if crates_io_published(&root) {
        return;
    }
    let listing = match Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
    {
        Ok(out) if out.status.success() => out.stdout,
        // A source archive without .git or a machine without git: nothing to scan.
        _ => return,
    };
    let mut offenders = Vec::new();
    for rel in listing.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let rel = String::from_utf8_lossy(rel).into_owned();
        if rel == SOURCE_OF_TRUTH
            || rel.starts_with("docs/plans/")
            || !SCANNED
                .iter()
                .any(|prefix| rel == *prefix || rel.starts_with(prefix))
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            if registry_commands(line) {
                offenders.push(format!("{rel}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "cuotai-core/cuotai-tui are not on crates.io (CRATES_IO_PUBLISHED = false): \
         use `cargo install --git https://github.com/jmarcon/cuotai --tag vX.Y.Z cuotai-tui --locked` \
         instead of a registry or binstall command:\n{}",
        offenders.join("\n")
    );
}
