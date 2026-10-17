//! Guard for GHSA-wrw7-89jp-8q8g / RUSTSEC-2024-0429: `glib` < 0.20 has an
//! unsound `VariantStrIter` (reached only through `VariantStrIter` itself or
//! `Variant::array_iter_str`). Tauri 2's Linux stack (gtk/webkit2gtk 0.18)
//! pins `glib` 0.18 and no Tauri 2 release moves to 0.20, so the Dependabot
//! alert is dismissed as "vulnerable code not used". This test keeps that
//! claim true: it fails if any crate in the desktop dependency graph, or the
//! app itself, starts using the vulnerable API while an affected `glib` is
//! resolved.

use std::{fs, path::Path, process::Command};

const VULNERABLE_API: [&str; 2] = ["VariantStrIter", "array_iter_str"];

#[test]
fn vulnerable_glib_variant_str_iter_is_not_used() {
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
    let packages = metadata["packages"].as_array().expect("packages");

    let affected_glib = packages.iter().any(|package| {
        package["name"] == "glib" && version_below(package["version"].as_str().unwrap(), 0, 20)
    });
    if !affected_glib {
        return; // glib >= 0.20 (or absent): the advisory no longer applies.
    }

    let mut uses = Vec::new();
    for package in packages.iter().filter(|package| package["name"] != "glib") {
        let manifest = Path::new(package["manifest_path"].as_str().expect("manifest path"));
        scan(
            &manifest.parent().expect("crate dir").join("src"),
            &mut uses,
        );
    }
    assert!(
        uses.is_empty(),
        "glib < 0.20 is resolved and the unsound VariantStrIter API is now used; \
         upgrade glib or reassess GHSA-wrw7-89jp-8q8g: {uses:#?}"
    );
}

fn version_below(version: &str, major: u64, minor: u64) -> bool {
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    let (found_major, found_minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    (found_major, found_minor) < (major, minor)
}

fn scan(dir: &Path, uses: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, uses);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            if VULNERABLE_API.iter().any(|api| source.contains(api)) {
                uses.push(path.display().to_string());
            }
        }
    }
}
