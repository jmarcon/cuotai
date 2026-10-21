# PRD 0019 — Packaging, license, changelog and distribution

| Field | Value |
|---|---|
| ID | 0019 |
| Status | In progress (RF1, RF2 partial, RF3 superseded by PRD 0021 RF1–RF4, RF7.1 to RF7.3 and RF7.6) |
| Owner | Repository, CI and release (`.github/`, `apps/desktop`, `crates/cuotai-tui`) |
| Consumers | end users, PRDs 0014 and 0018 |
| Depends on | none; it is a prerequisite of PRDs 0014 and 0018 |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-21)

| RF | Version | Delivery |
|---|---|---|
| RF1.1 | 0.17.2 | License `MIT OR Apache-2.0`: `LICENSE-MIT`, `LICENSE-APACHE`, the manifests, `about.rs` and a README section. |
| RF1.2 | 0.17.2 | `CONTRIBUTING.md`, `SECURITY.md` (private reporting through GitHub) and issue templates in `.github/ISSUE_TEMPLATE/`. |
| RF1.3 | 0.17.2, CI in 0.27.1 | `deny.toml` validated with `cargo deny` 0.20 on both workspaces (licenses, sources and bans without errors). Since 0.27.1 the CI `deny` job runs it on both workspaces. |
| RF1.4 | 0.17.2 to 0.20.0 | Rule in `AGENTS.md`; the README, `CONTRIBUTING.md`, `SECURITY.md`, the issue templates, `docs/*.md`, `apps/desktop/README.md`, `cuotai.example.toml` and every PRD (0004 included, translated after 0.20.0) are in English. Semantic commits are in English. |
| RF2.1 | 0.27.1 (partial) | `.github/workflows/ci.yml` runs on every push and pull request, on `ubuntu-latest`, `macos-latest` and `windows-latest`: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and `cargo test` for the root workspace and the desktop workspace, all `--locked`. To build on Windows the desktop gained `icons/icon.ico` (required by tauri-build's resource step) and a `build.rs` that embeds Tauri's Common Controls v6 manifest into every artifact, so the test executables load (tauri-apps/tauri#13419). |
| RF2.2 | 0.27.1 | `Swatinem/rust-cache` for both `target/` directories (written only from `main`), a timeout per job, `permissions: contents: read`, `persist-credentials: false`, every action pinned by commit hash with its tag in a comment, no secrets, `pull_request` only (never `pull_request_target`), so fork pull requests get no secrets. |
| RF2.3 | 0.27.1 | Tests that need Unix permissions, symlinks or `#!/bin/sh` fakes of `aws`/`bl` are `#[cfg(unix)]`; no test touches the real vault, tray, notifications or browser (in-memory `TokenStore`, desktop backend without Tauri). The per-system coverage table is in `docs/architecture.md` ("Continuous integration"). |
| RF2 acceptance | partial | Verified locally: workspace and desktop fmt, Clippy and tests pass on macOS (arm64) and Linux (Debian bookworm container, WebKitGTK 4.1); Clippy and the test build pass for `x86_64-pc-windows-gnu` on both workspaces; `actionlint` 1.7.12 accepts the workflow; `cargo deny` passes on both manifests; a planted `clippy::ptr_arg` violation makes the CI Clippy command exit 101. **Missing (owner):** pushing the workflow to GitHub to get a green run on the three hosted runners, including the first native Windows MSVC test run, and a pull request with broken Clippy that fails. |
| RF3 | 0.68.0 | Superseded and delivered through PRD 0021 RF1–RF4: deterministic tag/version gate, cross-platform artifacts, honest signing/checksum/provenance/SBOM state, draft-to-verified release and safe dry run. No production tag, published release or owner signing secrets are claimed. |
| RF7.1 | 0.20.0 (no bump) | `CHANGELOG.md` at the root, in English, Keep a Changelog format, `[Unreleased]` on top and one section per version. No tags exist yet, so the footer links compare commits instead of tags. |
| RF7.2 | 0.20.0 (no bump) | Entries are one sentence from the user's point of view, with the `PRD NNNN RFx` reference and the short commit hash; versions with no user-visible change say so. |
| RF7.3 | 0.20.0 (no bump) | Maintenance flow in `AGENTS.md` (the `[Unreleased]` update and the `Changelog: skip` footer). The `git-cliff`/`xtask` draft generator is still open. |
| RF7.6 | 0.20.0 (no bump) | Every version from 0.1.0 to 0.20.0 reconstructed from the commits that change the version and from the PRDs' status tables, each entry citing its commit. |

The first `cargo deny` also found four `rustls` advisories and a yanked package, fixed in 0.17.1 (`chore(security)`). The `rsa` advisory `RUSTSEC-2023-0071` remains ignored, as no fixed version exists, with the justification recorded in `deny.toml`.

> **Scope note (2026-10-21):** the build, signing, publication (RF3) and installation-channel (RF4) requirements below are detailed and superseded by [PRD 0021](0021_release-pipeline-and-package-managers.md) (release pipeline and package-manager publishing). They stay here as the original summary; new work on them follows PRD 0021. RF1, RF2, RF5, RF6 and RF7 remain in this plan.

Sections 1 and 2 describe the situation before the implementation.

## 1. Context

Cuotai can currently only be used from source: the README tells you to run `cargo run` in `apps/desktop/src-tauri` and `cargo run -p cuotai-tui` at the root, and to build the installable package locally. Missing are:
- **License:** the manifests declare `license = "MIT"` (workspace and desktop), but the public repository has no `LICENSE` file (GitHub recognizes no license), no `CONTRIBUTING` and no security policy.
- **CI and releases:** there is no `.github/workflows`, no tags or published releases; the version is duplicated in `Cargo.toml` and `tauri.conf.json` (0.17.0).
- **Packages:** `tauri.conf.json` has `bundle.targets = "all"`, but nothing builds, signs or publishes artifacts; the TUI has no distributed binaries.
- **Platforms:** the README mentions the Keychain, Credential Manager and Secret Service vaults, but there are no packages or automated tests for Windows and Linux.
- **Changelog:** there is no `CHANGELOG.md`. The version rises with each delivery (from 0.14.x to 0.17.0 in a few days, each `Cargo.toml` change accompanying a `feat` or `fix` commit), but what changed in each version can only be reconstructed by reading 62 commits and the status tables of some PRDs (like PRD 0002's). There is no way for the project owner to see, in one place, what was delivered in each version and which plan requirement it corresponds to.

In the 2026-10-19 market analysis, every open competitor with traction distributes binaries: CodexBar (DMG, Homebrew cask, AUR, CLI tarballs for macOS and Linux, a Qt app for Linux), ClaudeBar (a signed, notarized DMG, cask), Quotio (DMG, cask, npm and Homebrew for the CLI), OpenUsage (a universal DMG, cask), OpenQuota (Windows x64/ARM64 installers, a universal DMG, AppImage and deb), QuotaTempo (a signed, notarized ZIP with `SHA256SUMS`). OpenQuota openly says when native signing is off.

## 2. Problem

Without binaries, without a formal license and without releases, Cuotai cannot be adopted by anyone who does not compile Rust. The lack of a license prevents contributions and redistribution by third parties, even though the manifest declares MIT.

## 3. Goals

- Formalize the license, the contribution policy and the security policy.
- Automate build, test, packaging, signing and publication of releases for macOS, Windows and Linux.
- Also distribute the TUI/CLI, and define the supported installation channels.
- Record, version by version, what was delivered, tied to the PRDs and the matching functional requirements, and show that to the user.
- Provide integrity verification (checksums and signatures) and the artifacts that PRD 0018's updater uses.

## 4. Non-goals

- App stores (Mac App Store, Microsoft Store) in this version.
- Immediate feature parity on the three platforms: the PRD defines what is supported, what is "best effort" and what is left out, with tests.
- Installation telemetry.

## 5. Functional requirements

### RF1 — License and governance (high priority; prerequisite)

- **RF1.1** Add the license text that the project owner decides. **Decision of 2026-10-19: `MIT OR Apache-2.0`** (the Rust ecosystem convention, with an express patent grant), with the holder "Cuotai contributors" and the year 2026, in `LICENSE-MIT` and `LICENSE-APACHE` at the root, `license = "MIT OR Apache-2.0"` in the manifests and `PROJECT_LICENSE` in `about.rs`. `apps/desktop/ui/img/providers/NOTICE.md` and `THIRD_PARTY_NOTICES` stay consistent with the choice (the code adapted from 9router is MIT and requires keeping the notice).
- **RF1.2** Add `CONTRIBUTING.md` (how to build, test, the semantic-commit policy, the project's security rules), `SECURITY.md` (a private reporting channel, scope, response times, disclosure policy) and issue templates (a broken-provider report without credentials, a new-provider request).
- **RF1.3** Check the compliance of dependency licenses (`cargo deny` or equivalent, through Cargo) in CI, and keep the About screen and `THIRD_PARTY_NOTICES` up to date (`about.rs`'s drift test already protects the manifests).
- **RF1.4** Documentation language: the README and all repository documentation are written in **English**, as is usual for open-source repositories (a rule recorded in `AGENTS.md`). Semantic commits, pull request text and issue text are also in English from now on. The product interface stays multilingual (pt-BR, en-US and es catalogs). New documents are born in English; no file mixes languages.
- **Acceptance criterion:** GitHub recognizes the license; CI fails on a dependency with an incompatible license; the documents exist and are linked in the README.

### RF2 — Continuous integration (high priority)

- **RF2.1** A CI workflow on every *push* and *pull request*: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the desktop tests (a separate workspace in `apps/desktop/src-tauri`), on macOS, Windows and Linux.
- **RF2.2** Build cache (`target/`), a timeout per job, minimal `GITHUB_TOKEN` permissions and actions pinned by *commit hash*. No secret available to *pull requests* from forks.
- **RF2.3** Platform-dependent failures (credential vault, tray, notifications) are isolated by `cfg`/marked tests, with the list of what is covered on each system documented in `docs/architecture.md`.
- **Acceptance criterion:** green CI on the three systems; a *pull request* with broken `clippy` fails.

### RF3 — Build, signing and publication (high priority)

> Detailed in [PRD 0021](0021_release-pipeline-and-package-managers.md), RF1 to RF4.

- **RF3.1** A release triggered by a `v*` tag on `main`. The version comes from a single source: the workspace `Cargo.toml`, with a CI check that `apps/desktop/src-tauri/Cargo.toml` and `tauri.conf.json` match (they are duplicated today).
- **RF3.2** Desktop artifacts: macOS universal (`.dmg`), Windows x64 and ARM64 (installer), Linux x64 and ARM64 (`AppImage` and `.deb`). TUI/CLI artifacts: tarballs for macOS (arm64 and x86_64) and Linux (glibc and musl, where viable) and a zip for Windows.
- **RF3.3** Signing:
  - macOS: Developer ID and notarization;
  - Windows: Authenticode, if there is a certificate, or an explicit notice in the release notes that the installer is not signed (as OpenQuota does);
  - all platforms: a `SHA256SUMS` file and a signature on the Tauri update artifacts (PRD 0018).
  Certificates and keys live in CI secrets, never in the repository or in logs.
- **RF3.4** Release notes = the version's section in `CHANGELOG.md` (RF7), plus the signing state of each artifact. The release fails if the section is missing or empty.
- **RF3.5** Publication to GitHub Releases, with the updater's `latest.json` and the checksums. The process must be repeatable in a fork for testing (without the production secrets).
- **Acceptance criterion:** a test tag generates the artifacts for all platforms; a clean install on each system opens the app, loads the configuration and creates a sample account; `shasum -c` verifies the checksums; the macOS package passes the Gatekeeper check.

### RF4 — Installation channels (medium priority)

> Detailed in [PRD 0021](0021_release-pipeline-and-package-managers.md), RF5 and RF8.

- **RF4.1** Homebrew: a *tap* of its own with a desktop *cask* and a CLI *formula*; once stable, evaluate the official repository.
- **RF4.2** Windows: a `winget` manifest and, optionally, `scoop`. Linux: an AUR package and `AppImage`/`.deb` instructions. Cargo: `cargo install` of the TUI from the repository, documented.
- **RF4.3** Installs through a package manager must be able to turn the internal updater off (PRD 0018, RF3.2), to avoid two update sources.
- **RF4.4** README reorganized with installation per platform as the first step and building from source as the alternative.
- **Acceptance criterion:** install and remove through `brew`, `winget` (or the installer) and a Linux package on clean machines; verify that uninstalling does not delete accounts or preferences without asking.

### RF5 — Platform support (medium priority)

- **RF5.1** Declare in `docs/platforms.md` what works on each system: credential vault, tray (on GNOME it may need an extension), notifications, native browser login, command-line tools (`aws`, `bl`) when needed.
- **RF5.2** Run an automated *smoke test* of the TUI binary on each system (`--list-providers`, `--once` with a sample configuration and no network) and a manual desktop verification checklist for every release.
- **RF5.3** Features unsupported on a platform appear disabled with an explanation, never failing silently.
- **Acceptance criterion:** the support matrix is reviewed on every release; the TUI smoke test runs in CI on the three systems.

### RF6 — Release policies (low priority)

- **RF6.1** A version policy (semantic; what counts as a breaking change: TOML, `--json`, the `export::v1` contract) and a support policy (only the latest stable version). The changelog has its own requirement (RF7).
- **RF6.2** Reproducibility: a versioned `Cargo.lock`, toolchain versions pinned in CI and an SBOM generated per release.
- **Acceptance criterion:** a published document; the SBOM is attached to the release.

### RF7 — Changelog and tracking of deliveries per version (high priority)

- **RF7.1** A `CHANGELOG.md` file at the root, **in English** (RF1.4's language rule), in the Keep a Changelog format and with semantic versioning. A `## [Unreleased]` section at the top and, below it, one section per version in descending order (`## [0.17.0] - 2026-10-18`, date in UTC), with comparison links between tags in the footer. Fixed categories: **Added**, **Changed**, **Fixed**, **Security**, **Deprecated** and **Removed**. An empty category does not appear.
- **RF7.2** Each entry has one sentence from the user's point of view (what changed, not how it was done), plus the traceability references in parentheses: the plan requirement met (`PRD 0006 RF2`) and the short commit hash. A change that breaks compatibility (TOML, `--json`, the `export::v1` contract, desktop preferences) gets the **Breaking** marker and the migration instruction. Changes with no effect for the user (`refactor`, `test`, `style`, `chore`) do not enter, except security fixes in dependencies, which go under **Security**.
- **RF7.3** Maintenance flow:
  - every user-visible delivery updates `[Unreleased]` in the same commit or *pull request*;
  - semantic commits that meet a plan requirement carry the footer `Refs: PRD 0006 RF2` (one line per requirement) and, when breaking, `BREAKING CHANGE:` with the migration;
  - for support, evaluate `git-cliff` (a Rust tool, installed through Cargo) or an `xtask` of our own that generates the section's **draft** from the commits since the last tag, grouping by type and by `Refs:`. The final text is reviewed by a person. No JavaScript tooling.
- **RF7.4** CI checks:
  - a *pull request* that changes `crates/` or `apps/` with a `feat` or `fix` commit must change `CHANGELOG.md`, except with the footer `Changelog: skip` and the reason;
  - the file obeys the format (a *parser* test): versions in descending order, every version with a date, valid categories;
  - on release, the top version matches that of `Cargo.toml` and `tauri.conf.json`;
  - every `PRD NNNN RFx` reference points to an existing requirement in the corresponding plan.
- **RF7.5** The release pipeline (RF3) extracts the version's section and uses it as the GitHub Release body, in the updater's `latest.json` notes (PRD 0018, RF2.2) and in the What's new screen (RF7.7). It fails if the section is missing or empty.
- **RF7.6** Retroactive history: reconstruct the versions already published (0.x up to 0.17.0) from the commits that change the version in `Cargo.toml` and from the PRDs' "Implementation status" tables, with the same structure. Reconstructed entries cite the source commit; a version or date with no evidence is marked `(reconstructed)` and is never invented.
- **RF7.7** Display in the product, without network:
  - the core embeds `CHANGELOG.md` (`include_str!`) and exposes `changelog::entries()` already parsed (version, date, categories, entries, references), so the interfaces do not parse Markdown;
  - desktop: **About → What's new** with the current version and the history; after an update, it opens once automatically, with an option to turn that off in **Settings**;
  - TUI and CLI: a What's new screen from About and `cuotai --changelog [VERSION]`;
  - the category labels come out in the three languages; the text of the entries stays in English (the changelog's language), which the screen itself states.
- **RF7.8** Link with the plan control: whenever a requirement is marked as delivered in a PRD or in `0000_control.md`, the version cited must exist in `CHANGELOG.md`. A CI check compares the two sources and fails when they diverge.
- **Acceptance criterion:**
  - *parser* tests (valid format, a version with no date, an invalid category, wrong order);
  - a test tag produces a GitHub Release whose body is the section of `CHANGELOG.md`, and a release with no section fails;
  - a reference to a nonexistent requirement breaks CI;
  - the retroactive history covers every version since the first with entries traceable to commits;
  - a smoke test in three languages: **About → What's new** on the desktop and `--changelog` on the CLI show the same version and the same entries, with no network access.

## 6. Non-functional requirements

- **Supply-chain security:** secrets only in protected CI, actions pinned by hash, audited dependencies, artifacts verifiable by checksum and signature.
- **Honesty:** the signing state of each artifact is stated in the release notes.
- **Cost:** the pipeline must fit the free GitHub Actions plan for public repositories, with caching and a limited *matrix*.
- **External dependencies:** the Apple certificates and (optionally) the Windows one are decisions and costs for the project owner.

## 7. Order of execution and verification

1. **RF1:** license and governance (quick, and it unblocks the rest).
2. **RF7.1 to RF7.3 and RF7.6:** `CHANGELOG.md`, the footer convention and the retroactive history. It does not depend on the pipeline and takes effect immediately, so the next deliveries are already recorded.
3. **RF2:** CI on the three systems, already with the RF7.4 checks.
4. **RF3:** build, signing and publication, with a test tag in a fork and RF7.5.
5. **RF7.7 and RF7.8:** display in the product and the link with the plan control.
6. **RF4 and RF5:** channels, support matrix and smoke tests.
7. **RF6:** policies.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when a user without Rust installed downloads a package from the releases page, verifies the checksum, installs on macOS, Windows or Linux, opens the app with the configuration preserved, the repository's license is recognized, the pipeline publishes the `latest.json` that PRD 0018's updater consumes, and `CHANGELOG.md` shows, for each version, what was delivered and which plan requirements it corresponds to.
