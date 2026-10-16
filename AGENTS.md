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

## Testing & QA

Tests live beside Rust modules in `#[cfg(test)]` blocks and OAuth test modules. Core provider/OAuth contract tests use local `wiremock` HTTP fixtures; TUI tests cover dashboard classification and config persistence. Run `cargo test --workspace`, Clippy, formatting, and a relevant CLI/TUI smoke for changed behavior. Keep provider fixtures deterministic, exercise missing/partial values and error classification, and do not treat mocked HTTP as proof of real-account access. No coverage threshold or CI gate is configured.
