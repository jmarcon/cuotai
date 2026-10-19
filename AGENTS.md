# Repository Guidelines

## Project Overview

Cuotai is a read-only Rust monitor for AI-provider usage, quotas, and costs. It exposes a reusable async backend and a Ratatui terminal dashboard; the CLI also emits text or JSON snapshots. Provider interfaces can be undocumented or bill per query (notably AWS Cost Explorer), so preserve their documented limitations.

## Architecture & Data Flow

- `crates/cuotai-tui/src/main.rs` parses CLI flags, loads TOML through core `config_file::ConfigFile`, creates an `OAuthSession`, then runs either `Monitor::refresh()` once or the interactive loop in `ui.rs`.
- `cuotai-core` is UI-independent. `lib.rs` validates `Config`, owns the shared HTTP client and query semaphore, and returns a `Snapshot` in account order. One account failure becomes a report rather than aborting the whole refresh.
- `model.rs` defines `Provider`, `Account`, `Metric`, `Report`, `Status`, and `Snapshot`. Provider dispatch in `lib.rs` calls private `providers/*` fetchers through shared `Context` (`http`, `credentials`, `timeout`); fetchers normalize responses into metrics/notes or `QueryError`. Add a provider at the enum, dispatch, adapter, and example config; the dashboard renders reports generically.
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

Use `--once` instead of `--json` for human-readable, single-refresh output. `--once`/`--json` return 0 for all-ready, 2 for account failures, and 1 for local errors.

## Code Conventions & Common Patterns

- Rust 2021, conventional `rustfmt`, `snake_case` modules/functions/provider IDs, `PascalCase` types. Follow neighboring adapter/test patterns rather than adding UI behavior to core.
- Validate at boundaries (`Config::validate`, provider params); classify missing configuration, authentication/rate limits, transport failures, and incompatible schemas. Never turn absent usage into zero or expose response bodies, URLs, or tokens in errors.
- Async work uses Tokio/reqwest; `Monitor::refresh()` bounds concurrency while preserving configuration order. UI refresh/login work runs separately from navigation. Share an `OAuthSession` to serialize token rotation within a process.
- Config stores environment-variable names (`credential_env = "OPENAI_ADMIN_KEY"`) or OAuth credential references, not secrets. `ConfigFile` preserves unrelated TOML/comment content and rejects conflicting external edits.

## Important Files

`Cargo.toml` defines the two-crate workspace; `cuotai.example.toml` documents account/provider parameters; `README.md` documents supported integrations, auth caveats, and keybindings; `THIRD_PARTY_NOTICES` records adapted upstream contracts. Start with `crates/cuotai-core/src/lib.rs` and `model.rs` for backend changes, `crates/cuotai-tui/src/main.rs` and `ui.rs` for CLI/TUI changes.

## Runtime/Tooling Preferences

Use Rust/Cargo (no pinned toolchain or JS package manager). Default OAuth storage uses the OS credential vault; AWS Bedrock needs AWS CLI v2 and an explicitly configured profile. Config lookup: `--config`, `./cuotai.toml`, then the OS config directory. `.env` is not loaded. `run.sh` selects `--dev-credentials-sqlite ./cuotai-dev.sqlite3`: this debug-only store contains plaintext tokens; release builds reject the flag. Local `cuotai.toml`, `.env*`, SQLite files, and `target/` are git-ignored; do not publish secrets.

## Documentation Language

The README and all repository documentation are written in **English**, the norm for open-source repositories: `README.md`, `docs/` (including the PRDs in `docs/plans/`), `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, issue templates and code comments.

- The product stays multilingual: UI strings live in the Portuguese, English and Spanish catalogs, and core `Localized` fallbacks stay in Portuguese by design.
- Semantic commits (subject and body), pull request titles and descriptions, and issue text are also written in **English** from now on; see "Versioning, Commits & Changelog" below.
- Write every new document in English. The only remaining Portuguese document is `docs/plans/0004_animations-and-visual-effects.md` (another agent was editing it), still to be migrated; when a change touches one, either translate the whole file in that change or leave its language alone for the migration task. Never mix languages inside one file.

## Versioning, Commits & Changelog

Every user-visible delivery is one semantic commit that also raises the version. Do not leave the bump for a later commit.

- **Commit format:** `type(scope): subject`, with the subject **and the body in English** (earlier history is in Portuguese without accents; that stays as it is, with no rewriting). Types: `feat`, `fix`, `docs`, `refactor`, `test`, `style`, `chore`. The body explains what changed for the user and ends with `Version X.Y.Z.` when the version moved. Keep the `Co-Authored-By` trailer the session requires. Commit or push only when the user asks.
- **Which bump** (semantic versioning, still `0.x`): `feat` raises the minor (`0.17.0` → `0.18.0`); `fix` and dependency or security fixes (`chore(security)`, `fix(deps)`) raise the patch; a breaking change to the TOML, `--json`, `export::v1` or `desktop.json` raises the minor while in `0.x` and carries a `BREAKING CHANGE:` footer with the migration; a change to distributed metadata that the app displays (license, About) also raises the patch. `docs`, `test`, `style`, `refactor` and `chore` without user-visible effect do not change the version.
- **Where the version lives (all in the same commit, all equal):** `Cargo.toml` (`[workspace.package]`), `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`, and both lockfiles (`Cargo.lock` and `apps/desktop/src-tauri/Cargo.lock`). The desktop is a separate workspace: run `cargo check` there as well so its lockfile follows. Verify with `git grep -n '"0\.[0-9]*\.[0-9]*"\|^version = "0\.' -- Cargo.toml apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/tauri.conf.json` before committing.
- **Plans:** a commit that delivers a requirement of a PRD in `docs/plans/` adds the footer `Refs: PRD 0006 RF2` (one line per requirement), updates that plan's status and "Implementation status" with the version, and updates `docs/plans/0000_control.md` (status, "Delivered by version", history) in the same commit.
- **Changelog:** once `CHANGELOG.md` exists (PRD 0019, RF7), every user-visible `feat` or `fix` updates its `[Unreleased]` section in the same commit, with the PRD/RF reference and the short commit hash; releases move that section under the new version and date. A commit that is not user-visible and skips it states `Changelog: skip` with the reason. Until the file exists, `0000_control.md` is the only record of what each version delivered.
- **Shared worktree:** another agent may leave uncommitted work in the tree. Stage by explicit path, never `git add -A`, and never include files you did not change.

## Testing & QA

Tests live beside Rust modules in `#[cfg(test)]` blocks and OAuth test modules. Core provider/OAuth contract tests use local `wiremock` HTTP fixtures; TUI tests cover dashboard classification and config persistence. Run `cargo test --workspace`, Clippy, formatting, and a relevant CLI/TUI smoke for changed behavior. Keep provider fixtures deterministic, exercise missing/partial values and error classification, and do not treat mocked HTTP as proof of real-account access. No coverage threshold or CI gate is configured.
