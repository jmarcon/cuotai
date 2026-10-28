# Installing, verifying and uninstalling Cuotai

This page is the full version of the README's install section: every channel with its honest status, what each download contains, how to verify it and how to remove Cuotai. For a walkthrough of the app on your system, see the guides for [macOS](gui-macos.md), [Windows](gui-windows.md) and [Linux](gui-linux.md). The maintainers' side of the pipeline (how releases and channels are produced) is in [`releasing.md`](releasing.md) and [`../packaging/README.md`](../packaging/README.md).

## Channel status

Cuotai 0.81.0 is published on GitHub Releases, the Homebrew tap and the Scoop bucket. Every other package-manager channel is opt-in and off until the maintainer enables it (PRD 0021; see [`plans/0000_control.md`](plans/0000_control.md)). The table records what was checked on 2026-10-28 and why.

| Channel | Command (when live) | Status today | Evidence |
|---|---|---|---|
| GitHub Releases (direct download) | download from the [Releases page](https://github.com/jmarcon/cuotai/releases) | **Live** | Release `v0.81.0` is published with the desktop installers, the terminal archives and the integrity files (the release pipeline `.github/workflows/release.yml` is tag-driven). The macOS `.dmg` is signed and notarized; the Windows installers are not code-signed yet. |
| Homebrew cask (desktop, macOS) | `brew install --cask jmarcon/cuotai/cuotai-app` | **Live** | The tap `jmarcon/homebrew-cuotai` holds the 0.81.0 cask. It was named `cuotai` in 0.81.0; the tap's `cask_renames.json` moves those installs to `cuotai-app` on their next `brew upgrade`. |
| Homebrew formula (terminal, macOS/Linux) | `brew install jmarcon/cuotai/cuotai` | **Live** | Same tap, 0.81.0 formula. |
| winget (desktop, Windows) | `winget install Cuotai.Cuotai` | **Not yet** | The first submission to `microsoft/winget-pkgs` is a manual owner step and has not happened. |
| Scoop (terminal, Windows) | `scoop bucket add cuotai https://github.com/jmarcon/scoop-cuotai` then `scoop install cuotai` | **Live** | The bucket `jmarcon/scoop-cuotai` holds the 0.81.0 manifest. |
| AUR `cuotai-bin` (desktop, Arch) | `yay -S cuotai-bin` | **Not yet** | The AUR RPC finds no `cuotai-bin` package. |
| crates.io (terminal) | `cargo install cuotai-tui --locked` or `cargo binstall cuotai-tui` | **Not yet** | crates.io reports that neither `cuotai-tui` nor `cuotai-core` exists. |
| `cargo install --git` (terminal) | `cargo install --git https://github.com/jmarcon/cuotai cuotai-tui --locked` | **Works today** | Builds from the repository's default branch; needs Rust. |
| Build from source (desktop and terminal) | see [`building.md`](building.md) | **Works today** | |

Debian/Ubuntu (`.deb`), Fedora (`.rpm`), AppImage, macOS `.dmg`, Windows `.exe`/`.msi` and the terminal archives all come from the GitHub release; they are not separate channels.

Shell completions: the Homebrew formula installs the bash, zsh and fish scripts from `cuotai --completions SHELL`; the Scoop manifest prints the PowerShell one-liner in its notes. For the other channels, see [Shell completions](tui.md#shell-completions).

## What a release contains

Names are stable so package managers can rely on them (`<version>` is for example `0.76.0`):

| Platform | Desktop app | Terminal (TUI/CLI) |
|---|---|---|
| macOS | `Cuotai_<version>_universal.dmg` (Apple silicon and Intel) | `cuotai-<version>-aarch64-apple-darwin.tar.gz`, `cuotai-<version>-x86_64-apple-darwin.tar.gz` |
| Windows | `Cuotai_<version>_x64-setup.exe`, `Cuotai_<version>_x64_en-US.msi`; ARM64: `Cuotai_<version>_arm64-setup.exe` (optional) | `cuotai-<version>-x86_64-pc-windows-msvc.zip`; ARM64 zip (optional) |
| Linux | `Cuotai_<version>_amd64.AppImage`, `.deb`, `.rpm`; ARM64: `_aarch64.*` (optional) | `cuotai-<version>-x86_64-unknown-linux-gnu.tar.gz`, `…-x86_64-unknown-linux-musl.tar.gz`; ARM64 gnu (optional) |
| Integrity | `SHA256SUMS`, `SHA256SUMS.sigstore.json`, `manifest.json`, `signing-state.json`, `sbom.cdx.json` | |

The Linux terminal archives are built on the oldest supported Ubuntu runner; the musl build needs a D-Bus session for the credential vault (without it, use environment-variable credentials).

## Signing: what to expect

Each release records, per file, whether it is signed (`signing-state.json`, quoted in the release notes). Nothing claims a signature a file does not have.

- **macOS:** builds are signed and notarized only when the maintainer's Apple Developer ID credentials were available for that release. Otherwise the app is ad-hoc signed and Gatekeeper blocks it until you allow it ([steps](gui-macos.md#first-launch-and-gatekeeper)).
- **Windows:** the installers are Authenticode-signed only when a code-signing certificate was available. Otherwise Windows SmartScreen warns about an unknown publisher ([steps](gui-windows.md#first-launch-and-smartscreen)).
- **Linux and terminal archives:** the files are not individually signed; trust comes from the signed `SHA256SUMS` below.
- **In-app updates** are verified with a separate update key (see [`updater-security.md`](updater-security.md)).

## Verify your download

Every release publishes `SHA256SUMS`, signed with Sigstore (keyless, tied to this repository's GitHub Actions workflow), and a GitHub build-provenance attestation for each file.

```sh
# every downloaded asset in one go (run in the folder with the files)
shasum -a 256 -c SHA256SUMS                        # macOS / Linux
# Windows PowerShell: (Get-FileHash .\<file>).Hash and compare with the line in SHA256SUMS

# the build provenance of one file (needs the GitHub CLI)
gh attestation verify Cuotai_<version>_universal.dmg --repo jmarcon/cuotai

# the signature of the checksum file itself (needs cosign v3 or later)
cosign verify-blob \
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity-regexp '^https://github\.com/jmarcon/cuotai/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
```

`SHA256SUMS.sigstore.json` is a Sigstore bundle: it holds the signature, the short-lived signing certificate and the transparency-log proof in one file.

Release builds use a pinned toolchain and locked dependencies, and ship a per-release SBOM (see [`release-policy.md`](release-policy.md)).

## Uninstalling

Uninstalling removes the application or binary only. Your configuration (`<OS config dir>/cuotai/config.toml`), the local databases and the OS vault entries (macOS Keychain, Windows Credential Manager, Linux Secret Service) stay behind on purpose, so a reinstall picks up where you left off.

| Channel | What uninstall removes | What it always keeps |
|---|---|---|
| `brew uninstall --cask cuotai-app` | `/Applications/Cuotai.app` | config, databases, Keychain entries |
| `brew zap --cask cuotai-app` (opt-in) | also the app's web caches and preference plist, and only that | config, databases, Keychain entries |
| `winget uninstall Cuotai.Cuotai` | the installed program files | `%APPDATA%\cuotai`, `%LOCALAPPDATA%\cuotai`, Credential Manager entries |
| `scoop uninstall cuotai` | the binary | the same folders and entries |
| `pacman -R cuotai-bin` | packaged files (`/usr/bin/Cuotai`, ...) | `~/.config/cuotai`, `~/.local/share/cuotai`, Secret Service entries |
| `cargo uninstall cuotai-tui` | `~/.cargo/bin/cuotai` | everything else |

To also erase your data, remove it yourself (after quitting Cuotai):

```sh
# macOS: configuration + local databases, then the vault entries
rm -rf ~/Library/Application\ Support/cuotai
security delete-generic-password -s cuotai.oauth
security delete-generic-password -s cuotai.serve
# Linux
rm -rf ~/.config/cuotai ~/.local/share/cuotai
# Windows (PowerShell)
Remove-Item -Recurse -Force "$env:APPDATA\cuotai", "$env:LOCALAPPDATA\cuotai"
```

The maintainers' reference for this behavior (and how it is proven in CI) is [`releasing.md`](releasing.md#uninstall-keeps-your-data-rf82). On Windows and Linux, the vault entries are removed in Credential Manager or with your keyring tool (`cuotai.oauth`, `cuotai.serve`); see the platform guides.

## Updating

Copies installed by Homebrew, Scoop, winget or a Linux system package are updated by that manager; the in-app updater turns itself off for them and shows the command to use. For other installs, **Settings › About › Updates** checks once a day (off by preference) and sends only the version, the operating system and the architecture. Details and the key custody are in [`updater-security.md`](updater-security.md).
