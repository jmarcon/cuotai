//! Installation-source classification shared by every Cuotai frontend.
//!
//! The detector is deliberately path- and environment-based. It never queries a
//! package manager or the network, and it only identifies ownership when the
//! executable path is durable evidence. In particular, a normal current-user
//! winget NSIS install and a manual NSIS install share the same path, so the
//! winget manifest passes `/WINGET=1` and the NSIS installer then writes the
//! durable marker [`MARKER_FILE`] (containing `winget`) beside the executable;
//! a manual install removes it (PRD 0018 RF3.2).

use std::{ffi::OsString, io::Read, path::Path};

/// The installer-written receipt beside the executable that names its owner
/// (`apps/desktop/src-tauri/windows/hooks.nsh`).
pub const MARKER_FILE: &str = "install-source.txt";

/// The owner a marker beside `executable` names, read at most 64 bytes.
fn marker(executable: &Path) -> Option<String> {
    let file = std::fs::File::open(executable.parent()?.join(MARKER_FILE)).ok()?;
    let mut text = String::new();
    file.take(64).read_to_string(&mut text).ok()?;
    Some(text.trim().to_ascii_lowercase())
}

/// The owner and update policy of the running Cuotai executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallSource {
    /// Direct download (a copied app, zip or tarball).
    Direct,
    /// Linux AppImage, proven by the `APPIMAGE` environment variable.
    AppImage,
    /// macOS application still running from a mounted disk image.
    DiskImage,
    /// macOS Gatekeeper App Translocation.
    Translocated,
    /// The installation directory cannot be written.
    ReadOnly,
    /// Homebrew Caskroom (desktop cask).
    Homebrew,
    /// Homebrew Cellar (CLI formula).
    HomebrewFormula,
    /// The exact Cuotai package under Scoop's apps directory.
    Scoop,
    /// A package in winget's package cache.
    Winget,
    /// A distribution package or AUR installation under `/usr`.
    LinuxPackage,
    /// A CLI installed into Cargo's configured/default binary directory.
    Cargo,
}

impl InstallSource {
    /// Stable identifier used by frontends and diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::AppImage => "appimage",
            Self::DiskImage => "disk_image",
            Self::Translocated => "translocated",
            Self::ReadOnly => "read_only",
            Self::Homebrew => "homebrew",
            Self::HomebrewFormula => "homebrew",
            Self::Scoop => "scoop",
            Self::Winget => "winget",
            Self::LinuxPackage => "linux_package",
            Self::Cargo => "cargo",
        }
    }

    /// Whether the desktop updater may replace this installation in place.
    pub const fn can_install(self) -> bool {
        matches!(self, Self::Direct | Self::AppImage)
    }

    /// Why the in-app updater must stay off for a package-manager-owned copy.
    pub const fn updater_disabled_reason(self) -> Option<&'static str> {
        match self {
            Self::Homebrew | Self::HomebrewFormula => Some("homebrew"),
            Self::Scoop => Some("scoop"),
            Self::Winget => Some("winget"),
            Self::LinuxPackage => Some("linux_package"),
            Self::Cargo => Some("cargo"),
            _ => None,
        }
    }

    /// The command that upgrades this package-manager-owned installation.
    ///
    /// A generic Linux package can be deb, rpm or AUR, so it intentionally
    /// does not prescribe a distribution-specific command.
    pub const fn upgrade_command(self) -> Option<&'static str> {
        match self {
            Self::Homebrew => Some("brew upgrade --cask cuotai"),
            Self::HomebrewFormula => Some("brew upgrade cuotai"),
            Self::Scoop => Some("scoop update cuotai"),
            Self::Winget => Some("winget upgrade Cuotai.Cuotai"),
            Self::Cargo => Some("cargo install cuotai-tui --locked"),
            _ => None,
        }
    }
}

/// Classify an executable from its path, process environment and install-dir
/// writability.
///
/// `environment` is normally [`std::env::var_os`]. Accepting it as a lookup
/// keeps the classification deterministic in tests and lets embedders provide
/// their already-sanitized environment without mutating process-global state.
/// `install_dir_writable` is supplied by the frontend because the desktop
/// updater knows the bundle root while the CLI owns a plain executable.
pub fn detect_install_source(
    executable: &Path,
    environment: impl Fn(&str) -> Option<OsString>,
    install_dir_writable: impl FnOnce() -> bool,
) -> InstallSource {
    classify(executable, environment, install_dir_writable, || {
        marker(executable)
    })
}

fn classify(
    executable: &Path,
    environment: impl Fn(&str) -> Option<OsString>,
    install_dir_writable: impl FnOnce() -> bool,
    marker: impl FnOnce() -> Option<String>,
) -> InstallSource {
    if environment("APPIMAGE").is_some() {
        return InstallSource::AppImage;
    }

    // Keep these path rules in the same precedence as the desktop detector
    // that this module replaced (PRD 0018 RF2.5 / PRD 0021 RF8.1).
    let path = executable.to_string_lossy().replace('\\', "/");
    let normalized = path.to_ascii_lowercase();
    if path.starts_with("/Volumes/") {
        return InstallSource::DiskImage;
    }
    if path.contains("/AppTranslocation/") {
        return InstallSource::Translocated;
    }
    if path.contains("/Caskroom/cuotai/") {
        return InstallSource::Homebrew;
    }
    if path.contains("/Cellar/cuotai/") {
        return InstallSource::HomebrewFormula;
    }
    if normalized.contains("/scoop/apps/cuotai/") {
        return InstallSource::Scoop;
    }
    if normalized.contains("/appdata/local/microsoft/winget/packages/") {
        return InstallSource::Winget;
    }
    if path.starts_with("/usr/bin/")
        || path.starts_with("/usr/sbin/")
        || path.starts_with("/usr/lib/")
    {
        return InstallSource::LinuxPackage;
    }
    if cargo_home(&environment).is_some_and(|home| in_cargo_bin(&normalized, &home)) {
        return InstallSource::Cargo;
    }
    // PRD 0018 RF3.2: the installer's receipt for a winget install at the
    // shared NSIS path.
    if marker().as_deref() == Some("winget") {
        return InstallSource::Winget;
    }
    if !install_dir_writable() {
        return InstallSource::ReadOnly;
    }
    InstallSource::Direct
}

fn cargo_home(environment: &impl Fn(&str) -> Option<OsString>) -> Option<OsString> {
    if let Some(home) = environment("CARGO_HOME") {
        return (!home.is_empty()).then_some(home);
    }
    environment("HOME")
        .or_else(|| environment("USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(|mut home| {
            home.push("/.cargo");
            home
        })
}

fn in_cargo_bin(executable: &str, cargo_home: &std::ffi::OsStr) -> bool {
    let root = cargo_home
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    if root.is_empty() {
        return false;
    }
    executable == format!("{root}/bin/cuotai") || executable == format!("{root}/bin/cuotai.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(path: &str, environment: &[(&str, &str)], writable: bool) -> InstallSource {
        detect_install_source(
            Path::new(path),
            |key| {
                environment
                    .iter()
                    .find_map(|(name, value)| (*name == key).then(|| OsString::from(value)))
            },
            || writable,
        )
    }

    /// PRD 0018 RF3.2: the installer-written receipt tells a winget NSIS
    /// install from a manual one at the same path.
    #[test]
    fn the_winget_receipt_marks_the_shared_nsis_path() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Cuotai.exe");
        std::fs::write(&exe, b"").unwrap();
        let none = |_: &str| None;
        assert_eq!(
            detect_install_source(&exe, none, || true),
            InstallSource::Direct
        );
        std::fs::write(dir.path().join(MARKER_FILE), "winget\r\n").unwrap();
        let source = detect_install_source(&exe, none, || true);
        assert_eq!(source, InstallSource::Winget);
        assert_eq!(source.as_str(), "winget");
        assert!(source.updater_disabled_reason().is_some());
        // Anything else in the file is no evidence.
        std::fs::write(dir.path().join(MARKER_FILE), "manual").unwrap();
        assert_eq!(
            detect_install_source(&exe, none, || true),
            InstallSource::Direct
        );
        // A path rule still wins over a stray receipt.
        assert_eq!(
            classify(
                Path::new("C:/Users/demo/scoop/apps/cuotai/current/Cuotai.exe"),
                none,
                || true,
                || Some("winget".into())
            ),
            InstallSource::Scoop
        );
    }

    #[test]
    fn direct_appimage_and_read_only_are_distinct() {
        assert_eq!(
            detect("/Applications/Cuotai.app/Contents/MacOS/Cuotai", &[], true),
            InstallSource::Direct
        );
        assert_eq!(
            detect(
                "/usr/local/bin/Cuotai",
                &[("APPIMAGE", "/tmp/Cuotai-x86_64.AppImage")],
                true
            ),
            InstallSource::AppImage
        );
        assert_eq!(
            detect("/opt/Cuotai/Cuotai", &[], false),
            InstallSource::ReadOnly
        );
        assert_eq!(
            detect("/opt/Cuotai/Cuotai", &[], true),
            InstallSource::Direct
        );
    }

    #[test]
    fn disk_image_and_translocation_win_over_writability() {
        for writable in [false, true] {
            assert_eq!(
                detect(
                    "/Volumes/Cuotai/Cuotai.app/Contents/MacOS/Cuotai",
                    &[],
                    writable
                ),
                InstallSource::DiskImage
            );
        }
        assert_eq!(
            detect(
                "/private/var/folders/ab/AppTranslocation/1234/d/Cuotai.app/Contents/MacOS/Cuotai",
                &[],
                true
            ),
            InstallSource::Translocated
        );
    }

    #[test]
    fn package_managers_have_exact_sources_policies_and_commands() {
        let cases = [
            (
                "/opt/homebrew/Caskroom/cuotai/0.76.0/Cuotai.app/Contents/MacOS/Cuotai",
                &[][..],
                InstallSource::Homebrew,
                Some("brew upgrade --cask cuotai"),
            ),
            (
                "/opt/homebrew/Cellar/cuotai/0.76.0/bin/cuotai",
                &[][..],
                InstallSource::HomebrewFormula,
                Some("brew upgrade cuotai"),
            ),
            (
                "C:\\Users\\demo\\Scoop\\Apps\\Cuotai\\current\\Cuotai.exe",
                &[][..],
                InstallSource::Scoop,
                Some("scoop update cuotai"),
            ),
            (
                "C:\\Users\\demo\\AppData\\Local\\Microsoft\\WinGet\\Packages\\Cuotai.Cuotai_MZ394459P08D4Z\\Cuotai.exe",
                &[][..],
                InstallSource::Winget,
                Some("winget upgrade Cuotai.Cuotai"),
            ),
            (
                "/usr/bin/cuotai",
                &[][..],
                InstallSource::LinuxPackage,
                None,
            ),
            (
                "/usr/sbin/cuotai",
                &[][..],
                InstallSource::LinuxPackage,
                None,
            ),
            (
                "/usr/lib/cuotai/cuotai",
                &[][..],
                InstallSource::LinuxPackage,
                None,
            ),
            (
                "/Users/demo/.cargo/bin/cuotai",
                &[("HOME", "/Users/demo")][..],
                InstallSource::Cargo,
                Some("cargo install cuotai-tui --locked"),
            ),
            (
                "D:\\cargo\\bin\\cuotai.exe",
                &[("CARGO_HOME", "D:\\cargo")][..],
                InstallSource::Cargo,
                Some("cargo install cuotai-tui --locked"),
            ),
        ];

        for (path, environment, expected, command) in cases {
            let source = detect(path, environment, true);
            assert_eq!(source, expected, "{path}");
            assert!(!source.can_install(), "{path}");
            assert_eq!(source.upgrade_command(), command, "{path}");
            assert_eq!(
                source.updater_disabled_reason(),
                Some(source.as_str()),
                "{path}"
            );
        }
    }

    #[test]
    fn lookalikes_and_normal_tauri_nsis_paths_stay_direct() {
        for path in [
            "/home/demo/scoop/apps-really/cuotai/Cuotai",
            "C:/tools/scoop/apps/other/Cuotai.exe",
            // Tauri's real current-user NSIS path is shared by normal winget
            // and manual installs, so it is not durable ownership evidence.
            "C:/Users/demo/AppData/Local/Cuotai/Cuotai.exe",
            "C:/Users/demo/AppData/Local/Programs/Cuotai/Cuotai.exe",
            "/home/demo/Caskroom/not-cuotai/Cuotai.app/Contents/MacOS/Cuotai",
            "/home/demo/usr/sbin/cuotai",
            "/opt/Cuotai/Microsoft/WinGet/Packages/cuotai.exe",
        ] {
            assert_eq!(detect(path, &[], true), InstallSource::Direct, "{path}");
        }
        assert_eq!(
            detect(
                "/Users/other/.cargo/bin/cuotai",
                &[("HOME", "/Users/demo")],
                true
            ),
            InstallSource::Direct
        );
        assert_eq!(
            detect(
                "/Users/demo/.cargo/bin/not-cuotai",
                &[("HOME", "/Users/demo")],
                true
            ),
            InstallSource::Direct
        );
    }
}
