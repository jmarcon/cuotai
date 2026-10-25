# Repository Guidelines

> **Workflow changed on 2026-10-23 (PRD 0039).** Feature and fix commits no longer raise the version or edit `CHANGELOG.md`. Every change goes through a branch and a pull request that must pass `CI result`. The entry text goes in a `Changelog:` trailer of the commit message. Versions and the changelog are produced at release time by `cargo xtask release prepare`, and the tag by `cargo xtask release tag`. The owner runs them, or an agent does when the owner asks for that release (see "Release & tag rules for agents"). CI rejects version bumps and `CHANGELOG.md` edits outside a release commit. Read "How to deliver a change" before your next commit.

## Project Overview

Cuotai is a read-only Rust monitor for AI-provider usage, quotas, and costs. It exposes a reusable async backend and a Ratatui terminal dashboard; the CLI also emits text or JSON snapshots. Provider interfaces can be undocumented or bill per query (notably AWS Cost Explorer), so preserve their documented limitations.

## Architecture & Data Flow

- `crates/cuotai-tui/src/main.rs` parses CLI flags, loads TOML through core `config_file::ConfigFile`, creates an `OAuthSession`, then runs either `Monitor::refresh()` once or the interactive loop in `ui.rs`.
- `cuotai-core` is UI-independent. `lib.rs` validates `Config`, owns the shared HTTP client and query semaphore, and returns a `Snapshot` in account order. One account failure becomes a report rather than aborting the whole refresh.
- `model.rs` defines `Provider`, `Account`, `Metric`, `Report`, `Status`, and `Snapshot`. Provider dispatch in `lib.rs` calls private `providers/*` fetchers through shared `Context` (`http`, `credentials`, `timeout`); fetchers normalize responses into metrics/notes or `QueryError`. Add a provider at the enum, dispatch, adapter, **access descriptor** (mandatory: its arm in the exhaustive `access::classify` match plus `access.summary.<slug>`/`access.reason.<slug>` in the core catalogs; then regenerate the `docs/providers.md` matrix with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix`, whose plain run fails on drift), and example config; the dashboard renders reports generically. See "Adding a provider" in `docs/architecture.md`.
- Backend (`cuotai-core`): `oauth/` owns login/refresh/token storage; `account_edit.rs` publishes provider field/native-login descriptors, conditional visibility and draft validation, plus account create/update/remove operations; `config_file.rs` owns config discovery and atomic TOML persistence; `refresh.rs` owns effective per-account deadlines and incremental snapshot assembly; `metrics.rs` derives quota classification. `Credentials` injects secret lookup. Keep credentials out of snapshots and TOML.
- Frontend (`cuotai-tui`): `wizard.rs` maps core descriptors into widgets, collects field values and keeps transient draft/navigation state; `ui.rs` decides when to request refreshes and presents core `RefreshCycle`/`Monitor` results, invokes core account operations and `ConfigFile`; `auth_ui.rs` renders login progress. Do not define provider form rules, refresh policy, snapshot merge rules, queries, account policy, or TOML persistence in TUI.

## Key Directories

- `crates/cuotai-core/src/providers/`: provider-specific HTTP/CLI integrations; `router_*.rs` contain 9router-derived contracts.
- `crates/cuotai-core/src/oauth/`: OAuth/device/browser flows and system/dev token stores.
- `crates/cuotai-tui/src/`: CLI, dashboard rendering, wizard input, terminal event loop; `apps/desktop/` is a separate core consumer for Tauri presentation.

## Codebase Memory

For every repository/code query, use the `codebase-memory-mcp` MCP server first: `list_projects` checks the index; `index_repository` indexes it if missing; `search_code` handles text lookup, while `search_graph`, `trace_path`, and `detect_changes` handle structure, call chains, and impact. Verify graph results with direct source reads before editing. Cuotai was indexed as `Volumes-Work-Pessoal-Cuotai`; check the index rather than assuming it is current.

If its tools are absent in OMP, check `/mcp list`. The executable is `codebase-memory-mcp` (not `codebase-memory`); register it as a `stdio` server in the active profile's `~/.omp/agent/mcp.json` (or `.omp/mcp.json` for project scope), e.g. `{"mcpServers":{"codebase-memory-mcp":{"type":"stdio","command":"codebase-memory-mcp"}}}`. If listed but disabled, enable it with `/mcp enable codebase-memory-mcp`; run `/mcp reload`, then `/mcp test codebase-memory-mcp` and retry `list_projects`. Restart OMP if the current session still lacks the tools. If the MCP remains unavailable, use repository reads/LSP and explicitly report that the graph was not consulted; claim indexing or queries only after successful tool output.

## Development Commands

Run from the workspace root:

```sh
cargo run -p cuotai-tui -- --list-providers
cargo run -p cuotai-tui -- --config cuotai.toml --json
cargo run -p cuotai-tui                    # interactive TUI
sh run.sh [tui|gui]                        # dev build with the debug-only SQLite credential store
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo fmt --all -- --check
```

Use `--once` instead of `--json` for human-readable, single-refresh output (`--json-v1` prints the `export::v1` contract). `--once`/`--json`/`--json-v1` return 0 for all-ready, 2 for account failures, and 1 for local errors (invalid arguments included). `cuotai --serve` (`crates/cuotai-tui/src/serve.rs`) runs the core's local server in the foreground over a headless refresh cycle and binds exactly `--port` (or 6738, a taken port is a local error), and returns 0 on `SIGINT`/`SIGTERM` whatever the accounts' state (`--strict-exit` restores 2 when no account was healthy until shutdown; the same flag keeps 2 for `--watch` with a failing account), 1 for local errors.

**Before pushing:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` (plus `cargo clippy --all-targets -- -D warnings` and `cargo test` in `apps/desktop/src-tauri` when the desktop is touched); the commit gate and `changelog-check` from "How to deliver a change". For security-sensitive changes, also run `sh scripts/codeql-local.sh` (recommended, not required: CodeQL runs on every pull request). It runs the same suites as `.github/workflows/codeql.yml` for `rust python javascript-typescript actions` (pass languages to narrow; `--swift` is opt-in, macOS-only and needs full Xcode selected with `xcode-select`, not just the Command Line Tools), keeps databases/SARIF in `$CUOTAI_CODEQL_DIR` (default: a fresh `$TMPDIR/cuotai-codeql.XXXXXX` per run, so concurrent runs do not collide), and exits 0 clean, 3 with findings, 1 on tool errors. Rust takes several minutes; results can differ slightly from GitHub's (CLI/query versions). CodeQL on GitHub is the advanced-setup workflow `.github/workflows/codeql.yml` (the repository's code-scanning "default setup" is off, since it cannot skip documentation-only changes): it skips pushes and pull requests that touch only the documentation allow-list of `ci.yml`, and also runs weekly. Keep the `paths` list of `codeql.yml` equal to that allow-list.

## Code Conventions & Common Patterns

- Rust 2021, conventional `rustfmt`, `snake_case` modules/functions/provider IDs, `PascalCase` types. Follow neighboring adapter/test patterns rather than adding UI behavior to core.
- Validate at boundaries (`Config::validate`, provider params); classify missing configuration, authentication/rate limits, transport failures, and incompatible schemas. Never turn absent usage into zero or expose response bodies, URLs, or tokens in errors.
- Async work uses Tokio/reqwest; `Monitor::refresh()` bounds concurrency while preserving configuration order. UI refresh/login work runs separately from navigation. Share an `OAuthSession` to serialize token rotation within a process.
- Config stores environment-variable names (`credential_env = "OPENAI_ADMIN_KEY"`) or OAuth credential references, not secrets. `ConfigFile` preserves unrelated TOML/comment content and rejects conflicting external edits.

## Important Files

`Cargo.toml` defines the two-crate workspace; `cuotai.example.toml` documents account/provider parameters; `README.md` is the end-user entry point (what the app does, install by OS, desktop tour, TUI and theme previews, how to help) and links the technical docs: supported integrations and auth caveats in `docs/providers.md` and `docs/oauth.md`, keybindings and CLI flags in `docs/tui.md`, scripting/config recipes in `docs/advanced.md`, building in `docs/building.md`, install channels and verification in `docs/installing.md`, desktop guides in `docs/gui-features.md` and `docs/gui-{macos,windows,linux}.md`, themes in `docs/themes.md` (screenshots under `docs/assets/screenshots/`; never include real accounts or tokens); `THIRD_PARTY_NOTICES` records adapted upstream contracts. Start with `crates/cuotai-core/src/lib.rs` and `model.rs` for backend changes, `crates/cuotai-tui/src/main.rs` and `ui.rs` for CLI/TUI changes. `docs/design-system.md` holds the normative desktop UI rules (tokens, components, charts, wording, motion; colors only as theme roles) that every `apps/desktop/ui` change must follow.

## Runtime/Tooling Preferences

- Use the repository-pinned Rust 1.99.1 toolchain (`rust-toolchain.toml`). No JavaScript package manager is used. Default OAuth storage uses the OS credential vault; AWS Bedrock needs AWS CLI v2 and an explicitly configured profile. Config lookup (CLI/TUI): `--config`, then `CUOTAI_CONFIG`, then an existing `./cuotai.toml`, then `<OS config dir>/cuotai/config.toml`. The desktop app reads `CUOTAI_CONFIG`, then the file chosen in its settings, then the OS path, and never `./cuotai.toml`. `.env` is not loaded. `run.sh` selects `--dev-credentials-sqlite ./cuotai-dev.sqlite3`: this debug-only store contains plaintext tokens; release builds reject the flag. Local `cuotai.toml`, `.env*`, SQLite files, and `target/` are git-ignored; do not publish secrets.

## Documentation Language

The README and all repository documentation are written in **English**, the norm for open-source repositories: `README.md`, `docs/` (including the PRDs in `docs/plans/`), `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, issue templates and code comments.

- The product stays multilingual: UI strings live in the Portuguese, English, Spanish and Italian catalogs, and core `Localized` fallbacks stay in Portuguese by design.
- Semantic commits (subject and body), pull request titles and descriptions, and issue text are also written in **English** from now on; see "Versioning, Commits & Changelog" below.
- Write every new document in English; every existing document has already been migrated. Never mix languages inside one file.

## How to deliver a change

Every change reaches `main` through a pull request whose `CI result` check is green. Versions and `CHANGELOG.md` are produced only at release time. Follow these steps for every change:

1. **Branch.** Work in your own worktree, branched from an up-to-date `origin/main` (`git fetch origin` first). Name the branch `<type>/<slug>`, for example `feat/account-sorting` or `fix/serve-token-path`. Never commit in the shared main checkout: it is a read-only mirror of `origin/main` (fast-forward pulls only).
2. **Commit.** Use a conventional commit in English, with the trailers in the last paragraph:

   ```text
   feat(tui): sort accounts by name

   What changed and why, in English.

   Changelog: You can sort the dashboard accounts by name with the s key.
   Refs: PRD 0006 RF2
   Co-Authored-By: <the trailer your session requires>
   ```

   - `Changelog:` is one sentence for the user: it starts with a capital letter, ends with a period, and contains no `PRD` reference and no commit hash, because the release adds both. Use one `Changelog:` line per entry.
   - A `feat`, `fix`, `revert`, security or breaking commit that touches `crates/`, `apps/`, `LICENSE-*` or `THIRD_PARTY_NOTICES` needs a `Changelog:` sentence, or `Changelog: skip <reason>` when users see no change.
   - Optional trailers: `Changelog-Category: Added|Changed|Deprecated|Removed|Fixed|Security` overrides the default category; `Refs: PRD 0006 RF2` (one line per requirement, or `Refs: #12` for an issue); `BREAKING CHANGE: <migration>` for a break of the TOML, `--json`, `export::v1` or `desktop.json`.
3. **Do not touch versions or the changelog.** Leave alone the `version` in `Cargo.toml`, `apps/desktop/src-tauri/Cargo.toml` and `tauri.conf.json`, the `cuotai-core` requirement in `crates/cuotai-tui/Cargo.toml`, the workspace entries in both lockfiles, and `CHANGELOG.md`. CI rejects such changes. In plans, write the token `vNEXT` where the version goes (the plan's "Implementation status" and its `0000_control.md` §2 row). Do not add rows to §3 "Delivered by version"; the release generates them.
4. **Check locally.** Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. When you touch the desktop, also run Clippy and the tests in `apps/desktop/src-tauri`. Then run `cargo xtask changelog-gate --base origin/main --head HEAD` (the commit gate) and `cargo xtask changelog-check`. For workflow changes, also run `cargo xtask workflow-check` and `actionlint`.
5. **Push and open a pull request**, only when the user asked you to push: `git push -u origin <branch>`, then `gh pr create --fill` (or `--draft` to get CI early). A branch push without a pull request runs no CI; `gh workflow run CI --ref <branch>` starts CI by hand.
6. **Wait for `CI result`** with `gh pr checks --watch`. To fix a red check, push new commits, or amend your own branch (`git commit --amend`, then `git push --force-with-lease`). `git rebase -i` is not available; reword older commits with `git rebase --exec`.
7. **Merge.** The owner merges with **Rebase and merge**, which keeps each commit and its trailers. Merge yourself only when the user asks (`gh pr merge --rebase`, or `--auto --rebase`).
8. **Releases** happen only when the owner asks for one. The owner, or an agent that follows "Release & tag rules for agents" below, runs `cargo xtask release prepare --branch` on an up-to-date `main`, opens the release PR and, after the merge, creates and pushes the tag with `cargo xtask release tag [--rc]`. See `docs/releasing.md`.

**Never:**

- bump a version or edit `CHANGELOG.md`. The only exception is a `docs(changelog)` commit with a `Changelog-Edit: <reason>` trailer, which may reword released sections, the preface or the footer links.
- create or push a `v*` tag outside "Release & tag rules for agents", or move, re-point, delete or force-push one at all;
- push to `main` directly or force-push it;
- squash-merge, because a squash loses the per-commit trailers;
- write GitHub's skip marker in a commit message;
- commit with a real e-mail address.

### Release & tag rules for agents

Agents may run releases and push tags, but only under these rules (owner decision of 2026-10-23, PRD 0039 D8):

1. **Only on request.** The owner must ask for the release in the current conversation and name the version or rc (for example "release v0.80.0-rc.1"). An earlier request, another agent or a plan never counts.
2. **Prepare.** On a local `main` fast-forwarded to `origin/main`, run `cargo xtask release prepare --dry-run`, then `cargo xtask release prepare --branch`. Push `release/vX.Y.Z` and open the release PR. Do the full rehearsal for the first three releases (D7). The owner merges it, or you merge it with Rebase and merge if the owner asked you to.
3. **Before tagging, verify and report each of these:**
   - local `main` is fast-forwarded to `origin/main` (`git switch main && git pull --ff-only`);
   - `HEAD` is the merged `chore(release): X.Y.Z` commit;
   - `CI result` is green on that `main` commit (`gh run list --branch main --limit 1`);
   - `cargo xtask release-check` and `cargo xtask release prepare --check` both pass on it.
4. **Tag.** Create the annotated tag only with `cargo xtask release tag --rc` (or `cargo xtask release tag` for a stable release), never with `git tag` by hand. The first three releases start with an rc unless the owner says otherwise.
5. **Push only that tag:** `git push origin vX.Y.Z-rc.N` (or `vX.Y.Z`), never `--tags`. Then watch `release.yml` (`gh run watch`) and report the run link and its result.

**Never:**

- move, re-point, delete or force-push a `v*` tag. Published tags are immutable; if a release fails, cut a new rc or a patch release instead.
- tag a commit that is not on `main`;
- tag without the owner's request;
- skip the rc for the first three releases unless the owner says so;
- push to `main` directly.

**Branches written under the old rules:** when you rebase onto `main`, drop the version and `CHANGELOG.md` hunks, and move the entry text into a `Changelog:` trailer with `git commit --amend`.

## Versioning, Commits & Changelog

Reference for the steps above (PRD 0039). Versions and `CHANGELOG.md` are produced once per release by `cargo xtask release prepare`, from the commits since the last release.

- **Commit format:** `type(scope): subject`, with the subject **and the body in English** (earlier history is in Portuguese without accents; it stays as it is, with no rewriting). Types: `feat`, `fix`, `docs`, `refactor`, `test`, `style`, `chore`, `revert`. A `!` marks a breaking change. The trailers are listed in "How to deliver a change". `Version-Override: X.Y.Z` appears only in release commits. Keep the `Co-Authored-By` trailer the session requires. Commit or push only when the user asks.
- **Commit identity:** never commit, tag or co-author with anyone's real e-mail address. Author and committer are the author's name plus a GitHub noreply address. For the owner this is `Juliano Marcon <1767705+jmarcon@users.noreply.github.com>`, already set as the repo-local `user.email`. Bot trailers such as `noreply@anthropic.com` are fine. Check `git log -1 --format='%an <%ae> / %cn <%ce>'` before pushing. Fixtures, docs and examples use RFC 2606 placeholders (`example.com`); `xtask/tests/privacy.rs` fails on personal-mailbox addresses in tracked files.
- **Which bump** (semantic versioning, still `0.x`; computed by `release prepare` as the maximum over the commits):

  | Commit | Bump | Default category |
  |---|---|---|
  | Breaking (`!` or `BREAKING CHANGE:`) | minor (major from 1.0) | from its type, marked **Breaking** |
  | `feat` | minor | Added |
  | `fix`, `revert` | patch | Fixed |
  | `fix(deps)`, `fix(security)`, `chore(security)` | patch | Security |
  | Any other type with a `Changelog:` sentence (license, About metadata) | patch | Changed |
  | Anything else, or `Changelog: skip` | none | none |

- **Where the version lives** (written only by `release prepare`): `Cargo.toml` (`[workspace.package]`), `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, the `cuotai-core` requirement in `crates/cuotai-tui/Cargo.toml`, and both lockfiles. `cargo xtask release-check` proves they agree.
- **Changelog:** the release renders `## [X.Y.Z] - YYYY-MM-DD` with Keep a Changelog categories, one entry per `Changelog:` sentence, and the references plus the real short hash in parentheses. `[Unreleased]` stays empty in the tree. To see what is pending, run `cargo xtask changelog-draft`, or read the job summary of the latest `main` CI run. Wording is fixed in the release PR.
- **Plans:** a delivering commit carries `Refs:` trailers, updates the plan's status, and writes `vNEXT` in its "Implementation status" and in its `0000_control.md` §2 row. The release replaces `vNEXT` with the version and appends the §3 rows. Versions 0.1.0 to 0.79.0 are internal versions: they were never tagged or published.
- **Skipping CI:** a pull request or `main` push that changes only documentation (`README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `AGENTS.md`, `apps/desktop/README.md`, `docs/**/*.md`, `docs/assets/**`, `.github/ISSUE_TEMPLATE/**`, `.github/*.md`) is detected automatically by the `changes` job of `ci.yml`. The build, test, deny and version jobs are skipped, but `changelog-check`, the commit gate and the privacy guard still run. `CHANGELOG.md`, `docs/providers.md`, `docs/*.json`, `LICENSE-*`, `THIRD_PARTY_NOTICES`, `apps/desktop/ui/**` and workflow files count as code. Do not use GitHub's skip marker. It skips every workflow, the gates included, and GitHub matches it anywhere in a commit message, so write "the skip marker" instead of the marker itself. The required check is `CI result`, not the individual jobs.
- **Releases and tags:** only when the owner asks for that release, following "Release & tag rules for agents" above. Tags are annotated, created by `cargo xtask release tag`, pushed one at a time, and never moved or deleted.
- **Shared worktree:** another agent may leave uncommitted work in the tree. Stage by explicit path, never `git add -A`, and never include files you did not change.

## Testing & QA

Tests live beside Rust modules in `#[cfg(test)]` blocks and OAuth test modules. Core provider/OAuth contract tests use local `wiremock` HTTP fixtures; TUI tests cover dashboard classification and config persistence. Run `cargo test --workspace`, Clippy, formatting, and a relevant CLI/TUI smoke for changed behavior. Keep provider fixtures deterministic, exercise missing/partial values and error classification, and do not treat mocked HTTP as proof of real-account access. No coverage threshold is configured. `.github/workflows/ci.yml` gates every pull request and every push to `main` with Clippy and tests of both workspaces on Linux, macOS and Windows, fmt once on Linux, `cargo deny`, the commit gate, and the release preflight when release tooling changes; tests that need Unix permissions, symlinks or `#!/bin/sh` fakes are `#[cfg(unix)]` (coverage per OS in `docs/architecture.md`).
