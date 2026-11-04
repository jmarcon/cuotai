//! PRD 0052 RF4.3: every relative link of the user documentation resolves
//! to a file in the repository, and the feature documents that used to keep
//! their own inventories point to the generated `docs/privacy.md`.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// The documents checked: the root community files and `docs/*.md`
/// (plans and their own link rules live in `plan_links`).
fn documents() -> Vec<PathBuf> {
    let root = root();
    let mut out: Vec<PathBuf> = ["README.md", "SECURITY.md", "CONTRIBUTING.md", "SUPPORT.md"]
        .iter()
        .map(|name| root.join(name))
        .filter(|path| path.exists())
        .collect();
    let mut docs: Vec<PathBuf> = std::fs::read_dir(root.join("docs"))
        .expect("docs directory")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    docs.sort();
    out.extend(docs);
    out
}

/// Targets of `](target)` links, without anchors or titles.
fn links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find("](") {
            rest = &rest[start + 2..];
            let Some(end) = rest.find(')') else { break };
            let target = rest[..end].split_whitespace().next().unwrap_or_default();
            out.push(target.to_string());
            rest = &rest[end..];
        }
    }
    out
}

fn is_local(target: &str) -> bool {
    !(target.is_empty()
        || target.starts_with('#')
        || target.contains("://")
        || target.starts_with("mailto:")
        || target.starts_with('<'))
}

#[test]
fn every_relative_documentation_link_resolves() {
    let mut broken = Vec::new();
    for doc in documents() {
        let text = std::fs::read_to_string(&doc).expect("readable document");
        let dir = doc.parent().unwrap();
        for target in links(&text).into_iter().filter(|t| is_local(t)) {
            let file = target.split('#').next().unwrap_or_default();
            if file.is_empty() {
                continue;
            }
            if !dir.join(file).exists() {
                broken.push(format!("{} -> {target}", doc.display()));
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}

#[test]
fn the_feature_documents_point_to_the_generated_inventory() {
    let root = root();
    for (doc, link) in [
        ("README.md", "docs/privacy.md"),
        ("SECURITY.md", "docs/privacy.md"),
        ("docs/updater-security.md", "privacy.md"),
        ("docs/gui-features.md", "privacy.md"),
        ("docs/events.md", "privacy.md"),
        ("docs/widgets.md", "privacy.md"),
    ] {
        let text = std::fs::read_to_string(root.join(doc)).expect(doc);
        assert!(
            links(&text).iter().any(|target| target == link),
            "{doc} must link {link}"
        );
    }
    let privacy = std::fs::read_to_string(root.join("docs/privacy.md")).unwrap();
    assert!(privacy.contains("<!-- privacy-inventory:start"));
}
