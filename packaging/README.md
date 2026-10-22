# Package-manager templates (PRD 0021 RF5)

This directory holds the **templates** for every package-manager channel.
They are data files, not hand-maintained manifests: on every stable release
the pipeline renders the real manifests from the verified release
(`manifest.json` + `signing-state.json`) with

```sh
cargo xtask package-manifests \
  --release-manifest dist --repo owner/name --tag vX.Y.Z \
  --templates-dir packaging --out rendered
```

and the channel jobs of `.github/workflows/release.yml` validate and publish
the result. No rendered manifest is edited by hand after the first
submission (RF5.6): change the template here instead.

| Directory | Channel | Renders | Published to | Owner prerequisite |
|---|---|---|---|---|
| `homebrew/` | Homebrew tap (RF5.1) | `Casks/cuotai.rb` (desktop), `Formula/cuotai.rb` (CLI) | `jmarcon/homebrew-cuotai` | tap repository + scoped token (`HOMEBREW_TAP_TOKEN`) |
| `winget/` | winget (RF5.2) | `Cuotai.Cuotai{,.installer,.locale.en-US}.yaml` | `microsoft/winget-pkgs` via PR | first submission by hand; `WINGET_SUBMIT_TOKEN` afterwards |
| `scoop/` | Scoop bucket (RF5.3) | `bucket/cuotai.json` (CLI zip, `checkver`/`autoupdate`) | `jmarcon/scoop-cuotai` | bucket repository + scoped token (`SCOOP_BUCKET_TOKEN`) |
| `aur/` | AUR `cuotai-bin` (RF5.5) | `PKGBUILD` + `.SRCINFO` (desktop `.deb`) | `ssh://aur@aur.archlinux.org/cuotai-bin.git` | AUR account + SSH key (`AUR_SSH_PRIVATE_KEY`) |
| — | crates.io (RF5.4) | nothing to render: crate metadata in `crates/*/Cargo.toml` | crates.io | account, `CARGO_REGISTRY_TOKEN`, publish `cuotai-core` before `cuotai-tui` |

Every channel is **opt-in**: it runs only when its repository variable
(`PUBLISH_HOMEBREW`, `PUBLISH_WINGET`, `PUBLISH_SCOOP`, `PUBLISH_CARGO`,
`PUBLISH_AUR`) is set to `true`, always after the published release has been
re-verified, and its failure never affects the GitHub Release (RF5.7). A
`workflow_dispatch` dry run renders and validates everything but never
pushes.

## Uninstall preservation (RF8.2)

Uninstalling through any channel removes only the application and its
binaries. The configuration (`<OS config dir>/cuotai/config.toml`, e.g.
`~/Library/Application Support/cuotai/config.toml` on macOS), the local
databases (`<OS local data dir>/cuotai/`) and the OS credential-vault entries
(`cuotai.oauth`, `cuotai.serve`) survive unless the user explicitly opts in:

- **Homebrew**: `brew zap --cask cuotai` removes only the webview caches and
  preference plist listed in the cask's `zap` block — never the
  configuration, databases or Keychain entries. Purging those is a manual,
  documented step.
- **winget/Scoop**: the NSIS uninstaller and Scoop remove only program files;
  user data under `%APPDATA%\cuotai` and `%LOCALAPPDATA%\cuotai` stays.
- **AUR/distribution packages**: `pacman -R` removes packaged files only;
  `~/.config/cuotai` and `~/.local/share/cuotai` stay.
- **Cargo**: `cargo uninstall cuotai-tui` removes only the binary.

## Identifying managed installs (RF8.1 groundwork)

The evidence below is channel-specific only where noted:

- Homebrew cask: the resolved app path reaches the Caskroom receipt under
  `$(brew --prefix)/Caskroom/cuotai/`; formula:
  `$(brew --prefix)/Cellar/cuotai/<version>/bin/cuotai`.
- winget portable/package-cache copies:
  `%LocalAppData%\Microsoft\WinGet\Packages\...`.
- winget NSIS and manual NSIS, with Tauri's default current-user mode:
  `%LocalAppData%\Cuotai`. This path is shared and is **not** ownership
  evidence. The repository does not enable a per-machine `%ProgramFiles%` mode.
- Scoop: `~\scoop\apps\cuotai\current\cuotai.exe`.
- AUR/distribution: `/usr/bin/Cuotai` (package-owned).
- Cargo: `~/.cargo/bin/cuotai`.

The winget manifest invokes the same release NSIS installer as a direct
download and passes only `/S`. The desktop Tauri configuration defines no
custom NSIS hook, and Tauri's standard uninstall registry entry and shortcuts
are therefore identical for winget and manual installs. The repository
produces no winget-specific receipt, file, registry value or shortcut.

Until the owner chooses a durable marker contract, normal winget NSIS users
must set `CUOTAI_UPDATER=off`; Settings → Updates displays that override as
the disabled reason. Treating every shared NSIS path or standard uninstall
entry as winget-owned is rejected because it would disable safe in-app
updates for direct-download users. A reliable automatic distinction requires
either a dedicated winget artifact or an installer hook/switch that writes and
removes a Cuotai-owned source marker, plus the corresponding winget manifest
change and review.

Maintainers: templates are owned by the release pipeline (repository
maintainers); each channel's rendered output is owned by that channel's
repository after publishing.
