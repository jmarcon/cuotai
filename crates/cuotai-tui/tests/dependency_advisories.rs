//! Guard for GHSA-rhfx-m35p-ff5j: `lru` < 0.16.3 has an
//! `IterMut` that violates Stacked Borrows. `ratatui` 0.30 depends on a
//! patched `lru`; this test fails if the resolved graph ever brings back an
//! affected version (e.g. a downgrade or a new dependency on an old `lru`).

use std::process::Command;

#[test]
fn resolved_lru_is_not_affected_by_iter_mut_unsoundness() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata JSON");
    let affected: Vec<&str> = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .filter(|package| package["name"] == "lru")
        .filter_map(|package| package["version"].as_str())
        .filter(|version| below(version, [0, 16, 3]))
        .collect();
    assert!(
        affected.is_empty(),
        "lru versions affected by GHSA-rhfx-m35p-ff5j are resolved: {affected:?}"
    );
}

fn below(version: &str, fixed: [u64; 3]) -> bool {
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|part| part.parse::<u64>().unwrap_or(0));
    let found = [
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ];
    found < fixed
}
