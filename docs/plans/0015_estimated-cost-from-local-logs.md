# PRD 0015 — Estimated cost from local logs

| Field | Value |
|---|---|
| ID | 0015 |
| Status | In progress |
| Owner | `cuotai-core`, desktop and TUI |
| Consumers | `apps/desktop`, `crates/cuotai-tui`, PRD 0012 (contract field) |
| Depends on | PRD 0005 (`LocalCli`/estimate access label); it uses PRD 0006 |
| Date | 2026-10-19 |

## Implementation status (2026-10-21)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.57.0 | `cuotai_core::cost` (`crates/cuotai-core/src/cost/`). RF1.1: off by default; `SettingsStore` keeps the per-source consent in `<OS local data dir>/cuotai/cost/settings.json` (`0600`, directory `0700`) as the digest of the `Disclosure` the user accepted (source, exact directory list, file pattern, versioned wording): `Disclosure::new` names every directory read, what for (`cost.disclosure.purpose`), that only numbers are extracted (`extracted`), what is never read (`never`) and what turning off does (`off`); `SettingsStore::enable` refuses a digest that is not what would be read now (`CostError::ConsentChanged`), and a changed directory list or home turns the source off until it is consented again (`CostSettings::enabled`). The interfaces: `cuotai --cost-enable SOURCE [--cost-path DIR]…` prints the disclosure and asks for a typed confirmation in a terminal (none: a local error; `--cost-sources [--json]` lists each source, its state, directories, pattern and what the index follows without reading logs), and the desktop's **Settings → Local cost** has one switch per source that opens a dialog with the same disclosure and sends its digest (`cost_status`, `cost_enable` Tauri commands). RF1.2: a crate-private `Source` trait (`id`, `default_roots`, `pattern`, `is_log`, `parse`) with two adapters, formats confirmed on 2026-10-21 against ccusage's documentation and the openai/codex sources: Claude Code reads `**/*.jsonl` under each `CLAUDE_CONFIG_DIR` entry's `projects/` or, without it, `~/.config/claude/projects` and `~/.claude/projects`; Codex CLI reads `**/rollout-*.jsonl` under each `CODEX_HOME` entry (default `~/.codex`) in `sessions/` and `archived_sessions/`; chosen directories (`SourceSettings::paths`, `~/` expanded) replace the defaults. RF1.3: the parsers deserialize only `timestamp`, the model (validated: 1 to 96 characters of `[A-Za-z0-9.-_:/@+]`, so free text never passes as a model), the token counts and the session identifier, which is kept only as a SHA-256 digest; Claude Code: `message.usage` input, output, cache creation and cache read, `<synthetic>` and zero-token lines skipped, a request copied into another file (resumed session) counted once by the digest of `message.id` and `requestId`; Codex CLI: `turn_context.payload.model` for the following `token_count` events, `last_token_usage` with `input_tokens` split into non-cached input, `cached_input_tokens` (cache read) and `cache_write_input_tokens`, a repeated snapshot (same cumulative `total_tokens`) skipped, and an archived copy of a rollout counted once by the digest of the session and its cumulative total; non-object, malformed or mistyped lines are invalid, skipped and counted (per scan and per file in the index). RF1.4: only regular files owned by the user (`geteuid`, Unix) are opened, symbolic links are not followed, nothing elevates privileges; a refused or non-directory root, or a chosen directory that is gone (a missing volume), is `RootState::NoAccess` and the source `SourceState::NoAccess` without any workaround; absent default directories are `NoLogs`. Acceptance: synthetic logs with a marked sensitive value in prompts, answers, working directories, instructions, project directory and file names; the test reads every byte of `cost.sqlite3` (after a WAL checkpoint) and `settings.json`, the serialized and `Debug` scan reports and usage, and the summaries in pt-BR, en-US and es, and finds neither the marker nor the raw session, message, request or rollout identifiers. |
| RF2 | 0.57.0 | RF2.1: `CostIndex` (`<OS local data dir>/cuotai/cost/cost.sqlite3`, `0600`, WAL, busy timeout, versioned schema, `secure_delete`): `usage` sums input, output, cache read, cache write and requests per `(source, local day, model)` split by the digest of the source file, `sessions` counts anonymous sessions per day and model, `seen` holds the digests of requests already counted, `files` is the per-file control (path digest, device and inode or creation time, bytes examined, position read, the parser's carried model, session digest and cumulative total, invalid lines). Only complete lines are read from the stored position (a line without a newline waits, unless the file has been quiet for 10 minutes); a file that shrank or was replaced loses its share before anything is read (so a rotated copy elsewhere counts again whatever the order) and restarts; every file chunk is written in one `BEGIN IMMEDIATE` transaction that first checks, inside the transaction, that the source is still on. RF2.2: `Limits` (64 files per scan, 50 000 lines per file per scan, 5 s per scan, 8 MiB per line, 100 000 directory entries and 8 levels per root, and a pause after each file as long as it took, up to 50 ms); a limited scan continues in the next one. `CostWorker` scans on a thread of its own with background priority (darwin background band on macOS, nice 19 on Linux, `THREAD_MODE_BACKGROUND_BEGIN` on Windows) every 5 minutes, 20 s while a scan left work behind, and at once after a preference change; it pauses in energy-saving mode (`cost::energy_saving`: Low Power Mode from `pmset -g`, the `low-power` platform profile, Windows battery saver) and reads only the preferences while every source is off. The TUI and the desktop app start it beside their refresh cycle, which never waits for it; `cuotai --cost-scan [--json]` scans in the foreground (exit 0, 2 when no source is on or one has no logs or access, 1 for local errors). RF2.3: `Period` (`today`, `last_7_days` by default, `last_30_days`, `current_month`, `all`) and `Retention` (120 days by default, 7 to 730) in the preferences, from `--cost-period`/`--cost-retention-days` or Settings → Local cost; `CostIndex::usage`/`usage_in` answer per source, day and model; lines and files older than the retention are not indexed, and each scan prunes days before it. RF2.4: `cost::turn_off` writes the preference first and then removes every row of the source (`--cost-disable SOURCE`, the switch's confirmation dialog), and `CostIndex::erase` removes every row and truncates the WAL (`--cost-erase`, **Erase the index**); a source turned off is never listed or read again. Acceptance: tests of growth, a line being written, truncation, rotation, a resumed-session copy, an archived Codex rollout, idempotent rescans, the line, file, time and line-size limits continuing in later scans, retention and periods, complete removal per source and in full, the in-transaction turn-off check, no access, private permissions, symbolic links, and the worker pausing while saving energy. |
| RF3 | 0.58.0 | `cuotai_core::cost` (`prices.rs`, `estimate.rs`). RF3.1: `PriceTable::embedded()` parses `crates/cuotai-core/src/cost/prices.json` (USD per million tokens: `input`, `output`, `cache_read`, `cache_write`; `effective` 2026-10-21 and `source`, the Anthropic and OpenAI API pricing pages checked that day, standard tier: Claude 5-minute cache writes, OpenAI up to 272K input tokens and the input price for cache writes where no write price is listed); prices are kept as integer micro-dollars per million tokens (`Rate`, at most six decimals). `Prices::price` looks a model up case-insensitively by its exact name, then without a dated snapshot suffix (`-20250929`, `-2025-08-07`), never by a sibling's prefix; a model in no table is `None`, "cost unknown". RF3.2 (owner decision: manual `pricing.json` only, no signed download): `PricingFile` reads `<OS local data dir>/cuotai/cost/pricing.json` in the same format, written by the user and never by Cuotai; each model there wins over the embedded entry (its exact name and its undated name before any embedded entry), the others keep the embedded price; the whole file is validated (`currency` only `USD`, `effective` `YYYY-MM-DD`, `source` up to 300 characters without control characters, model names by the parsers' rule and not `unknown`, no unknown or missing field, no repeated model, prices from 0 to 10 000) and an invalid file is ignored as a whole with a localized `PricingError` (`Prices::with_user_file`, `cost::pricing_ignored`). `cuotai --cost-prices [--json]` lists the prices in effect per model with their origin, each table's date and source and the state of `pricing.json` (missing, used, ignored), exit 0, or 1 when the file is invalid; it reads no log. RF3.3: `cost::estimate(&[DailyUsage], &Prices) -> Estimate`, pure: per model the tokens, requests, cost (`Option<Usd>`) and the price used with its origin; the total of the priced part (`None` when there is usage and none of it is priced), all tokens, the unknown tokens and their share, and the date and source of each table that priced something. Amounts are exact integers (pico-dollars) summed before any rounding; `Usd::micros`/`cents` round half up. RF3.4: `currency` is always `USD` (a `pricing.json` in another currency is refused) and every `Estimate` carries `notice` (`cost.estimate.notice`, "estimate at API prices, not a charge") in pt-BR, en-US and es. Acceptance: table tests with known tokens and prices including cache, an unknown model (never zero, no total when nothing is priced), exact sums and half-up rounding, the user's table overriding the embedded one model by model, every invalid `pricing.json` case, dated-suffix lookup without sibling matches, and an estimate of indexed synthetic logs whose JSON and `Debug` carry no content. Known limits: prices tiered by prompt length per request (Claude Haiku 5.5 above 100K tokens, OpenAI long context above 272K) cannot be applied to daily aggregates, so Haiku 5.5 has no embedded price (unknown unless `pricing.json` sets one) and OpenAI models use the short-context price; Claude 1-hour cache writes are priced as 5-minute writes; regional, batch, fast-mode and Bedrock/Vertex prices are not modelled. The desktop shows prices and estimates with RF4. |

**Decisions (2026-10-21, 0.57.0):** the cost preferences and index are per computer and per user (the logs are), shared by the TUI, the CLI and the desktop app, and kept in the local data directory rather than the accounts' TOML; consent is the digest of the exact disclosure, so a different directory list (chosen paths, `CLAUDE_CONFIG_DIR`, `CODEX_HOME` or home) needs a new consent. Days are the computer's local days when indexed; a later change of time zone does not move them. Usage of a log file that the tool deleted stays in the index until the retention (Claude Code deletes transcripts after 30 days by default). Usage without a model in Codex CLI logs is kept under `unknown`. Erasing the index keeps the sources as they are, so a source that stays on reads its logs again from the start. Known limits: compressed `.jsonl.zst` rollouts are not read, and a Codex subagent rollout that replays its parent's history counts that history again. The displays of tokens and cost (cards, details, `cuotai --cost`, `local_cost` in `export::v1`) belong to RF4; Settings → Local cost will gain the directory editor and the price table with RF4.1.

**Decisions (2026-10-21, 0.58.0):** owner decision for RF3.2: the price table is updated only by the user's manual `pricing.json`, never by a download (signed or not), so nothing in the feature touches the network. A model's price is found by its exact name or its name without a date suffix and never by a prefix, since a sibling (`gpt-5` and `gpt-5-mini`) can cost ten times less or more; an unlisted model stays unknown and the user can price it in `pricing.json`. An invalid `pricing.json` is ignored as a whole rather than in part, so a typo never silently mixes the user's and the embedded prices.

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## 1. Context

All of Cuotai's queries are **remote**: quotas, balances and bills reported by the provider. Nothing measures what the user's own command-line tools (Claude Code, Codex CLI and similar) spent locally. Those tools write session transcripts to user files, from which projects like ccusage derive tokens and estimated cost per model.

In the 2026-10-19 market analysis, CodexBar has a cost scan of local Codex and Claude logs (`codexbar cost`), with a configurable period, a SQLite store with caps on entries and size, and dynamically refreshed per-model pricing; ClaudeBar shows today's cost and tokens next to yesterday's; OpenUsage computes Today, Yesterday and 30 Days from CLI logs with an updatable price table; OpenQuota records a token history and estimated spend.

## 2. Problem

Window-based subscriptions (Claude, Codex) do not report how much a session would cost at API prices. Someone who wants to compare the subscription's value with the equivalent cost, or understand which projects and models consume the most, cannot do so in Cuotai. At the same time, reading local logs is sensitive: transcripts contain code and prompts.

## 3. Goals

- Compute, locally and optionally, tokens and estimated cost per day, per model and per tool from CLI logs, **without ever reading or storing the content of conversations**.
- Label the result as an estimate at API prices, separate from the providers' real billing.
- Work offline, with no credentials and no network (except an optional price-table update).

## 4. Non-goals

- Showing, indexing or exporting prompts, responses, project paths or code file names.
- The cost of tools that do not write readable local logs.
- Mixing the estimate with providers' `Spend` or summing different currencies.
- Continuous disk scanning: only the known directories, when the feature is on.

## 5. Functional requirements

### RF1 — Sources and consent (high priority; prerequisite)

- **RF1.1** The feature is off by default. When turning it on, the interface shows exactly which directories will be read, what it is for and that only numbers (model, token counts, time) are extracted. Consent is per source.
- **RF1.2** Initial sources, with default paths **to be confirmed at implementation** against the current version of each tool and overridable in configuration: the session logs of Claude Code and Codex CLI. A source is an adapter with an `id`, paths, a file pattern and a parser, behind a trait, so others can be added later.
- **RF1.3** No adapter reads content fields: the parser extracts only `timestamp`, `model`, token counts (input, output, cache read and write) and an **anonymous** session identifier (a digest). Invalid lines are ignored and counted.
- **RF1.4** System permissions: read only the user's own files; never elevate privileges; access errors (`EACCES`, a missing volume) become a "no access" state, without trying to work around them.
- **Acceptance criterion:** tests with synthetic logs that contain marked "sensitive content"; no value from those fields appears in the database, the JSON, the debug logs or the interface.

### RF2 — Incremental indexing (high priority)

- **RF2.1** A SQLite index of its own (`cost.sqlite3`, in the data directory with `0600` permission), with a table of aggregated usage by `(source, local day, model)` and a per-file control table (a digest of the path, size and position already read). Re-read only what grew; a shrunk or replaced file restarts that file.
- **RF2.2** Limits: a cap on files per scan, a cap on lines per file, a maximum time per cycle, low CPU and I/O priority, and a pause in energy-saving mode, so as not to compete with the user's work. The scan runs in a separate task and never delays the quota refresh.
- **RF2.3** Configurable period (today, 7 days, 30 days, current month, everything available) and index retention (default 120 days).
- **RF2.4** Erasing the index and turning the source off remove everything derived.
- **Acceptance criterion:** incremental read tests (growth, truncation, rotation), idempotence, time and size limits, and complete removal.

### RF3 — Prices and computation (high priority)

- **RF3.1** A per-model price table embedded in the binary (USD per million tokens: input, output, cache), with an effective date and a source. A model with no known price appears as "cost unknown", never as zero.
- **RF3.2** An optional table update through a signed file download, or manual review by the user in `pricing.json` (the user's preference wins over the embedded one). No automatic update by default.
- **RF3.3** The computation is `cost::estimate(usage, prices) -> Estimate` in the core, a pure function. The result carries the total, the cost per model, tokens, the share with an unknown price and the date of the table used.
- **RF3.4** The text always says "estimate at API prices, not a charge". Values never sum different currencies; everything is in USD.
- **Acceptance criterion:** table tests with known tokens and prices, an unknown model, cache, rounding and the user's table overriding the embedded one.

### RF4 — Display (medium priority)

- **RF4.1** Desktop: a "Local usage" card per source (today, yesterday, period), in the same place for every account of that tool; in the details, a breakdown by day and by model (an accessible bar chart with an equivalent table). **Settings → Local cost** controls sources, paths, period, the price table and erasing data.
- **RF4.2** TUI: a section in the account details and `cuotai --cost [--days N] [--json]`, with the same estimate; exit codes like the other non-interactive outputs.
- **RF4.3** The result enters the export contract (PRD 0012) as `local_cost`, separate from the providers' reports.
- **Acceptance criterion:** a smoke test in three languages with the source on, off and without access; an explained empty state.

## 6. Non-functional requirements

- **Privacy:** no content is read, stored or transmitted; the index contains only aggregates. The file and the configuration stay on the user's computer, and export is explicit.
- **Honesty:** cost is an estimate at API prices; fixed-price subscriptions may cost less or more. Incomplete windows are labelled.
- **Performance and energy:** an incremental scan, with limits and a pause; do not degrade the system.
- **Risks:** log formats change between versions of the tools; each adapter has versioned fixture tests and fails in isolation.

## 7. Order of execution and verification

1. **RF1 and RF2:** an adapter for one source, the index and limits, with synthetic logs.
2. **RF3:** prices and computation.
3. **RF4:** desktop, TUI and CLI.
4. A second source, when the first is stable.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when, with the feature on, the app shows today's and the period's tokens and estimated cost without reading conversation content, models without a price appear as unknown, erasing the data removes it, and turning the source off stops any disk read.
