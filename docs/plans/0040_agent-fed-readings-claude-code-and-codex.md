# PRD 0040 — Agent-fed readings: Claude Code status line and Codex CLI as local sources

| Field | Value |
|---|---|
| ID | 0040 |
| Status | Proposed |
| Owner | `cuotai-core` (feed store, feed readers, provider source mode, access descriptor, status-line composer), `cuotai-tui` (`--feed`, `--statusline-claude`), desktop (live-session chip, Settings rows) |
| Consumers | Every front end (cards, details, tray, `--once`, `--json-v1`, `--serve`, `--watch`), status-bar recipes of PRD 0031 |
| Depends on | PRD 0005 (access descriptor), PRD 0010 (freshness and age), PRD 0029 (stable account id, to bind a feed to an account) |
| Benefits from | PRD 0007 (pace), PRD 0008 and 0024 (alerts), PRD 0015 and 0030 (local cost, same log directories), PRD 0028 (contract canary, `verified_on`), PRD 0031 (status line, `--from-server`), PRD 0033 (source-dated readings) |
| Extends | PRD 0005 (a new credential class and evidence kind), PRD 0031 RF2 (the status line gains a Claude Code mode), `export::v1` (additive `source` and `session` objects) |
| Date | 2026-10-25 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **feed** is a reading that an agent running on this computer handed to Cuotai (through standard input or a file it writes itself), as opposed to a **query**, which Cuotai makes over the network. "Claude Code" and "Codex CLI" are the vendors' coding agents; Cuotai never runs them and never sends them a prompt.

## Implementation status (2026-10-25)

| RF | Version | Delivery |
|---|---|---|
| RF1–RF8 | — | Proposed. |

## 1. Context

Today the `claude` and `chat_gpt` providers read usage through **undocumented** interfaces with a subscription session (an OAuth token Cuotai obtains with the native login or that the person pastes). Two things changed in 2026:

- Anthropic's legal and compliance documentation states that OAuth tokens of the Free, Pro and Max plans are meant for Claude Code and Claude.ai only, and that using them in other tools breaches the consumer terms; enforcement against third-party clients was reported in January–February 2026 ([GIGAZINE, 2026-02-20](https://gigazine.net/gsc_news/en/20260220-anthropic-third-party-block); [WinBuzzer, 2026-02-19](https://winbuzzer.com/2026/02/19/anthropic-bans-claude-subscription-oauth-in-third-party-apps-xcxwbn/)). `docs/oauth.md` already warns that implementing the protocol "is not authorization or a guarantee of acceptance". The `claude` provider is therefore the account most people add and the one with the highest contract and policy risk.
- Claude Code's **status line** is a documented, supported extension point: Claude Code runs a user command and passes JSON on standard input with `rate_limits.five_hour` and `rate_limits.seven_day` (`used_percentage`, `resets_at` in Unix seconds), `rate_limits.spend_limit` (behind a Claude apps gateway), `context_window` (`used_percentage`, `remaining_percentage`, `context_window_size`) and `cost.total_cost_usd` (the client-side estimate at list price) ([Claude Code docs, "Customize your status line", read 2026-10-25](https://code.claude.com/docs/en/statusline)). `rate_limits` appears only for Pro and Max subscribers and only after the first API response of a session; each window may be absent, and Claude Code drops a window once its `resets_at` passes.

Codex CLI exposes its own rate-limit windows to the local machine too: CodexBar reads them through the local Codex CLI RPC with a PTY fallback ([CodexBar docs, read 2026-10-25](https://mintlify.wiki/steipete/codexbar)), and the Codex session logs under `CODEX_HOME` carry `token_count` events that include the primary and secondary windows. Neither shape is a published API; both are **local** and need no network request and no token reuse, so they sit at a different risk level from calling the vendor's backend with the agent's OAuth token.

Competitors already treat the agent itself as a data source: Claude Code Usage Monitor "can read official rate-limit data via the statusline integration" ([SourcePulse listing, read 2026-10-25](https://www.sourcepulse.org/projects/5962823)); several status-line tools (`ccstatusline`, `cc-statusline`, `claudeline`) print block timers and cost from the same stdin JSON ([ClaudeLog](https://claudelog.com/claude-code-mcps/ccstatusline); [pkg.go.dev, claudeline](https://pkg.go.dev/github.com/lexfrei/claudeline)). None of them combines that feed with the quotas of other providers, which is Cuotai's job.

## 2. Problem

- The most-used provider depends on an undocumented interface and a token the vendor says belongs to Claude Code. If Anthropic rejects the call, every Claude card goes red and the person has no alternative inside Cuotai.
- The person already has the official numbers on their screen, inside Claude Code, several times a minute; Cuotai asks the backend again every 5 minutes for the same two percentages.
- Nothing in Cuotai shows what the agent is doing **right now**: context-window pressure and the session's estimated cost are the figures that make a developer stop and `/compact` or switch model, and they live only in the agent's footer.
- A status line that prints Claude's windows cannot say "Codex has 64% left" or "Copilot is blocked until 14:30"; a Cuotai status line can.

## 3. Goals

- A **documented, network-free, token-free** reading of the Claude Code 5-hour and 7-day windows (and the gateway spend limit when present), fed by Claude Code itself through its status-line contract, usable as the account's source or beside the existing query.
- The same for Codex CLI from local data, gated on the PRD 0028 canary (shape verified before shipping, re-verified by `verified_on`).
- `cuotai --statusline-claude`: one line for Claude Code's status line that shows Claude's own windows **plus** the tightest other account, built by the PRD 0031 summary.
- A **live session** block in the desktop and the TUI (context window used, estimated session cost, model) for each active agent session, opt-in and local only.
- Honest freshness: a feed that stops (the agent closed) ages like any reading (PRD 0010) and never becomes `0`.

## 4. Non-goals

- Running, launching or prompting Claude Code or Codex CLI; Cuotai never spawns an agent and never sends a prompt.
- Replacing the existing `claude` and `chat_gpt` query modes; they stay available with their access level. The feed is one more mode, and the recommended one when the agent is installed.
- Reading conversation content, prompts, file paths, git branches or the `cwd` field of the status-line JSON: the feed reader keeps the numeric and time fields of §7 and discards the rest before anything is written.
- Context-window alerts or automatic `/compact`: the live block is information only (a later PRD may add a threshold alert through PRD 0008's machinery).
- Reading the agent's keychain or auth files (`~/.claude/.credentials.json`, `~/.codex/auth.json`): PRD 0027 RF6 detects presence only, and this PRD does not use their tokens.
- Other agents (Gemini CLI, Cursor, Copilot CLI) in this PRD: each needs its own verified local contract; the feed store is generic so they can follow.

## 5. Functional requirements

### RF1 — Feed store in the core (high priority; prerequisite)

- **RF1.1** `cuotai-core::feed` owns a directory `<OS local data dir>/cuotai/feed/` (created `0700` on Unix) with one JSON file per source and session: `claude_code/<session digest>.json`, `codex_cli/<session digest>.json`. Each file is written atomically (temp file + rename), is `0600`, and holds `{ "schema": 1, "source", "written_at", "windows": [...], "session": {...} }` as defined in §7. The session digest is the SHA-256 of the agent's `session_id` truncated to 16 hex characters; the raw id is never stored.
- **RF1.2** `feed::latest(source) -> Option<FeedReading>` returns the newest file per source (by `written_at`); `feed::sessions(source)` returns every file younger than the retention. Files older than **24 hours** are deleted on the next write or read; the directory never exceeds 64 files per source (oldest deleted first).
- **RF1.3** Readers validate every field: percentages are finite numbers in `0..=100` (the spend limit may exceed 100 and is kept verbatim up to 1 000), `resets_at` is a Unix-seconds integer within ±400 days of now, `written_at` is RFC 3339. A file that fails validation is ignored as a whole and counted in `feed::health()` ("1 unreadable feed file"); nothing is turned into zero.
- **RF1.4** Concurrency: two Claude Code sessions writing at once are two files; the newest `written_at` wins for the account reading. A writer never blocks: a failed write is logged to stderr of the writer only.
- **Acceptance criterion:** unit tests for the atomic write, the `0600`/`0700` modes (`#[cfg(unix)]`), the retention and cap, a truncated and a tampered file (ignored, no zero), and two sessions with the newest winning.

### RF2 — `cuotai --feed claude-statusline` (high priority)

- **RF2.1** The command reads one JSON object from standard input (at most 64 KiB; more is an error with exit `1`), parses it with `serde_json::Value`, extracts **only** `rate_limits.five_hour`, `rate_limits.seven_day`, `rate_limits.spend_limit` (`used_percentage`, `resets_at`, `used_usd`, `limit_usd`, `period`), `context_window.used_percentage`, `context_window.remaining_percentage`, `context_window.context_window_size`, `cost.total_cost_usd`, `model.id` and `model.display_name` (when they match `^[A-Za-z0-9 ._-]{1,64}$`), `session_id` (digested, RF1.1) and `version`. `cwd`, `workspace`, `transcript_path`, `output_style` and every unknown field are never read into a struct, written or logged.
- **RF2.2** It writes the RF1 file and prints **nothing** on stdout by default (so it can be chained before the person's own status-line script with `tee`), or, with `--print`, delegates to RF5 and prints the composed line. Exit `0` always on a valid object, `1` on malformed input, never non-zero because a window is absent (absence is normal before the first API response).
- **RF2.3** The command performs **no network request**, reads no configuration file beyond locating the data directory, opens no vault entry and finishes in under 50 ms on a warm run (measured in CI with a release build).
- **RF2.4** A documented snippet for `~/.claude/settings.json` (`"statusLine": { "type": "command", "command": "cuotai --feed claude-statusline --print" }`) and a chained variant that keeps the person's existing script. The snippet is in `docs/integrations/claude-code.md` and the TUI prints it with `cuotai --feed claude-statusline --setup` (text only; Cuotai never edits the agent's settings).
- **Acceptance criterion:** fixtures from the documented example (full object, object without `rate_limits`, object with `spend_limit` percentage but no dollars, `used_percentage` null, oversized input): the file holds exactly the §7 fields, `cwd` and `transcript_path` never appear in the file or in any log, the exit codes match and the command runs without `CUOTAI_CONFIG`.

### RF3 — Feed as an account source for `claude` (high priority)

- **RF3.1** The `claude` provider gains `query_mode = "claude_code"` (account field descriptor through `account_edit.rs`, label "Fed by Claude Code"). In this mode the adapter makes **no HTTP request**: it calls `feed::latest("claude_code")` and converts the windows into the same metrics the query mode produces: `Window5h` and `Weekly` quotas with `unit = "%"`, `used = used_percentage`, `limit = 100`, `remaining = 100 − used`, `resets_at` from Unix seconds; the gateway spend limit becomes a `Spend` metric in USD (`used_usd`/`limit_usd`) with `window` from `period` and `billing = Included`, plus a `Quota` percentage when the dollars are absent (they arrive later than the percentage, by contract).
- **RF3.2** The report's `measured_at` (PRD 0033) is the feed's `written_at`, not the refresh time, so age and freshness (PRD 0010) count from when Claude Code last spoke. No feed at all gives `Status::NeedsConfiguration` with the note `note.claude_code.no_feed` ("No reading from Claude Code yet: add the status line (see docs) and send one prompt."); a feed older than 24 hours is reported as stale through the existing freshness classes, never deleted into zero.
- **RF3.3** A window absent from the newest feed (Claude Code drops a window once `resets_at` passes) is **not** a reset by itself: the adapter keeps the metric absent and the reset is detected by PRD 0033's reset-aware logic when the next feed carries the new window. The note `note.claude_code.window_absent` explains it once.
- **RF3.4** Hybrid mode `query_mode = "claude_code_or_api"`: the feed when it is younger than the refresh interval, otherwise the existing query. The report says which one answered (`note.claude_code.answered_by_feed` / `answered_by_api`), and the access descriptor of RF6 reports the level of the source that answered.
- **RF3.5** Metered and billing semantics are unchanged: `is_metered()` stays `false`; history, pace, alerts, binding, reset notices, stats, groups and `best` work through the generic code with no change. The account wizard and the desktop dialog explain the three modes in one sentence each and recommend `claude_code` when PRD 0027 RF6 detected Claude Code on the machine.
- **Acceptance criterion:** adapter tests with the RF2 fixtures (metrics, `measured_at`, no HTTP call asserted with a `wiremock` server that must receive zero requests), the no-feed and stale cases, the hybrid fallback, and a `pace::assess`/`alerts::evaluate` test on the fed windows.

### RF4 — Codex CLI local feed (medium priority; canary-gated)

- **RF4.1** `feed::codex` reads the newest `token_count` events of the Codex CLI session logs under `CODEX_HOME` or `~/.codex/sessions/**` (the same directories PRD 0015 already discloses for local cost) and extracts the primary and secondary rate-limit windows (`used_percent`, `window_minutes`, `resets_in_seconds` or an absolute reset when present). The exact field names are **verified during implementation** against the Codex CLI source and recorded with `verified_on` (PRD 0028); the reader ships behind the canary and refuses to parse a shape it does not recognise (schema note, never zero).
- **RF4.2** `chat_gpt` gains `query_mode = "codex_cli"` with the same semantics as RF3 (no network, `measured_at` from the event timestamp, stale when the agent is idle) and the hybrid `codex_cli_or_api`.
- **RF4.3** Reading the logs follows PRD 0015's rules: consent dialog listing the directories, only what grew since the last read, no message content, a low-priority scan. When the local cost source `codex_cli` is already on, the same consent covers the feed and the scan is shared.
- **RF4.4** Codex CLI's local RPC (used by other tools) is **not** used in this PRD: it starts the agent's process, which §4 forbids. An open question (§12) records it.
- **Acceptance criterion:** fixture logs (two windows, one window, an unknown shape, a stale log): metrics with `measured_at`, the unknown shape rejected with a schema note, the disclosure text listing the directories, and a test that the reader never opens a file outside `CODEX_HOME`/`~/.codex`.

### RF5 — `cuotai --statusline-claude` (high priority)

- **RF5.1** `cuotai --statusline-claude [--style ...] [--account ID]... [--from-server [URL]] [--max-width N]` reads the status-line JSON from stdin (RF2), stores the feed, and prints **one line** composed by the PRD 0031 status-line summary: Claude's own windows first ("5h 23% · Wk 41% ↺ 19:30"), then the tightest other account ("· Codex 64% left"), then, when the fed `context_window.used_percentage` is present, "· ctx 8%". The words and chips come from the core (DS-CMP-TIME), so the terminal and the desktop say the same.
- **RF5.2** Other accounts come from `--from-server` (a running `cuotai --serve` or the desktop server, loopback only, token from the default file) or, without it, from the last recorded history reading (PRD 0006) marked with the age; the status line **never queries a provider itself**, because Claude Code calls it often (its own cadence, at most every 300 ms per the documentation).
- **RF5.3** Colour only with `--color` on a terminal with `NO_COLOR` unset, as in PRD 0031; `--max-width` truncates with an ellipsis, Claude's windows last to go.
- **RF5.4** Performance budget: under 50 ms without `--from-server`, under 150 ms with it (loopback); a failed server read falls back to the history and never delays the agent.
- **Acceptance criterion:** golden tests of the line for the five RF2 fixtures with and without server data, the width truncation order, the colour gate, and a timing test in CI (release profile) under the budgets.

### RF6 — Access descriptor, catalog and documentation (high priority; prerequisite for RF3 and RF4)

- **RF6.1** `access::classify` gets the arms for `claude` / `claude_code`, `claude` / `claude_code_or_api`, `chat_gpt` / `codex_cli`, `chat_gpt` / `codex_cli_or_api`: `level = Official` for the Claude Code feed (`evidence = Documentation("https://code.claude.com/docs/en/statusline")`, `verified_on = 2026-10-25`), `level = Undocumented` for the Codex log shape (`evidence = Reference(<codex source file>)`), a new `credential = AgentFeed` (no credential: the agent is already signed in), `scope = Account`, `metered = false`, `local = true` (a new descriptor flag meaning "no network request").
- **RF6.2** Catalog entries (`access.summary.claude.claude_code`, `access.hint.*`, the notes of RF3) in every catalog of PRD 0034/0035; `docs/providers.md` regenerated; `docs/integrations/claude-code.md` with the settings snippet, the chained variant, the Codex section and the privacy statement (what is read, what is dropped).
- **RF6.3** `docs/oauth.md` gains a paragraph recommending the feed for Claude and pointing to Anthropic's policy; `README.md` mentions "fed by the agent itself, no token reuse" in one sentence.
- **Acceptance criterion:** the `access` tests pass for the new modes, the matrix drift test passes after regeneration, every id exists in every catalog and no text carries a path or token.

### RF7 — Live session block (medium priority; desktop and TUI)

- **RF7.1** The desktop card of a `claude_code`/`codex_cli` account (or of the account the feed is bound to, RF8) shows a **Live session** row while a feed younger than 10 minutes exists: model display name, "ctx 8%" meter (DS-CMP-MTR with the `warn` role from 70% and `bad` from 90%, both display-only), and "≈ $0.42 this session" (the agent's own estimate, labelled "agent estimate at list price, not a charge", HON rules). Several sessions stack as rows (newest first, at most 5, "and N more").
- **RF7.2** The row disappears 10 minutes after the last feed (the agent is idle or closed); nothing is notified. **Settings › Dashboard › Show live agent session** turns it off (on by default when a feed exists). The tray popover shows a single "ctx 8%" chip on the account row.
- **RF7.3** The TUI card shows `ctx 8% · ≈$0.42` under the account when present; details list the sessions. `--once` prints a `Live session:` line; `--json` is unchanged.
- **Acceptance criterion:** desktop fixture renders (0, 1, 6 sessions; ctx at 50/75/95%), the 10-minute expiry, the setting, the TUI card and `--once` line golden tests.

### RF8 — Binding a feed to an existing account (medium priority)

- **RF8.1** A person who keeps `query_mode = "api"` can still get the live block and the hybrid fallback: the account field `feed = "claude_code"` binds the feed to that account (stable id of PRD 0029). Without it, a feed with no account shows one dismissible banner "Claude Code is sending readings; add a Claude account fed by Claude Code" with an **Add account** action prefilled.
- **RF8.2** Two Claude accounts (work and personal) cannot be told apart from the feed, which carries no account identity: the binding is explicit, and the docs say so. A session digest never maps to an account by itself.
- **Acceptance criterion:** TOML round trip of `feed`, the banner state machine (no account, bound, dismissed for 7 days), and a test that two accounts with the same `feed` are a validation error.

## 6. Non-functional requirements

- **Privacy:** the feed holds numbers, timestamps, a model name and a digested session id; never `cwd`, transcript paths, prompts, branch names or raw ids. `export::v1`, `--watch` and `/v1/*` expose only §7's `source` and `session` objects.
- **Honesty:** absent is unknown; a feed never fills a window with `0`, and a stale feed is labelled with its age, not hidden.
- **Isolation:** the feed writer runs in the agent's process context, so it must be fast, silent and failure-proof: no network, no vault, no config parse, no panic (a panic hook prints one line to stderr and exits `1`).
- **Compatibility:** Claude Code's status-line schema is additive; the reader ignores unknown fields and tolerates absent ones (documented contract); the Codex reader is canary-gated.
- **Policy:** the Claude Code feed uses a documented extension point of the agent and reuses no token. The existing OAuth modes keep their warnings; the docs do not claim the feed is "approved", only that it is documented.

## 7. Data and contract changes

- **Feed file (`schema: 1`):** `source` (`claude_code` | `codex_cli`), `written_at`, `agent_version` (string, optional), `session` `{ digest, model_id?, model_name?, context_used_pct?, context_remaining_pct?, context_size?, cost_usd? }`, `windows` `[ { id: five_hour|seven_day|spend_limit|primary|secondary, used_pct, resets_at?, used_usd?, limit_usd?, period? } ]`.
- **TOML:** `query_mode = "claude_code" | "claude_code_or_api"` on `claude`, `"codex_cli" | "codex_cli_or_api"` on `chat_gpt`; `feed = "claude_code" | "codex_cli"` on an account (RF8). Additive; older versions reject unknown `query_mode` values as they do today.
- **`export::v1` (additive):** `AccountExport.source` `{ kind: query|feed|hybrid, answered_by: api|feed, measured_at }` and `AccountExport.sessions` `[ { model_name?, context_used_pct?, cost_usd?, written_at } ]`; `--watch` gains no new event in this PRD (resets and thresholds come from the generic diff).
- **Access descriptor:** `credential = AgentFeed`, flag `local: bool`.
- **CLI:** `--feed claude-statusline [--print] [--setup]`, `--statusline-claude [...]`.

## 8. Surfaces

| Surface | Change |
|---|---|
| `cuotai-core` | `feed` module, adapters' feed modes, access arms, status-line composer extension, export fields |
| TUI/CLI | `--feed`, `--statusline-claude`, card and details rows, `--once` line, wizard modes |
| Desktop | Account dialog modes, live session row and chip, banner, Settings row, Integrations page snippet with **Copy** |
| Docs | `docs/integrations/claude-code.md`, `providers.md` matrix, `oauth.md`, `advanced.md`, README sentence |

## 9. Dependencies and sequencing

RF1 and RF6 first; RF2 and RF5 together (they share the stdin reader); RF3 then RF7 and RF8; RF4 after the Codex shape is verified by the PRD 0028 canary. Lane of its own: it touches `feed`, two adapters and the status line; it does not conflict with the desktop refresh of PRD 0022.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Claude Code changes or removes `rate_limits` from the status-line JSON | Documented contract, additive reader, hybrid mode falls back to the query; the canary watches the fixture shape |
| People read "ctx 8%" as a quota | Different component (DS-CMP-MTR display-only, no ring), the words "context" and "this session", no alerts |
| Two Claude accounts, one feed | Explicit binding (RF8), validation error for a duplicate binding, documented limitation |
| The writer slows the agent's footer | 64 KiB cap, no config/vault/network, timing test in CI, silent failure |
| The Codex log shape is unpublished | Canary-gated, `Undocumented` level, refuses unknown shapes |
| The feed path leaks a session id | Digest only, 16 hex characters, 24-hour retention |

## 11. Order of execution and verification

1. **RF1**, **RF6** (store, descriptor, catalogs, matrix).
2. **RF2** and **RF5** (writer and status line), with `docs/integrations/claude-code.md`.
3. **RF3** and **RF8**; **RF7**.
4. **RF4** once the Codex shape is verified.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke with a real Claude Code session: the status line shows Claude's windows plus the tightest other account within one prompt, the card reads "Fed by Claude Code · 1 min ago", and closing Claude Code ages the reading instead of zeroing it.

## 12. Open questions and decisions to take

| Question | Recommendation |
|---|---|
| Should `claude_code` become the **default** mode for new Claude accounts when Claude Code is detected (PRD 0027 RF6)? | Yes, recommended but not forced: the dialog preselects it with the sentence "Documented, no token reuse; needs the status line". |
| Should Cuotai offer to edit `~/.claude/settings.json` for the person? | No in this PRD (Cuotai never edits another tool's files); print the snippet with **Copy**. Revisit if people ask. |
| Codex CLI local RPC instead of logs? | Not now: it starts the agent's process. Keep the log reader, canary-gated. |
| Context-window alert ("ctx above 85%")? | Later, through PRD 0008's evaluator with its own kind; needs a decision on noise. |
| Should the hybrid mode be the default for people who already have the API mode? | Offer a one-time banner "Also read from Claude Code" after the first feed arrives; never change a saved mode silently. |
