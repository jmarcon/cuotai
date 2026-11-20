# PRD 0042 — Local cost: sessions, 5-hour blocks, projects and the live block

| Field | Value |
|---|---|
| ID | 0042 |
| Status | Awaiting owner verification (2026-11-17 re-validation fixes in 0.86.10) |
| Owner | `cuotai-core` (`cost` index schema v2, `cost::blocks`, `cost::sessions`, `cost::projects`, live summary), `cuotai-tui` (`--cost --by`, `--cost-live`), desktop (Local cost tabs, card line) |
| Consumers | Every front end; `export::v1` `local_cost` (additive); PRD 0026 stats |
| Depends on | PRD 0015 (local cost index and scanner), PRD 0030 (price table, plan value) |
| Benefits from | PRD 0040 (the live session feed gives the current block's real-time cost), PRD 0024 (the slope helper for burn rate), PRD 0022 (Local cost page layout), PRD 0041 (a budget on the block or day estimate, §12) |
| Extends | PRD 0015 RF4 (`CostSummary` gains breakdowns), PRD 0030 RF2 (every source gains the session and project dimensions) |
| Date | 2026-10-25 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **session** is one agent conversation (one JSONL file); a **block** is a 5-hour billing window as the vendor's subscription counts it, aligned to the first message after the previous block ended; a **project** is the working directory the agent was started in, stored only as a digest and a person-chosen label. All figures are estimates at API prices, never charges.

## Implementation status (2026-11-05)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.83.0 | `cost.sqlite3` schema 2 (migration 3): `requests` (one row per request: time, model, session digest, project fingerprint, token counts), `projects` (label, opt-in `folder_hint`, `hidden`, first/last seen) and `session_labels`. The migration re-reads each followed file once from its start and adds only the request rows of lines already counted (`files.backfill`), so day totals never change; history whose logs are gone keeps its day totals but has no sessions or blocks. The three readers hash `cwd` (Claude Code lines, Codex `session_meta`/`turn_context`) into a 16-hex fingerprint and drop it (keyed with the per-install key since 0.85.4, PRD 0015 S-03; session, request and log-file digests keyed too since 0.86.10, see the 2026-11-17 re-validation); Gemini CLI has no folder, so its fingerprint is a digest of its own `projectHash`. The disclosure wording is version 2 (new `extracted_v2`/`never_v2` sentences); a source consented under version 1 is reported as `reconsent` and reads nothing until accepted again. `block_start` is not stored: blocks are computed at query time, because the block length is a setting. |
| RF2 | 0.83.0 | `Scanner::sessions` (`cost::sessions` pure): per session digest, source, first/last, duration, models, tokens, cost, requests and the main project; paging; labels with `--cost-label session PREFIX LABEL` or inline in the desktop. |
| RF3 | 0.83.0 | `Scanner::blocks`/`cost::blocks`: a block starts at the hour of the first request after the previous one ended (as `ccusage`), lasts `block_hours` (per source, 1–24; default 5 for Claude Code and Codex CLI, 24 for Gemini CLI), and the provider's 5-hour window of a ready account is attached beside the active block, never merged. `cost::live`: burn rate over the last 10 minutes (`None` under 3 requests or 3 minutes), projection only from 20% elapsed and never for unpriced usage. |
| RF4 | 0.83.0 | `Scanner::projects`/`cost::projects`: label or "Project ab12cd34", "No project" for lines without a folder, hidden projects listed and left out of the totals; `--cost-label project`, `--cost-hide`/`--cost-unhide`; the opt-in folder-name hint (`--cost-folder-hint SOURCE on|off`, Settings), off by default, deleted when turned off, never exported. |
| RF5 | 0.83.0 | `cuotai --cost --by day|model|session|block|project [--top N|--all] [--json]`, `cuotai --cost-live [--source S] [--watch-interval S] [--json]` (in place with `q` on a terminal, NDJSON with `--json`), `--cost-block-hours SOURCE HOURS`; exit codes as `--cost`. The provider window appears only in front ends that hold readings (desktop, TUI details), not in the CLI. |
| RF6 | 0.83.0 | Settings › Advanced › Local cost › **Sessions, blocks and projects**: the per-source block length and folder-name hint, a Days · Models · Sessions · Blocks · Projects segmented control with the KPI header, inline labels, Hide, and the active block's live figures refreshed from the index every 30 s. The card's "This block ≈ $4.10 · 38K tok/min" line under the local usage (tooltip: honest-data suffix and projection); the TUI details show the same line. |
| RF7 | 0.83.0 | `export::v1` `local_cost` gains `blocks[]` (last 7 days), `live[]` (one per source with an active block) and `projects[]` (labels or digest prefixes, never hidden projects or folder names), all omitted when empty; docs updated. The cross-check with PRD 0040's fed session cost is not done (optional, display only). |
| 2026-11-17 re-validation | 0.86.10 | Fixes of RF1.1, RF2.2, RF3.2, RF3.3, RF4.1–RF4.3, RF5.1–RF5.3, RF6.1, RF6.2 and RF7 (1 high, 12 medium, 26 low findings); see "2026-11-17 re-validation" below. |
| Owner verification | — | The nine-item checklist under "2026-11-17 re-validation" (§2.1 of `0000_control.md`). |

### 2026-11-17 re-validation (0.86.10)

The re-validation (Visual, Security, Review) found 1 high, 12 medium and 26 low findings; the fixes ship in 0.86.10, one commit per group:

| Group | Findings | RF | Delivery (0.86.10) |
|---|---|---|---|
| K1 | H-01 (high), M-04, M-08, R-05 (live) | RF3.3, RF6.2 | The live line always carries "+ unknown cost of N tokens" for models without a price, and its money is formatted in the reader's language (one formatter, `cost_part`, shared with every usage line). The projection splits priced and unpriced tokens over the burn window: below half unpriced it reads "≈ $X + unknown cost of about N tokens by 18:53" (`money_partial`); at half or more it projects tokens only and `projected_block_cost` is null (`tokens`). `rate_missing` gives the reason there is no burn rate: too few recent requests, with the count seen, or a block younger than 3 minutes. `export::v1` `live` gains `unknown_tokens`, `projected_tokens`, `projected_unknown_tokens`, `projection_kind` and `rate_missing` (additive). |
| K2 | M-05, R-02 (figure), R-05 (hidden) | RF4.1, RF6.1, RF7 | A hidden project is left out of every total and figure: days, models, sessions, blocks, the live block, the plan value, the card, `--cost --json` and `export::v1` (`hidden_projects`, `hidden_note`). It stays listed, flagged, in the Projects view, and every view says "N hidden projects are not counted in these figures." (`cost.hidden.note`); `--by project` adds the `--cost-unhide` hint. Blocks stay aligned on every request, because the provider's window counts them all; a block with only hidden requests is not listed. The Blocks total is the sum of the listed blocks. |
| K3 | S-01, S-01b, S-02, S-03 (hints, times, orphans), L-08, R-07 | RF1.1, RF4.3, RF7 | Index migration 6: folder names live per source in `project_hints`, and `projects` keeps only the label and hidden flag; first and last seen are computed from the requests, so they follow the retention, a source turned off and an erase. A commit re-reads the settings inside its transaction (`Still`), so a scan already running when the hint is turned off stores no name; turning it off deletes that source's names, truncates the WAL and removes the pre-upgrade `.bak`. Turning a source off deletes its names and turns its hint off. Pruning removes names and projects without requests, unless the project carries a label or the hidden flag. `ProjectSummary.folder_hint` is never serialized; only the desktop's Settings page receives the names, as a `hints` map. The hint and its help texts are protected from user packs. A commit made with a key that a restore has since replaced writes nothing. |
| K4 | S-05 | RF4.2 | Labels refuse control, direction and other invisible characters (`text::is_hidden`, shared with the report filter; `cost.error.label_characters`); folder names drop them; labels and names stored before are cleaned when read. |
| K5 | S-04, S-07, S-03 (future days) | RF1.1 | A log line dated more than 1 hour after the scan is invalid and stores nothing; pruning removes rows dated after tomorrow; `--cost-sources` shows the index size (text and JSON). No row cap: it would remove request rows while the day totals stay, so sessions and blocks would miss requests silently; `docs/advanced.md` states the expected growth (about 1 KB per request). |
| K6 | S-08 | RF6.1 | The desktop's `cost_breakdown`, `cost_set_label`, `cost_set_hidden` and `cost_set_source_options` answer only the Settings window, and labels and hiding take only a full digest (64 hex for a session, 16 for a project). |
| K7 | M-06, L-01–L-06, L-13, R-03, R-04, R-05 (goldens) | RF2.2, RF3.2, RF5.1–RF5.3 | `--by session` names each session's project (`SessionSummary.project_name`) and its requests; project rows show their days, requests and models; a labelled session keeps its time; a range that ends on another day shows the end date; the `[code]` is printed once and explained under the notice ("Cuotai's own code … not the tool's session id"). The active block is listed first everywhere. The provider's window is attached only when exactly one ready account of the source has one. `--top` takes 1 or more and, like `--all`, is refused outside `--by session|project` (exit 1); `--cost-live --source X` with X off exits 2; the in-place view redraws in the alternate screen, cut to the terminal. Golden files cover every `--by` in text. |
| K8 | M-01–M-03, M-09, L-01 (desktop), L-07, L-09–L-11, R-02 (caption), session code (desktop) | RF6.1, RF6.2 | The KPI header wraps instead of clipping, with the unpriced part in its caption; a session shows its name as text with its code, and the label field has a short "Add a label" placeholder; the code is explained under the tabs (protected text); counts are grouped and times use the app's language; ranges across days show the end date; Sessions loads older pages on request ("Show N older sessions", the backend takes an `offset`); the card tooltip names the block length. One polite announcer reads only a changed live line; the panel has no live region and is named by its tab. |
| K9 | S-06 | RF1.1 | Session, request and log-file digests are `HMAC-SHA256` with the per-install key over the earlier SHA-256, as project fingerprints already were (migration 7 rewrites every stored digest and session label in place, without re-reading any log). The file state keeps the plain session digest only sealed under the key, so a resumed read after the upgrade adds no duplicate. Labels backups move to schema 2 (a schema-1 backup's session digests are keyed on import). A backup's key is adopted only while the index holds no row. |
| K10 | M-07, L-12, R-05 (UI) | RF6 | The UI check gains `settings-cost-by-{day,model,session,block,project}` (narrow and wide), `settings-cost-by-off` and three 200% zoom pages; the `?cost` demo shows only states the core produces (a half-unpriced live block, a hidden project with its note, the `hints` map, local times, active block first). |

**Decisions (orchestrator, recorded for the owner):**
- **Hidden means hidden everywhere:** hiding a project leaves it out of every total and figure (sessions, blocks, the live block, days, models, the card, `--cost --json` and `export::v1`). It stays listed, flagged, only in the Projects view, and every view that leaves something out says how many hidden projects are not counted.
- **The 50% rule:** a projection shows money only while less than half of the projected tokens are unpriced; from half on it projects tokens. The line for the block so far always shows the money with "+ unknown cost of N tokens".
- **Retention covers what is derived:** projects, folder names and first/last seen follow the index retention; the person's own labels and hidden flags stay until erased, like session labels.
- **The bracketed code** is Cuotai's own: the first characters of a keyed one-way fingerprint, which `--cost-label` accepts. It is not the tool's session id, which is never kept (the earlier wording that it "matches the tool's session id prefix" was wrong and would have stored the Claude Code log file name).
- **No size cap** on the index, for the reason in K5.

**Deviations, documented:**
- RF1's first row said "16-hex SHA-256 fingerprint"; project fingerprints have been keyed since 0.85.4 (PRD 0015 S-03), and session, request and file digests are keyed since 0.86.10 (K9).
- `--cost-live --json` stays a plain array of blocks; the hidden note appears in its text output only.
- The desktop KPI keeps the `.long` shorter-figure step that the design system's DS-CMP-KPI-1 amendment requires; the figure now wraps.
- Turning the folder-name hint off also deletes `cost.sqlite3.bak`, the migration's rollback copy, as an erase already does.
- `--all` is refused with `--by day|model|block` as `--top` is.

**Open items:**
- **R-06 (low):** projects sort by the priced part only, so a project spending mostly on unpriced models sorts last; sorting by tokens when nothing is priced would change the meaning of "costliest first".
- Index compaction (integer references for files and sessions, shorter digests) to reduce the about 1 KB per request.
- The per-day session count after hiding is approximate when one session mixes hidden and shown projects on the same day and model (only the session count; never tokens or cost).
- The cross-check with PRD 0040's fed session cost (RF7, optional) is still not done.
- Native review of the new catalog strings (listed in each catalog's `pending_review`).
- ~~**Owner decision:** confirm the bracketed-code wording~~: **confirmed by the owner on 2026-11-20**, together with the keyed fingerprint, the hint-off deletion of `cost.sqlite3.bak` and the KPI's shorter-figure step (all three deviations above stand).

**Awaiting owner verification (2026-11-17 checklist):**
1. **Blocks on the real logs.** With Claude Code on, `cuotai --cost --by block` lists today's blocks with the active one first; its window contains now.
2. **Live in place.** `cuotai --cost-live --watch-interval 5` in a normal terminal and in one 10 lines tall redraws in place without growing the scrollback; `q` quits and restores the prompt; exit 0.
3. **Same figures on three surfaces.** `--cost-live`, the desktop Blocks tab and the card line show the same tokens and the same "≈ $X + unknown cost of N tokens" while an unpriced model is in the block, with money in the app's language.
4. **Hiding a real project.** Hide one project in Settings: the card's local usage, the Days tab, `cuotai --cost`, `cuotai --cost --json` and `--json-v1` drop by that project's amount and each says "1 hidden project is not counted"; Count brings it back.
5. **The folder hint.** Turn the hint on, wait for a scan, turn it off, then `grep -a` the folder name and the real project path in `cost.sqlite3`, `cost.sqlite3-wal` and `cost.sqlite3.bak`: nothing found. Turn the source off and on again: the hint switch shows off.
6. **The upgrade migrations on the real index (6 and 7).** Labels and hidden flags survive; `--cost --by session` lists the same sessions with new codes; `--cost-sources` prints the index size; the next scan adds no duplicate requests (day totals equal those before the upgrade).
7. **Re-consent.** If not verified yet: a source enabled before schema 2 asks for the new disclosure once and reads nothing until it is accepted.
8. **VoiceOver on the Blocks tab** (Tauri app): silent across 30 s refreshes while the line is unchanged; reads only the new "This block ≈ …" line when it changes; the panel is named by its tab.
9. **Inline labels in the Tauri app.** Enter and blur save; an empty field removes the label; a label pasted with an invisible direction character is refused with the new message; the code hint is under the tabs; "Show N older sessions" loads the next page.

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
