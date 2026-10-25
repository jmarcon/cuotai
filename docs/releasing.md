# Releasing Cuotai

This document describes the release flow (PRD 0039) and the release pipeline
(PRD 0021 RF1–RF5 and RF9): how a release is prepared from the commits, how
a tag becomes a verified GitHub Release, what is signed and what is not, how
to rehearse in a fork, the opt-in package-manager channels (RF5), and what the
owner must configure. A channel is contacted **only** on a stable tag when its
repository variable (`PUBLISH_HOMEBREW`, `PUBLISH_WINGET`, `PUBLISH_SCOOP`,
`PUBLISH_CARGO`, `PUBLISH_AUR`) is set to `true`, always after the published
release has been re-verified; until the owner enables one, the tag pipeline
renders and validates it locally and pushes nothing.

## How a release works

Feature and fix commits never change a version or `CHANGELOG.md`; each
user-visible commit carries its entry as a `Changelog:` trailer (see
[`AGENTS.md`](../AGENTS.md), "How to deliver a change"). A release is made
on demand by the owner (PRD 0039 D8):

1. **See what is pending.** `cargo xtask changelog-draft` (or the job
   summary of the latest `main` CI run) prints the next version and its
   would-be section.
2. **Prepare.** On an up-to-date `main` in a clean worktree:

   ```sh
   git switch main && git pull --ff-only
   cargo xtask release prepare --dry-run        # version, rationale, every diff
   cargo xtask release prepare --branch         # writes and commits on release/vX.Y.Z
   ```

   `prepare` computes the version from the commits since the release base
   (the newest stable `vX.Y.Z` tag, or while none exists the last commit that
   changed the workspace version), writes the five version locations, the
   `cuotai-core` requirement of `cuotai-tui`, both lockfiles (workspace
   entries only, verified with `cargo metadata --locked --offline`), the
   `CHANGELOG.md` section with the real short hashes and footer links, every
   `vNEXT` of `docs/**/*.md` and `README.md`, and the "Delivered by version"
   rows of `0000_control.md`; then it runs `release-check` and
   `changelog-check` and commits `chore(release): X.Y.Z`. It never pushes,
   tags or opens a pull request. `--version X.Y.Z` overrides the computed
   version upward (recorded as `Version-Override:`).
3. **Release PR.** `git push -u origin release/vX.Y.Z`, then
   `gh pr create --base main --head release/vX.Y.Z --title "chore(release): X.Y.Z" --body "$(git log -1 --format=%b)"`.
   CI runs `release-preflight` with `release prepare --check`. Fix wording
   in the section on the branch (amend the release commit); if more commits
   land on `main` first, rebase the branch and run
   `cargo xtask release prepare --refresh`, which keeps every edited entry
   and adds the new commits. For the first three releases (D7), also run
   **Actions → Release → Run workflow** on `release/vX.Y.Z` with
   **full_rehearsal**.
4. **Merge** with **Rebase and merge**.
5. **Tag** (the owner, or an agent under the rules below, after the release PR merged):

   ```sh
   git switch main && git pull --ff-only
   cargo xtask release tag --rc                 # vX.Y.Z-rc.N, a GitHub pre-release
   git push origin vX.Y.Z-rc.N
   # once the rc is good:
   cargo xtask release tag                      # vX.Y.Z
   git push origin vX.Y.Z
   ```

   `release tag` finds the newest release commit, checks that it is on
   `origin/main`, re-runs `release prepare --check`, and creates the
   **annotated** tag whose message is the section. It refuses a version that
   already has a stable tag. Fixes after an rc land normally; then
   `cargo xtask release prepare --refresh --branch` on a fresh branch
   re-renders the **same** version's section as
   `chore(release): X.Y.Z (refresh)`, followed by `release tag --rc` (rc.2) or `release tag`.
6. On that pushed tag, `.github/workflows/release.yml` runs: **check → builds
   → assemble → artifact smoke → publish → verify-public → package-manifests
   → channel jobs**.
   Its `check` job also runs `release prepare --check` on the tagged commit.

The first release after the cutover is a release candidate of the next
computed version (`v0.80.0-rc.1` when a feature landed, D9). Versions 0.1.0
to 0.79.0 are internal versions and get no retroactive tags (D10).

```text
check ──┬─ desktop-macos ───┐
        ├─ desktop-windows ─┤
        ├─ desktop-windows-arm64 (optional) ─┤
        ├─ desktop-linux ───┤
        ├─ desktop-linux-arm64 (optional) ───┤
        ├─ cli (7 legs, 2 optional) ─────────┤
        └─ sbom ────────────┴─► assemble ─┬─ smoke-cli (7 legs) ──────┐
                                          ├─ smoke-desktop-macos ────┤
                                          ├─ smoke-desktop-windows ──┤
                                          └─ smoke-desktop-linux ────┴─► publish ─► verify-public
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

## Artifact smoke tests (RF6)

Between `assemble` and `publish`, four secret-free jobs test the artifacts
users download, on a runner of each system. They have the read-only token,
no environment, no condition and no tolerated failure, and `publish` needs
all of them, so one failure means nothing is published (`workflow-check`
rejects any change that weakens this). `assemble` uploads a small
`release-checksums` artifact (`SHA256SUMS`, `manifest.json`,
`signing-state.json`); every smoke first runs
`scripts/release/smoke-checksum.sh`, which proves its input is the exact file
`SHA256SUMS` lists, so a stray build output can never pass for a release
asset.

| Job | Runner | What must hold |
|---|---|---|
| `smoke-cli` (RF6.1) | the native runner of each of the seven CLI targets (`x86_64-apple-darwin` under Rosetta 2) | `scripts/release/smoke-cli.sh`: the archive holds exactly the binary and both licenses; `--version` reports the release version; `--list-providers` exits 0 and lists the sample's providers; `--once` and `--json` on `crates/cuotai-tui/tests/fixtures/smoke.toml` exit 2, and the JSON classifies every account as `needs_configuration`, which proves no provider was queried. |
| `smoke-desktop-macos` (RF6.2) | `macos-latest` | `scripts/release/smoke-desktop-macos.sh`: `codesign --verify --strict` on the DMG and `--deep` on the app; the signature kind matches `signing-state.json` (Developer ID or ad-hoc, never more, never less); `lipo` shows `arm64` and `x86_64`; `CFBundleShortVersionString` is the release version; the app is still running 15 s after a start with a throw-away `HOME`. |
| `smoke-desktop-windows` (RF6.2) | `windows-latest` | `scripts/release/smoke-desktop-windows.ps1`: the Authenticode status of both installers and of the installed `Cuotai.exe` matches `signing-state.json` (`Valid` for `authenticode`, `NotSigned` for `none`); the NSIS installer installs silently (`/S`) with the release version, the app is still running after 15 s, and the silent uninstaller removes it; the MSI installs and uninstalls with `msiexec /qn`. |
| `smoke-desktop-linux` (RF6.2) | `ubuntu-latest` | `scripts/release/smoke-desktop-linux.sh`: in a clean Ubuntu 22.04 container (the oldest supported glibc) the `.deb` installs with apt, its `Version`/`Architecture` match, `ldd` resolves every library and the app keeps running under Xvfb; the extracted AppImage passes the same `ldd` and start checks on that base; in a Fedora container with `--network none` the rpm's name, version, architecture, payload digests and `/usr/bin` entry are checked. |

**No network for the CLI.** On Linux the binary runs in a pinned minimal
container with `--network none` (Ubuntu 22.04 for the glibc targets, Alpine
for musl, which also proves the musl binary is static); on macOS it runs
under `sandbox-exec` with every network operation denied. Windows has no
unprivileged per-process block on the runner, so there, as everywhere, the
proxy variables point at a closed local port and the `needs_configuration`
classification is the proof.

**Optional legs.** An optional CLI archive (ARM64) that was produced is
published and therefore must pass its smoke; one that was not produced is not
listed in `SHA256SUMS` and its row reports "not produced". The optional ARM64
desktop packages are not smoke-tested and stay in the manual checklist.

**Container images** are the `SMOKE_*_IMAGE` variables at the top of
`release.yml`, pinned by multi-arch index digest; `workflow-check` rejects a
tag-only image. To move one, resolve the new digest with
`docker buildx imagetools inspect <image>:<tag>` and update the variable.

**Rehearsal.** A `full_rehearsal` dispatch runs the same smoke jobs on the
unsigned artifacts, so they are proven without a tag:
`gh workflow run release.yml --ref <branch> -f full_rehearsal=true`.

**Locally**, each script runs on its own system against a downloaded
artifact (for example the `release-assembly` artifact of a rehearsal run):

```sh
scripts/release/smoke-checksum.sh dist/SHA256SUMS dist/cuotai-X.Y.Z-aarch64-apple-darwin.tar.gz
scripts/release/smoke-cli.sh dist/cuotai-X.Y.Z-aarch64-apple-darwin.tar.gz X.Y.Z \
  crates/cuotai-tui/tests/fixtures/smoke.toml
SMOKE_IMAGE=ubuntu:22.04 scripts/release/smoke-cli.sh \
  dist/cuotai-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz X.Y.Z crates/cuotai-tui/tests/fixtures/smoke.toml
```

**Format contracts (RF6.3).** `xtask/tests/release_formats.rs` runs the
assembly's own `xtask` subcommands on a deterministic artifact set and
compares `SHA256SUMS`, `signing-state.json` (signed tag and rehearsal) and
`latest.json` byte for byte with the golden copies in
`xtask/tests/fixtures/release-formats/`, then checks each against an
independent schema of what its consumers read (`shasum -c`, the smoke
scripts, the release notes and package renderers, `tauri-plugin-updater`).
A format change therefore fails `cargo test --workspace`. After an intended
change, run
`CUOTAI_REGENERATE_RELEASE_FORMATS=1 cargo test -p xtask --test release_formats`,
review the diff and update every consumer in the same commit.

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

## Secretless rehearsal modes (RF1.4, RF9.5)

Choose **Actions → Release → Run workflow**, optionally provide a tag name
(default: `v<workspace version>`), and select one of two modes:

- Leave **full_rehearsal** off for the routine cheap preflight.
  `release-check` validates the five version locations, both lockfiles, the
  changelog section and requested tag syntax; `workflow-check` audits
  permissions, timeouts, action pins, step-owned secrets, the bounded matrix
  and execution gates. Only `check` runs.
- Select **full_rehearsal** to exercise RF1.4 deliberately. The prerequisite
  gate and bounded desktop/CLI/SBOM jobs run in the secretless `rehearsal`
  environment, assembly signs and verifies `SHA256SUMS` with the rehearsal's
  branch identity (the bundle is discarded), records updater and checksum
  signatures as unavailable, and the complete unsigned artifact set plus
  rendered package manifests is uploaded to the workflow run.

Neither dispatch mode creates a GitHub Release, publishes an attestation,
contacts a package-manager repository or receives a production secret.
`full_rehearsal` is intentionally opt-in because it consumes the same hosted
runner matrix as a release; leaving it off keeps the routine path within the
free-tier budget.

## Workflow security and cost guardrails (RF9.2, RF9.5)

- All workflows declare `permissions: contents: read` at workflow level.
  Only `publish` receives `contents: write`; only `assemble`, which performs
  Sigstore signing and provenance attestation, receives `id-token: write` and
  `attestations: write`. The publish and public-verification jobs receive only
  `attestations: read`. No job grants `packages: write`: crates.io uses its
  own registry token in the single publish step.
- CI uses `pull_request`, never `pull_request_target`, and references no
  secret. Release triggers are exactly `v*` tag pushes and
  `workflow_dispatch`; dispatch is cheap by default and heavy work requires
  the explicit `full_rehearsal` input.
- Signing credentials and per-target publishing tokens appear only in the
  `env` block of the step that consumes them. `HOMEBREW_TAP_TOKEN`,
  `SCOOP_BUCKET_TOKEN` and `WINGET_SUBMIT_TOKEN` are separate credentials,
  stored only in the owner-configured `release` environment. The AUR SSH key
  and crates.io token are likewise isolated to their own publish steps.
- Desktop bundles, the seven-row CLI matrix and aggregate SBOM run only for a
  pushed tag or an explicit full rehearsal. Windows ARM64, Linux ARM64 and
  the two ARM64 CLI rows are optional; every other RF2 artifact is required.
- `Swatinem/rust-cache` is enabled for Cargo builds. Jobs that compile the
  desktop cache both `.` and `apps/desktop/src-tauri`; root-only Cargo jobs
  cache `.`. Every job has `timeout-minutes`.
- `cargo xtask workflow-check` enforces these invariants in CI and before a
  release build. Tests prove failures for broad permissions, missing timeouts,
  unpinned actions, dotted/bracketed/dynamic secret misuse, an unlisted job,
  a bypassable heavy-job condition and a missing prerequisite dependency.
  For RF6 it also fails when `publish` does not wait for every smoke job or
  could run after a failed one, when a smoke job or its script step is
  conditional or tolerates failure, when `smoke-cli` does not cover exactly
  the `cli` targets, and when a smoke container image is not pinned by digest.

## Monthly external-rule and action-pin review (RF9.3)

`.github/workflows/external-rules.yml` runs at 08:17 UTC on the first day of
every month and also supports `workflow_dispatch`. It invokes the same
implementation tested by the workspace:

```sh
cargo xtask external-rules-check \
  --json-out external-rules-report.json \
  --markdown-out external-rules-report.md
```

The command fetches every policy source twice, requires the two byte sequences
to match, and only then compares their SHA-256 with
`.github/external-rules-baseline.json`. The four authoritative documents are
Homebrew's Acceptable Casks and Package Acceptance Policy pages, Microsoft's
source Markdown for the winget manifest-repository submission page, and
Scoop's App Manifest Autoupdate conventions.

The rendered Microsoft Learn page is deliberately not the hashed form. Live
checks showed that its policy article can be identical while Akamai injects
request-dependent `bazadebezolkohpepadr` script data, a rotating `/akam/...`
asset path and a `noscript` tracking-pixel payload; those variants produced
different hashes for different user agents. The canonical winget form is the
raw UTF-8 Markdown bytes at the fixed official
`MicrosoftDocs/windows-dev-docs` `docs`-branch path. That moving branch still
detects upstream article edits, while excluding rendered site chrome. The
two-fetch equality gate reports a source that changes during one check as
`could_not_check`, never as policy drift or no change.

The baseline records each source URL, hash and fetch date. It is intentionally
read-only to automation: after reviewing a changed source, update the baseline
in a pull request; the scheduled workflow never accepts a new hash by itself.

Every external `uses:` line in every `.github/workflows/*.yml` or `.yaml` file
is discovered. Full commit SHAs remain mandatory. For a `# vX.Y.Z` comment,
the checker uses GitHub's official Releases API to find the latest published
release and the Git Data API to prove that the commented tag resolves to the
pinned SHA. A newer semantic version, a tag/SHA mismatch or a policy hash
change requires review. The release pipeline currently calls `cargo tauri`
directly, not `tauri-apps/tauri-action`; if a Tauri action is introduced later,
the directory scan includes it without a second allowlist.

The workflow gives its check job only read access. A separate job receives
only `issues: write`, and only when the report needs attention. It opens the
fixed-title issue **External rules or action pins need review**, or replaces
the body of the existing open issue with that title, so repeated runs cannot
create duplicates. A clean run creates and updates nothing.

Limits are explicit rather than optimistic:

- `dtolnay/rust-toolchain` is pinned by commit but its comment is the Rust
  toolchain version, not a `vX.Y.Z` action release, so it is the explicit
  `not_verifiable` exception; its SHA is still enforced by `workflow-check`.
- A missing or malformed `# vX.Y.Z` comment on any other GitHub action is
  `could_not_check` and requires attention instead of silently disabling its
  release comparison.
- A GitHub repository with no published Releases is `not_verifiable` only
  after the commented tag resolves to the pinned SHA. A missing repository or
  tag is `could_not_check`. An action from another registry remains
  `not_verifiable` because no GitHub API endpoint can be derived safely.
- A network, API, authentication, rate-limit or response-parsing failure is
  `could_not_check` and makes the report require attention. It is never
  converted to “unchanged” or “current”.

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
| Integrity | assembly job | `SHA256SUMS`, `SHA256SUMS.sigstore.json` (tag runs), `manifest.json`, `signing-state.json`, `sbom.cdx.json` |
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
  explicitly by CI/release, Cargo caching for the workspace it compiles, and
  a per-job timeout.
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
  (cosign v3, identity = this repository's GitHub Actions OIDC token),
  producing one Sigstore bundle, `SHA256SUMS.sigstore.json` (signature,
  certificate and transparency-log proof). The assembly verifies the bundle
  against the run's own workflow identity right after signing. A
  `full_rehearsal` dispatch runs the same sign-and-verify step with its branch
  identity (`release.yml@refs/heads/<branch>`), so the commands are exercised
  before a tag needs them; that rehearsal bundle stays outside the assembly,
  is never a release asset and `signing-state.json` still records the
  checksum signature as unavailable. Its only outside trace is a public
  Sigstore transparency-log entry for a branch identity, which the documented
  verification (`release.yml@refs/tags/v…`) never accepts. The cheap dispatch
  preflight produces no checksum file and makes no signing claim. There is no
  project key to rotate; the identity is the workflow.
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
  A tag run without this key, or with only a partial required signature set,
  fails assembly before publication. The dispatch preflight never requests
  the key or builds updater payloads.
- Every published release asset gets a GitHub **build-provenance attestation**
  (verifiable with `gh attestation verify`) on tag runs. The dispatch
  preflight produces no artifact to attest.

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
  the Sigstore bundle against this repository's tag workflow identity, and verifies every GitHub provenance attestation before flipping
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

- `package-manifests` runs after `verify-public` on a stable tag; it renders,
  validates and uploads the `package-manifests` artifact, and never pushes
  anything itself.
- Each channel job is gated by a pushed stable tag, its repository variable
  (`PUBLISH_HOMEBREW=true`, …), and a successful `package-manifests` job. They
  run with `continue-on-error`, so a channel failure is visible in the run
  summary but cannot fail the already-published GitHub Release; re-running
  failed jobs retries only those channels.
- The routine dispatch does not build release assets or run channel tools.
  An explicit full rehearsal renders and uploads `package-manifests`, while
  all external channel jobs remain tag-only; renderers and validators also
  remain covered by `cargo test -p xtask`.
- On a stable **tag run** the publish steps use only their owning environment
  secret (`HOMEBREW_TAP_TOKEN`, `WINGET_SUBMIT_TOKEN`,
  `SCOOP_BUCKET_TOKEN`, `AUR_SSH_PRIVATE_KEY`, or
  `CARGO_REGISTRY_TOKEN`) and stop with an explicit error when it is missing,
  instead of half-publishing. Optional gates: `WINGET_SUBMIT=true` (the first
  winget submission stays manual) and the repository variables
  `HOMEBREW_TAP_REPO`, `SCOOP_BUCKET_REPO` (default
  `jmarcon/homebrew-cuotai`, `jmarcon/scoop-cuotai`).
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
unambiguous. With Tauri's default current-user NSIS mode, a normal winget
install under `%LocalAppData%\Cuotai` is indistinguishable by path from a
manual NSIS install, and Cargo-installed copies are not yet detected. The winget
template invokes the same release installer with `/S`; `tauri.conf.json`
defines no custom NSIS hook, so both routes create the same standard Tauri
uninstall entry and shortcuts. There is no repository-defined receipt, file,
registry value or shortcut that proves winget ownership.

Normal winget users must set `CUOTAI_UPDATER=off` for now; Settings → Updates
then names that override as its disabled reason. Completing RF8.1 needs an
owner packaging decision:

1. ship a dedicated winget installer artifact, or add a winget-only installer
   switch/hook that writes and removes a Cuotai-owned source marker; this is
   reliable but adds an artifact or installer contract and requires a winget
   manifest update and moderator review; or
2. keep the shared installer unchanged and retain the documented environment
   override; this avoids false positives but requires user configuration.

Classifying the shared path or Tauri's generic uninstall entry as winget is
not an option: it would wrongly turn off in-app updates for manual NSIS
installs. Until a marker contract is chosen, normal winget ownership remains
an explicit PRD 0021 RF8.1 blocker rather than delivered behaviour.

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
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity-regexp '^https://github\.com/jmarcon/cuotai/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
```

## Owner prerequisites (one-time, RF9.1)

The pipeline never creates production secrets, environments or repositories.
Before the first real release:

1. Create the **`release`** environment (Settings → Environments), with a
   **required reviewer** and deployment branches restricted to tags. Tag-push
   jobs enter it; dispatch rehearsals instead enter the separate, secretless
   `rehearsal` environment and cannot read signing or publishing secrets. See
   [`docs/updater-security.md`](updater-security.md) for the updater key's own
   requirements.
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
     `HOMEBREW_TAP_TOKEN` (a fine-grained token scoped only to the tap
     repository, contents: write), `SCOOP_BUCKET_TOKEN` (a different
     fine-grained token scoped only to the bucket repository, contents:
     write), `WINGET_SUBMIT_TOKEN` (a token scoped only to the owner's
     `winget-pkgs` fork, with the contents/pull-request access required by
     `wingetcreate`), `AUR_SSH_PRIVATE_KEY` (only the AUR account's SSH key)
     and `CARGO_REGISTRY_TOKEN` (only the crates.io scopes needed to publish
     `cuotai-core` and `cuotai-tui`).
3. For each channel the owner wants, create its repository first
   (`jmarcon/homebrew-cuotai`, `jmarcon/scoop-cuotai`, an AUR account for
   `cuotai-bin`) and set the repository variable that enables it:
   `PUBLISH_HOMEBREW`, `PUBLISH_WINGET`, `PUBLISH_SCOOP`, `PUBLISH_CARGO`,
   `PUBLISH_AUR` = `true`. Optional: `HOMEBREW_TAP_REPO` /
   `SCOOP_BUCKET_REPO` when the repositories have other names, and
   `WINGET_SUBMIT=true` once the first `microsoft/winget-pkgs` submission has
   been reviewed by hand. Unset variables keep a channel off, and the run
   summary says so.
#### Why channel jobs are skipped (v0.80.0 findings)

The v0.80.0 run skipped `channel-winget`, `channel-aur` and `channel-cargo`
by design, not by failure. Each channel job has an `if:` that requires a tag
push, a stable tag **and** its enabling repository variable to equal the
string `true` (`vars.PUBLISH_WINGET`, `vars.PUBLISH_AUR`, `vars.PUBLISH_CARGO`).
Those variables were unset, so GitHub marks the job as skipped before it
enters the `release` environment, and no secret check runs. Only
`PUBLISH_HOMEBREW` was enabled, which is why the Homebrew job ran. (Its
online audit then failed on the unauthenticated GitHub API rate limit; the
audit step now passes the read-only job token as `HOMEBREW_GITHUB_API_TOKEN`.)

What the owner must create before each skipped channel can run. Variables go
in Settings → Secrets and variables → Actions → Variables (repository level);
secrets go in the `release` environment. No value is ever committed.

| Channel | Accounts and repositories to create | Secret (`release` environment) | Variables |
|---|---|---|---|
| winget | A fork of `microsoft/winget-pkgs` under the owner's account; make the **first** submission by hand (PRD 0021 RF5.2) | `WINGET_SUBMIT_TOKEN`: token scoped to that fork only | `PUBLISH_WINGET=true`; later `WINGET_SUBMIT=true` for unattended pull requests |
| AUR | An AUR account and the `cuotai-bin` package (first push by hand), with a dedicated SSH key registered on the account | `AUR_SSH_PRIVATE_KEY`: that key's private half | `PUBLISH_AUR=true` |
| crates.io | A crates.io account; confirm `cuotai-core` and `cuotai-tui` are free; first publication by hand is acceptable | `CARGO_REGISTRY_TOKEN`: scoped to publish these two crates | `PUBLISH_CARGO=true` |
| Scoop | Repository `jmarcon/scoop-cuotai` | `SCOOP_BUCKET_TOKEN` | `PUBLISH_SCOOP=true` |
| Homebrew | Repository `jmarcon/homebrew-cuotai` | `HOMEBREW_TAP_TOKEN` (contents: write on the tap only) | `PUBLISH_HOMEBREW=true` (set) |

A skipped channel job is therefore the expected state of an off channel. To
re-run one alone after fixing it, use "Re-run failed/skipped jobs" on the
tag's run; the variable is read at run time.

4. Retroactive tags `v0.1.0`… (RF1.3) were **declined** (PRD 0039 D10):
   the internal versions are summarized by one "Pre-release history
   (0.1.0–0.79.0, …)" section at the bottom of `CHANGELOG.md`, with no
   footer link of their own, and tags start with the first real release.
5. Apply the branch and tag rulesets and the merge settings of
   "Branch and tag protection" below.

The dispatch modes never request signing credentials. For a tag release, the
required updater key and password are checked before any heavy job starts;
the gate lists either missing secret by name. Optional platform code-signing
credentials may still be absent and are reported as unsigned. A missing
*channel* token fails only that channel job (RF5.7), with the message naming
the secret to add or the variable to unset.

### Agents releasing and tagging

The release flow is agentic: an agent may prepare a release, open the
release PR, and create and push the tag, under the "Release & tag rules for
agents" of [`AGENTS.md`](../AGENTS.md). In short:

- The owner must ask for the release in the current conversation and name
  the version or rc.
- Before tagging, the agent verifies and reports all of these: local `main`
  is fast-forwarded to `origin/main`; `HEAD` is the merged
  `chore(release): X.Y.Z` commit; `CI result` is green on it; and
  `release-check` and `release prepare --check` pass.
- The annotated tag comes only from `cargo xtask release tag [--rc]`, never
  from `git tag` by hand.
- The agent pushes only that one tag (never `--tags`), then watches
  `release.yml` and reports the result.
- No `v*` tag is ever moved, re-pointed, deleted or force-pushed. A failed
  release is followed by a new rc or a patch release, never a moved tag.
- The first three releases start with an rc unless the owner says otherwise
  (D7).

## Branch and tag protection (PRD 0039 RF7, owner)

Repository settings are not managed from code. The owner applies them once
with `gh` (an admin token), right after the cutover pull request merges:

```sh
# Merge strategy: rebase only (keeps every commit and its trailers),
# auto-merge allowed, merged branches deleted.
gh api -X PATCH repos/jmarcon/cuotai \
  -F allow_rebase_merge=true -F allow_squash_merge=false \
  -F allow_merge_commit=false -F allow_auto_merge=true \
  -F delete_branch_on_merge=true

# main: pull request required (0 approvals: the owner and the agents share
# one identity), the "CI result" check required and up to date, linear
# history, no force push, no deletion. Nobody pushes to main directly: the
# admin role may bypass the checks only through a pull request merge.
gh api -X POST repos/jmarcon/cuotai/rulesets --input - <<'JSON'
{
  "name": "main",
  "target": "branch",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
  "bypass_actors": [
    { "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "pull_request" }
  ],
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    { "type": "required_linear_history" },
    { "type": "pull_request", "parameters": {
        "required_approving_review_count": 0,
        "dismiss_stale_reviews_on_push": false,
        "require_code_owner_review": false,
        "require_last_push_approval": false,
        "required_review_thread_resolution": false,
        "allowed_merge_methods": ["rebase"] } },
    { "type": "required_status_checks", "parameters": {
        "strict_required_status_checks_policy": true,
        "do_not_enforce_on_create": false,
        "required_status_checks": [ { "context": "CI result", "integration_id": 15368 } ] } }
  ]
}
JSON

# v* tags, creation: only the admin role may create them (the owner, and
# agents acting with the owner's token under the AGENTS.md release rules).
gh api -X POST repos/jmarcon/cuotai/rulesets --input - <<'JSON'
{
  "name": "release tags: creation",
  "target": "tag",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["refs/tags/v*"], "exclude": [] } },
  "bypass_actors": [
    { "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" }
  ],
  "rules": [ { "type": "creation" } ]
}
JSON

# v* tags, immutability: nobody may move or delete one, admins included (no
# bypass actors). A bad release is recovered with a new version, never by
# re-pointing a tag.
gh api -X POST repos/jmarcon/cuotai/rulesets --input - <<'JSON'
{
  "name": "release tags: immutable",
  "target": "tag",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["refs/tags/v*"], "exclude": [] } },
  "bypass_actors": [],
  "rules": [ { "type": "update" }, { "type": "deletion" } ]
}
JSON

gh api repos/jmarcon/cuotai/rulesets          # verify all three are active
```

`actor_id: 5` is the repository admin role, and `integration_id: 15368` is
GitHub Actions, the app that reports `CI result`.

The agents push with the owner's identity, so they hold the same rights.
On `main` the bypass is `pull_request`: it applies only to merging a pull
request and never to a direct push, for the owner and for agents alike.
Agents can create `v*` tags. The release rules of `AGENTS.md` decide when
they may; the immutability ruleset makes sure no one, the owner included,
can move or delete a published tag. Code
scanning does not block merges yet (D13); enable code-scanning merge
protection for high and critical alerts once the flow has run for a few
weeks. Check the result once: a direct push to `main` without the bypass is
refused, and a pull request with a red `CI result` cannot merge.

## Rehearsing in a fork

1. Fork and enable Actions. Do not copy any production secret.
2. **Actions → Release → Run workflow** with **full_rehearsal** off. Confirm
   that only `check` ran and that its summary identifies the skipped work.
3. Run it again with **full_rehearsal** selected. The full bounded matrix,
   SBOM and assembly run without production secrets; publish, public
   verification and every external channel job stay skipped.
4. Download `release-assembly`. Confirm `manifest.json` and
   `signing-state.json` truthfully mark updater/checksum signatures
   unavailable, no `latest.json` is claimed, and the package-manager manifests
   are present in their separate artifact.
5. If production-like updater signing also needs rehearsal, use an
   **ephemeral** updater key in the fork's tag-protected `release` environment
   and push an annotated `v<version>-rc.1` tag. This creates a GitHub
   pre-release in the fork, never a stable release or package-manager update.

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

The pipeline verifies checksums and provenance automatically. Before announcing
a release, a person completes every item:

- [ ] Open the DMG/app on macOS and record the Gatekeeper verdict when it is
  notarized.
- [ ] Run the NSIS installer and the MSI interactively on Windows (the silent
  install, start and uninstall are automated by `smoke-desktop-windows`).
- [ ] Open the desktop window and the tray on a real Linux desktop: the
  automated `smoke-desktop-linux` proves the `.deb`/AppImage install, link
  and start headless, not the user interface. Install the `.rpm` on Fedora
  (its metadata only is automated) and, when produced, the optional ARM64
  installer and packages.
- [ ] Confirm the `smoke-*` jobs of the tag run passed (they block `publish`;
  every CLI archive's `--version`, `--list-providers` and `--once` is
  automated there).
- [ ] Re-check [`docs/platforms.md`](platforms.md) (PRD 0019 RF5.1): every cell
  still matches the code, and items verified only in CI remain marked that way.
- [ ] Complete the [rollback drill](#rollback-drill) in a fork and test channel
  repositories. Do not discover that a token, repository path or moderation
  workflow has changed during a production incident.

## Pull back a bad release (RF9.4)

This is an incident procedure, not a way to rewrite release history. Never
move or reuse the tag, force-push a channel repository, overwrite a published
crate, or claim that a moderated winget change is live before its pull request
has merged and propagated. Record every command and URL in the incident log.
First identify the bad and last known-good versions:

```sh
export BAD_VERSION=X.Y.Z
export PREVIOUS_VERSION=A.B.C
export BAD_TAG="v$BAD_VERSION"
export REPO=jmarcon/cuotai
```

Rolling a manifest back protects new installations and upgrades. It does not
downgrade a copy that already installed the bad version; tell those users to
uninstall it and install `v$PREVIOUS_VERSION` manually.

### 1. Quarantine the GitHub Release first

The fastest reversible choice is to make the release a pre-release. GitHub's
`/releases/latest` endpoint excludes pre-releases, so the updater endpoint
stops resolving to this release:

```sh
gh release edit "$BAD_TAG" --prerelease --repo "$REPO"
gh release view "$BAD_TAG" --repo "$REPO" \
  --json tagName,isPrerelease,isDraft,url \
  --jq '{tag: .tagName, prerelease: .isPrerelease, draft: .isDraft, url: .url}'
gh release view --repo "$REPO" --json tagName,url \
  --jq '{latest_stable: .tagName, url: .url}'
```

If the bytes themselves must no longer be served, delete every asset while
leaving the release and tag as incident evidence:

```sh
gh release view "$BAD_TAG" --repo "$REPO" --json assets \
  --jq '.assets[].name' |
while IFS= read -r asset; do
  gh release delete-asset "$BAD_TAG" "$asset" --yes --repo "$REPO"
done
gh release view "$BAD_TAG" --repo "$REPO" --json assets \
  --jq '.assets | length'
```

The final command must print `0`. Asset deletion cannot recall bytes that were
already downloaded, and restoring the assets later requires uploading them
again. Do not delete the release or tag: the release record, signing state and
attestations are evidence.

**Verify before continuing:**

- [ ] `isPrerelease` is `true`, or the asset count is `0`.
- [ ] `gh release view --repo "$REPO"` names the previous stable tag, not
  `$BAD_TAG`.
- [ ] Opening
  `https://github.com/jmarcon/cuotai/releases/latest/download/latest.json`
  no longer returns the bad manifest.
- [ ] The incident notice names the affected version and links to the known-good
  release.

### 2. Restore the project-controlled package repositories

The channel jobs commit generated files with the message `cuotai <version>`.
Revert that channel commit; do not hand-edit the rendered manifest and do not
change the templates in this repository during an incident.

#### Homebrew tap — `jmarcon/homebrew-cuotai`

The controlled paths are `Casks/cuotai.rb` and `Formula/cuotai.rb`:

```sh
gh repo clone jmarcon/homebrew-cuotai homebrew-cuotai-rollback
cd homebrew-cuotai-rollback
bad_commit="$(git log -1 --format=%H --fixed-strings \
  --grep="cuotai $BAD_VERSION" -- Casks/cuotai.rb Formula/cuotai.rb)"
test -n "$bad_commit"
git revert --no-edit "$bad_commit"
grep -F "version \"$PREVIOUS_VERSION\"" Casks/cuotai.rb
grep -F "/v$PREVIOUS_VERSION/" Formula/cuotai.rb
git push origin HEAD
```

**Verify:** the two `grep` commands succeed; the pushed commit contains both
paths. Then run:

```sh
brew update
brew info --cask jmarcon/cuotai/cuotai
brew info jmarcon/cuotai/cuotai
brew audit --strict --online --cask jmarcon/cuotai/cuotai
brew audit --strict --online --formula jmarcon/cuotai/cuotai
```

Both `brew info` commands must report the previous version and both audits must
pass before declaring the tap recovered.

#### Scoop bucket — `jmarcon/scoop-cuotai`

The controlled path is `bucket/cuotai.json`:

```sh
gh repo clone jmarcon/scoop-cuotai scoop-cuotai-rollback
cd scoop-cuotai-rollback
bad_commit="$(git log -1 --format=%H --fixed-strings \
  --grep="cuotai $BAD_VERSION" -- bucket/cuotai.json)"
test -n "$bad_commit"
git revert --no-edit "$bad_commit"
jq -e --arg version "$PREVIOUS_VERSION" '.version == $version' \
  bucket/cuotai.json
git push origin HEAD
gh api -H 'Accept: application/vnd.github.raw+json' \
  repos/jmarcon/scoop-cuotai/contents/bucket/cuotai.json |
  jq -e --arg version "$PREVIOUS_VERSION" '.version == $version'
```

**Verify:** both `jq` checks return `true`; after `scoop update`, run
`scoop info cuotai` and confirm it reports the previous version, then install
and run it on a clean test machine.

#### AUR — `cuotai-bin`

The controlled paths are `PKGBUILD` and `.SRCINFO` in the package repository:

```sh
git clone ssh://aur@aur.archlinux.org/cuotai-bin.git aur-cuotai-rollback
cd aur-cuotai-rollback
bad_commit="$(git log -1 --format=%H --fixed-strings \
  --grep="cuotai $BAD_VERSION" -- PKGBUILD .SRCINFO)"
test -n "$bad_commit"
git revert --no-edit "$bad_commit"
grep -Fx "pkgver=$PREVIOUS_VERSION" PKGBUILD
grep -F "pkgver = $PREVIOUS_VERSION" .SRCINFO
git push origin HEAD
curl -fsS 'https://aur.archlinux.org/rpc/v5/info?arg[]=cuotai-bin' |
  jq -e --arg version "$PREVIOUS_VERSION-1" \
    '.results[0].Version == $version'
```

**Verify:** `PKGBUILD` and `.SRCINFO` agree, the AUR RPC check returns `true`,
and `makepkg --verifysource` succeeds in a clean Arch environment. Pacman does
not automatically downgrade an installed higher version; affected users need
an explicit `pacman -U` of the known-good package.

### 3. Yank the crates.io versions

Both published packages carry the same release version. Authenticate with a
crates.io token allowed to yank these crates, then run:

```sh
cargo yank --version "$BAD_VERSION" cuotai-tui
cargo yank --version "$BAD_VERSION" cuotai-core
curl -fsS "https://crates.io/api/v1/crates/cuotai-tui/$BAD_VERSION" |
  jq -e '.version.yanked == true'
curl -fsS "https://crates.io/api/v1/crates/cuotai-core/$BAD_VERSION" |
  jq -e '.version.yanked == true'
```

**Irreversibility warning:** publishing the crate name/version and its archive
is irreversible: crates.io does not delete it and that version can never be
reused. A yank itself can be undone with `cargo yank --undo`, but it only stops
new dependency resolution. Existing lockfiles and direct archive downloads
continue to work. Publish a fixed compatible version as soon as possible.

### 4. Ask winget to remove the bad manifest

`microsoft/winget-pkgs` is a third-party, moderated repository. Cuotai cannot
push a rollback or remove a catalog version directly. Submit a normal pull
request that deletes only
`manifests/c/Cuotai/Cuotai/<BAD_VERSION>`; the previous version directory
remains. On Windows PowerShell:

```powershell
$BadVersion = "X.Y.Z"
$PreviousVersion = "A.B.C"
gh repo fork microsoft/winget-pkgs --clone -- --filter=blob:none --no-checkout
Set-Location winget-pkgs
git sparse-checkout init --cone
git sparse-checkout set manifests/c/Cuotai/Cuotai
git checkout master
git pull --ff-only upstream master
git switch -c "remove-cuotai-$BadVersion" upstream/master
git rm -r "manifests/c/Cuotai/Cuotai/$BadVersion"
winget validate --manifest "manifests/c/Cuotai/Cuotai/$PreviousVersion"
git commit -m "Remove version: Cuotai.Cuotai version $BadVersion"
git push -u origin HEAD
gh pr create --repo microsoft/winget-pkgs --base master `
  --head "jmarcon:remove-cuotai-$BadVersion" `
  --title "Remove version: Cuotai.Cuotai version $BadVersion" `
  --body "Removes a publisher-withdrawn Cuotai release; $PreviousVersion remains available."
```

The fork, validation, one-version path and PR are the repository's normal
submission process. Do not call this rollback complete while the PR is merely
open.

**Verify after moderation:** the PR has `Validation-Completed` and is merged
and its publish pipeline succeeded. Then run:

```powershell
winget source update
winget show --id Cuotai.Cuotai --exact --versions
```

The bad version must be absent and the previous version must remain. Record
the merged PR and the catalog check in the incident.

## What already-running updaters do after a yank

There is no remote uninstall or revocation list in the desktop updater.
Release notes and updater notes are generated from the same versioned
`CHANGELOG.md` section
([`xtask/src/main.rs`](../xtask/src/main.rs#L240-L253),
[`xtask/src/main.rs`](../xtask/src/main.rs#L257-L270), and
[`xtask/src/updater.rs`](../xtask/src/updater.rs#L124-L193)); the desktop
displays the manifest's notes only after `updater.check()` returns that release
([`updater.rs`](../apps/desktop/src-tauri/src/updater.rs#L648-L703)).

The timing of the rollback therefore matters:

- A client that checks after the release became a pre-release (or after
  `latest.json` was deleted) no longer discovers that version. If it had only
  seen **Available** and had not started the download, removing the package
  asset makes the later download fail.
- A client that already completed the download stays in **Ready** with the
  version and the verified bytes in memory
  ([`updater.rs`](../apps/desktop/src-tauri/src/updater.rs#L746-L761)). The
  restart path restores the cached metadata, checks the repository URL, and
  installs those in-memory bytes without checking the release endpoint again
  ([`updater.rs`](../apps/desktop/src-tauri/src/updater.rs#L798-L869)).
  Yanking cannot retract that download. Closing the app before installation
  discards it because a new service starts in `Idle`
  ([`updater.rs`](../apps/desktop/src-tauri/src/updater.rs#L455-L470)).
- A client that already installed the bad version remains on it. The updater
  cannot uninstall or downgrade it, and its greater-than version comparator
  will not offer the older release. The user must uninstall the bad version
  and reinstall the previous release manually.

Communicate these three cases explicitly. Do not say that marking a release
pre-release remotely removes, disables or rolls back an installed application.

## Rotate a leaked signing key

For the Tauri updater key, follow the authoritative
[`docs/updater-security.md` leak procedure](updater-security.md#if-the-private-key-leaks-rf42).
It includes the exact `cargo tauri signer generate` and `gh secret set`
commands, the required old-key/new-public-key transition release, the
single-public-key limitation of pinned `tauri-plugin-updater` 2.14.0, and the
point at which replacing the environment secret actually revokes the old
value. A client that misses the transition release needs a manual reinstall.

The checksum identity, Apple credentials and Windows Authenticode certificate
have different revocation authorities. Do not apply the updater transition
model to them; use
[`Rotating the other release credentials`](updater-security.md#rotating-the-other-release-credentials)
for their exact containment, replacement and verification steps.

### Rollback drill

Run this drill in a fork and throwaway tap/bucket/AUR package before enabling a
production channel, every six months, and whenever this procedure or a channel
tool changes:

1. Publish a disposable pre-release with dummy assets and an ephemeral updater
   key; never use production signing material.
2. Exercise the pre-release and asset-removal `gh` paths and prove the fork's
   `/releases/latest` no longer selects it.
3. Push a dummy bad-version commit to each test channel repository, revert it
   with the commands above, and verify the previous manifest from the remote.
4. Prepare, validate and close (without merging) a winget removal PR from the
   fork, unless Microsoft provides a designated test repository.
5. Do **not** rehearse `cargo yank` against real crates. Verify its syntax with
   `cargo yank --help` and use a private test registry if one is available.
6. Record who ran the drill, the date, command outputs, changed external rules
   and every owner action still blocked.
