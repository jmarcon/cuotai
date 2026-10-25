//! PRD 0039 RF10: build identity between releases.
//!
//! Exposes `CUOTAI_BUILD_DESCRIBE`, the output of `git describe --tags --long
//! --dirty --always` for the checkout this crate is built from, or an empty
//! string when there is no such checkout (a crates.io build, a source archive,
//! Git missing). The value is display-only: [`cuotai_core::build_info`] turns
//! it into the "(dev abc1234)" suffix, and no comparison ever reads it.

use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let root = Path::new(&manifest).join("../..");
    // Only the workspace checkout counts: a packaged crate is built under
    // `target/package/`, whose parents may belong to an unrelated repository.
    let in_workspace = root.join("Cargo.toml").is_file()
        && root.join("crates/cuotai-core/Cargo.toml").is_file()
        && root.join(".git").exists();
    let describe = if in_workspace {
        git(
            &root,
            &["describe", "--tags", "--long", "--dirty", "--always"],
        )
    } else {
        None
    };
    if in_workspace {
        // A new commit, checkout or tag changes what the build is.
        if let Some(dir) = git(&root, &["rev-parse", "--absolute-git-dir"]) {
            println!("cargo:rerun-if-changed={dir}/HEAD");
            println!("cargo:rerun-if-changed={dir}/refs");
            println!("cargo:rerun-if-changed={dir}/packed-refs");
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rustc-env=CUOTAI_BUILD_DESCRIBE={}",
        describe.unwrap_or_default()
    );
}
