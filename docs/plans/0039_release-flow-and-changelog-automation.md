# PRD 0039 — Release flow and changelog automation

| Field | Value |
|---|---|
| ID | 0039 |
| Status | Implemented (RF1–RF11; the optional RF8.2(b) split is declined and D13 CodeQL merge protection is a later owner step) |
| Owner | Repository process and tooling: `AGENTS.md` ("Versioning, Commits & Changelog"), `xtask` (`changelog_gate.rs`, `draft.rs`, `check.rs`, `plan_links.rs`, new `commits.rs` and `release/`), `.github/workflows/ci.yml`, `.github/workflows/release.yml` (check job only), `docs/releasing.md`, `docs/release-policy.md`, `CONTRIBUTING.md` |
| Consumers | the owner and every agent that delivers a change; the release pipeline of PRD 0021; the changelog and What's new of PRD 0019 RF7; the plan control (`0000_control.md`) |
| Depends on | PRD 0019 (RF2 CI, RF7 changelog format, gate, draft and plan links), PRD 0021 (RF1 tag trigger and `release-check`, RF4 release notes, RF9 workflow policy) |
| Coordinates with | the documentation-only CI skip already on `main` (`6638c69`, `73aa423`: the `changes` job and the `CI result` aggregate in `ci.yml`). This PRD extends that job's outputs and the aggregate; it does not re-implement the classifier |
| Amends | PRD 0019 RF7.3 (maintenance flow) and RF7.4 (pull-request gate); PRD 0021 RF1.1/RF1.3 (how the tagged commit is produced; retroactive tags) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **release commit** is the single commit `chore(release): X.Y.Z` that `cargo xtask release prepare` writes. A **release PR** is the pull request that carries it. An **entry** is one bullet of a `CHANGELOG.md` section. A **trailer** is a `Key: value` line at the end of a commit message (Git's term).

## Implementation status (2026-10-23)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.80.0 | `xtask/src/commits.rs`: the shared header and trailer parser (`Changelog:`, `Changelog-Category:`, `Refs:` with plans and issues, `BREAKING CHANGE:`, `Changelog-Edit:`, `Version-Override:`), the skip-marker check and the sentence lint, with unit tests for every trailer form. |
| RF2 | 0.80.0 | `changelog-gate` rewritten as the commit gate (rules 1–7 plus `Refs:` resolution against the plans), wired on pull requests and on pushes to `main`; temp-repository tests in `xtask/tests/commit_gate.rs`. |
| RF3 | 0.80.0 | Release base (newest stable tag, else the last version-changing commit; rc tags never), bump table and pure `next_version`, `--version` override rules; table-driven tests. |
| RF4 | 0.80.0 | `cargo xtask release prepare` with `--dry-run`, `--branch`, `--refresh`, `--check` and `--rev`; line-targeted edits of the five locations, the `cuotai-tui` requirement and both lockfiles (verified with `cargo metadata --locked --offline`), the section with real short hashes and footer links; golden and temp-repository tests in `xtask/tests/release_prepare.rs`. The entry renders as `<sentence>[. Migration: <migration>] (<refs>; <hash>).` (the sentence keeps a period before the migration). `changelog-draft` became the would-be-section view. |
| RF5 | 0.80.0 | `cargo xtask release tag [--rc]` (annotated, message = section, rc numbering, refusal on an existing stable tag or a commit not on `origin/main`) and `release prepare --check` in `release.yml`'s `check` job. Delivered in use: `v0.80.0-rc.1`, `v0.80.0-rc.2` and `v0.80.0` were tagged with it and published (checked on 2026-10-25); RF5.5 automation declined (D6). An rc never offers an in-app update (the updater reads GitHub's latest non-prerelease), so the first update proof is `v0.80.0` → `v0.80.1` (`0000_control.md` §2.1). |
| RF6 | 0.80.0 | `vNEXT` replacement outside code in `docs/**/*.md` and `README.md`, the control header and the generated §3 rows; `--check` and `release.yml` reject a leftover token. |
| RF7 | applied | Documented in `docs/releasing.md` ("Branch and tag protection") with the exact `gh api` commands; the rulesets are active on the repository (`main`, `release tags: creation` and `release tags: immutable`, checked on 2026-10-25). Agent rules in `AGENTS.md` ("How to deliver a change"). |
| RF8 | 0.80.0 | `ci.yml` triggers (pull requests, `main` pushes, dispatch, merge groups; no tags, no other branches), `release_tooling` output and `release-preflight` job, the gate on `main` pushes, the next-version job summary, `cargo fmt` once on Linux, `CI result` aggregating the preflight; `workflow-check` enforces the triggers and the aggregate. The optional `root`/`desktop` split of RF8.2(b) is deliberately not done (decision of 2026-10-25): `crates/cuotai-core/**` and the lockfiles count for both sides, so a split saves runs only for TUI-only or UI-only changes, while it adds a second classification to keep in step with the matrix and with the `ui-checks` job; revisit if the macOS/Windows minutes become a problem. |
| RF9 | 0.80.0 | Cutover: `AGENTS.md` rewritten ("How to deliver a change" and "Versioning, Commits & Changelog", with the dated notice), `docs/releasing.md`, `docs/release-policy.md`, `CONTRIBUTING.md`, `docs/architecture.md`, amendment notes in PRDs 0019 and 0021; the footer links of 0.66.0–0.79.0 compare commits and `changelog-check` validates them; no tag at cutover. |
| RF10 | vNEXT | Delivered (D12 lifted by the owner on 2026-10-25, before the first release): `crates/cuotai-core/build.rs` sets `CUOTAI_BUILD_DESCRIBE` from `git describe --tags --long --dirty --always` (empty outside the workspace checkout, so crates.io builds, source archives and a missing Git show the plain version); `cuotai_core::build_info` formats it (`dev_suffix`, `display_version`); `cuotai --version` and the TUI About print `0.80.0 (dev abc1234)` (`, modified` with uncommitted edits) unless the build is exactly at a tag (an rc tag counts); the desktop `UpdateView.build` and `AboutView.build` carry the suffix and Settings › Updates shows it after the version. Comparisons keep `CARGO_PKG_VERSION`. Release workflows are unchanged: their shallow checkouts have no tag history to describe, so they stay plain. |
| RF11 | 0.80.0 | `changelog-check` compares the §2 strong dependencies, the §4 solid edges and the "What each plan unblocks" table of `0000_control.md` (`plan_links::plan_graph_problems`), with fixture tests; it runs in the always-on `changelog-check` CI job. |

## 1. Context

### 1.1 Owner complaint (2026-10-23)

> "The pipelines run only on main and the changelog goes along with every commit; it should be generated/updated at the moment a new version is published. This CI/CD flow has to improve a lot."

### 1.2 How the repository works today

Checked against `origin/main` at `239aa90` (workspace version 0.77.0) on 2026-10-23:

- **Every user-visible commit is a release.** `AGENTS.md` requires each `feat`/`fix` to raise the version in the same commit, in five places (`Cargo.toml` `[workspace.package]`, `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, `Cargo.lock`, `apps/desktop/src-tauri/Cargo.lock`) plus the `cuotai-core` requirement in `crates/cuotai-tui/Cargo.toml` (`version = "0.77"`) on every minor, and to move its `CHANGELOG.md` entry from `[Unreleased]` into a new dated section. Of the 254 commits on `main`, **101 change the workspace version line** (versions 0.1.0 to 0.77.0 in seven days); `CHANGELOG.md` has 101 released sections and an always-empty `[Unreleased]`.
- **Nothing was ever published.** There is no Git tag and no GitHub Release; every channel of PRD 0021 is pending. The 101 "versions" are tree states, not shipped artifacts.
- **Bumps conflict.** Two branches that each deliver a feature both rewrite the same six version lines and both insert a section at the top of `CHANGELOG.md`; the second to land must recompute its version, re-date its section, and re-run `cargo check` in two workspaces. Three branches had to be sequenced by hand this week.
- **A commit cannot cite its own hash**, so the rule tells the *next* changelog-touching commit to back-fill it. Entries are inconsistent about it (some cite the hash inside the refs, some after them, some not at all).
- **Footer links are already broken.** From `[0.67.0]` upward the footer compares tags (`compare/v0.66.0...v0.67.0`) that do not exist; `[0.66.0]` compares `ae4c0ea...main`, a moving target.
- **CI triggers.** `ci.yml` runs on `push` (every branch *and* every tag, because the trigger is unfiltered) and `pull_request`. In practice agents commit straight to `main` in the shared checkout, so CI only ever runs **after** a change is on `main`; the only other remote branch is `ci-docs-skip-probe`. `release.yml` runs on `v*` tags and `workflow_dispatch` (cheap preflight, or `full_rehearsal`). `external-rules.yml` runs monthly. CodeQL runs through GitHub's default setup plus the local `scripts/codeql-local.sh` that `AGENTS.md` asks agents to run before pushing.
- **Documentation-only skip (other agent, already on `main`).** `6638c69` added the `changes` job (allow-list of documentation paths; `CHANGELOG.md` deliberately counts as code because the binaries embed it), gated `test`/`deny`/`version-check` on it, kept `changelog-check` always on, added the `CI result` aggregate as the single required check, and keyed concurrency of `main`/tag runs by commit. `73aa423` documented that GitHub's skip-CI marker matches anywhere in a commit message.
- **Tooling that already exists** (`xtask`, all Rust, no shell):
  - `release-check` — the five locations, both lockfiles' `cuotai-core`/`cuotai-tui`/`cuotai-desktop` entries, and the first released `CHANGELOG.md` section agree; with `--tag` and `--require-main`, the tag is annotated, well formed (`vX.Y.Z` or `vX.Y.Z-rc.N`) and on `main`. Runs in CI (`version-check`) and in `release.yml`'s `check` job.
  - `changelog-check` — the strict shared parser (`crates/cuotai-core/src/changelog.rs`, `#[path]`-included into `xtask` and embedded in the product for What's new), every `PRD NNNN RFx` reference exists, and every version a plan or `0000_control.md` marks as delivered has a section (PRD 0019 RF7.8).
  - `changelog-gate` — on pull requests, a `feat`/`fix` commit touching `crates/` or `apps/` needs a `CHANGELOG.md` change in the PR or `Changelog: skip <reason>`.
  - `changelog-draft` — parses commits since the last `v*` tag (falling back to the last commit that changed the workspace version), maps `feat`→Added, `fix`→Fixed, `fix(deps|security)`/`chore(security)`→Security, carries `Refs:` and the breaking marker, and prints a draft; it never writes the file.
  - `workflow-check` — static policy of every workflow (permissions, secret owners, the known job list of `release.yml`).

### 1.3 What must be preserved

- Conventional commits (`type(scope): subject`, English) as the history's source of truth, `Refs: PRD NNNN RFx` footers and `BREAKING CHANGE:` footers with the migration.
- The `CHANGELOG.md` format exactly as the shared parser accepts it (Keep a Changelog, `## [X.Y.Z] - YYYY-MM-DD`, six fixed categories, one user-voice sentence per entry with its references and short hash). The format is a product contract: the TUI/desktop What's new, the GitHub Release body and the updater's `latest.json` notes all read it.
- The version policy of `docs/release-policy.md` (0.x: `feat` and breaking → minor; `fix`/security → patch).
- The release pipeline of PRD 0021 (annotated tag on `main` → `release.yml` → draft → verified → published). This PRD changes **how the tagged commit is produced**, not how it is built.

## 2. Problem

Versioning is coupled to delivery instead of to publication. The result is: version-bump merge conflicts between every pair of parallel branches; a manual, error-prone five-place edit in every feature commit; changelog entries without the hash of their own commit; plans citing "delivered in 0.74.0" for versions nobody can download; and CI that only sees changes after they are on `main`, because there is no branch/PR flow. The changelog and the version should be produced **once, at release time, from the commits** — and every change should be tested **before** it reaches `main`.

## 3. Goals

1. Feature and fix commits never touch a version location or `CHANGELOG.md`; parallel branches no longer conflict on versioning.
2. One command (`cargo xtask release prepare`) computes the next version from the commits since the last release, writes all version locations and lockfiles, renders the `CHANGELOG.md` section with the real short hashes, resolves plan references, and produces a single release commit on a release branch for a release PR.
3. Merging the release PR and pushing one annotated tag starts the existing pipeline unchanged; pre-releases (`-rc.N`) work the same way.
4. Every change reaches `main` through a pull request whose CI must pass (with an explicit owner bypass), and the shared main checkout is never a working area.
5. The generated changelog stays byte-compatible with the shared parser and the What's new screen.

## 4. Non-goals

- Changing the release build, signing, publication or package-manager jobs of PRD 0021 (only the `check` job gains checks).
- Changing the version policy or the `CHANGELOG.md` format.
- Rewriting history: the 101 existing sections, their dates and the existing commits stay as they are.
- Managing GitHub settings from code (PRD 0021 §4): rulesets, environments and apps are owner actions this PRD lists.
- Publishing crates on every release (crates.io stays the opt-in `PUBLISH_CARGO` channel).

## 5. Options evaluated

Repository constraints that decide the comparison: a Rust root workspace **plus a separate Tauri workspace** with its own lockfile; **five** version locations plus the `cuotai-core` requirement in `cuotai-tui`; a **strict, product-embedded** changelog parser with a fixed heading (`## [X.Y.Z] - YYYY-MM-DD`) and fixed categories; `PRD NNNN RFx` references validated against the plans; plan tables that cite versions; no JavaScript package manager (PRD 0019 RF7.3: "No JavaScript tooling"); agents committing from worktrees; and an `xtask` that already parses commits, finds the last release point and checks every location.

| Option | How it would work here | Pros | Cons for this repository |
|---|---|---|---|
| **release-please** (GitHub Action, release PR) | A bot keeps a release PR open, bumping versions from conventional commits and prepending its changelog; merging it tags and creates a GitHub Release. | Mature release-PR model; zero local tooling; computes SemVer from commits. | Its changelog heading (`## [0.78.0](compare-link) (2026-10-23)`) and sections (`### Features`, `### Bug Fixes`, entries with bold scope and hash links) do not match the strict parser, which the product embeds: either the parser (a product contract) changes or the output is post-processed. Entries are commit subjects, not user-voice sentences (overrides go in the PR body, outside history). The desktop workspace, `tauri.conf.json` and the second lockfile need `extra-files` JSONPath/TOML updaters; `cuotai-core`'s entry in the desktop lockfile is not covered. It wants to create the GitHub Release itself, which conflicts with `release.yml`'s draft → verify → publish. A PR opened with `GITHUB_TOKEN` does not trigger CI, so it needs a PAT or GitHub App. JavaScript action executed in CI (no local install, but against the spirit of RF7.3). |
| **git-cliff** (+ xtask) | `git cliff --bumped-version` computes the version; a Tera template renders the section; `xtask` edits the five locations. | Rust, `cargo install`-able, very flexible templates (can produce the Keep a Changelog heading, categories from `commit_parsers`, refs from footers, short hashes); bump rules configurable for 0.x. | A second conventional-commit parser and a template language beside `xtask`'s gate and draft, which can disagree; a sizable new dependency tree to pin and audit in `cargo deny`; still needs `xtask` for every version location, the lockfiles, the plan references and the footer links, so it replaces only the ~150 lines of `draft.rs` that already exist. |
| **cargo-release** | `cargo release minor --execute` bumps workspace crates, runs `pre-release-replacements` (regex) on `CHANGELOG.md`/`tauri.conf.json`, commits, tags and pushes. | Standard Rust tool; good at "rename `[Unreleased]` to `[X.Y.Z] - date`" when entries already live in `[Unreleased]`. | Built for the model we are leaving (entries accumulate in `[Unreleased]` in feature commits — the conflict source). It does not compute the level from commits, does not know the separate desktop workspace (hooks needed for its manifest and lockfile), and its tag-and-push step bypasses the release PR and CI. Its `cargo publish` step must be disabled. |
| **knope** | Changesets and/or conventional commits feed a `PrepareRelease` workflow that bumps `versioned_files` and writes the changelog. | Rust single binary; supports both fragment files and conventional commits; release-PR recipes exist. | Its changelog headings and section names are its own; matching the strict parser's `## [X.Y.Z] - YYYY-MM-DD` and the six categories must be verified and may be impossible without changing the parser. Generic JSON/TOML "dot path" versioned files must be verified for `tauri.conf.json` and for lockfile entries of a path dependency in another workspace. It also wants to own the GitHub Release step. Plan-reference resolution would still be `xtask`. |
| **Changelog fragments** (changesets-like `changes/*.md`) | Each PR adds `changes/<slug>.md` with front matter (category, breaking, refs) and the user sentence; `release prepare` concatenates and deletes them. | Conflict-free (unique file names); the wording is reviewed in the PR diff and editable until release; independent of merge strategy. | One more file per change that agents must remember; the entry's hash is not the commit's own (recoverable as "the commit that added the fragment", which is wrong for multi-commit PRs and after squash); `changes/**` would be classified as code by the documentation-only filter unless allow-listed; two sources of truth for "what changed" (commit and fragment). |
| **Home-grown `cargo xtask release prepare`** (entries from commit trailers) | Extends `changelog-draft` into a writer: entry text comes from a `Changelog:` trailer in the commit, everything else (category, bump, refs, breaking, hash) from the conventional header and footers; edits all locations; opens nothing by itself. | Reuses the parser the product embeds, the commit parsing of `draft.rs`, the last-release-point logic and every check of `release-check`; one parser for gate, draft and prepare; no new runtime and few or no new dependencies; hashes are exact because prepare runs **after** the commits are on `main`; knows the desktop workspace, the `cuotai-tui` requirement, the plans and the footer links natively; testable with the existing temp-repo fixtures. | We maintain it (estimated 600–900 lines plus tests); wording lives in immutable commit messages (fixed in the release PR, see RF4.6); the trailer must survive the merge strategy (rebase or merge commits keep it; squash needs the PR body, RF7.3). |

**Recommendation: the home-grown `cargo xtask release prepare`, with entries taken from `Changelog:` trailers, and a release PR as the review and editing point.** Every external tool would need the same `xtask` glue for the desktop workspace, the plan references and the footer links, and two of them (release-please, knope) would force a change to a changelog format that is a product contract. Fragments are the strongest alternative and remain the documented fallback if trailers prove error-prone (§12, Q1). git-cliff remains the fallback renderer if `xtask` maintenance grows.

## 6. Functional requirements

### RF1 — Commit contract (high priority; prerequisite)

Conventional commits stay the source of truth; a commit's message carries everything a changelog entry needs, and a feature or fix commit never touches versioning.

- **RF1.1** Header: `type(scope)!: subject` with the types `feat`, `fix`, `docs`, `refactor`, `test`, `style`, `chore`, `revert` (`!` optional). The release commit is `chore(release): X.Y.Z`.
- **RF1.2** Trailers (case-sensitive keys, in the last paragraph of the message, Git trailer syntax with continuation lines indented by one space):
  - `Changelog: <one sentence from the user's point of view, ending with a period>` — the entry text. Several `Changelog:` trailers in one commit produce several entries. The sentence carries no references and no hash (they are added from the other trailers).
  - `Changelog: skip <reason>` — no entry; the reason is mandatory (today's rule).
  - `Changelog-Category: Added|Changed|Deprecated|Removed|Fixed|Security` — overrides the default category of RF3.2 (for example a `feat` that changes behavior is **Changed**; a removal is **Removed**).
  - `Refs: PRD 0006 RF2` (one or several per line, comma- or semicolon-separated, as today) and `Refs: #12` / `Refs: issue #12` for GitHub issues.
  - `BREAKING CHANGE: <migration>` — non-empty migration text; it renders as the **Breaking** marker plus the migration (RF4.3).
- **RF1.3** A commit that is not a release commit must not change: the workspace `version`, the desktop `version`, `tauri.conf.json` `version`, the `cuotai-core` requirement in `crates/cuotai-tui/Cargo.toml`, the workspace packages' entries in either lockfile, or `CHANGELOG.md`. Exception: a `docs(changelog)` commit with a `Changelog-Edit: <reason>` trailer may correct prose of **already released** sections (never add entries, never touch `[Unreleased]`).
- **RF1.4** Plan files cite the release-to-be with the literal token `vNEXT` (RF6); a non-release commit never writes a version number that has no `CHANGELOG.md` section (already enforced by RF7.8's check).
- **RF1.5** One shared parser module (`xtask/src/commits.rs`) implements RF1.1–RF1.2 and replaces the two ad-hoc parsers of `changelog_gate.rs` and `draft.rs`. It reads commits with `git log --no-merges --format=%H%x1f%s%x1f%B%x1e` (the existing record format).
- **Acceptance:** unit tests parse every trailer form (single, several, continuation lines, CRLF, trailing whitespace, lowercase key rejected, `Changelog: skip` without reason rejected, `BREAKING CHANGE` without text rejected, `feat!` without footer accepted as breaking with an error asking for the migration, `Refs:` with mixed PRD and issue references).

### RF2 — Commit gate (high priority; replaces `changelog-gate`)

- **RF2.1** `cargo xtask changelog-gate --base <sha> --head <sha>` (same name, so CI wiring stays) checks every non-merge commit of the range against RF1 and reports all problems together:
  1. the header is conventional (RF1.1);
  2. a **user-visible** commit (`feat`, `fix`, `revert`, `chore(security)`, any breaking commit, or any commit that has a `Changelog:` sentence) that touches product paths (`crates/`, `apps/`, `LICENSE-*`, `THIRD_PARTY_NOTICES`) has a `Changelog:` sentence or `Changelog: skip <reason>`;
  3. a breaking commit has a non-empty `BREAKING CHANGE:` migration;
  4. RF1.3: no version location, workspace lockfile entry or `CHANGELOG.md` change outside a release commit (or an allowed `docs(changelog)` edit);
  5. a release commit changes only the release file set of RF4.4 and passes `release prepare --check` (RF4.7);
  6. no trailer contains GitHub's skip-CI markers (they would later be copied into the release commit message and skip CI on the release PR; `73aa423`);
  7. light lint of each sentence: starts with a capital letter, ends with a period, has no `PRD ` reference and no hexadecimal hash (those come from the trailers), at most 600 characters.
- **RF2.2** It runs on pull requests (`base.sha..head.sha`) **and** on pushes to `main` (`before..after`), so a push that bypassed the ruleset (RF7.2) is still checked after the fact; a forced push or an unknown `before` falls back to `merge-base(origin/main, after)` like the `changes` job does.
- **RF2.3** Error messages name the commit (short hash and subject), the rule, and the fix ("add a `Changelog:` trailer with the user-facing sentence, or `Changelog: skip <reason>`"; "move the version change into `cargo xtask release prepare`").
- **Acceptance:** unit tests per rule (green and red), plus a temp-repo integration test (`xtask/tests/commit_gate.rs`) with a feature branch that bumps a version (fails), a `feat` without trailer (fails), a `feat` with trailer (passes), a docs-only `feat` without trailer (passes), a release commit produced by `release prepare` (passes) and a hand-edited release commit that also changes `crates/` (fails).

### RF3 — Next-version computation (high priority)

- **RF3.1** The **release base** is the newest stable tag `vX.Y.Z` reachable from `HEAD`; while none exists, the last commit that changed the workspace `version` line in `Cargo.toml` (today's `draft.rs` fallback, which after the cutover is always the last release commit). Release-candidate tags are never a base.
- **RF3.2** Each commit since the base contributes a bump and, when user-visible, entries:

  | Commit | Bump (0.x) | Bump (≥ 1.0) | Default category |
  |---|---|---|---|
  | breaking (`!` or `BREAKING CHANGE:`) | minor | major | from its type, with the **Breaking** marker |
  | `feat` | minor | minor | Added |
  | `fix`, `revert` | patch | patch | Fixed |
  | `fix(deps)`, `fix(security)`, `chore(security)` | patch | patch | Security |
  | any other type with a `Changelog:` sentence (for example license or About metadata) | patch | patch | Changed |
  | any other type without a sentence, or `Changelog: skip` | none | none | — |

  `Changelog-Category:` overrides the category, never the bump. The release's bump is the maximum over its commits.
- **RF3.3** `next_version(current, bump)` is a pure function: patch → `X.Y.(Z+1)`; minor → `X.(Y+1).0`; major → `(X+1).0.0`, or minor while `X == 0`.
- **RF3.4** `--version X.Y.Z` overrides the result (for example `1.0.0` when the §5.1 criteria of `0000_control.md` are met); it must be greater than the current version and not lower than the computed one.
- **RF3.5** No user-visible commit since the base → `prepare` refuses with "nothing to release" unless `--version` is given, in which case the section is the parser's plain paragraph "No user-visible changes."
- **Acceptance:** table-driven unit tests: every row of RF3.2, maximum over mixed commits, 0.x versus ≥ 1.0 breaking, `0.9.3`+minor → `0.10.0`, `0.77.0`+patch → `0.77.1`, override lower than computed refused, override not greater than current refused, empty range refused, rc tags ignored as base, tag-less fallback to the last version-changing commit.

### RF4 — `cargo xtask release prepare` (high priority)

- **RF4.1** `cargo xtask release prepare [--version X.Y.Z] [--date YYYY-MM-DD] [--dry-run] [--branch] [--refresh] [--check]` runs from a clean worktree on an up-to-date `main` (it refuses a dirty tree in any file it writes, and warns when `HEAD` is not `origin/main`). `--date` defaults to today in UTC.
- **RF4.2** It renders the section `## [X.Y.Z] - YYYY-MM-DD` with the parser's category order, one entry per `Changelog:` sentence, commits in chronological order inside each category. Entries carried over from a non-empty `[Unreleased]` (cutover only, RF9.3) come first in their category, verbatim.
- **RF4.3** Entry format (canonical, golden-tested): `- [**Breaking**: ]<sentence without its final period>[. Migration: <migration without its final period>] (<references>; <hash>).` — references grouped by plan in first-appearance order (`PRD 0018 RF1.2, RF1.3; PRD 0021 RF2`), then issues (`issue #2`), then the 7-character short hash (longer when Git needs it to be unambiguous). A commit with several sentences repeats its references and hash on each.
- **RF4.4** It writes, and only writes, the **release file set**:
  1. `Cargo.toml` `[workspace.package] version`;
  2. `apps/desktop/src-tauri/Cargo.toml` `[package] version`;
  3. `apps/desktop/src-tauri/tauri.conf.json` `version`;
  4. `crates/cuotai-tui/Cargo.toml` `cuotai-core` requirement (`"X.Y"` while `X == 0`, `"X"` from 1.0), only when it changes;
  5. both lockfiles, through `cargo update --workspace --offline` in the root and `cargo update --offline --manifest-path apps/desktop/src-tauri/Cargo.toml -p cuotai-desktop -p cuotai-core` (workspace entries only; no dependency moves);
  6. `CHANGELOG.md`: the new section under an emptied `[Unreleased]`, the `[Unreleased]` footer link (`compare/vX.Y.Z...HEAD`) and a new `[X.Y.Z]` link comparing the previous **existing** tag, or the base commit's short hash while no tag exists;
  7. plan files: RF6.
  Manifest edits are line-targeted (the value on one known line) and re-parsed afterwards, so comments and formatting survive; every write is atomic (temporary file and rename).
- **RF4.5** After writing it runs `release-check` and `changelog-check` in-process and fails (leaving the tree for inspection) when either fails. With `--branch` it creates `release/vX.Y.Z`, commits `chore(release): X.Y.Z` with a body that lists the bump rationale (which commits drove it), the rendered section, `Changelog: skip (release commit)` and `Version X.Y.Z.`, with every skip-CI marker neutralized. It never pushes, never tags and never opens a PR by itself; it prints the `git push` and `gh pr create` commands (the PR body = the section, the rationale and the manual checklist of `docs/releasing.md`).
- **RF4.6** **Editing point.** The release PR is where wording is fixed: the owner (or an agent asked to) edits the section in the release branch. `--refresh` re-renders on the release branch while keeping every existing entry whose hash already appears in the section (edited text wins), appending entries for commits that landed since, and moving the date to `--date`; a commit is "covered" when its short hash appears anywhere in the section, so merging two entries into one by hand is preserved.
- **RF4.7** `--check` (used by CI on the release PR, by the gate's rule 5 and by `release.yml`) recomputes the release from the base and fails unless: the version equals the computed (or a valid override recorded in the release commit body as `Version-Override: X.Y.Z`), every user-visible commit of `base..release-commit^` is covered, no covered hash is outside that range, and no `vNEXT` remains.
- **RF4.8** `--dry-run` prints the computed version, the rationale and the unified diff of every file it would write, and writes nothing. `cargo xtask changelog-draft` becomes an alias that prints only the would-be section (mid-cycle "what is unreleased").
- **Acceptance:** golden tests of the rendered section (every category, breaking with migration, several references and issues, multi-sentence commit, carried `[Unreleased]` entries); the rendered file passes `cuotai_core::changelog::parse`; a temp-repo integration test (`xtask/tests/release_prepare.rs`, extending the `release_check.rs` fixture with both workspaces and lockfiles) runs prepare, then `release-check` and `changelog-check` pass, and running prepare again reports "nothing to release"; `--refresh` keeps a hand-edited entry and adds a new one; `--check` fails on a missing commit, a foreign hash, a wrong version and a leftover `vNEXT`; `--dry-run` leaves the tree byte-identical; the test runs on Linux, macOS and Windows CI.

### RF5 — Release PR, tagging and pre-releases (high priority)

- **RF5.1** Flow: `release prepare --branch` → push `release/vX.Y.Z` → release PR → CI (RF8, including `release-preflight` and `prepare --check`) → optional full rehearsal (Actions → Release → Run workflow on the release branch with `full_rehearsal`, owner decision D7) → merge → `cargo xtask release tag` → push the tag → `release.yml` (unchanged build/publish).
- **RF5.2** `cargo xtask release tag [--rc]` (run by the owner, or by an agent under the release rules of `AGENTS.md` when the owner asked for that release) runs on an up-to-date `main`, finds the newest release commit, checks `release-check --tag` and `prepare --check` against it, creates the **annotated** tag `vX.Y.Z` (or `vX.Y.Z-rc.N`, with N = 1 + the highest existing rc for that version) whose message is the section, and prints `git push origin <tag>`. It refuses when the version already has a stable tag, or when the release commit is not on `origin/main`.
- **RF5.3** Pre-releases: an rc tag points at a release commit (files carry `X.Y.Z`, as `release-check` already expects). Fixes after `rc.1` land normally; `release prepare --refresh` on a new `release/vX.Y.Z` branch re-renders the **same** version's section (no bump) with the new commits and a new date, producing `chore(release): X.Y.Z (refresh)`; then `release tag --rc` (rc.2) or `release tag` (stable). Rc tags remain GitHub pre-releases that never reach package managers (PRD 0021 RF1.1).
- **RF5.4** `release.yml`'s `check` job additionally runs `release prepare --check` on the tagged commit and fails when any `vNEXT` remains in `docs/**/*.md` or `README.md`, so a tag can only publish a complete, consistent section.
- **RF5.5** Optional automation (owner decisions D6/D8): a `release-prepare.yml` `workflow_dispatch` that runs `prepare --branch`, pushes and opens the PR with a **GitHub App** token from a `release-pr` environment (a `GITHUB_TOKEN`-opened PR would not trigger CI); and an auto-tag job on pushes to `main` whose head is a release commit, with the same App token (a `GITHUB_TOKEN`-pushed tag would not trigger `release.yml`). Both are registered in `workflow-check`'s policy with least privilege. The recommended default is neither: the owner tags by hand (one command), which keeps the human gate PRD 0021 relies on.
- **Acceptance:** temp-repo tests for `release tag` (annotated, message = section, rc numbering, refusal on an existing stable tag and on a commit not on `main`); a fork rehearsal (`docs/releasing.md`, "Rehearsing in a fork") of the full flow ending in an rc pre-release.

### RF6 — Plans cite the release-to-be (medium priority)

- **RF6.1** A delivering commit records the version as the literal token `vNEXT` (for example `| RF2 | vNEXT | … |` in the plan's "Implementation status", and `RF2 (vNEXT)` in its `0000_control.md` §2 row), keeps its `Refs:` trailers, and updates the plan's status as today.
- **RF6.2** `release prepare` replaces every `vNEXT` in `docs/**/*.md` and `README.md` with `X.Y.Z`, sets `0000_control.md`'s "Last updated" and "Cuotai version at that date", and appends the §3 "Delivered by version" rows generated from the `Refs:` trailers of the released commits (`| X.Y.Z | 0029 | RF0, RF1 (abc1234, def5678). |`, one row per plan). Feature commits therefore stop editing §3, the table where parallel branches collide most.
- **RF6.3** `changelog-check` (RF7.8) accepts `vNEXT` anywhere on a branch or `main`; `release prepare --check` and RF5.4 reject it at a release.
- **RF6.4** Wording rule for plans: a version in a plan means "first **published** in"; the pre-cutover versions 0.1.0–0.77.x are kept as they are and called **internal versions** in the `CHANGELOG.md` preface (owner decision D11).
- **Acceptance:** unit tests for the replacement (only the exact token; `vNEXTx` and code spans in prose are untouched by the §3 generator), the §3 row generation (grouping, order, hashes) and the control-header update; plan-links test accepting `vNEXT`.

### RF7 — Branches, pull requests and `main` protection (high priority)

- **RF7.1** Every change is made on a branch (`<type>/<slug>`, for agents in their own worktree under `.claude/worktrees/`) and reaches `main` through a pull request. The shared main checkout is a read-only mirror of `origin/main` (fast-forward pulls only; nobody commits there). Several agents can work in parallel because nothing they commit conflicts on versioning.
- **RF7.2** Owner configures a **repository ruleset on `main`** (documented in `docs/releasing.md`, not applied from code): require a pull request (0 approvals, because the owner and the agents share one identity), require the `CI result` status check, require linear history (if D4 = rebase), block force pushes and deletion. **Bypass:** the repository admin role in `pull_request` mode only (amended 2026-10-23): the bypass applies when merging a PR and never to a direct push; RF2.2 still checks every push to `main`. Optionally enable code-scanning merge protection (D13).
- **RF7.3** Merge strategy (D4): **rebase and merge** recommended — each conventional commit keeps its message and trailers; hashes change at merge, which is harmless because `prepare` reads them from `main`. Merge commits also work (`--no-merges`). Squash is supported only if the PR title is conventional and the PR body holds the trailers (GitHub setting "Default to pull request title and description"); a PR-title check is then added to the gate.
- **RF7.4** **Tag rulesets on `v*`** (amended 2026-10-23): only the admin role may create matching tags. The agents act with the owner's token and create tags only under the release rules of `AGENTS.md`. Update and deletion are blocked with no bypass actor, so a published tag can never move, even for the owner. Where available, enable GitHub's immutable releases.
- **RF7.5** Agents: push the branch and open the PR (`gh pr create --fill`, or `--draft` while work is in progress, to get CI early) only when the user asked to push, as `AGENTS.md` already requires; enabling auto-merge (`gh pr merge --auto --rebase`) is allowed when the user asked to ship the change (requires "Allow auto-merge" in the repository settings).
- **Acceptance:** `docs/releasing.md` lists the exact ruleset settings; a dry check by the owner that a direct push without bypass is refused and a PR with a red `CI result` cannot merge.

### RF8 — CI workflows (high priority)

- **RF8.1** Triggers and where each workflow runs:

  | Workflow | Event | Runs | Notes |
  |---|---|---|---|
  | `ci.yml` | `pull_request` (to `main`) | `changes` → `test` (3 OS), `deny`, `version-check` when code changed; `changelog-check` + gate (RF2) always; `release-preflight` (RF8.3) when release tooling changed → `CI result` | The PR is the main quality gate. |
  | `ci.yml` | `push` to `main` | same jobs; gate on `before..after`; Rust/apt caches saved here only | Post-merge verification and cache warming; catches bypass pushes. |
  | `ci.yml` | `push` to other branches | nothing by default (D5) | Avoids double runs with `pull_request`; open a draft PR for CI. |
  | `ci.yml` | `workflow_dispatch` | full run on any ref | Manual CI on a branch without a PR. |
  | `ci.yml` | `merge_group` | full run | Only if the owner enables a merge queue. |
  | `ci.yml` | tags | nothing | Today the unfiltered `push:` also runs CI on tags; `release.yml` owns tags. |
  | `release.yml` | `push` tag `v*`, `workflow_dispatch` | unchanged, plus RF5.4 in `check` | |
  | `external-rules.yml` | monthly `schedule`, dispatch | unchanged | |
  | `release-prepare.yml` | `workflow_dispatch` | only if D6 is chosen | GitHub App token, `release-pr` environment. |

  Concretely: `on: { pull_request: {}, push: { branches: [main] }, workflow_dispatch: {}, merge_group: {} }`; the concurrency rule of `6638c69` stays (main runs keyed by commit and never cancelled; PR runs cancel superseded ones).
- **RF8.2** Documentation-only skip: keep the `changes` job and its allow-list as delivered (`6638c69`); this PRD only (a) adds a `release_tooling` output (`xtask/**`, `.github/workflows/**`, `scripts/release/**`, `packaging/**`, `apps/desktop/src-tauri/tauri*.json`, `deny.toml`) for RF8.3, and (b) optionally, after measuring the saving, splits `code` into `root` and `desktop` outputs so a TUI-only change skips the desktop clippy/test steps and a desktop-UI-only change skips the root workspace tests (`crates/cuotai-core/**` and `CHANGELOG.md` count for both). Under this PRD `CHANGELOG.md` changes only in release PRs, which always touch manifests, so its "code" classification costs nothing.
- **RF8.3** `release-preflight` job (read-only token, no secrets, Ubuntu, Rust cache restored from `main`): `xtask workflow-check`, `release-check`, `release prepare --dry-run` on a scratch clone, `bash scripts/release/test-collect-linux.sh`, and `package-manifests` rendering against the committed test fixture. It runs when `release_tooling` is true and on every release PR (`release/v*` head), where it also runs `release prepare --check`. The full artifact rehearsal stays manual (`workflow_dispatch` with `full_rehearsal`), within PRD 0021 RF9.5's free-allowance budget.
- **RF8.4** On pushes to `main`, a cheap step writes the would-be next section (`changelog-draft`) and the computed next version to the job summary, so "what is unreleased" is visible without a commit.
- **RF8.5** Caching and cost: Rust and apt caches keep `save-if: main` (PRs restore `main`'s); run `cargo fmt --check` once on Linux instead of on three OSes (formatting is OS-independent; clippy stays on all three because of `cfg`-gated code); every job keeps a timeout.
- **RF8.6** Required checks: only `CI result`, which now also aggregates the gate and `release-preflight` (a skipped job counts as passed, as today).
- **RF8.7** CodeQL: GitHub's default setup analyses pull requests once the PR flow exists, so `scripts/codeql-local.sh` moves from a mandatory pre-push step in `AGENTS.md` to a recommended one for security-sensitive changes; whether CodeQL results block merging is D13.
- **RF8.8** `xtask workflow-check` learns the new jobs and triggers (permissions `contents: read`, no secrets in `ci.yml`, no `pull_request_target`), so the policy guard keeps covering every workflow.
- **Acceptance:** `workflow-check` passes; a PR touching only `docs/` shows the heavy jobs skipped and `CI result` green; a PR touching `xtask/` runs `release-preflight`; a release PR runs `prepare --check`; a tag push starts no `ci.yml` run; a push to an unrelated branch starts no run.

### RF9 — Migration from today's rules (high priority)

- **RF9.1** **Cutover commit** (`chore(release-flow): …`, not user-visible, no bump) lands RF1–RF4, RF6 and RF8 together with the documentation: `AGENTS.md` "Versioning, Commits & Changelog" rewritten (Appendix A), `docs/release-policy.md` "Maintenance and changelog", `docs/releasing.md` "How a release works" (steps 1–2 become RF5.1) and the owner checklist (rulesets, optional App), `CONTRIBUTING.md`, and amendment notes in PRD 0019 (RF7.3, RF7.4 point here) and PRD 0021 (RF1.1, RF1.3).
- **RF9.2** **Baseline without a tag.** No tag is created at cutover (pushing a `v*` tag would start `release.yml`). The base is the last version-changing commit (RF3.1), i.e. the last per-commit bump before cutover (0.77.0 at `239aa90` today, or whatever is newest then). From the cutover on, the workspace version means "the last prepared release".
- **RF9.3** **Existing `[Unreleased]`.** It is empty on `main` today. If entries exist at cutover, `prepare` carries them verbatim into the first section (RF4.2) and the gate does not ask their commits for trailers (their hashes are covered).
- **RF9.4** **In-flight branches** written under the old rules drop their version and `CHANGELOG.md` hunks when rebasing onto the cutover and move the entry text into a `Changelog:` trailer (`git rebase -i` is not available to agents here: amend the commit message with `git commit --amend` on a single-commit branch, or rewrite with `git rebase --exec`). The gate's message for RF1.3 says so.
- **RF9.5** **Footer links.** `changelog-check` validates footer links: a version with a tag compares tags; a version without a tag compares commits. The cutover rewrites the links of 0.66.0–0.77.x (today pointing at non-existent tags or at `main`) to commit comparisons between the version-changing commits, and the preface sentence about tags is updated.
- **RF9.6** **First release.** Recommended (D9): after cutover, and once 0029 RF0 (the `feat` that makes it a minor) and the other wave-5a blockers have landed, the first `release prepare` produces the next version computed at cutover (expected `v0.80.0`, since `main` was at 0.79.0 when D9 was approved) and is tagged `v0.80.0-rc.1` first, so the never-exercised tag path (PRD 0021 prerequisites: `release` environment, updater key) is proven on a pre-release that cannot reach a package manager; then the final tag.
- **RF9.7** **Retroactive tags** (PRD 0021 RF1.3): recommended declined (D10). The internal versions keep commit-comparison links; tags start with the first real release.
- **Acceptance:** after the cutover, `cargo xtask release prepare --dry-run` on `main` shows the expected version and section; the CI of the cutover PR is green; `AGENTS.md` no longer mentions per-commit bumps.

### RF10 — Build identity between releases (low priority; may be deferred)

- **RF10.1** Because the version no longer moves per feature, a build from `main` between releases is indistinguishable from the release by its version. A `build.rs` in `cuotai-core` exposes `BUILD_DESCRIBE` (`git describe --tags --long --dirty`, or the short hash when no tag exists; empty when Git is unavailable, as in a crates.io build); `cuotai --version` and the desktop About show `0.78.0 (dev abc1234)` when the build is not exactly a release tag. Release builds (tag runs) show the plain version.
- **RF10.2** The version used for comparisons (updater, What's new, `export::v1`) stays the plain `X.Y.Z`; the suffix is display-only.
- **Acceptance:** unit test of the display formatting; a tag build in the fork rehearsal shows no suffix.

### RF11 — Plan-graph consistency check (medium priority; owner-approved 2026-10-23)

- **RF11.1** `cargo xtask changelog-check` (or a separate `plans-check` wired into the same CI job) proves that the three views of plan dependencies in `0000_control.md` agree: every strong dependency of a §2 row ("Depends on (strong)", plan numbers with optional requirement qualifiers) is a solid edge `PNN --> PMM` of the §4 mermaid graph, every solid edge is a §2 strong dependency, and the "What each plan unblocks" table lists, for each plan, exactly the plans whose strong dependencies name it (qualifiers such as `(RF6)` compared by plan number).
- **RF11.2** Problems name the plan, the view that disagrees and the missing or extra edge; weak edges (`-.->`) are allowed to be a subset of §2's weak dependencies, because the graph draws only the ones that affect a wave.
- **Acceptance:** fixture tests for a missing edge, an extra edge and a stale "unblocks" cell; the check passes on the current `0000_control.md`.

## 7. Non-functional requirements

- **NFR1 Determinism:** the same repository state and `--date` produce byte-identical files and commit messages (no wall-clock reads except the default date, no locale-dependent formatting).
- **NFR2 Offline and local:** `prepare`, `tag`, the gate and every check run without network (`cargo update --offline`); only the optional workflow of RF5.5 talks to GitHub.
- **NFR3 Cross-platform:** `xtask` runs on Linux, macOS and Windows, invokes `git`/`cargo` without a shell and handles CRLF commit messages.
- **NFR4 No new runtimes:** Rust only; no Node, Python or JavaScript action is added. New dependencies are limited to what line-targeted edits need (none expected; `toml_edit` only if line edits prove insufficient), each passing `cargo deny`.
- **NFR5 Safety:** `prepare` never pushes, tags or edits files outside the release file set; it refuses a dirty tree in those files; `tag` only creates a local annotated tag. Generated messages never contain GitHub's skip-CI markers.
- **NFR6 Least privilege:** `ci.yml` keeps `contents: read`, `pull_request` (never `pull_request_target`) and no secrets; any App token of RF5.5 lives in its own environment, available only to that job.
- **NFR7 Format compatibility:** every generated `CHANGELOG.md` passes `cuotai_core::changelog::parse` unchanged; the parser is not modified by this PRD.
- **NFR8 Messages:** English, actionable, naming the commit, file and rule; no tokens, URLs with credentials or response bodies.
- **NFR9 Cost:** CI minutes stay within the free allowance (PRD 0021 RF9.5); the PR flow adds at most one heavy run per merged PR compared with today's one run per push to `main`.

## 8. Data and contract changes

- `CHANGELOG.md`: format unchanged; `[Unreleased]` stays empty in the tree (the would-be section is visible through `changelog-draft` and the `main` job summary); footer links of untagged versions compare commits; preface updated (internal versions, tags start with the first release).
- Commit messages: new trailers `Changelog:` (sentence), `Changelog-Category:`, `Changelog-Edit:`, `Version-Override:` (release commits only); `Refs:` gains issue references.
- Plans: the `vNEXT` token; `0000_control.md` §3 rows generated at release.
- No change to the TOML, `--json`, `export::v1` or `desktop.json`; no user-visible change, so this PRD's own delivery bumps nothing.

## 9. Surfaces

- `AGENTS.md`, `CONTRIBUTING.md`, `docs/releasing.md`, `docs/release-policy.md`, `docs/architecture.md` (CI coverage paragraph).
- `xtask`: `commits.rs` (new), `release/{version,render,prepare,tag}.rs` (new), `changelog_gate.rs` (rewritten), `draft.rs` (becomes a view over `release/render.rs`), `check.rs` (`vNEXT` and footer checks), `plan_links.rs` (`vNEXT`, generated §3 rows), `workflow.rs` (new jobs/triggers), `main.rs` (subcommands).
- `.github/workflows/ci.yml` (triggers, `release_tooling` output, gate on push, `release-preflight`, summary, fmt once), `.github/workflows/release.yml` (`check` job only).
- Owner settings (not code): rulesets on `main` and `v*`, merge strategy, auto-merge, optional GitHub App and `release-pr` environment.

## 10. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| An agent forgets the `Changelog:` trailer | PR red | The gate (RF2) names the commit and the fix; `AGENTS.md` Appendix A gives the template; amending a branch commit is cheap before merge. |
| Poor wording frozen in a commit message | Weak release notes | The release PR is the editing point and `--refresh` keeps edits (RF4.6). |
| Squash merge drops per-commit trailers | Missing entries | Rebase or merge commits recommended (D4); squash only with conventional PR title and body (RF7.3); `prepare --check` fails on any uncovered user-visible commit. |
| Commits land on `main` while a release PR is open | Release tree contains unlisted changes | "Require branches to be up to date" in the ruleset forces a rebase; `prepare --check` in CI and in `release.yml` fails until `--refresh` covers them. |
| Owner bypass pushes skip PR CI | Untested change on `main` | Push CI on `main` plus the gate on `before..after` (RF2.2) turn it red immediately. |
| `GITHUB_TOKEN` cannot trigger workflows from a bot PR or tag | Release PR without CI, tag without release | Default flow is local `prepare` and a human-pushed tag; automation only with a GitHub App (RF5.5). |
| Long gaps between releases | Huge sections, delayed fixes | `main` job summary shows the pending section and next version (RF8.4); cadence is an owner decision (D8). |
| The cutover strands in-flight branches | Rebase friction once | RF9.4 procedure; the gate's RF1.3 message points to it. |
| Home-grown tooling grows | Maintenance load | One shared commit parser; golden and temp-repo tests; git-cliff or fragments remain documented fallbacks (§5). |
| Dev builds report the last release's version | Confusing bug reports | RF10 (display-only build suffix). |

## 11. Order of execution and verification

1. **RF1 + RF3** — `commits.rs` and the pure version function, unit tests only (no behavior change).
2. **RF4 + RF6** — `prepare` (with `--dry-run`, `--check`, `--refresh`) and the plan token, tested on temp repositories; run `--dry-run` on `main` to compare with what the old rules would have produced.
3. **RF2 + RF8 + RF9** — the cutover PR: the new gate, the CI triggers and jobs, and the documentation rewrite land together, so there is never a window in which neither rule set is enforced.
4. **RF7** — the owner applies the rulesets and the merge strategy right after the cutover merges.
5. **RF5** — `release tag`, the `release.yml` check additions, and the first release PR → `v0.80.0-rc.1` (expected; computed at cutover) → final tag (D9, adjusted).
6. **RF10** — delivered on 2026-10-25 (D12 lifted).

Verification: `cargo test -p xtask` (unit and temp-repo tests on three OSes), `cargo xtask workflow-check`, a fork rehearsal of RF5.1 end to end, and the owner checks of RF7.

## 12. Open questions (with recommendations)

- **Q1 Trailers or fragments for the entry text?** Recommendation: trailers (exact hash, no extra files, one source of truth). Revisit after two releases; if more than a few entries per release need rewording or are missing, switch to `changes/*.md` fragments, which `prepare` can read through the same renderer.
- **Q2 Should non-security dependency updates (`chore(deps)`) appear?** Recommendation: no entry and no bump, unless the commit carries a `Changelog:` sentence (for example a dependency fix users notice).
- **Q3 Add `perf`, `build` and `ci` types?** Recommendation: not now; `perf` with a `Changelog:` sentence already yields a patch-level **Changed** entry through RF3.2's "any other type" row if the type list is widened later.
- **Q4 Who may run `release prepare` and `release tag`?** Recommendation: `prepare` by the owner or by an agent the owner asks; `tag` by the owner only (enforced by the tag ruleset, RF7.4).
- **Q5 Should the release PR always run the full artifact rehearsal?** Recommendation: yes for the first three releases and for any release that touched release tooling; otherwise optional (cost, PRD 0021 RF9.5).
- **Q6 Is `vNEXT` acceptable in prose outside tables?** Recommendation: yes; `prepare` replaces it everywhere in `docs/**/*.md` and `README.md`.
- **Q7 Should `0000_control.md` §2 "Delivered" cells also be generated?** Recommendation: no; they are curated prose per plan and conflict rarely; only §3 is generated.
- **Q8 Merge queue?** Recommendation: not needed for one owner; the `merge_group` trigger costs nothing and is ready if wanted.

## 13. Owner decisions needed

| # | Decision | Recommendation |
|---|---|---|
| D1 | Tooling | Home-grown `cargo xtask release prepare` (git-cliff as fallback renderer). |
| D2 | Entry source | `Changelog:` commit trailers (fragments as fallback, Q1). |
| D3 | `main` protection | Ruleset: PR required, `CI result` required, linear history, no force push; admin bypass allowed for emergencies. |
| D4 | Merge strategy | Rebase and merge. |
| D5 | CI on branch pushes without a PR | No; PRs (draft PRs for early CI) plus `workflow_dispatch`. Alternative: run on every branch push and keep `pull_request` only for forks. |
| D6 | Who opens the release PR | Local `prepare --branch` + `gh pr create`; no GitHub App for now. |
| D7 | Full rehearsal on release PRs | Mandatory for the first three releases, then on release-tooling changes (Q5). |
| D8 | Tagging and cadence | Owner runs `cargo xtask release tag` and pushes; releases on demand, not on a schedule. |
| D9 | First release | Cutover first; then an RC of the next computed version, then the final tag. Alternative: tag the current release commit now under the old rules. |
| D10 | Retroactive tags (PRD 0021 RF1.3) | Decline; commit-comparison links for internal versions. |
| D11 | Meaning of pre-cutover versions in plans | Keep them as "internal versions", documented in the `CHANGELOG.md` preface. |
| D12 | Dev build identity (RF10) | Yes, low priority, after the first release. **Lifted 2026-10-25 (owner): implemented before the first release.** |
| D13 | CodeQL blocks merges | Not at first; enable code-scanning merge protection for high/critical alerts once the PR flow has run for a few weeks. |

**Pre-release history (owner decision, 2026-10-23).** D11 is refined: the internal versions 0.1.0–0.79.0 no longer have one `CHANGELOG.md` section each. One last section, `## Pre-release history (0.1.0–0.79.0, internal builds, never published)`, summarizes them by area, and their footer links are gone, so the first published version, v0.80.0, is the first versioned section. The shared parser reads that section as a distinct entry, not a version. `changelog-check` accepts plan and §3 rows for internal versions through its range and rejects a range past 0.79.0. `release-check` accepts the internal workspace version before the first release but never tags it. The commit gate lets a `Changelog-Edit` commit fold internal versions into it, but never remove it, re-range it or fold a published version. `release prepare` still takes the internal 0.79.0 baseline as its base (RF3.1) and writes the next section above it. The What's new screen shows it last, and `cuotai --changelog 0.5.0` prints it after a note (exit 0). The §3 rows of `0000_control.md` stay the per-version detail.

**Agentic releases (owner decision, 2026-10-23).** D8 is refined: agents may also prepare releases and create and push tags, but only when the owner asked for that release in the current conversation. Before tagging they verify and report that `main` is fast-forwarded to `origin/main`, that `HEAD` is the merged release commit, that `CI result` is green on it, and that `release-check` and `release prepare --check` pass. The annotated tag comes only from `cargo xtask release tag [--rc]`, and only that one tag is pushed. A `v*` tag is never moved, deleted or force-pushed; a failed release is followed by a new version. The rc rule of D7 holds for the first three releases (`AGENTS.md`, "Release & tag rules for agents"). The rulesets change accordingly:

- `main` gets the admin bypass in `pull_request` mode only: nobody pushes to `main` directly.
- `v*` tag creation is limited to the admin role, which the agents also hold through the owner's token.
- `v*` tag update and deletion have **no** bypass actors: tags are immutable for everyone.

**Decision record (2026-10-23).** The owner **approved all recommendations D1–D13** with two adjustments:

- **D9 adjusted.** `main` is already at 0.79.0, so the tags `v0.78.0-rc.1` and `v0.78.0` no longer apply. After the cutover, the first release is a release candidate of the next version computed at cutover time by RF3 (expected `v0.80.0-rc.1`), followed by the final tag of that version once the owner verifies the candidate.
- **D3 addition.** `AGENTS.md` documents the new way of working explicitly (commit contract, `Changelog:` trailers, branches and pull requests, the `main` ruleset, `vNEXT` in plans, and that agents never bump versions or edit released changelog sections). This is done in the cutover pull request (RF9), using Appendix A as the draft.

## 14. Relationship to other plans

- **PRD 0019:** RF7.1, RF7.2, RF7.5–RF7.8 stay as they are. RF7.3 (maintenance flow: "every user-visible delivery updates `[Unreleased]` in the same commit") and RF7.4's first bullet (the PR must change `CHANGELOG.md`) are **superseded** by RF1, RF2 and RF4 here; RF7.3's evaluation of git-cliff versus an own `xtask` is decided in §5. RF2 (CI on every push and PR) is refined by RF8.1.
- **PRD 0021:** RF1.1 stays (annotated tag on `main`); this PRD defines how the tagged commit is produced (RF4, RF5) and adds `prepare --check` to the `check` job (RF5.4). RF1.3 (retroactive tags) is answered by D10. RF9.2/RF9.5 bound RF5.5 and RF8.
- **Documentation-only CI skip (`6638c69`):** extended (RF8.2 outputs, RF8.6 aggregate), not replaced.
- **Every later feature PRD:** delivers with trailers and `vNEXT` instead of version bumps, which is why this plan goes before the next feature wave.

## Appendix A — Draft of the new `AGENTS.md` "Versioning, Commits & Changelog" section

> Versions and the changelog are produced at release time. A feature or fix commit never changes a version or `CHANGELOG.md`.
>
> - **Commit format:** `type(scope): subject` in English (types `feat`, `fix`, `docs`, `refactor`, `test`, `style`, `chore`, `revert`; `!` for breaking). The body explains the change. Trailers, last paragraph: `Changelog: <one user-facing sentence ending with a period>` for every user-visible change (or `Changelog: skip <reason>`); optional `Changelog-Category: Changed|Removed|Deprecated|Security|Fixed|Added`; `Refs: PRD 0006 RF2` (one line per requirement, also `Refs: #12`); `BREAKING CHANGE: <migration>` for a break of the TOML, `--json`, `export::v1` or `desktop.json`. Keep the `Co-Authored-By` trailer the session requires. Never write GitHub's skip-CI markers in a message.
> - **Never in a feature/fix commit:** the version in `Cargo.toml`, `apps/desktop/src-tauri/Cargo.toml`, `tauri.conf.json`, the `cuotai-core` requirement, the lockfiles' workspace entries, or `CHANGELOG.md`. CI rejects it.
> - **Plans:** a delivering commit writes `vNEXT` where the version goes in the plan's "Implementation status" and its `0000_control.md` row, and carries the `Refs:` trailers; the release fills in the version and the "Delivered by version" rows.
> - **Branches:** work on a branch in your own worktree; never commit in the shared main checkout. Push and open a PR (`gh pr create --fill`) only when the user asks; CI must be green to merge (rebase and merge).
> - **Releases (only when the owner asks):** `cargo xtask release prepare --branch` → push → release PR → merge → the owner runs `cargo xtask release tag` and pushes the tag. Do not tag.
> - **Shared worktree:** stage by explicit path, never `git add -A`, never include files you did not change.
