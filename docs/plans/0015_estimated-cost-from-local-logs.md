# PRD 0015 — Estimated cost from local logs

| Field | Value |
|---|---|
| ID | 0015 |
| Status | Proposed |
| Owner | `cuotai-core`, desktop and TUI |
| Consumers | `apps/desktop`, `crates/cuotai-tui`, PRD 0012 (contract field) |
| Depends on | PRD 0005 (`LocalCli`/estimate access label); it uses PRD 0006 |
| Date | 2026-10-19 |

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
