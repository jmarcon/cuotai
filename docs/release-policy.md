# Release policy

PRD 0019 RF6: the version policy, the support policy and the
reproducibility guarantees of every Cuotai release. The operational
pipeline (tags, builds, signing, verification) is documented in
[`docs/releasing.md`](releasing.md).

## Version policy (RF6.1)

Cuotai follows semantic versioning while in `0.x`:

The version is computed once per release by `cargo xtask release prepare`
from the conventional commits since the last release (PRD 0039 RF3): the
release takes the largest bump any of its commits asks for.

- **`feat`** (new user-visible feature) raises the **minor**
  (`0.17.0` → `0.18.0`).
- **`fix`** and dependency or security fixes (`chore(security)`,
  `fix(deps)`) raise the **patch**.
- A change to distributed metadata that the app displays (license, About)
  also raises the patch: any other commit type with a `Changelog:` sentence
  is a patch-level **Changed** entry.
- `docs`, `test`, `style`, `refactor` and `chore` without user-visible
  effect, and any commit with `Changelog: skip <reason>`, do not change the
  version.
- From `1.0.0` a breaking change raises the **major**. `--version X.Y.Z`
  overrides the computed version upward only (for example `1.0.0`).

### What counts as a breaking change

While in `0.x` a breaking change raises the **minor** and carries a
`BREAKING CHANGE:` footer with the migration. Breaking means an
incompatible change to any of the four published contracts:

1. the **TOML configuration** (`cuotai.toml`: accounts, params, sessions);
2. the **`--json` snapshot output** of the CLI;
3. the **`export::v1` contract** (`--json-v1` and the local server routes;
   see [`docs/export-v1.md`](export-v1.md));
4. **`desktop.json`** (the desktop interface payload).

Renaming or removing a field, changing a type or meaning, or dropping a
precedence rule is breaking; adding a new optional field is not.

### Migration expectations

A breaking release states the migration in its `BREAKING CHANGE:` footer
and in the changelog entry (with the **Breaking** marker): what to rename,
move or reconfigure, and whether the old form is still read for one
release. Configuration files keep working unchanged within a minor line;
`ConfigFile` preserves unrelated TOML content and never rewrites what it
did not change.

## Support policy (RF6.1)

Only the **latest stable** release is supported. Fixes and security
patches ship as the next release from `main`; there are no backport
branches for older minors. Release candidates (`vX.Y.Z-rc.N`) are
preview builds and are not covered.

## Reproducibility (RF6.2)

- **Committed lockfiles:** both `Cargo.lock` and
  `apps/desktop/src-tauri/Cargo.lock` are versioned, and CI and the release
  pipeline build with `--locked`.
- **Pinned toolchain:** CI and the release workflow pin Rust **1.99.1**
  (also recorded in `rust-toolchain.toml`).
- **Per-release SBOM:** the release pipeline generates a CycloneDX 1.5 SBOM
  per release — one BOM per workspace merged into the single
  `sbom.cdx.json` release asset (`xtask/src/sbom.rs`, `sbom` job of
  `.github/workflows/release.yml`). The merge is deterministic: components
  are de-duplicated by `bom-ref`, edges are sorted, and the serial number
  and timestamp derive from the inputs, so the same inputs reproduce the
  same SBOM byte for byte. The SBOM is attached to every GitHub Release;
  publishing fails without it.

## Maintenance and changelog

Since 2026-10-23 (PRD 0039) versions and `CHANGELOG.md` are produced at
release time, never in feature or fix commits. Each user-visible commit
carries its entry as a `Changelog:` trailer (plus `Refs:`,
`Changelog-Category:` and `BREAKING CHANGE:` when they apply), and the
commit gate in CI rejects version or changelog edits outside a release
commit. `cargo xtask release prepare` turns the trailers into the
`## [X.Y.Z] - YYYY-MM-DD` section with the real short hashes, and the
release PR is where the wording is reviewed (see
[`AGENTS.md`](../AGENTS.md), "How to deliver a change", and
[`docs/releasing.md`](releasing.md)). The release workflow extracts the
version's section into the GitHub Release notes, fails when it is missing
or empty, and re-runs `release prepare --check` on the tagged commit.

Versions 0.1.0 to 0.79.0 are **internal versions**: each was raised by the
commit that delivered it and none was tagged or published. They are not
tagged retroactively (PRD 0039 D10); the first published release follows
them. `CHANGELOG.md` summarizes them in one "Pre-release history" section
instead of one section per version, and `cuotai --changelog` with an
internal version prints that history.
