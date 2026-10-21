# Releasing Cuotai

This document describes the release pipeline (PRD 0021 RF1–RF4): how a tag
becomes a verified GitHub Release, what is signed and what is not, how to
rehearse in a fork, and what the owner must configure. Package-manager
channels (PRD 0021 RF5: Homebrew, winget, Scoop, Cargo, AUR) are **not**
implemented yet; the pipeline never contacts them.

## How a release works

1. The delivering commit raises the version everywhere (see
   [`AGENTS.md`](../AGENTS.md): workspace `Cargo.toml`,
   `apps/desktop/src-tauri/Cargo.toml`, `tauri.conf.json`, both lockfiles,
   `CHANGELOG.md` section). CI's `version-check` job proves they agree.
2. The owner creates an **annotated** tag on `main` and pushes it:
   `git tag -a v0.68.0 -m "v0.68.0" <commit>` then `git push origin v0.68.0`.
3. `.github/workflows/release.yml` runs: **check → builds → assemble →
   publish → verify-public**.

```text
check ──┬─ desktop-macos ───┐
        ├─ desktop-windows ─┤
        ├─ desktop-windows-arm64 (optional) ─┤
        ├─ desktop-linux ───┤
        ├─ desktop-linux-arm64 (optional) ───┤
        ├─ cli (7 legs, 2 optional) ─────────┤
        └─ sbom ────────────┴─► assemble ─► publish ─► verify-public
```

An interrupted or failing run leaves at most a **draft** release — never a
public partial one.

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
| Updater | assembly job | `latest.json` (only when the updater archive is signed) |

Notes and limits:

- **musl**: `x86_64-unknown-linux-musl` builds (`rusqlite` is bundled and
  `keyring`'s Secret Service store uses the pure-Rust zbus). The Secret
  Service credential vault needs a **D-Bus session at run time**; on a musl
  system without D-Bus, vault-backed accounts are unavailable and
  environment-variable credentials are the fallback.
- **`aarch64-unknown-linux-musl` is not produced**: the Ubuntu runners ship
  no aarch64 musl cross toolchain, and downloading an external toolchain was
  rejected on supply-chain grounds. Revisit when a hosted toolchain exists.
- **ARM64 legs are optional** (RF2.5): supported hosted ARM runners are used,
  every optional job/matrix row has job-level failure tolerance, and assembly
  runs with an explicit mandatory-job gate. Missing optional artifacts are
  listed in `manifest.json` (`missing_optional`) and in the release notes;
  checkout, tool-install, packaging, or runner failures in these legs do not
  block mandatory release assembly.
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
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`): used **only** to sign
  `Cuotai.app.tar.gz` (PRD 0018 consumes it). When absent, no updater
  archive, no `.sig` and no `latest.json` are produced, and
  `signing-state.json` says so. Note: until PRD 0018 lands, the updater
  plugin is not configured in `tauri.conf.json`; the pipeline only produces
  the signed assets it will consume.
- Every published release asset gets a GitHub **build-provenance attestation**
  (verifiable with `gh attestation verify`) on tag runs. Dry runs do not
  request or publish attestations and say so in the workflow summary.

`signing-state.json` (schema version 1) is a first-class output: per artifact
`{name, signed, method, status, note?}`, the checksum-signature state, and
the updater state. The assembly **fails** when an artifact has no signing
entry, when an entry describes a file that does not exist, or when an entry
claims `signed` dishonestly. Methods: `developer-id`, `ad-hoc`,
`authenticode`, `tauri-updater-key`, `checksum-signature`, `none`.

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

The pipeline never creates secrets, environments or repositories. Before the
first real release:

1. Create the **`release` environment** (Settings → Environments) with a
   **required reviewer**. Without it the jobs still run but no secret is
   available (everything ships honestly unsigned); a dry run in the owner
   repository waits for reviewer approval because the build jobs reference
   the environment.
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
     `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; the generated public key belongs in
     `tauri.conf.json` when PRD 0018 lands.
3. Decide on the **retroactive tags** `v0.1.0`… (RF1.3) so the changelog
   footer links compare tags instead of commits.
4. Nothing else: no package manager is contacted yet.

Empty or missing secrets never fail a run midway: every secret-consuming
script checks presence first and degrades to the honest unsigned state.

## Rehearsing in a fork

1. Fork, enable Actions.
2. **Actions → Release → Run workflow** for a full dry run, or push an
   annotated `v<version>-rc.1` tag for the full path including a draft
   pre-release in the fork.
3. Check `signing-state.json` in the `release-assembly` artifact: everything
   unsigned, checksum signature absent (dry run) or Sigstore-signed (tag
   run), updater unavailable.

## Manual desktop checklist per release

The pipeline verifies checksums and provenance automatically. Before
announcing a release, a person still: opens the DMG/app on macOS (Gatekeeper
verdict when notarized), runs the NSIS installer and the MSI on Windows
(silent `/S` for NSIS), installs the `.deb` on the oldest supported Ubuntu
and runs the AppImage, and runs `cuotai --once` from each CLI archive.

## Revocation / yanking (RF9.4)

To pull a bad release: mark the GitHub Release as pre-release (or delete its
assets) — never reuse the tag; if a signing key leaked, rotate it (the
Sigstore keyless identity needs no rotation; the Tauri updater key does: new
pair, new `TAURI_SIGNING_PRIVATE_KEY`, the next release carries the new
public key when PRD 0018 embeds it) and state it in the release notes.
