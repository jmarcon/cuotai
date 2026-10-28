# PRD 0021 — Release pipeline and package-manager publishing

| Field | Value |
|---|---|
| ID | 0021 |
| Status | Awaiting owner verification (RF1–RF4 implemented in 0.68.0 and hardened in 0.75.1–0.75.3; RF5.1–RF5.7, RF7.2, RF7.3 and RF8.2 delivered in 0.70.0; RF8.1's shared core/desktop/CLI source contract delivered in 0.77.0 after its 0.74.0/0.75.1 desktop work, with normal-winget ownership marked by an installer receipt in 0.81.0; RF9.2 and RF9.5 delivered in 0.75.2 and hardened in 0.75.3; RF9.3 and RF9.4 delivered without a product version bump; RF6 delivered in 0.80.0; RF8.2's uninstall-keeps-data assertions on Windows in 0.81.0; RF8.3 satisfied by RF2 (update artifacts are always produced); RF7 delivered in 0.81.0; every channel stays opt-in and off until the owner enables it) |
| Owner | Repository, CI and release (`.github/`, `packaging/`, `apps/desktop`, `crates/cuotai-tui`) |
| Consumers | end users, PRD 0018 (updater), PRD 0014 (widgets) |
| Depends on | PRD 0019 RF2 (green CI on three systems) and RF7.1 to RF7.3 (`CHANGELOG.md`) |
| Benefits from | PRD 0018 (update artifacts), PRD 0019 RF7.5 (release notes from the changelog) |
| Extended by | PRD 0028 (RF8.2 and RF8.3: the CLI update notice reuses the core detector delivered in 0.77.0; review item DIST-12), PRD 0052 (one-command purge beside RF8.2), PRD 0053 (Dependabot, scheduled advisories, secret scanning, licence payloads, independent release approval), PRD 0054 (installed-channel autostart and no-tray smokes) |
| Date | 2026-10-21; reconciled 2026-10-23 with the 2026-10-21 requirements review |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-24)

| RF | Version | Status and evidence |
|---|---|---|
| RF1 | 0.68.0, 0.75.1, 0.75.2 and 0.75.3 | Implemented — deterministic tag/version gate in `xtask/src/{check,git,main}.rs` and `.github/workflows/release.yml`; 0.75.2 keeps routine `workflow_dispatch` runs as secretless policy/version preflights and requires an explicit `full_rehearsal` opt-in before building and uploading the complete artifact set without publishing. In 0.75.3 neither dispatch path evaluates production updater credentials. Procedure in `docs/releasing.md`. |
| RF2 | 0.68.0 and 0.75.1 | Implemented — cross-platform artifact collection, optional ARM legs, checksums, provenance and aggregate CycloneDX SBOM in `.github/workflows/release.yml`, `scripts/release/` and `xtask/src/{manifest,sbom}.rs`; 0.75.1 makes each Windows/Linux updater payload one artifact with both its platform/checksum proof and detached updater proof. |
| RF3 | 0.68.0 and 0.75.1 | Implemented — honest signing state and checksum-signing fallback in `xtask/src/signing.rs` and `scripts/release/{macos,windows}-*`; merge-level fixtures reject duplicate artifact records while accepting the dual-proof shape, and updater secrets reach signing steps only. Owner production certificates and keys remain prerequisites. |
| RF4 | 0.68.0 and 0.75.1 | Implemented — idempotent draft assembly, publish gate and public checksum verification in `scripts/release/{publish-release,verify-release}.sh`, `.github/workflows/release.yml` and `xtask/src/notes.rs`; an unsigned rehearsal now carries an explicit unavailable-updater note. |
| RF5 | 0.70.0 | Implemented — one `cargo xtask package-manifests` renderer turns the verified `manifest.json` + `signing-state.json` into every channel manifest (RF5.6): the Homebrew cask and formula (RF5.1), the winget version/installer/locale manifests (RF5.2), the Scoop bucket manifest (RF5.3) and the AUR `PKGBUILD`/`.SRCINFO` (RF5.5), validated per channel before being written and by the channel's own linter in CI. RF5.4 is partial: crates.io metadata, the versioned path dependency, `[package.metadata.binstall]` and `publish = false` on the desktop are in place and the `channel-cargo` job publishes core before tui, but the crates.io token and the first publication are owner actions. RF5.7: five independent opt-in channel jobs (`PUBLISH_*` variables, pre-releases skipped, `continue-on-error`, tag-run-only publishing) can never fail the GitHub Release. The tap/bucket/AUR repositories, the first winget submission and every token remain owner prerequisites. |
| RF6 | 0.80.0 | Implemented — `.github/workflows/release.yml` runs four secret-free smoke jobs between `assemble` and `publish`, which needs all of them; each first proves with `scripts/release/smoke-checksum.sh` that its input is the exact file `SHA256SUMS` lists (from the new `release-checksums` hand-off). RF6.1: `smoke-cli` runs `scripts/release/smoke-cli.sh` on the native runner of each of the seven CLI targets (`--version`, `--list-providers`, `--once` and `--json` on `crates/cuotai-tui/tests/fixtures/smoke.toml`, every account classified `needs_configuration`); Linux runs in digest-pinned Ubuntu 22.04/Alpine containers with `--network none`, macOS under `sandbox-exec` with networking denied, Windows with a closed proxy sink plus the classification proof; a produced optional archive must pass. RF6.2: `smoke-desktop-macos` (`codesign --verify` of DMG and app, signature kind equal to `signing-state.json`, `lipo` arm64+x86_64, bundle version, headless start), `smoke-desktop-windows` (Authenticode state of both installers and the installed exe equal to `signing-state.json`, silent NSIS install/start/uninstall, silent MSI install/uninstall) and `smoke-desktop-linux` (`.deb` installed in a clean Ubuntu 22.04 container with metadata, `ldd` and an Xvfb start; the extracted AppImage with `ldd` and an Xvfb start on that base; rpm metadata and payload digests in Fedora with `--network none`). `cargo xtask workflow-check` rejects a `publish` that does not wait for every smoke or could run after a failed one, a conditional or failure-tolerant smoke, a `smoke-cli` matrix that differs from `cli`, and a tag-only smoke image. RF6.3: `xtask/tests/release_formats.rs` runs the assembly's own subcommands on a deterministic artifact set and compares `SHA256SUMS`, `signing-state.json` (signed tag and rehearsal) and `latest.json` byte for byte with golden copies in `xtask/tests/fixtures/release-formats/`, checks them against independent consumer schemas and round-trips them. Proven by the full rehearsal on the delivering branch ([run 40099546759](https://github.com/jmarcon/cuotai/actions/runs/40099546759)); the acceptance case "a binary missing a library fails" was reproduced locally: the optional ARM64 AppImage (built on Ubuntu 24.04) fails the `ldd` check on the Ubuntu 22.04 base with `GLIBC_2.38 not found`. The optional ARM64 desktop packages, the rpm install and the interactive UI stay in the manual checklist of `docs/releasing.md`. |
| RF7 | 0.70.0 and 0.81.0 | Delivered — RF7.2 and RF7.3 in 0.70.0; RF7.1 (procedure, dry run, fork rehearsal, secrets, rotation, rejected submissions, and since 0.81.0 the explanation of skipped channel jobs with a per-channel owner table) and RF7.4 (`CONTRIBUTING.md` links the procedure; `AGENTS.md` flow updated by PRD 0039) are in `docs/releasing.md` and `CONTRIBUTING.md`. |
| RF8 | 0.74.0, 0.75.1 and 0.77.0 | Delivered (live acceptance is an owner run) — RF8.1's implementation is delivered: `cuotai_core::install_source::detect_install_source` is the single typed classifier used by the desktop and by the offline `cuotai --install-source` report. It covers direct/AppImage, DMG/translocation/read-only, Homebrew Caskroom and Cellar with their distinct commands, the exact `cuotai` Scoop component, winget's package-cache path, generic Linux package roots and Cargo's configured/default bin. Core boundary tests keep the real Tauri current-user and `Programs\Cuotai` NSIS paths direct, and the desktop regression test preserves updater reasons, commands and installability. A normal winget NSIS install shares `%LocalAppData%\Cuotai` with a manual install, so ownership is read from a durable marker instead of a heuristic (0.81.0): the winget installer manifest passes `/WINGET=1`, an NSIS hook writes `install-source.txt` beside `Cuotai.exe` (a manual install and the uninstaller remove it) and the classifier reports `Winget` from it after the path rules. RF8.2 is implemented, and the Windows desktop smoke now plants sentinels in the user config and data folders and asserts the NSIS and MSI uninstallers keep them (the Scoop and winget jobs already did; 0.81.0). RF8.3 is satisfied by RF2: the update artifacts are always produced, and the CLI notice is PRD 0028 RF8. Live package-manager acceptance (install, upgrade, uninstall through each channel) remains an owner run. |
| RF9 | 0.70.0, 0.75.2 and 0.75.3; no product version bump for RF9.3 or RF9.4 | Partial — RF9.1 is the owner checklist with the per-channel secrets, enabling variables and repositories in `docs/releasing.md`. RF9.2 and RF9.5 are implemented in 0.75.2: workflow-level read-only permissions, write scopes only on owning jobs, every secret confined to its owning step and unavailable in the `rehearsal` environment, fork-safe CI/release triggers, a bounded/optional build matrix gated to tag pushes or an explicit full rehearsal, Cargo caches and per-job timeouts. `cargo xtask workflow-check`, run by CI and the release preflight, enforces permissions, timeouts, action pins, secret owners, triggers, gates and the exact matrix with failure fixtures; 0.75.3 additionally confines prerequisite credential inspection to pushed tags and rejects dynamic whole-secret-context expressions. RF9.3 is the monthly/manual `.github/workflows/external-rules.yml`, the reviewed `.github/external-rules-baseline.json` policy hashes and the fixture-tested `cargo xtask external-rules-check`; it checks every workflow action through GitHub's Releases and Git Data APIs when a semantic release comment exists, treats a missing/malformed comment or unresolved tag as `could_not_check`, and opens or refreshes one deduplicated issue after paginating all open issues. Policy sources must return identical bytes across two fetches; winget hashes the official source Markdown rather than request-dependent Microsoft Learn HTML. RF9.4 is the executable rollback/yank and credential-rotation runbook. The `release` environment exists with a required reviewer and a deployment branch policy (checked on 2026-10-25) and `v0.80.0` was published through it; the tap and bucket repositories, tokens and the first winget submission remain owner actions. |

### Reconciliation with the 2026-10-21 review (2026-10-23)

The review proposed splitting this plan into two phases (DIST-01 to DIST-12). The split is **not adopted**: by 0.70.0 the "phase 1" items DIST-01 to DIST-06 and DIST-08 were delivered (and DIST-03/04 exceeded: ARM64, musl, MSI, SBOM and cosign are already produced), so the plan keeps its single RF list. What stays open here:

| Open item | Requirement | Who |
|---|---|---|
| Smoke of the produced CLI artifacts (`--version`, `--list-providers`, `--once` with the example configuration, no network) on every system, blocking the release (review item DIST-09) | RF6.1 (and RF6.2, RF6.3) | Repository; wave 5a of `0000_control.md`. **Delivered in 0.80.0.** |
| Monthly workflow re-reading the Homebrew and winget policies and the action pins | RF9.3 | Repository |
| Tap and bucket repositories, the first manual winget submission, the crates.io token and first publication, the AUR key and account | RF5.1–RF5.5, RF9.1 | Owner actions |
| Hosted APT/RPM repository | RF5.5 (optional) | Owner decision (§8) |

The review's DIST-12 (the core detects the install origin and the updater hands over to the manager) was delivered with RF8.1 in 0.77.0; the opt-in CLI update notice that reuses it is **PRD 0028 RF8.2 and RF8.3**, delivered in 0.81.0. RF9.3 (the monthly external-rules workflow) was already delivered, as the status table says.

## 1. Context

Cuotai can only be installed from source. PRD 0019 mentions distribution in two places, but only as a sketch:

- **RF3** (build, signing, publication): one paragraph per topic, "high priority", never started. No release workflow, tag, GitHub Release or `latest.json` exists; the repository has only `.github/workflows/ci.yml`.
- **RF4** (installation channels): four one-line bullets, "medium priority": a Homebrew tap, `winget`, optional `scoop`, an AUR package and `cargo install`. They give no manifest layout, no automation, no tests, no handling of the signing state and no per-channel acceptance criteria.

Package managers also impose rules that change the plan. Facts checked on 2026-10-21 against the official documents:

| Channel | Rule that matters | Source |
|---|---|---|
| Homebrew (official cask repository) | A cask for an app Gatekeeper can assess must pass Homebrew's Gatekeeper checks and must not need Gatekeeper to be bypassed. Homebrew 5.0 announced that casks failing the signing and notarization audit are removed by September 2026. A new GitHub-hosted package needs notability: at least 75 stars, 30 forks or 30 watchers (225, 90 or 90 for a self-submission), and a repository older than 30 days. | [Acceptable Casks](https://docs.brew.sh/Acceptable-Casks), [Package Acceptance Policy](https://docs.brew.sh/Package-Acceptance-Policy) |
| Homebrew (own tap) | Software that does not meet the official criteria can live in a third-party tap, with no Homebrew endorsement. An unsigned app still hits Gatekeeper on the user's machine. | same |
| winget | The manifest goes to `microsoft/winget-pkgs` by pull request. The pipeline checks the schema, downloads the installer, scans it with several antivirus engines, installs and uninstalls it silently, and requires an HTTPS `InstallerUrl` that comes directly from the publisher (redirectors are rejected) with a matching `InstallerSha256`. A moderator reviews it, and Microsoft may refuse a submission. `wingetcreate update --submit` automates a new version. | [Submit your manifest](https://learn.microsoft.com/en-us/windows/package-manager/package/repository) |
| Scoop | A bucket is a Git repository of JSON manifests. `checkver` (for example `"github"`) and `autoupdate` let a bucket follow GitHub Releases, with a SHA-256 per file. | [App Manifest Autoupdate](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifest-Autoupdate) |

The signing state is therefore not a detail. Without an Apple Developer ID the official Homebrew cask is out of reach, and an own-tap cask only works with the user's manual Gatekeeper override. The facts above are quoted from the documents on that date and must be re-checked before each channel is opened (RF9.3).

Market context (2026-10-19 analysis): every open competitor with traction ships a DMG plus a Homebrew cask (CodexBar, ClaudeBar, Quotio, OpenUsage), several add AUR, deb or AppImage (CodexBar, OpenQuota), and the better ones attach `SHA256SUMS` (QuotaTempo) or state openly when signing is off (OpenQuota).

## 2. Problem

- A user without a Rust toolchain cannot install Cuotai at all.
- Even with binaries, people on macOS, Windows and Linux expect `brew install`, `winget install` or their distribution's tool, and expect upgrades through the same tool.
- Publishing by hand is slow and error-prone: version numbers, checksums and release notes drift between the changelog, `Cargo.toml`, `tauri.conf.json` and each package manifest.
- A badly done release is a supply-chain risk: unsigned or unverified artifacts, secrets in logs, a package manager pointing at a tampered file.

## 3. Goals

- **One tag, every channel.** A `v*` tag builds, tests, packages, signs (where possible), publishes a GitHub Release and updates every enabled package manager without manual copying.
- **Honest about trust.** Each artifact and each channel states its signing state; nothing claims more than it has.
- **Reproducible and testable in a fork.** The whole pipeline runs against a fork with dry-run secrets, and a release candidate tag can be rehearsed without touching the real channels.
- **Free tier.** The pipeline fits the free GitHub Actions allowance for public repositories.
- Make the channel list explicit and ordered by value and cost, so the owner can enable one at a time.

## 4. Non-goals

- App stores (Mac App Store, Microsoft Store, Snap Store) in this plan. Flathub and Chocolatey are listed in RF5 as later channels, not in the first delivery.
- The in-app updater itself (PRD 0018). This plan only produces the artifacts and `latest.json` it consumes, and tells it when an install is managed by a package manager (RF8).
- A paid code-signing service chosen by the pipeline. Certificates are the owner's decision (RF3.3); the pipeline works with or without them.
- Installation telemetry or download counters beyond what GitHub already shows.
- Managing the repository's GitHub settings (secrets, environments, branch protection) from code. The pipeline documents what the owner must configure (RF9.1).

## 5. Functional requirements

### RF1 — Versioning and release trigger (high priority; prerequisite)

- **RF1.1** (Amended by PRD 0039 RF4/RF5: the tagged commit is the `chore(release): X.Y.Z` release commit that `cargo xtask release prepare` produces, tagged with `cargo xtask release tag`.) The release is triggered by pushing an annotated tag `vX.Y.Z` that points at a commit on `main`. A tag on any other commit, or whose version differs from the workspace `Cargo.toml`, fails the first job with a clear message. Pre-release tags `vX.Y.Z-rc.N` are published as GitHub pre-releases and never reach package managers (RF5 skips them).
- **RF1.2** A single `xtask`-style check, written in Rust and run through Cargo, verifies in CI and in the release job that `Cargo.toml` (`[workspace.package]`), `apps/desktop/src-tauri/Cargo.toml`, `tauri.conf.json`, both lockfiles and the top versioned section of `CHANGELOG.md` agree with the tag. This is the check PRD 0019 RF3.1 and RF7.4 ask for; it lives here and PRD 0019 points to it.
- **RF1.3** (Answered by PRD 0039 D10: declined; internal versions keep commit-comparison links.) The first release creates the retroactive tags the changelog footer links expect. Tagging history is outward-facing, so it is a listed owner decision (§8), not an automatic step. If the owner declines, the footer keeps comparing commits until the first real tag.
- **RF1.4** `workflow_dispatch` has two honest secretless modes. Its default is a cheap preflight that checks the requested version/tag and workflow policy without starting heavy jobs. Selecting the explicit `full_rehearsal` input runs the build/package/SBOM/assembly path, uploads its artifacts only to the workflow run, and neither creates a GitHub Release nor contacts any package manager.
- **Acceptance criterion:** a tag with a mismatched version, a missing changelog section or a non-`main` commit fails before any build; a routine dispatch proves the version/policy gate without requesting a secret; an explicit full rehearsal produces the artifacts without publishing anything.

### RF2 — Build matrix and artifacts (high priority)

Replaces PRD 0019 RF3.2 with a concrete list. Names are stable so package manifests can reference them.

| Artifact | Built on | Name |
|---|---|---|
| Desktop, macOS universal | `macos-latest` | `Cuotai_<version>_universal.dmg` and the updater archive `Cuotai.app.tar.gz` with its `.sig` |
| Desktop, Windows x64 | `windows-latest` | `Cuotai_<version>_x64-setup.exe` (NSIS) and `Cuotai_<version>_x64_en-US.msi` |
| Desktop, Windows ARM64 | `windows-11-arm` where available | `Cuotai_<version>_arm64-setup.exe` |
| Desktop, Linux x64 | `ubuntu-22.04` (oldest supported glibc) | `Cuotai_<version>_amd64.AppImage`, `.deb` and `.rpm` |
| Desktop, Linux ARM64 | `ubuntu-24.04-arm` where available | `Cuotai_<version>_aarch64.AppImage`, `.deb` and `.rpm` |
| CLI/TUI, macOS | `macos-latest`, both targets | `cuotai-<version>-aarch64-apple-darwin.tar.gz`, `…-x86_64-apple-darwin.tar.gz` |
| CLI/TUI, Linux | Linux runners, glibc and musl | `cuotai-<version>-x86_64-unknown-linux-gnu.tar.gz`, `…-aarch64-…`, and musl variants where the dependency set builds (`rusqlite` is bundled; `keyring`'s Secret Service store needs D-Bus at run time, which the musl notes must state) |
| CLI/TUI, Windows | `windows-latest` | `cuotai-<version>-x86_64-pc-windows-msvc.zip` (and `aarch64`) |
| Integrity | one job after all builds | `SHA256SUMS`, `SHA256SUMS.sigstore.json` (RF3.2, a Sigstore bundle; first planned as `SHA256SUMS.sig`), `sbom.cdx.json` (CycloneDX, generated by a Cargo tool) |
| Updater | the Tauri build | `latest.json` per PRD 0018 RF1.3, with the changelog section as notes |

- **RF2.1** The desktop is built with the official `tauri-action` or with `cargo tauri build` directly; the choice is recorded in `docs/releasing.md` with the reason. Whichever is used is pinned by commit hash, like every action in `ci.yml`.
- **RF2.2** The CLI build uses plain `cargo build --release --locked -p cuotai-tui` per target. A tool such as `cargo-dist`, installed through Cargo, is allowed only if it keeps the hash-pinned actions, the read-only default token and the secretless preflight; otherwise the workflow stays hand-written. Dependencies are fetched with Cargo only.
- **RF2.3** Every job uses `--locked`, the toolchain pinned in CI (PRD 0019 RF6.2), a timeout and `permissions: contents: read` by default. Only the publish job gets `contents: write`; only the signing/attestation job gets `id-token: write` and `attestations: write`; attestation verification gets read only.
- **RF2.4** Each release artifact gets a GitHub build-provenance attestation, so a user can verify it with `gh attestation verify`.
- **RF2.5** The matrix tolerates an unavailable runner: a missing optional architecture (Windows or Linux ARM64) does not block the release, and the release notes list the artifacts that were not produced.
- **Acceptance criterion:** a release-candidate tag in a fork produces every artifact in the table (minus declared optional ones), each opens or runs on a clean machine, and `shasum -a 256 -c SHA256SUMS` passes.

### RF3 — Signing, notarization and checksums (high priority)

Refines PRD 0019 RF3.3.

- **RF3.1** Pipeline behaviour per platform:
  - macOS: with the Apple secrets present, sign with Developer ID, notarize and staple the DMG and the app; without them, build an ad-hoc-signed DMG and mark it **unsigned** in the release notes.
  - Windows: with an Authenticode certificate or a cloud-signing service configured, sign the installers and executables; without, ship them **unsigned**, stated in the release notes.
  - Linux: the AppImage, `.deb` and `.rpm` are signed through the checksum signature (RF3.2) and, for repositories (RF5.5), through the repository key.
- **RF3.2** `SHA256SUMS` is signed with a project key whose public half is committed to the repository and printed in `docs/releasing.md`. Prefer keyless signing (Sigstore, through GitHub's OIDC) if it is supported when this is implemented; otherwise a minisign or GPG key held in a CI secret. The Tauri updater key (PRD 0018 RF1.2) is separate and is never reused for anything else.
- **RF3.3** All certificates and keys are CI secrets in a protected GitHub environment (`release`) with a required reviewer. They never appear in the repository, logs or artifacts, and the workflow masks and never echoes them. The release notes carry a table of artifact → signing state.
- **RF3.4** A signing state is a first-class output: the pipeline writes `signing-state.json` (artifact, signed or not, how), which the release-notes step, the Homebrew step (RF4) and the winget step (RF5.2) read. A channel that needs a signed artifact stops with a message when the state says unsigned, instead of publishing something that will be rejected.
- **Acceptance criterion:** with no signing secrets, the pipeline still completes and every release note says "unsigned" for the right artifacts; with a test certificate (or an ad-hoc identity), the macOS app passes `codesign --verify --deep --strict`; `spctl` and notarization are checked only with the real identity.

### RF4 — GitHub Releases (high priority)

Refines PRD 0019 RF3.4 and RF3.5.

- **RF4.1** The publish job creates a draft GitHub Release for the tag, uploads every artifact, then publishes it only after all required jobs succeeded. A failed run leaves no half-published release visible to users.
- **RF4.2** The release body is the version's section of `CHANGELOG.md` (PRD 0019 RF7.5), followed by the signing-state table and the verification commands (`shasum -c`, `gh attestation verify`). A missing or empty section fails the release.
- **RF4.3** The pipeline is idempotent: re-running it for the same tag replaces assets of a draft release and refuses to overwrite assets of a published one. A published release is never deleted by the pipeline.
- **RF4.4** After publishing, a verification job downloads the public assets and re-checks sizes and checksums against `SHA256SUMS`, so a truncated upload is caught before any package manager sees the URLs.
- **Acceptance criterion:** an interrupted run leaves only a draft; the published body equals the changelog section plus the table; the verification job fails on a corrupted asset.

### RF5 — Package-manager channels (medium priority; each channel is opt-in)

Replaces PRD 0019 RF4. A channel is enabled by a repository variable (`PUBLISH_HOMEBREW=true`, and so on) so the owner turns channels on one by one. Pre-release tags never publish to a channel. Every channel job runs **after** RF4.4, reads asset URLs and checksums from the verified release (never from a rebuilt file), and is isolated so its failure cannot undo the release.

Channels are ordered by value and cost:

| Order | Channel | Why | Prerequisite |
|---|---|---|---|
| 1 | **Homebrew tap** (`jmarcon/homebrew-cuotai`): a cask for the desktop app and a formula for the CLI | Biggest macOS audience; the competitors all have it; a tap needs no review | The signed DMG (RF3) for a good experience; an unsigned cask works only with a manual Gatekeeper override |
| 2 | **winget** (`microsoft/winget-pkgs`) | Built into Windows 11 | A stable HTTPS release URL and a silent installer; an unsigned installer may be flagged by the antivirus scan |
| 3 | **Scoop** (own bucket `jmarcon/scoop-cuotai`) | Cheap, popular with developers, follows GitHub Releases | The Windows zip |
| 4 | **`cargo install`** (and `cargo binstall` metadata) | Zero extra infrastructure for Rust users | See RF5.4 |
| 5 | **AUR** (`cuotai-bin`) | Arch users; a `-bin` package needs no build | A maintained AUR account |
| 6 | **Linux repositories**: `.deb` and `.rpm` published in GitHub Releases, optionally an APT or RPM repository | Debian and Fedora families | RF3.2 repository key |
| 7 | Later, outside the first delivery: **Chocolatey**, **Flathub**, **Nix**, **MacPorts** | Wider reach | Their own review and sandbox rules (Flathub needs a manifest and a sandbox compatible with the credential vault) |

- **RF5.1 Homebrew.** `packaging/homebrew/` holds the templates; a job renders `Casks/cuotai.rb` and `Formula/cuotai.rb` with the version, URLs and SHA-256 values from the verified release, then opens a pull request (or pushes, if the owner allows it) to the tap repository with a fine-scoped token. The cask declares `app "Cuotai.app"`, `zap` paths for preferences and cache only, and a `caveats` note when the app is unsigned (the Gatekeeper override). The formula installs the `cuotai` binary and a shell-completion file if the CLI gains one. `brew audit --strict --online` and `brew install --cask` run in CI on a macOS runner for the tap. The official `homebrew-cask` repository is a later, separate decision (§8), only after signing and notability (§1).
- **RF5.2 winget.** `packaging/winget/` holds the manifest templates (version, installer, default locale) with `PackageIdentifier` `Cuotai.Cuotai` (owner to confirm). A job runs `wingetcreate update` against the verified release URLs with `--submit` for a version bump after the first manual submission. The first submission is manual so the owner reviews the package and the moderator feedback. `winget validate` runs in CI; the installer must support a silent switch and a clean uninstall that keeps user data unless the user asks (RF8.2). Installer type: the NSIS `-setup.exe` (silent `/S`) or the MSI, whichever passes the Windows Sandbox script first. The submission is skipped when `signing-state.json` says the installer is unsigned **and** the previous submission was rejected for it.
- **RF5.3 Scoop.** A bucket repository with `bucket/cuotai.json` (desktop installer or CLI zip, `checkver: github`, `autoupdate`, `hash` from `SHA256SUMS`). The bucket's own workflow, or the pipeline's job, bumps it on a release. `scoop install` is tested on a `windows-latest` runner. Submission to the official `extras` bucket is a later owner decision.
- **RF5.4 Cargo.** `cargo install cuotai-tui --locked` requires `cuotai-core` on crates.io, because a published crate cannot depend on a path-only crate. So this channel needs both crates published, with complete metadata (`description`, `repository`, `readme`, `keywords`, `categories`, `rust-version`), the crate name checked as free first (the `cargo search cuotai` check on 2026-10-21 found no crate), and `publish` guards on the desktop crate (`publish = false`, since it is a Tauri app). Until then the README documents `cargo install --git https://github.com/jmarcon/cuotai cuotai-tui --locked`. The release job publishes `cuotai-core` and then `cuotai-tui` with a crates.io token held in the `release` environment, after a `cargo publish --dry-run`. `[package.metadata.binstall]` points `cargo binstall` at the GitHub Release tarballs (RF2).
- **RF5.5 Linux.** `.deb`, `.rpm` and AppImage are release assets from the start (RF2). The AUR `cuotai-bin` package keeps a `PKGBUILD` template in `packaging/aur/`; a job updates `pkgver` and checksums and pushes to the AUR over SSH with a key in the `release` environment. A hosted APT or RPM repository (for example on GitHub Pages, signed with the repository key) is optional and ships only if the owner wants `apt install`; installing a downloaded `.deb` by hand is the documented fallback.
- **RF5.6** Every channel's manifest or template lives in `packaging/<channel>/` in this repository, is rendered from one data source (version, URLs, checksums, signing state) and has a test that renders it and validates the output with the channel's own linter. No manifest is edited by hand after the first submission.
- **RF5.7** A channel failure never fails the GitHub Release. Each channel job is independent, reports its own status in the run summary and can be re-run alone after a stable tag; the dispatch preflight intentionally does not launch channel runners.
- **Acceptance criterion:** for each enabled channel, a clean machine installs the previous version through the channel, upgrades to the new one through the same channel, runs `cuotai --version` or opens the app, then uninstalls; uninstalling keeps `cuotai.toml`, the credential vault entries and the preferences unless the user opts in to remove them.

### RF6 — Smoke tests on the produced artifacts (medium priority)

Extends PRD 0019 RF5.2 from "the TUI in CI" to "the artifacts that users download".

- **RF6.1** After the build, a job per system downloads the packaged CLI artifact and runs `--version`, `--list-providers` and `--once` with a sample configuration and no network. A failure blocks the release.
- **RF6.2** The desktop artifact is checked as far as automation allows: the macOS app launches headless long enough to prove it starts (`codesign --verify`, `lipo -info` for the universal binary), the Windows installer installs silently and uninstalls on a runner, the `.deb` installs in a clean container (`dpkg -i`, `ldd` shows no missing libraries) and the AppImage is extracted and checked with `ldd`. A manual desktop checklist remains for each release, in `docs/releasing.md`.
- **RF6.3** `signing-state.json`, `SHA256SUMS` and `latest.json` get a schema or round-trip test in the pipeline's own test suite, so a changed format breaks CI instead of a user.
- **Acceptance criterion:** a broken artifact (for example a binary missing a library) fails the smoke job and stops the release.

### RF7 — Documentation and repository layout (medium priority)

- **RF7.1** `docs/releasing.md` (English): the full release procedure, the dry run, how to rehearse in a fork, the list of secrets and environments, how to rotate each key, and what to do when a channel rejects a submission.
- **RF7.2** README reorganized as PRD 0019 RF4.4 asks: installation per platform first (Homebrew, winget, Scoop, Linux, Cargo, direct download), building from source second, plus a short "Verify your download" section with the checksum and attestation commands. Each channel is documented only after it is enabled.
- **RF7.3** `packaging/README.md` explains what each template is and who maintains it.
- **RF7.4** `CONTRIBUTING.md` links to the release procedure; the `AGENTS.md` versioning flow stays valid. *Amended by PRD 0039: the version no longer moves in the delivering commit; `cargo xtask release prepare` writes it in the release commit and `cargo xtask release tag` creates the tag after the release PR merges.*
- **Acceptance criterion:** someone who has never released Cuotai can follow `docs/releasing.md` and produce a rehearsal release in a fork.

### RF8 — Coexistence with the in-app updater (medium priority)

- **RF8.1** A copy installed by a package manager must not fight it. The desktop and the CLI detect the installation source (path under Homebrew's Caskroom or Cellar, a winget or Scoop path, an AUR or distribution package, a Cargo install) in the core, and report it through a single function. PRD 0018 RF3.2 and RF2.5 use it to turn the updater off and show "Update with `brew upgrade`" (or the right command) instead of installing in place.
- **RF8.2** Uninstall behaviour is documented and tested per channel: the application and its binaries go; configuration, the local database, preferences and vault entries stay unless the user explicitly opts in (cask `zap`, a documented purge command). No uninstaller deletes user data silently.
- **RF8.3** The update artifacts for PRD 0018 are produced in RF2 even when a package manager is the main channel, because users who download from GitHub still update in place.
- **Acceptance criterion:** a Homebrew-installed desktop shows the package-manager hint and never offers in-place install; a direct download keeps the in-app updater.

### RF9 — Operations, security and maintenance (medium priority)

- **RF9.1** A checklist of what the owner must configure before the first release, kept in `docs/releasing.md`: the `release` environment with a required reviewer; the secrets (Apple identity and notarization credentials, the Windows certificate or signing service, the Tauri updater private key, the checksum-signing key, the crates.io token, the tap and bucket tokens, the AUR key); the repository variables that enable channels; and the tap and bucket repositories. The pipeline never creates secrets or changes repository settings.
- **RF9.2** Least privilege: one token per target repository (tap, bucket, winget fork), scoped to that repository and to pull requests or contents only, stored in the `release` environment and available only to the job that needs it. No secret is exposed to pull requests from forks, and the release workflow runs only on tags and `workflow_dispatch`.
- **RF9.3** A scheduled workflow (monthly) re-checks the external rules this plan depends on: it fetches the Homebrew and winget policy pages and the Scoop and Tauri action pins, and opens an issue when something changed or a pinned action has a newer release. Policy text changes (§1) are therefore noticed instead of discovered at release time.
- **RF9.4** Revocation and yanking: a documented procedure to pull a bad release (mark the GitHub Release as a pre-release or delete the assets, bump the tap, bucket and AUR packages back, `cargo yank`, ask winget to remove the manifest) and to rotate a leaked key.
- **RF9.5** The pipeline must stay within the free GitHub Actions allowance: the matrix is limited to the table in RF2, caching is on for Cargo, and heavy jobs run only for tag pushes or the explicit RF1.4 `full_rehearsal` opt-in; the routine dispatch path stays cheap.
- **Acceptance criterion:** the checklist exists and an empty `release` environment makes a tag pipeline stop with a message listing the missing secrets before any heavy build; the secretless full rehearsal remains available without entering that environment.

## 6. Non-functional requirements

- **Supply chain:** actions pinned by commit hash, `--locked` everywhere, secrets only in the protected environment, attestations and a signed `SHA256SUMS` on every release, an SBOM attached.
- **Honesty:** the release notes, the README and each channel's description state the signing state; a channel never advertises "verified" for an unsigned artifact.
- **Idempotence and rollback:** every step can be re-run; no step deletes published artifacts; each channel can be rolled back independently.
- **Portability:** both the cheap preflight and the explicit complete artifact rehearsal run in a fork without production secrets.
- **Dependencies:** build and publish tooling is installed through Cargo or comes as a pinned GitHub Action; no Node or Python package manager enters the project. Package-manager manifests (Ruby cask, YAML, JSON, `PKGBUILD`) are data files, not dependencies.
- **Cost:** within the free tier for public repositories; the owner's possible costs are the Apple Developer Program, a Windows signing certificate or service, and nothing else.

## 7. Order of execution and verification

1. **RF1** version check, tag trigger and secretless dispatch modes: cheap by default, complete artifact rehearsal only by explicit opt-in (nothing published; safest to land first).
2. **RF2 and RF4** build matrix and draft GitHub Release for an `rc` tag in a fork. This is the first user-visible delivery: downloadable binaries with checksums.
3. **RF3** signing and the signing-state output, first without certificates (honest "unsigned"), then with them as the owner provides them.
4. **RF6** smoke tests on the artifacts.
5. **RF5** channels, one at a time, in the order of the table: Homebrew tap, winget (first submission by hand), Scoop, Cargo, AUR, Linux repositories.
6. **RF8** coexistence with the updater, together with PRD 0018.
7. **RF7 and RF9** documentation and operations, written alongside each step and finished last.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, `actionlint` on every workflow, and a rehearsal in a fork before any real tag.

The requirement is only delivered when a user with no Rust toolchain installs the previous version through at least Homebrew (macOS), winget or Scoop (Windows) and a Linux package, upgrades through the same tool after a new tag, verifies the checksum and attestation, and the release notes state exactly which artifacts are signed.

## 8. Owner decisions and prerequisites

| Decision / prerequisite | Needed for | Default if undecided |
|---|---|---|
| Apple Developer ID and notarization credentials | Signed macOS DMG; the official Homebrew cask; widgets (PRD 0014) | Ship an ad-hoc-signed DMG marked unsigned; Homebrew only through the own tap |
| Windows signing (certificate or cloud signing service) | A clean SmartScreen and antivirus experience; fewer winget rejections | Ship unsigned installers, stated in the release notes |
| Create the `jmarcon/homebrew-cuotai` and `jmarcon/scoop-cuotai` repositories | Homebrew and Scoop channels | Channels stay disabled |
| Create the `release` GitHub environment and secrets (RF9.1) | Any real or `rc` artifact release | Both secretless dispatch modes remain available; no tag release |
| Create retroactive tags for 0.1.0 to the current version (RF1.3) | Changelog footer links | Keep comparing commits until the first real tag |
| winget `PackageIdentifier` (`Cuotai.Cuotai`) | winget | `Cuotai.Cuotai` |
| Durable normal-winget ownership marker: either publish a dedicated winget installer or add an NSIS hook/switch that writes and removes a Cuotai-owned source value | Complete RF8.1 without classifying manual NSIS installs as winget | Keep normal winget detection off and tell users to set `CUOTAI_UPDATER=off`; never infer ownership from the shared install path or standard Tauri uninstall entry |
| Publish `cuotai-core` and `cuotai-tui` on crates.io under these names (RF5.4) | `cargo install` | Document `cargo install --git` |
| Submit to the official `homebrew-cask` and Scoop `extras` | Wider reach, after notability and signing | Own tap and bucket only |
| Sign `SHA256SUMS` with Sigstore keyless or a project key (RF3.2) | Integrity verification | Sigstore keyless if supported, else minisign |
| Enable APT/RPM repository hosting (RF5.5) | `apt install` | Release `.deb`/`.rpm` assets only |

## 9. Relationship to other plans

- **PRD 0019** keeps the license, governance, CI, platform-support and changelog requirements. Its **RF3** (build, signing, publication) and **RF4** (installation channels) are superseded by RF1 to RF5 here; its RF3.1 and RF7.4/RF7.5 checks are implemented by RF1.2 and RF4.2. RF5 (platform support) and RF6 (release policies) of PRD 0019 stay there; this plan only adds RF6 (artifact smoke tests).
- **PRD 0018** consumes the update artifacts and `latest.json` from RF2, the changelog notes from RF4.2 and the installation-source function from RF8.1.
- **PRD 0014** needs the signed, notarized macOS app from RF3.
