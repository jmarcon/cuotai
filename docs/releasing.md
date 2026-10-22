# Releasing Cuotai

This document describes the release pipeline (PRD 0021 RF1–RF5): how a tag
becomes a verified GitHub Release, what is signed and what is not, how to
rehearse in a fork, the opt-in package-manager channels (RF5), and what the
owner must configure. A channel is contacted **only** when its repository
variable (`PUBLISH_HOMEBREW`, `PUBLISH_WINGET`, `PUBLISH_SCOOP`,
`PUBLISH_CARGO`, `PUBLISH_AUR`) is set to `true`, always after the published
release has been re-verified; until the owner enables one, the pipeline
renders and validates it locally and pushes nothing.

## How a release works

1. The delivering commit raises the version everywhere (see
   [`AGENTS.md`](../AGENTS.md): workspace `Cargo.toml`,
   `apps/desktop/src-tauri/Cargo.toml`, `tauri.conf.json`, both lockfiles,
   `CHANGELOG.md` section). CI's `version-check` job proves they agree.
2. The owner creates an **annotated** tag on `main` and pushes it:
   `git tag -a v0.68.0 -m "v0.68.0" <commit>` then `git push origin v0.68.0`.
3. `.github/workflows/release.yml` runs: **check → builds → assemble →
   publish → verify-public → package-manifests → channel jobs**.

```text
check ──┬─ desktop-macos ───┐
        ├─ desktop-windows ─┤
        ├─ desktop-windows-arm64 (optional) ─┤
        ├─ desktop-linux ───┤
        ├─ desktop-linux-arm64 (optional) ───┤
        ├─ cli (7 legs, 2 optional) ─────────┤
        └─ sbom ────────────┴─► assemble ─► publish ─► verify-public
                                                       │
                                                       └─► package-manifests
                                                             ├─► channel-homebrew
                                                             ├─► channel-winget
                                                             ├─► channel-scoop
                                                             ├─► channel-cargo
                                                             └─► channel-aur
```

An interrupted or failing run leaves at most a **draft** release — never a
public partial one. The channel jobs run last, are independent of each
other, and nothing depends on them: a channel failure never affects the
GitHub Release (RF5.7).

## Trigger rules (RF1)

- The tag must be **annotated** (`git tag -a`), named `vX.Y.Z` (stable) or
  `vX.Y.Z-rc.N` (release candidate), and its commit must be contained in
  `main`.
- The tag's version must equal the workspace version, the desktop version,
  `tauri.conf.json`, both lockfiles and the top released `CHANGELOG.md`
  section, which must exist and be non-empty. Any mismatch fails the `check`
  job **before any build starts**.
- `rc` tags become GitHub **pre-releases** and never reach package managers.

The check is one Rust command, run locally with:

```sh
cargo xtask release-check                          # internal consistency
cargo xtask release-check --tag v0.68.0            # + tag format and version match
cargo xtask release-check --tag v0.68.0 --require-main origin/main   # + git ancestry (needs the tag locally)
```

## Dry run (RF1.4)

**Actions → Release → Run workflow**, optionally with a tag name (default:
`v<workspace version>`). A dispatch run:

- builds and packages every artifact exactly like a tag run;
- uploads them as workflow artifacts (`release-assembly`);
- **never** creates or modifies a GitHub Release, never contacts a package
  manager, never notarizes, and skips Sigstore signing and provenance
  attestations (stated in the run summary and in `signing-state.json`);
- behaves as if no signing secrets existed, so the honest-unsigned path is
  always exercised.

The whole pipeline also runs in a **fork**: push an `rc` tag there (or use
the dry run) and every step works without any production secret.

## Artifacts (RF2)

Stable names (package manifests can rely on them):

| Artifact | Runner | Name |
|---|---|---|
| Desktop macOS universal | `macos-latest` | `Cuotai_<version>_universal.dmg`; updater archive `Cuotai.app.tar.gz` + `.sig` when the updater key exists |
| Desktop Windows x64 | `windows-latest` | `Cuotai_<version>_x64-setup.exe`, `Cuotai_<version>_x64_en-US.msi` |
| Desktop Windows ARM64 (optional) | `windows-11-arm` | `Cuotai_<version>_arm64-setup.exe` |
| Desktop Linux x64 | `ubuntu-22.04` (oldest supported glibc) | `Cuotai_<version>_amd64.AppImage`, `Cuotai_<version>_amd64.deb`, `Cuotai_<version>_amd64.rpm` |
| Desktop Linux ARM64 (optional) | `ubuntu-24.04-arm` | `Cuotai_<version>_aarch64.AppImage`, `Cuotai_<version>_aarch64.deb`, `Cuotai_<version>_aarch64.rpm` |
| CLI macOS | `macos-latest` | `cuotai-<version>-aarch64-apple-darwin.tar.gz`, `…-x86_64-apple-darwin.tar.gz` |
| CLI Linux | `ubuntu-22.04` | `cuotai-<version>-x86_64-unknown-linux-gnu.tar.gz`, `…-x86_64-unknown-linux-musl.tar.gz`; `…-aarch64-unknown-linux-gnu.tar.gz` (optional) |
| CLI Windows | `windows-latest` / `windows-11-arm` | `cuotai-<version>-x86_64-pc-windows-msvc.zip`; `…-aarch64-pc-windows-msvc.zip` (optional) |
| Integrity | assembly job | `SHA256SUMS`, `SHA256SUMS.sig` + `SHA256SUMS.pem` (tag runs), `manifest.json`, `signing-state.json`, `sbom.cdx.json` |
| Updater (PRD 0018) | assembly job | `latest.json` (only when the updater artifacts are signed), `Cuotai.app.tar.gz` + `.sig` (macOS), `<installer>.sig` next to the Windows NSIS/MSI and the Linux AppImage/deb/rpm |

Notes and limits:

- **musl**: `x86_64-unknown-linux-musl` builds (`rusqlite` is bundled and
  `keyring`'s Secret Service store uses the pure-Rust zbus). The Secret
  Service credential vault needs a **D-Bus session at run time**; on a musl
  system without D-Bus, vault-backed accounts are unavailable and
  environment-variable credentials are the fallback.
- **`aarch64-unknown-linux-musl` is not produced**: the Ubuntu runners ship
  no aarch64 musl cross toolchain, and downloading an external toolchain was
  rejected on supply-chain grounds. Revisit when a hosted toolchain exists.
- **ARM64 legs are optional** (RF2.5): hosted ARM runners are used with job-
  level `continue-on-error`, and the assembly job runs under `always()` while
  requiring only the mandatory jobs (`check`, `desktop-macos`, `desktop-
  windows`, `desktop-linux`, `sbom`). The `cli` matrix row is listed under
  `needs` but its aggregate result does **not** gate the assemble job, so a
  single unavailable ARM runner (or a partial `cli` failure such as run
  39673307029) cannot skip assembly. The authoritative required-artifact gate
  is `xtask release-manifest`: a missing mandatory x64/macOS/musl/Windows CLI
  artifact fails the job; missing optional artifacts are listed in
  `manifest.json` (`missing_optional`) and in the release notes. Checkout,
  tool-install, packaging, collection and signing failures on any required
  job still fail `assemble` the same way as before.
- Every build uses `--locked` (the desktop additionally verifies its lockfile
  with `cargo metadata --locked` before `cargo tauri build`, whose trailing
  args are passed to Cargo: `cargo tauri build … -- --locked`), the exact Rust
  `1.99.1` compiler pinned repository-wide by `rust-toolchain.toml` and used
  explicitly by CI/release, and a per-job timeout.
- **`sbom.cdx.json` is an aggregate**, not a single workspace dump: `cargo
  cyclonedx` runs for the CLI/TUI workspace and for the desktop workspace, and
  `cargo xtask merge-sbom` merges them. The merge validates `bomFormat`,
  `specVersion` and `metadata.component` of both inputs, de-duplicates
  components by `bom-ref`, rejects any dependency reference that does not
  resolve to a component, sorts components and dependency entries, and hangs
  both workspace roots off one aggregate root component, so the published
  document is a single connected graph covering `cuotai-core`, the CLI and the
  Tauri desktop dependencies. `serialNumber` and `metadata.timestamp` come
  from the inputs and the tagged commit timestamp, so a rerun reproduces the
  same bytes.

**Desktop build tool decision (RF2.1):** the desktop is built with
`cargo tauri build` directly (`tauri-cli` 2.12.4, installed with
`cargo install --locked`) instead of `tauri-apps/tauri-action`. Reasons: the
project has no JavaScript package manager (the frontend is static files), the
action manages GitHub Releases on its own which would conflict with the
draft→verify→publish flow here, and plain Cargo keeps every tool pinned and
the supply chain to crates.io only.

## Signing and trust (RF3)

Per platform:

- **macOS**: trusted signing is enabled only when the Developer ID certificate
  trio (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_SIGNING_IDENTITY`) and one complete notarization set are all present:
  either App Store Connect API key `APPLE_API_KEY` / `APPLE_API_KEY_ID` /
  `APPLE_API_ISSUER`, or `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`.
  The complete app payload is signed with the hardened runtime and timestamp,
  submitted, stapled and Gatekeeper-validated; the DMG is then recreated from
  that stapled app, signed with the same Developer ID, independently
  submitted, stapled and Gatekeeper-validated. With any incomplete set, both
  the app payload and DMG container are explicitly ad-hoc signed and verified,
  but remain untrusted and are marked **unsigned**.
- **Windows**: with `WINDOWS_CERTIFICATE` (base64 `.pfx`) +
  `WINDOWS_CERTIFICATE_PASSWORD`, the application executable is signed and
  verified before Tauri bundles it without binary patching; the resulting
  NSIS and MSI installers are then signed and verified. Without credentials,
  the application and installers remain **unsigned**, as stated in the notes.
- **Linux and CLI archives**: unsigned files; trust comes from the signed
  `SHA256SUMS` (and, later, from package-repository keys).
- **`SHA256SUMS`**: on tag runs it is signed with **Sigstore keyless**
  (cosign, identity = this repository's GitHub Actions OIDC token), producing
  `SHA256SUMS.sig` + `SHA256SUMS.pem`. On dry runs the signature is honestly
  marked unavailable. There is no project key to rotate; the identity is the
  workflow.
- **Tauri updater key** (`TAURI_SIGNING_PRIVATE_KEY` +
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`): used **only** for the in-app updater
  signatures of PRD 0018, never for code signing or checksums. The public key
  is committed in `tauri.conf.json` (`plugins.updater.pubkey`), and
  [`docs/updater-security.md`](updater-security.md) holds the key custody,
  the leak procedure and the loss procedure. Signing happens where the bytes
  are final, which is a different step per platform:
  | Platform | Where | Why |
  |---|---|---|
  | macOS | `collect-macos.sh`, after notarization | the updater artifact is a tarball of the **notarized** app; a bundle-time archive would carry the pre-notarization app and fail Gatekeeper |
  | Windows | `scripts/release/updater-sign.sh`, after Authenticode | Authenticode rewrites the installer, so a bundle-time updater signature would not verify |
  | Linux | the Tauri bundler, via `--config src-tauri/tauri.updater.conf.json` | nothing rewrites the AppImage/deb/rpm after bundling, so the bundler's signature is final |
  When the key is absent (dry run, fork), no `.sig` and no `latest.json` are
  produced and `signing-state.json` says why.
- Every published release asset gets a GitHub **build-provenance attestation**
  (verifiable with `gh attestation verify`) on tag runs. Dry runs do not
  request or publish attestations and say so in the workflow summary.

`signing-state.json` (schema version 2) is a first-class output: per artifact
`{name, signed, method, status, updater_signature?, note?}`, the
checksum-signature state, and the updater state. `updater_signature` records
the detached Tauri proof beside a payload that keeps its Authenticode or
checksum proof. The assembly **fails** when an artifact has no signing entry,
when an entry describes a file that does not exist, or when an entry claims
`signed` dishonestly. Methods: `developer-id`, `ad-hoc`, `authenticode`,
`tauri-updater-key`, `checksum-signature`, `none`.

## GitHub Releases (RF4)

- The release body is the version's `CHANGELOG.md` section, followed by the
  signing-state table, the list of unproduced optional artifacts and the
  verification commands. A missing or empty changelog section fails the
  release.
- The publish job creates a **draft**, uploads every desired asset, downloads
  the complete draft set, rejects missing or extra names and compares every
  byte with the local assembly. It also rechecks payload checksums, verifies
  the Sigstore signature/certificate against this repository's tag workflow
  identity, and verifies every GitHub provenance attestation before flipping
  `draft=false`. A final `verify-public` job repeats the complete comparison
  and all integrity/provenance checks against the public release.
- **Idempotence**: a draft rerun deletes stale draft-only assets, then replaces
  current names with `--clobber`. A published release is never modified or
  deleted: the publish job refuses to touch it.

## Package-manager channels (RF5)

Every channel's manifest is **rendered**, never hand-written. One command
turns the verified release into all of them:

```sh
# locally, from the assembly of a run (manifest.json + signing-state.json)
cargo xtask package-manifests \
  --release-manifest dist --repo jmarcon/cuotai --tag v0.68.0 \
  --templates-dir packaging --out rendered
```

Inputs are the release `manifest.json` (stable file names, sizes, SHA-256)
and `signing-state.json`; URLs are derived from the repository slug and the
tag. Rendering is deterministic — the same inputs always produce the same
bytes — and each channel's output is structurally validated before it is
written (Ruby block balance and required stanzas, winget YAML fields and
SHA-256 shape, Scoop JSON with `checkver`/`autoupdate`, PKGBUILD assignments
and `.SRCINFO` agreement). The templates live in
[`packaging/`](../packaging/README.md), one directory per channel.

| Channel | Renders | Own linter in CI | Publishes to |
|---|---|---|---|
| Homebrew tap (RF5.1) | `Casks/cuotai.rb` (desktop), `Formula/cuotai.rb` (CLI) | `brew style`, `brew audit --strict --online`, `brew install --cask` | `jmarcon/homebrew-cuotai` |
| winget (RF5.2) | `Cuotai.Cuotai{,.installer,.locale.en-US}.yaml` | `winget validate`, silent install/uninstall | `microsoft/winget-pkgs` |
| Scoop (RF5.3) | `bucket/cuotai.json` (CLI zip, `checkver: github`) | JSON validation, `scoop install` | `jmarcon/scoop-cuotai` |
| Cargo (RF5.4) | nothing: crate metadata in `crates/*/Cargo.toml` | `cargo publish --dry-run` | crates.io (`cuotai-core` before `cuotai-tui`) |
| AUR (RF5.5) | `PKGBUILD` + generated `.SRCINFO` (desktop `.deb`) | `bash -n`, source/checksum agreement | `ssh://aur@aur.archlinux.org/cuotai-bin.git` |

Workflow behaviour (RF5.7):

- `package-manifests` runs after `verify-public` on a tag and after
  `assemble` on a dispatch; it renders, validates and uploads the
  `package-manifests` artifact, and never pushes anything.
- Each channel job is gated by its repository variable
  (`PUBLISH_HOMEBREW=true`, …) **and** `needs.check.outputs.prerelease != 'true'`:
  a `-rc` tag never reaches a channel. They run with
  `continue-on-error`, so a channel failure is visible in the run summary
  but cannot fail the release; re-running a failed job on GitHub retries only
  that channel, and a `workflow_dispatch` with a tag rehearses all of them.
- On a **dry run** every channel job validates only: audit, install and push
  steps are skipped, because the URLs of a rehearsal tag do not exist and a
  rehearsal must never write to a tap, bucket, the AUR or crates.io.
- On a **tag run** the publish steps run with the environment secrets
  (`HOMEBREW_TAP_TOKEN`, `WINGET_SUBMIT_TOKEN`, `SCOOP_BUCKET_TOKEN`,
  `AUR_SSH_PRIVATE_KEY`, `CARGO_REGISTRY_TOKEN`) and stop with an explicit
  error when the secret is missing, instead of half-publishing. Optional
  gates: `WINGET_SUBMIT=true` (the first winget submission stays manual) and
  the repository variables `HOMEBREW_TAP_REPO`, `SCOOP_BUCKET_REPO`
  (default `jmarcon/homebrew-cuotai`, `jmarcon/scoop-cuotai`).
- The AUR push pins the host key against the fingerprints published on
  `aur.archlinux.org` and `wingetcreate.exe` is verified against a pinned
  SHA-256; nothing is fetched unpinned.

### Uninstall keeps your data (RF8.2)

Uninstalling through any channel removes only the application and its
binaries:

| Channel | What uninstall removes | What it always keeps |
|---|---|---|
| `brew uninstall --cask cuotai` | `/Applications/Cuotai.app` | config, databases, Keychain entries |
| `brew zap --cask cuotai` (opt-in) | + webview caches and the preference plist listed in the cask | config, databases, Keychain entries |
| `winget uninstall Cuotai.Cuotai` | the installed program files | `%APPDATA%\cuotai`, `%LOCALAPPDATA%\cuotai`, Credential Manager entries |
| `scoop uninstall cuotai` | the binary | `%APPDATA%\cuotai`, `%LOCALAPPDATA%\cuotai`, Credential Manager entries |
| `pacman -R cuotai-bin` | packaged files (`/usr/bin/Cuotai`, …) | `~/.config/cuotai`, `~/.local/share/cuotai`, Secret Service entries |
| `cargo uninstall cuotai-tui` | `~/.cargo/bin/cuotai` | everything else |

Purging the rest is always a manual, explicit step:

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

The desktop currently recognises the exact Homebrew Caskroom and Scoop
`apps\cuotai` paths, winget's package-cache path, and Linux system package
roots (`/usr/bin`, `/usr/sbin`, `/usr/lib`). PRD 0018 turns the in-app updater
off for those detected installs and shows the manager's command when one is
unambiguous. A normal winget NSIS install under
`%LocalAppData%\Programs\Cuotai` is indistinguishable by path from a manual
NSIS install, and Cargo-installed copies are not yet detected; those packages
must use `CUOTAI_UPDATER=off` until packaging defines a durable ownership
marker. The repository does not currently define that marker, so normal
winget ownership detection remains an explicit PRD 0021 RF8.1 blocker rather
than delivered behaviour.

## Updater (PRD 0018)

The in-app updater is signed by the Tauri updater key, distinct from the
SHA256SUMS signature and from Authenticode/Developer ID. Key custody, the
environment protection the owner must configure, the leak procedure (new pair,
a transition release signed with the **old** key that embeds the new public
key, user communication, then revoking the old secret), the loss procedure
(why users must reinstall manually) and the exact contents of an update-check
request are documented in
[`docs/updater-security.md`](updater-security.md). That document is the one to
read before the first signed release.

## Verifying a download (what users do)

```sh
shasum -a 256 -c SHA256SUMS
gh attestation verify Cuotai_0.68.0_universal.dmg --repo jmarcon/cuotai
cosign verify-blob \
  --signature SHA256SUMS.sig --certificate SHA256SUMS.pem \
  --certificate-identity-regexp '^https://github.com/jmarcon/cuotai/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
```

## Owner prerequisites (one-time, RF9.1)

The pipeline never creates production secrets or repositories. GitHub may
create the explicitly secretless `rehearsal` environment on first use. Before
the first real release:

1. Create two environments (Settings → Environments):
   - **`release`**, with a **required reviewer** and deployment branches
     restricted to tags. Only tag-push jobs route here and can read signing
     or publishing secrets.
   - **`rehearsal`**, with no secrets, reviewers or deployment-branch rule.
     `workflow_dispatch` desktop builds and opt-in channel validation jobs
     route here, so a branch rehearsal is never blocked by `release`'s
     tag-only rule. GitHub creates it on the first dispatch if it does not
     exist yet; creating it explicitly makes the intended lack of secrets
     and protection visible before that run.
   See [`docs/updater-security.md`](updater-security.md) for the updater key's
   own requirements.
2. Add to the `release` environment, as desired:
   - `APPLE_CERTIFICATE` — base64-encoded PKCS#12 (`.p12`);
     `APPLE_CERTIFICATE_PASSWORD` — that file's password; and
     `APPLE_SIGNING_IDENTITY` — the exact `Developer ID Application: …`
     identity. These are used only with one complete notarization set: either
     `APPLE_API_KEY` (base64-encoded App Store Connect `.p8`) plus
     `APPLE_API_KEY_ID` and `APPLE_API_ISSUER`, or `APPLE_ID` plus its
     app-specific `APPLE_PASSWORD` and `APPLE_TEAM_ID`. An incomplete set
     selects the verified ad-hoc path.
   - `WINDOWS_CERTIFICATE` (base64-encoded PKCS#12 `.pfx`) and
     `WINDOWS_CERTIFICATE_PASSWORD` — Authenticode;
   - `TAURI_SIGNING_PRIVATE_KEY` — the private-key text emitted by
     `cargo tauri signer generate` — and its
     `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The public key is already
     committed in `tauri.conf.json`; custody, leak and loss procedures are in
     [`docs/updater-security.md`](updater-security.md), including the exact
     `gh secret set --env release` commands and the environment protection
     (required reviewer, tag-only deployment branches) the owner must set.
   - per channel, one token or key each, never shared (RF9.2):
     `HOMEBREW_TAP_TOKEN` (contents: write on the tap repository),
     `SCOOP_BUCKET_TOKEN` (contents: write on the bucket repository),
     `WINGET_SUBMIT_TOKEN` (a GitHub token allowed to open pull requests in
     `microsoft/winget-pkgs`), `AUR_SSH_PRIVATE_KEY` (the AUR account's SSH
     private key) and `CARGO_REGISTRY_TOKEN` (a crates.io token with
     `publish-new-crate` for `cuotai-core` and `cuotai-tui`).
3. For each channel the owner wants, create its repository first
   (`jmarcon/homebrew-cuotai`, `jmarcon/scoop-cuotai`, an AUR account for
   `cuotai-bin`) and set the repository variable that enables it:
   `PUBLISH_HOMEBREW`, `PUBLISH_WINGET`, `PUBLISH_SCOOP`, `PUBLISH_CARGO`,
   `PUBLISH_AUR` = `true`. Optional: `HOMEBREW_TAP_REPO` /
   `SCOOP_BUCKET_REPO` when the repositories have other names, and
   `WINGET_SUBMIT=true` once the first `microsoft/winget-pkgs` submission has
   been reviewed by hand. Unset variables keep a channel off, and the run
   summary says so.
4. Decide on the **retroactive tags** `v0.1.0`… (RF1.3) so the changelog
   footer links compare tags instead of commits.

Missing signing credentials degrade honestly during a dispatch rehearsal.
For a tag release, platform code-signing credentials may still be absent and
are reported as unsigned, but the Tauri updater key is a release prerequisite:
no updater signatures, or only part of the required platform set, fails
assembly instead of publishing a release that strands installed clients. A
missing *channel* token fails only that channel job (RF5.7), with the message
naming the secret to add or the variable to unset.

## Rehearsing in a fork

1. Fork, enable Actions, and create the secretless/unrestricted `rehearsal`
   environment.
2. **Actions → Release → Run workflow** for a full unsigned dry run. To push an
   annotated `v<version>-rc.1` tag and exercise the signed path, put an
   **ephemeral** updater key pair in the fork's tag-protected `release`
   environment; never copy the production key.
3. Check `signing-state.json` in the `release-assembly` artifact: a dispatch
   reports the updater unavailable and the checksum signature absent; a tag
   run reports the updater available and Sigstore-signed checksums.
4. Check the `package-manifests` artifact: cask, formula, the three winget
   manifests, the Scoop bucket manifest and `PKGBUILD`/`.SRCINFO`, all
   rendered from the run's own manifest. Enable a channel variable in the
   fork to exercise its validation steps; nothing is pushed from a dispatch.

### Release-script harness

`scripts/release/collect-linux.sh` selects exactly one final regular
non-symlink package by expected extension in each Tauri bundle directory
(`*.AppImage`, `*.deb`, `*.rpm`), ignoring Tauri's linuxdeploy `Cuotai.AppDir`
staging tree and unrelated entries, and copies the updater signature the
bundler wrote beside each package (PRD 0018 RF1.1). A runnable shell test,
`scripts/release/test-collect-linux.sh`, asserts the collection green path,
the three ambiguous/unsafe red paths, and the consumer-visible assembly
contract: Linux checksum trust plus updater proof and Windows Authenticode
plus updater proof each merge into one artifact record through the real
`xtask signing-state`; an unsigned rehearsal merges with
`updater.available=false`. Run it locally before pushing a desktop-related
change:

```sh
bash scripts/release/test-collect-linux.sh
```

### Building the updater manifest locally

The same command the `assemble` job runs, against a `dist/` you built. Without
the private key there are no `.sig` files and the command refuses, exactly as
the pipeline does — rehearse with an **ephemeral** pair, never the production
key:

```sh
# 1. ephemeral key pair in a scratch directory
cargo tauri signer generate --ci -p smoke-only -w /tmp/cuotai-smoke

# 2. sign one artifact per platform you want in the manifest
TAURI_SIGNING_PRIVATE_KEY="$(cat /tmp/cuotai-smoke)" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=smoke-only \
  cargo tauri signer sign --app-version 0.68.0 dist/Cuotai.app.tar.gz

# 3. merge the platforms into one latest.json
cargo xtask release-check --changelog-out notes.txt
cargo xtask updater-manifest \
  --version 0.68.0 --repo jmarcon/cuotai --tag v0.68.0 \
  --notes-file notes.txt --pub-date 2026-10-21T12:00:00Z \
  --platform darwin-aarch64-app=dist/Cuotai.app.tar.gz \
  --platform darwin-x86_64-app=dist/Cuotai.app.tar.gz \
  --platform windows-x86_64-nsis=dist/Cuotai_0.68.0_x64-setup.exe \
  --platform linux-x86_64-appimage=dist/Cuotai_0.68.0_amd64.AppImage \
  --require darwin-aarch64-app --require windows-x86_64-nsis \
  --out dist/latest.json
```

A missing platform is simply absent from `platforms`; a collected artifact
without a `.sig` and a missing `--require` key both fail the command rather
than producing a manifest that cannot be installed.

## Manual desktop checklist per release

The pipeline verifies checksums and provenance automatically. Before
announcing a release, a person still: opens the DMG/app on macOS (Gatekeeper
verdict when notarized), runs the NSIS installer and the MSI on Windows
(silent `/S` for NSIS), installs the `.deb` on the oldest supported Ubuntu
and runs the AppImage, and runs `cuotai --once` from each CLI archive.

The same review re-checks the platform support matrix in
[`docs/platforms.md`](platforms.md) (PRD 0019 RF5.1): every cell still
matches the code, and items marked as verified only in CI stay marked so.

## Revocation / yanking (RF9.4)

To pull a bad release: mark the GitHub Release as pre-release (or delete its
assets) — never reuse the tag; if a signing key leaked, rotate it (the
Sigstore keyless identity needs no rotation; the Tauri updater key does: new
pair, new `TAURI_SIGNING_PRIVATE_KEY`, a transition release signed with the
old key that embeds the new public key, then revoke the old secret) and state
it in the release notes. The full leak and loss procedures, including what
users must do when the key is lost, are in
[`docs/updater-security.md`](updater-security.md).

Each channel rolls back on its own, in its own repository: commit the
previous version's cask/formula back in the tap, restore the previous
`bucket/cuotai.json`, and `git revert` the last commit in the AUR's
`cuotai-bin`. crates.io cannot delete a version: `cargo yank --vers <old>`
plus a new release. For winget, ask the moderators to remove the manifest.
