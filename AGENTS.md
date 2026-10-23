# Repository Guidelines

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
sh run.sh                                  # debug-only SQLite credential store
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo fmt --all -- --check
```

Use `--once` instead of `--json` for human-readable, single-refresh output (`--json-v1` prints the `export::v1` contract). `--once`/`--json`/`--json-v1` return 0 for all-ready, 2 for account failures, and 1 for local errors (invalid arguments included). `cuotai --serve` (`crates/cuotai-tui/src/serve.rs`) runs the core's local server in the foreground over a headless refresh cycle and returns 0 on `SIGINT`/`SIGTERM`, 2 when no account was healthy until shutdown, 1 for local errors.

**Before pushing:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` (plus `cargo clippy --all-targets -- -D warnings` and `cargo test` in `apps/desktop/src-tauri` when the desktop is touched); the version `git grep` from "Versioning" when the version moved; then `sh scripts/codeql-local.sh`, which runs GitHub's code-scanning suites for `rust python javascript-typescript actions` (pass languages to narrow; `--swift` is opt-in, macOS-only and needs full Xcode selected with `xcode-select`, not just the Command Line Tools), keeps databases/SARIF in `${CUOTAI_CODEQL_DIR:-$TMPDIR/cuotai-codeql}`, and exits 0 clean, 3 with findings, 1 on tool errors. Rust takes several minutes; results can differ slightly from GitHub's default setup (CLI/query versions).

## Code Conventions & Common Patterns

- Rust 2021, conventional `rustfmt`, `snake_case` modules/functions/provider IDs, `PascalCase` types. Follow neighboring adapter/test patterns rather than adding UI behavior to core.
- Validate at boundaries (`Config::validate`, provider params); classify missing configuration, authentication/rate limits, transport failures, and incompatible schemas. Never turn absent usage into zero or expose response bodies, URLs, or tokens in errors.
- Async work uses Tokio/reqwest; `Monitor::refresh()` bounds concurrency while preserving configuration order. UI refresh/login work runs separately from navigation. Share an `OAuthSession` to serialize token rotation within a process.
- Config stores environment-variable names (`credential_env = "OPENAI_ADMIN_KEY"`) or OAuth credential references, not secrets. `ConfigFile` preserves unrelated TOML/comment content and rejects conflicting external edits.

## Important Files

`Cargo.toml` defines the two-crate workspace; `cuotai.example.toml` documents account/provider parameters; `README.md` is the end-user entry point (what the app does, install by OS, desktop tour, TUI and theme previews, how to help) and links the technical docs: supported integrations and auth caveats in `docs/providers.md` and `docs/oauth.md`, keybindings and CLI flags in `docs/tui.md`, scripting/config recipes in `docs/advanced.md`, building in `docs/building.md`, install channels and verification in `docs/installing.md`, desktop guides in `docs/gui-features.md` and `docs/gui-{macos,windows,linux}.md`, themes in `docs/themes.md` (screenshots under `docs/assets/screenshots/`; never include real accounts or tokens); `THIRD_PARTY_NOTICES` records adapted upstream contracts. Start with `crates/cuotai-core/src/lib.rs` and `model.rs` for backend changes, `crates/cuotai-tui/src/main.rs` and `ui.rs` for CLI/TUI changes.

## Runtime/Tooling Preferences

- Use the repository-pinned Rust 1.99.1 toolchain (`rust-toolchain.toml`). No JavaScript package manager is used. Default OAuth storage uses the OS credential vault; AWS Bedrock needs AWS CLI v2 and an explicitly configured profile. Config lookup: `--config`, `./cuotai.toml`, then the OS config directory. `.env` is not loaded. `run.sh` selects `--dev-credentials-sqlite ./cuotai-dev.sqlite3`: this debug-only store contains plaintext tokens; release builds reject the flag. Local `cuotai.toml`, `.env*`, SQLite files, and `target/` are git-ignored; do not publish secrets.

## Documentation Language

The README and all repository documentation are written in **English**, the norm for open-source repositories: `README.md`, `docs/` (including the PRDs in `docs/plans/`), `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, issue templates and code comments.

- The product stays multilingual: UI strings live in the Portuguese, English, Spanish and Italian catalogs, and core `Localized` fallbacks stay in Portuguese by design.
- Semantic commits (subject and body), pull request titles and descriptions, and issue text are also written in **English** from now on; see "Versioning, Commits & Changelog" below.
- Write every new document in English; every existing document has already been migrated. Never mix languages inside one file.

## Versioning, Commits & Changelog

Every user-visible delivery is one semantic commit that also raises the version. Do not leave the bump for a later commit.

- **Commit format:** `type(scope): subject`, with the subject **and the body in English** (earlier history is in Portuguese without accents; that stays as it is, with no rewriting). Types: `feat`, `fix`, `docs`, `refactor`, `test`, `style`, `chore`. The body explains what changed for the user and ends with `Version X.Y.Z.` when the version moved. Keep the `Co-Authored-By` trailer the session requires. Commit or push only when the user asks.
- **Commit identity:** never commit, tag or co-author with anyone's real e-mail address. Author and committer are the author's name plus a GitHub noreply address — for the owner, `Juliano Marcon <1767705+jmarcon@users.noreply.github.com>`, already set as the repo-local `user.email`; bot trailers such as `noreply@anthropic.com` are fine. Check `git log -1 --format='%an <%ae> / %cn <%ce>'` before pushing. Fixtures, docs and examples use RFC 2606 placeholders (`example.com`); `xtask/tests/privacy.rs` fails on personal-mailbox addresses in tracked files.
- **Which bump** (semantic versioning, still `0.x`): `feat` raises the minor (`0.17.0` → `0.18.0`); `fix` and dependency or security fixes (`chore(security)`, `fix(deps)`) raise the patch; a breaking change to the TOML, `--json`, `export::v1` or `desktop.json` raises the minor while in `0.x` and carries a `BREAKING CHANGE:` footer with the migration; a change to distributed metadata that the app displays (license, About) also raises the patch. `docs`, `test`, `style`, `refactor` and `chore` without user-visible effect do not change the version.
- **Where the version lives (all in the same commit, all equal):** `Cargo.toml` (`[workspace.package]`), `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, and both lockfiles (`Cargo.lock` and `apps/desktop/src-tauri/Cargo.lock`). The desktop is a separate workspace: run `cargo check` there as well so its lockfile follows. Verify with `git grep -n '"0\.[0-9]*\.[0-9]*"\|^version = "0\.' -- Cargo.toml apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/tauri.conf.json` before committing.
- **Plans:** a commit that delivers a requirement of a PRD in `docs/plans/` adds the footer `Refs: PRD 0006 RF2` (one line per requirement), updates that plan's status and "Implementation status" with the version, and updates `docs/plans/0000_control.md` (status, "Delivered by version", history) in the same commit.
- **Changelog:** every user-visible `feat` or `fix` updates the `[Unreleased]` section of `CHANGELOG.md` in the same commit (Keep a Changelog categories; one sentence from the user's point of view, then the PRD/RF reference and the short commit hash in parentheses (a commit cannot cite its own hash: leave it out and let the next commit that touches the changelog fill it in), with the **Breaking** marker and migration when it breaks compatibility). A commit that raises the version moves the matching entries from `[Unreleased]` into a new `## [X.Y.Z] - YYYY-MM-DD` section (UTC) and updates the footer links. A commit that is not user-visible and skips it states `Changelog: skip` with the reason. Security fixes in dependencies go under **Security**. `0000_control.md` keeps the plan-requirement view of each version.
- **Skipping CI:** a push or pull request that changes only documentation (`README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `AGENTS.md`, `docs/**/*.md`, `docs/assets/**`, `.github/ISSUE_TEMPLATE/**`) is detected automatically by the `changes` job of `ci.yml`: the build, test, deny and version jobs are skipped, while `changelog-check` and the privacy guard still run. `CHANGELOG.md`, `docs/providers.md`, `docs/*.json`, `LICENSE-*`, `THIRD_PARTY_NOTICES`, `apps/desktop/ui/**` and workflow files count as code. `[skip ci]` in the commit message is GitHub's manual escape hatch (it skips every workflow that push would start, the changelog and privacy guards included): use it only on documentation commits, never on source changes. GitHub matches the marker anywhere in the commit message, so a commit that merely talks about it (even in backticks) skips CI too: write "the skip marker" instead. Require the `CI result` check, not the individual matrix jobs, in branch protection.
- **Shared worktree:** another agent may leave uncommitted work in the tree. Stage by explicit path, never `git add -A`, and never include files you did not change.

## Testing & QA

Tests live beside Rust modules in `#[cfg(test)]` blocks and OAuth test modules. Core provider/OAuth contract tests use local `wiremock` HTTP fixtures; TUI tests cover dashboard classification and config persistence. Run `cargo test --workspace`, Clippy, formatting, and a relevant CLI/TUI smoke for changed behavior. Keep provider fixtures deterministic, exercise missing/partial values and error classification, and do not treat mocked HTTP as proof of real-account access. No coverage threshold is configured. `.github/workflows/ci.yml` gates every push and pull request with fmt, Clippy and tests of both workspaces on Linux, macOS and Windows, plus `cargo deny`; tests that need Unix permissions, symlinks or `#!/bin/sh` fakes are `#[cfg(unix)]` (coverage per OS in `docs/architecture.md`).
