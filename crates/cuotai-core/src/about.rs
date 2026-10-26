//! "Sobre": project identity and the open-source software Cuotai relies on.
//!
//! One list for every front end (TUI and desktop) so they never disagree.
//! Entries carry facts only (name, SPDX license, repository, role, which
//! component uses it); presentation and language belong to the front ends.
//! The `manifests_are_covered` test fails when a crate manifest gains a
//! dependency that is not listed here, so the list cannot silently rot.

use serde::Serialize;

pub const PROJECT_NAME: &str = "Cuotai";
pub const PROJECT_LICENSE: &str = "MIT OR Apache-2.0";
pub const PROJECT_REPOSITORY: &str = "https://github.com/jmarcon/cuotai";

/// Where a library takes part in Cuotai.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Linked into the shipped binaries.
    Runtime,
    /// Used only while compiling.
    Build,
    /// Used only by tests.
    Test,
    /// Non-code material that ships with an app (icons).
    Asset,
    /// Open color scheme whose palette a built-in theme follows.
    Theme,
    /// Upstream project whose public contract Cuotai reimplements.
    Reference,
}

/// Cuotai component that uses a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    Core,
    Tui,
    Desktop,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Library {
    pub name: &'static str,
    /// SPDX expression as published by the project.
    pub license: &'static str,
    pub repository: &'static str,
    pub role: Role,
    pub used_by: &'static [Component],
    /// Created by Brazilians; front ends give these a special thanks.
    pub from_brazil: bool,
}

use Component::{Core, Desktop, Tui};

const fn lib(
    name: &'static str,
    license: &'static str,
    repository: &'static str,
    role: Role,
    used_by: &'static [Component],
) -> Library {
    Library {
        name,
        license,
        repository,
        role,
        used_by,
        from_brazil: false,
    }
}

impl Library {
    const fn made_in_brazil(self) -> Self {
        Self {
            from_brazil: true,
            ..self
        }
    }
}

/// Direct dependencies, alphabetical within each role. Transitive crates are
/// not listed; `Cargo.lock` is the complete inventory.
static LIBRARIES: &[Library] = &[
    lib(
        "aes-gcm",
        "Apache-2.0 OR MIT",
        "https://github.com/RustCrypto/AEADs",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "base64",
        "MIT OR Apache-2.0",
        "https://github.com/marshallpierce/rust-base64",
        Role::Runtime,
        &[Core, Desktop],
    ),
    lib(
        "chrono",
        "MIT OR Apache-2.0",
        "https://github.com/chronotope/chrono",
        Role::Runtime,
        &[Core, Tui, Desktop],
    ),
    lib(
        "clap",
        "MIT OR Apache-2.0",
        "https://github.com/clap-rs/clap",
        Role::Runtime,
        &[Tui],
    ),
    lib(
        "clap_complete",
        "MIT OR Apache-2.0",
        "https://github.com/clap-rs/clap",
        Role::Runtime,
        &[Tui],
    ),
    lib(
        "crossterm",
        "MIT",
        "https://github.com/crossterm-rs/crossterm",
        Role::Runtime,
        &[Tui],
    ),
    lib(
        "dirs",
        "MIT OR Apache-2.0",
        "https://github.com/soc/dirs-rs",
        Role::Runtime,
        &[Core, Desktop],
    ),
    lib(
        "futures-util",
        "MIT OR Apache-2.0",
        "https://github.com/rust-lang/futures-rs",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "hmac",
        "MIT OR Apache-2.0",
        "https://github.com/RustCrypto/MACs",
        Role::Runtime,
        &[Core],
    ),
    // Only with the core's `serve` feature (PRD 0012 local server; the TUI
    // turns it on for `cuotai --serve`, the desktop for Settings → Integration).
    lib(
        "httparse",
        "MIT OR Apache-2.0",
        "https://github.com/seanmonstar/httparse",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "keyring",
        "MIT OR Apache-2.0",
        "https://github.com/open-source-cooperative/keyring-rs",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "libc",
        "MIT OR Apache-2.0",
        "https://github.com/rust-lang/libc",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "objc2",
        "MIT",
        "https://github.com/madsmtm/objc2",
        Role::Runtime,
        &[Desktop],
    ),
    lib(
        "objc2-app-kit",
        "Zlib OR Apache-2.0 OR MIT",
        "https://github.com/madsmtm/objc2",
        Role::Runtime,
        &[Desktop],
    ),
    lib(
        "objc2-foundation",
        "MIT",
        "https://github.com/madsmtm/objc2",
        Role::Runtime,
        &[Desktop],
    ),
    lib(
        "rand",
        "MIT OR Apache-2.0",
        "https://github.com/rust-random/rand",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "ratatui",
        "MIT",
        "https://github.com/ratatui/ratatui",
        Role::Runtime,
        &[Tui],
    ),
    lib(
        "reqwest",
        "MIT OR Apache-2.0",
        "https://github.com/seanmonstar/reqwest",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "rsa",
        "MIT OR Apache-2.0",
        "https://github.com/RustCrypto/RSA",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "rusqlite",
        "MIT",
        "https://github.com/rusqlite/rusqlite",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "rustls",
        "Apache-2.0 OR ISC OR MIT",
        "https://github.com/rustls/rustls",
        Role::Runtime,
        &[Core, Desktop],
    ),
    lib(
        "serde",
        "MIT OR Apache-2.0",
        "https://github.com/serde-rs/serde",
        Role::Runtime,
        &[Core, Desktop],
    ),
    lib(
        "serde_json",
        "MIT OR Apache-2.0",
        "https://github.com/serde-rs/json",
        Role::Runtime,
        &[Core, Tui, Desktop],
    ),
    lib(
        "serde_path_to_error",
        "MIT OR Apache-2.0",
        "https://github.com/dtolnay/path-to-error",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "sha1",
        "MIT OR Apache-2.0",
        "https://github.com/RustCrypto/hashes",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "sha2",
        "MIT OR Apache-2.0",
        "https://github.com/RustCrypto/hashes",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "sys-locale",
        "MIT OR Apache-2.0",
        "https://github.com/1Password/sys-locale",
        Role::Runtime,
        &[Tui],
    ),
    // Cofounded by Lucas Fernandes Nogueira (Brasil).
    lib(
        "tauri",
        "Apache-2.0 OR MIT",
        "https://github.com/tauri-apps/tauri",
        Role::Runtime,
        &[Desktop],
    )
    .made_in_brazil(),
    lib(
        "tauri-plugin-autostart",
        "Apache-2.0 OR MIT",
        "https://github.com/tauri-apps/plugins-workspace",
        Role::Runtime,
        &[Desktop],
    ),
    lib(
        "tauri-plugin-dialog",
        "Apache-2.0 OR MIT",
        "https://github.com/tauri-apps/plugins-workspace",
        Role::Runtime,
        &[Desktop],
    ),
    lib(
        "tauri-plugin-notification",
        "Apache-2.0 OR MIT",
        "https://github.com/tauri-apps/plugins-workspace",
        Role::Runtime,
        &[Desktop],
    ),
    lib(
        "tauri-plugin-updater",
        "Apache-2.0 OR MIT",
        "https://github.com/tauri-apps/plugins-workspace",
        Role::Runtime,
        &[Desktop],
    ),
    lib(
        "thiserror",
        "MIT OR Apache-2.0",
        "https://github.com/dtolnay/thiserror",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "tokio",
        "MIT",
        "https://github.com/tokio-rs/tokio",
        Role::Runtime,
        &[Core, Tui, Desktop],
    ),
    lib(
        "toml",
        "MIT OR Apache-2.0",
        "https://github.com/toml-rs/toml",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "toml_edit",
        "MIT OR Apache-2.0",
        "https://github.com/toml-rs/toml",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "url",
        "MIT OR Apache-2.0",
        "https://github.com/servo/rust-url",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "uuid",
        "Apache-2.0 OR MIT",
        "https://github.com/uuid-rs/uuid",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "unicode-width",
        "MIT OR Apache-2.0",
        "https://github.com/unicode-rs/unicode-width",
        Role::Runtime,
        &[Tui],
    ),
    lib(
        "windows-sys",
        "MIT OR Apache-2.0",
        "https://github.com/microsoft/windows-rs",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "x25519-dalek",
        "BSD-3-Clause",
        "https://github.com/dalek-cryptography/curve25519-dalek",
        Role::Runtime,
        &[Core],
    ),
    lib(
        "tauri-build",
        "Apache-2.0 OR MIT",
        "https://github.com/tauri-apps/tauri",
        Role::Build,
        &[Desktop],
    ),
    lib(
        "minisign",
        "MIT",
        "https://github.com/jedisct1/rust-minisign",
        Role::Test,
        &[Desktop],
    ),
    lib(
        "parking_lot",
        "MIT OR Apache-2.0",
        "https://github.com/Amanieu/parking_lot",
        Role::Runtime,
        &[Core, Desktop],
    ),
    lib(
        "rcgen",
        "MIT OR Apache-2.0",
        "https://github.com/rustls/rcgen",
        Role::Test,
        &[Desktop],
    ),
    lib(
        "tempfile",
        "MIT OR Apache-2.0",
        "https://github.com/Stebalien/tempfile",
        Role::Test,
        &[Core, Desktop],
    ),
    lib(
        "tokio-rustls",
        "MIT OR Apache-2.0",
        "https://github.com/rustls/tokio-rustls",
        Role::Test,
        &[Core],
    ),
    lib(
        "wiremock",
        "MIT OR Apache-2.0",
        "https://github.com/LukeMathWalker/wiremock-rs",
        Role::Test,
        &[Core],
    ),
    lib(
        "Lobe Icons",
        "MIT",
        "https://github.com/lobehub/lobe-icons",
        Role::Asset,
        &[Desktop],
    ),
    lib(
        "Simple Icons",
        "CC0-1.0",
        "https://github.com/simple-icons/simple-icons",
        Role::Asset,
        &[Desktop],
    ),
    lib(
        "Catppuccin",
        "MIT",
        "https://github.com/catppuccin/catppuccin",
        Role::Theme,
        &[Desktop],
    ),
    // Zeno Rocha (Brasil); default palette of the TUI and the desktop.
    lib(
        "Dracula",
        "MIT",
        "https://github.com/dracula/dracula-theme",
        Role::Theme,
        &[Tui, Desktop],
    )
    .made_in_brazil(),
    lib(
        "GitHub Primer",
        "MIT",
        "https://github.com/primer/primitives",
        Role::Theme,
        &[Desktop],
    ),
    lib(
        "One Dark (Atom)",
        "MIT",
        "https://github.com/atom/atom",
        Role::Theme,
        &[Desktop],
    ),
    lib(
        "Tokyo Night",
        "MIT",
        "https://github.com/enkia/tokyo-night-vscode-theme",
        Role::Theme,
        &[Desktop],
    ),
    lib(
        "Yaru (Ubuntu)",
        "GPL-3.0",
        "https://github.com/ubuntu/yaru",
        Role::Theme,
        &[Desktop],
    ),
    lib(
        "9router",
        "MIT",
        "https://github.com/decolua/9router",
        Role::Reference,
        &[Core],
    ),
    lib(
        "Qwen Code",
        "Apache-2.0",
        "https://github.com/QwenLM/qwen-code",
        Role::Reference,
        &[Core],
    ),
];

/// Every listed library, grouped by role in declaration order.
pub fn libraries() -> &'static [Library] {
    LIBRARIES
}

/// Libraries of one role, in alphabetical order.
pub fn libraries_with(role: Role) -> Vec<&'static Library> {
    let mut found: Vec<_> = LIBRARIES.iter().filter(|l| l.role == role).collect();
    found.sort_by_key(|l| l.name.to_ascii_lowercase());
    found
}

/// Listed projects created by Brazilians, alphabetical.
pub fn from_brazil() -> Vec<&'static Library> {
    let mut found: Vec<_> = LIBRARIES.iter().filter(|l| l.from_brazil).collect();
    found.sort_by_key(|l| l.name.to_ascii_lowercase());
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Names under `[dependencies]`, `[dev-dependencies]` and
    /// `[build-dependencies]`, top-level or per target (workspace-inherited
    /// ones included), excluding
    /// Cuotai's own crates.
    fn declared(manifest: &Path) -> Option<BTreeSet<String>> {
        let text = std::fs::read_to_string(manifest).ok()?;
        let doc: toml::Table = text.parse().ok()?;
        let mut names = BTreeSet::new();
        // Platform sections (`[target.'cfg(..)'.dependencies]`) count too.
        let mut tables = vec![&doc];
        if let Some(targets) = doc.get("target").and_then(|v| v.as_table()) {
            tables.extend(targets.values().filter_map(|v| v.as_table()));
        }
        for table in tables {
            for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(deps) = table.get(section).and_then(|v| v.as_table()) {
                    names.extend(deps.keys().filter(|k| !k.starts_with("cuotai")).cloned());
                }
            }
        }
        Some(names)
    }

    #[test]
    fn manifests_are_covered() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifests = [
            ("crates/cuotai-core/Cargo.toml", Core),
            ("crates/cuotai-tui/Cargo.toml", Tui),
            ("apps/desktop/src-tauri/Cargo.toml", Desktop),
        ];
        for (path, component) in manifests {
            let Some(names) = declared(&root.join(path)) else {
                continue; // published crate without the monorepo around it
            };
            for name in names {
                let entry = LIBRARIES
                    .iter()
                    .find(|l| l.name == name && l.used_by.contains(&component));
                assert!(
                    entry.is_some(),
                    "{name} ({path}) is not listed in about::LIBRARIES for {component:?}"
                );
            }
        }
    }

    #[test]
    fn entries_are_well_formed_and_unique() {
        let mut seen = BTreeSet::new();
        for library in LIBRARIES {
            assert!(seen.insert(library.name), "duplicate {}", library.name);
            assert!(
                library.repository.starts_with("https://"),
                "{}",
                library.name
            );
            assert!(!library.license.is_empty(), "{}", library.name);
            assert!(!library.used_by.is_empty(), "{}", library.name);
        }
    }

    #[test]
    fn listed_runtime_crates_are_really_declared() {
        // The reverse direction: a removed dependency must leave the list.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut all = BTreeSet::new();
        for path in [
            "crates/cuotai-core/Cargo.toml",
            "crates/cuotai-tui/Cargo.toml",
            "apps/desktop/src-tauri/Cargo.toml",
        ] {
            match declared(&root.join(path)) {
                Some(names) => all.extend(names),
                None => return,
            }
        }
        for library in LIBRARIES
            .iter()
            .filter(|l| matches!(l.role, Role::Runtime | Role::Build | Role::Test))
        {
            assert!(
                all.contains(library.name),
                "{} is listed but no manifest declares it",
                library.name
            );
        }
    }

    #[test]
    fn groups_are_sorted() {
        let names: Vec<_> = libraries_with(Role::Runtime)
            .iter()
            .map(|l| l.name)
            .collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_ascii_lowercase());
        assert_eq!(names, sorted);
        assert_eq!(libraries().len(), LIBRARIES.len());
    }
}
