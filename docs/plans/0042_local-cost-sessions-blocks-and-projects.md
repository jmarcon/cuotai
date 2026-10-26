# PRD 0042 — Local cost: sessions, 5-hour blocks, projects and the live block

| Field | Value |
|---|---|
| ID | 0042 |
| Status | Proposed |
| Owner | `cuotai-core` (`cost` index schema v2, `cost::blocks`, `cost::sessions`, `cost::projects`, live summary), `cuotai-tui` (`--cost --by`, `--cost-live`), desktop (Local cost tabs, card line) |
| Consumers | Every front end; `export::v1` `local_cost` (additive); PRD 0026 stats |
| Depends on | PRD 0015 (local cost index and scanner), PRD 0030 (price table, plan value) |
| Benefits from | PRD 0040 (the live session feed gives the current block's real-time cost), PRD 0024 (the slope helper for burn rate), PRD 0022 (Local cost page layout), PRD 0041 (a budget on the block or day estimate, §12) |
| Extends | PRD 0015 RF4 (`CostSummary` gains breakdowns), PRD 0030 RF2 (every source gains the session and project dimensions) |
| Date | 2026-10-25 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **session** is one agent conversation (one JSONL file); a **block** is a 5-hour billing window as the vendor's subscription counts it, aligned to the first message after the previous block ended; a **project** is the working directory the agent was started in, stored only as a digest and a person-chosen label. All figures are estimates at API prices, never charges.

## Implementation status (2026-10-25)

| RF | Version | Delivery |
|---|---|---|
| RF1–RF7 | — | Proposed. |

## 1. Context

PRD 0015 reads the local logs of Claude Code, Codex CLI and (PRD 0030) Gemini CLI and gives the estimated cost **per day and per model**. The reference tool in this space, `ccusage`, reports **daily, monthly, session and block** views, aligns blocks to Claude Code's 5-hour windows, and offers `blocks --live` with the active block's progress, token burn rate and projected cost ([Apidog guide to open-source Claude Code monitors, read 2026-10-25](https://apidog.com/es/blog/open-source-tools-to-monitor-claude-code-usages/)). Status-line tools (`ccstatusline`, `cc-statusline`) show the block timer and burn rate in the agent's footer ([ClaudeLog](https://claudelog.com/claude-code-mcps/ccstatusline); [cc-statusline docs](https://www.mintlify.com/chongdashu/cc-statusline/features/overview)). CodexBar's Usage & Spend view keeps per-account cost history and "opt-in subtotals for identified requests across local and SSH histories" ([CodexBar v0.75.0 notes](https://newreleases.io/project/github/steipete/CodexBar/release/v0.75.0)). Claude Code Usage Monitor's whole pitch is the live block: burn rate, time to the reset, prediction ([gittrend listing](https://gittrend.io/repo/Maciek-roboblog/Claude-Code-Usage-Monitor)).

Cuotai already has the index (`cost.sqlite3`), the scanner, the disclosure and the price table; the index records time, model, token counts and an anonymous **session digest**, and the readers see `cwd` in the logs but drop it (PRD 0015 non-goal, kept here as a rule: the path is never stored).

## 2. Problem

- "What did this afternoon's refactor cost?" has no answer: the day total mixes every session and project.
- The 5-hour block is the unit a subscriber plans around, and Cuotai's cost view does not know it; the quota card knows the window but not what was spent in it.
- People comparing their plan with API prices (PRD 0030) want it **per block and per project** to decide where the plan pays off.
- A status line or a card cannot show "this block: 1.2 M tokens, ≈ $4.10, 38 k tok/min" without a live block summary.

## 3. Goals

- Three new dimensions in the local cost index and summary: **session**, **block** and **project**, for every source (Claude Code, Codex CLI, Gemini CLI), with the same privacy rules (digests, no content, no paths).
- A **live block** summary: tokens, estimated cost, burn rate (tokens/min over the last 10 minutes), time left in the block, projected block cost at the current rate, with the honest conditions of PRD 0024 (no projection from one reading or a short span).
- The same breakdowns in `--cost --by session|block|project`, `--cost-live`, the desktop Local cost page and `export::v1` `local_cost` (additive).
- Project labels chosen by the person, with the digest as the only identifier stored by default.

## 4. Non-goals

- Reading message content, prompts, tool calls, file names or git metadata: the index stores the fields of §7 only.
- Mapping a block to the provider's real reset: the provider's window (from the quota reading) is shown beside the local block when both exist, with the words "local estimate" and "provider window"; they are not merged.
- Billing accuracy: everything is an estimate at list prices (or the person's `pricing.json`), stated on every surface (PRD 0030 HON rules).
- Remote or SSH histories, cloud sync of the index.
- Per-project budgets (PRD 0041 §12) and alerts on block cost: later.

## 5. Functional requirements

### RF1 — Index schema v2 (high priority; prerequisite)

- **RF1.1** `cost.sqlite3` migrates to schema 2 in place: the `requests` (or equivalent) rows gain `session_digest` (already present), `project_digest` (SHA-256 of the normalized `cwd`, 16 hex characters, computed at scan time and the path discarded), `block_start` (computed by RF3 at query time; stored for speed, recomputed on reindex), and the `projects` table `{ project_digest, label, first_seen, last_seen, hidden }`. Migration is idempotent and reversible by **Erase the index**.
- **RF1.2** Scanning rules of PRD 0015 are unchanged (what grew, low priority, energy saving); the readers of the three sources extract `cwd` **only** to hash it and never log, store or display it. The disclosure dialog of PRD 0015 gains the sentence "and a one-way fingerprint of the project folder, so you can label it"; a source enabled before this PRD shows the new sentence once and asks again (the consent text changed).
- **RF1.3** When `cwd` is absent from a log line, `project_digest` is `NULL` and the row counts under "No project".
- **Acceptance criterion:** migration tests from schema 1 fixtures, the re-consent flag, a fixture with a `cwd` containing a personal name that never appears in the database, the index or any output.

### RF2 — Sessions (high priority)

- **RF2.1** `cost::sessions(range) -> Vec<SessionSummary>`: per `session_digest`: source, first and last timestamp, duration, models used, tokens by kind, estimated cost, requests, `project_digest`. Sorted by last activity; `limit` and `offset` for paging.
- **RF2.2** A session has no name (the title lives in the content, which is never read): it is shown as "Session · Oct 25 14:02–16:40 · 2h 38m"; the person may add a label from the desktop or `cuotai --cost-label session <digest prefix> "<label>"` (stored in the index, never in the TOML).
- **Acceptance criterion:** fixture logs with three sessions across two projects: totals, durations, paging, label round trip.

### RF3 — Blocks (high priority)

- **RF3.1** `cost::blocks(source, range)`: a block starts at the first request after the previous block's end and lasts **5 hours** (the vendor's subscription window, as `ccusage` does; configurable per source only through `block_hours` in the cost settings, 1–24, default 5); gaps are closed blocks; the current block is `active` while `now < start + 5h`. Blocks are computed, not read from the provider.
- **RF3.2** Each block: start, end, `active`, tokens by kind, estimated cost, requests, models, sessions count, projects count. The provider's own window (when the matching account's quota reading has `resets_at`) is attached as `provider_window { resets_at, used_pct }` for display beside it, never merged.
- **RF3.3** `cost::live(source, now)`: the active block plus `burn_rate_tokens_per_min` (over the last 10 minutes, `None` under 3 requests or 3 minutes), `projected_block_cost` (`None` when the burn rate is `None` or the block has under 20% elapsed), `time_left`. The words follow PRD 0024's honesty rules ("At this rate ≈ $6.40 by 18:53").
- **Acceptance criterion:** pure tests for block alignment (gap, straddle, DST), the active block, the burn-rate gating, `block_hours = 1`, and the provider window attachment when the account exists.

### RF4 — Projects (medium priority)

- **RF4.1** `cost::projects(range)`: per `project_digest`: label (or "Project ab12cd34" from the digest prefix), first/last seen, sessions, tokens, cost, models. **Hidden** projects are excluded from totals shown under a "Hide" toggle.
- **RF4.2** Labels: `cuotai --cost-label project <prefix> "<label>"`, or inline edit in the desktop; `--cost-label --clear`. The label is free text ≤ 48 characters stored in the index; Cuotai never suggests the folder name (it does not keep it).
- **RF4.3** Optional, opt-in **folder name hint**: a per-source setting "Keep the last folder name of each project" stores the final path segment (≤ 64 characters) beside the digest to prefill the label. Off by default; the disclosure lists it; turning it off deletes the stored names.
- **Acceptance criterion:** aggregation tests, label round trip, the hint on/off with deletion, hidden projects excluded from totals but listed.

### RF5 — Command line (high priority)

- **RF5.1** `cuotai --cost --by day|model|session|block|project [--days N] [--json]` (default `day`, today's behaviour); `--by session` and `--by project` accept `--top N` (default 20) and `--all`.
- **RF5.2** `cuotai --cost-live [--source claude_code|codex_cli|gemini_cli] [--json] [--watch-interval S]`: prints the live block once (or refreshes in place every `S` seconds on a terminal, `q` to quit): block window, elapsed/left, tokens, estimated cost, burn rate, projection, and the provider window when known. `--json` prints one object per refresh (NDJSON with `--watch-interval`).
- **RF5.3** Exit codes as `--cost` (0, 2 when no source is on or no logs, 1 local error).
- **Acceptance criterion:** golden outputs for every `--by`, the live view in both modes with the fixture, the exit codes.

### RF6 — Desktop (medium priority)

- **RF6.1** **Settings › Advanced › Local cost** (or the Stats view's cost tab, per PRD 0022's layout) gains a segmented control **Days · Models · Sessions · Blocks · Projects** (DS-CMP-SEG), each a table/list with the DS-CMP-KPI header (total, requests, tokens); Sessions and Projects have inline label editing and the Hide action; Blocks shows the active block first with the live figures (updated every 30 s from the index, no extra scan).
- **RF6.2** The card of a Claude or Codex account with the source on shows "This block ≈ $4.10 · 38 k tok/min" under the "Local usage" line (PRD 0015), with the HON suffix in the tooltip; absent when the live summary has no burn rate.
- **Acceptance criterion:** fixture renders of every tab (empty, partial, full), the label edit, the hidden project, the card line present/absent.

### RF7 — Export and documentation (low priority)

- `export::v1` `local_cost` gains `blocks[]` (last 7 days), `live { ... }` and `projects[]` (labels only, never digests of hidden projects); `docs/export-v1.md`, `docs/advanced.md` ("Sessions, blocks and projects"), `docs/gui-features.md` and `docs/tui.md` updated; the disclosure text documented verbatim.

## 6. Non-functional requirements

- **Privacy:** paths, content, git data and raw session ids never enter the index; project identity is a one-way digest; the folder-name hint is opt-in and deletable; labels are the person's own words.
- **Performance:** `cost::blocks` and `cost::live` answer from the index in under 100 ms for 120 days of logs (indexed by timestamp and source); the desktop refreshes the live block from the index, not from the logs.
- **Honesty:** every figure says "estimate at API prices"; projections follow PRD 0024 gating; a block never shows `$0` for unknown models (PRD 0030's "cost unknown" rule).
- **Compatibility:** schema migration in place; `--cost` without `--by` is unchanged.

## 7. Data and contract changes

- **Index schema 2:** `project_digest`, `block_start`, table `projects` (`label`, `folder_hint` nullable, `hidden`), table `session_labels`.
- **Cost settings:** `block_hours` per source (1–24, default 5), `folder_hint` per source (bool, default false).
- **CLI:** `--cost --by ...`, `--top`, `--all`, `--cost-live`, `--cost-label`.
- **`export::v1` (additive):** `local_cost.blocks[]`, `local_cost.live`, `local_cost.projects[]`.

## 8. Surfaces

| Surface | Change |
|---|---|
| `cuotai-core` | index v2, `cost::{sessions,blocks,projects,live}`, labels, settings, export |
| TUI/CLI | `--cost --by`, `--cost-live`, `--cost-label`, details lines |
| Desktop | Local cost tabs, card line, re-consent dialog |
| Docs | advanced, gui-features, tui, export-v1, disclosure text |

## 9. Dependencies and sequencing

RF1 first (migration and re-consent), then RF2/RF3 together (they share the aggregation), RF5 with them, RF4, RF6, RF7. When PRD 0040 ships, `cost::live` may use the fed `cost.total_cost_usd` of the active session as a cross-check (display only). Lane: `crates/cuotai-core/src/cost` and the Local cost page; no conflict with provider lanes.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Local blocks disagree with the provider's window | Shown side by side with distinct words; never merged; documented |
| Re-consent annoys people who enabled a source already | One dialog, one sentence of change, keeps the source on after acceptance |
| A folder name is personal | Hint off by default, deletable, never exported |
| Index growth with per-request rows | Already per request in schema 1; retention unchanged (120 days), size shown |
| Burn rate swings | 10-minute window, 3-request minimum, projection gated at 20% elapsed |

## 11. Order of execution and verification

1. **RF1** (migration, consent text).
2. **RF2**, **RF3**, **RF5**.
3. **RF4**, **RF6**, **RF7**.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests; a smoke on the owner's machine: `--cost --by block` lists today's blocks with the active one, `--cost-live` updates in place, the desktop Blocks tab matches, and `grep` of the index for the real project path finds nothing.

## 12. Open questions and decisions to take

| Question | Recommendation |
|---|---|
| Default block length for Codex CLI and Gemini CLI | 5 hours for Codex (its primary window), 24 hours for Gemini CLI (daily quota); per-source default in the settings. |
| Store the folder-name hint by default? | No: off by default (privacy principle); one switch per source. |
| Budget on the active block (PRD 0041)? | Later, after both PRDs ship, as a `local_cost` target kind in PRD 0041. |
| Show the live block in the tray? | Not in this PRD; the card line and `--cost-live` first, tray after PRD 0025's display presets settle. |
