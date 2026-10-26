# PRD 0031 — Integrations and automation contract

| Field | Value |
|---|---|
| ID | 0031 |
| Status | Implemented (RF1–RF13 in vNEXT; RF4.2's `best` is a top-level field of `Limits` by decision, see the RF4 row) |
| Owner | `cuotai-core` (`export::v1`, `serve`, `events`, `tray::summary` consumers), TUI/CLI (`--serve`, `--watch`, `--status-line`, `--completions`), desktop (Settings → Integration) |
| Consumers | status bars and scripts (Waybar, SketchyBar, tmux, i3/sway status), `systemd`/`launchd` units, hook authors, shell users, PRD 0025 (status-line text), PRD 0026 (`/v1/stats`) |
| Depends on | PRD 0012 (RF1–RF5: `export::v1`, `LocalServer`, security), PRD 0013 (RF1–RF3: events, `--watch`, hooks), PRD 0025 (RF1: `tray::summary`) for RF2, PRD 0026 (RF1: `stats::summarize`) for RF11 |
| Benefits from | PRD 0029 (RF4: `id` in every contract), PRD 0024 (RF5: `quota_exhaustion_projected`; decision C `best`), PRD 0028 (failure object), PRD 0033 (RF3: `as_of`) |
| Extends | PRD 0012 RF2/RF3/RF4/RF5 (routes, port, HEAD, caching, policy), PRD 0013 RF1/RF2 (new events, exit codes, signals), PRD 0025 RF8 (moved here as RF2) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **recipe** is a tested, versioned snippet that wires Cuotai into another tool. Code references are to `origin/main` at `0ba9c9b`.

## Implementation status (2026-10-23)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | `docs/integrations/` pages and `examples/integrations/` scripts (Waybar, SketchyBar, tmux, i3bar) over `/v1/limits`; a `cuotai-tui` test runs them against fixtures with a fake `curl` and a core test validates the fixtures against the schema. The tmux, Waybar, SketchyBar and i3bar pages also document the serverless `--status-line` variant. |
| RF2 | vNEXT | `cuotai --status-line [--style tightest_percent\|logos_percent] [--account ID]... [--color]` prints one line from the core `tray::summary` after one refresh, with `--once` exit codes; plain text by default, ANSI only with `--color` on a terminal and `NO_COLOR` unset. `--from-server [URL]` (RF2.1) reads `/v1/limits` of a running server through the core's `serve::client` (loopback `http://` only, default `http://127.0.0.1:6738`, no redirects or proxies, bounded body), with the token from `--token-file` or the `--serve` default file when it exists; exit 2 when a selected account is stale, 1 when the server cannot be read. |
| RF3 | vNEXT | `cuotai --completions bash\|zsh\|fish\|powershell` from the clap definition; the completions test checks for `--status-line`, the Homebrew formula installs the bash, zsh and fish scripts and the Scoop manifest prints the PowerShell hint. The AUR package ships the desktop `.deb` only, so it has no CLI to complete. |
| RF4 | vNEXT | `AccountLimit.incident` (`state`, `stale`) and `Limits.summary` (`worst_class`, `tightest`, `next_reset_at`, `worst_incident`); `id` is in `AccountLimit` and in `summary.tightest`; `best` (PRD 0024 RF7, vNEXT) is a top-level field of `Limits` beside `summary`, not inside it, because it ranks accounts per provider rather than summarising the worst case. |
| RF5 | vNEXT | Policy in `docs/export-v1.md` and `docs/events.md` (additive within a schema, 90-day window), every schema enum open (`anyOf` known values or string), `/health` `schemas: [1]`, the three "stale" fields in one table and `showing_last_good` alias. |
| RF6 | vNEXT | `Source::generation`, one cached export per generation (at most 10 s), `ETag: "<build>-<route>"` and `304` for `If-None-Match`; `--serve` and the desktop publish generations. |
| RF7 | vNEXT | `session_expiring` and `quota_reset_soon` events (`--reset-lead MIN`, default 15, 0 off), selectable in `--events` and hooks; the pace event stays PRD 0024 RF5. |
| RF8 | vNEXT | `provider_incident` per provider and status source with `accounts` (the keys of every affected account in configuration order; the first account's stable id travels in `account_id`, since PRD 0029 RF4); `unknown` tracked. |
| RF9 | vNEXT | `cuotai --serve` binds exactly the port (naming a Cuotai holder, suggesting `--port`); `port_fallback` defaults to false; `HEAD` on every route with `Allow: GET, HEAD`. |
| RF10 | vNEXT | `--watch`/`--serve` exit 0 on a signal stop; `--strict-exit` keeps 2. A Windows-only binary test sends `CTRL_BREAK_EVENT` to both and asserts 0, and 2 with `--strict-exit`. |
| RF11 | vNEXT | `GET /v1/stats?range=7d\|30d` (`StatsExport`, `$defs/Stats`) from `stats::summarize`, behind `cuotai --serve --serve-stats` (`ServeConfig::stats`): token required (`404` without one; `--serve-stats` conflicts with `--no-token`), `403` beyond loopback, 6 requests per minute per peer (`429`), the export's redaction on names, `503 history_off`. Desktop: Settings › Advanced › Integrations "Share usage statistics" (`serve.stats` in `desktop.json`, searchable in the four catalogs) sets the desktop server's `ServeConfig::stats`; its source answers from the app's local history. |
| RF12 | vNEXT | On Unix, `SIGHUP` to `--serve`/`--watch` rereads the TOML through `ConfigFile` (keeping `--interval`), builds the new `Monitor` with `Monitor::with_config`, and applies it with `Driver::reload` over `RefreshCycle::reload` (an arbitrary diff: accounts equal to exactly one previous account keep their readings, deadlines and failures; new or changed ones are queried at once), replacing the cycle the server reads in place; with `--hooks`, `hooks.toml` is reread and the rules stay suspended until confirmed on a terminal (refused without one). An invalid file keeps the old configuration and logs why. Windows has no `SIGHUP` and no equivalent (documented); Ctrl+C/Ctrl+Break end the process. Binary test: an added account appears in `/v1/snapshot` and `/v1/limits` with one new query, an invalid TOML keeps serving. |
| RF13 | vNEXT | `Export::from_cycle_with_history` feeds the app's recent samples to the export's pace (`--serve`, `--watch`, `--json-v1`, desktop server and hooks); without history it stays `average`. |

## 1. Context

PRDs 0012 and 0013 gave Cuotai a stable surface: `--json-v1`, the loopback server (`/health`, `/v1/snapshot`, `/v1/accounts/{key}`, `/v1/limits`; `crates/cuotai-core/src/serve.rs:1-20`), `--watch` with NDJSON events and hooks. The 2026-10-21 review and the open items of 0012/0013 list what keeps it from being a dependable automation contract:

- **No recipes or completions** (INT-01, INT-02): people copy shell snippets from issues; `cuotai` has no `--completions`.
- **`/v1/limits` has no provider status and no summary** (INT-03): `AccountLimit` (`export/v1.rs:211-224`) carries class, binding, age and freshness only; a bar script must read the full snapshot to know whether the provider is down or which account is tightest.
- **No versioning policy** (INT-05): `/health` answers one `schema` (`serve.rs:638-645`); the JSON Schema declares closed `enum` lists (22 in `docs/export-v1.schema.json`), so a new enum value breaks strict validators; three different "stale" meanings exist (`AccountExport.stale` = a preserved reading is shown, `export/v1.rs:87-90`; `freshness`, `:81-84`; `incident.stale` = the status query failed, `:119-121`).
- **Every request rebuilds the export** (INT-06): `with_export` asks the `Source` per request (`serve.rs:670-690`), which rebuilds `Export::from_cycle` and rereads the cost index.
- **Missing events** (INT-07): a near session expiry is evaluated (`alerts.rs:303-330`) but skipped by the event engine (`events.rs:627`); upcoming resets (`resets::upcoming`, `resets.rs:199`) produce no event. The pace event is PRD 0024 RF5 (`quota_exhaustion_projected`).
- **Provider-incident events are wrong** (FIX-15): `Unknown` states are dropped (`events.rs:696-704`), so a provider whose status is only ever unknown (AWS, xAI) never resolves; state is tracked per **account** (`events.rs:706`), so two accounts of one provider emit two identical events.
- **Pace differs between app and export** (DOC-05): `account_export` assesses pace with `None` history (`export/v1.rs:280-288`); the app uses recent samples (PRD 0006 RF2).
- **Port and method** (0012 open item): `cuotai --serve` without `--port` falls back to one of the next nine ports (`crates/cuotai-tui/src/serve.rs:95-102`, `main.rs:162-163`; core default `port_fallback: true`, `serve.rs:186`) while the desktop binds exactly; a script can talk to the wrong server. `HEAD` is answered `405` (`serve.rs:631-633`) although the writer already supports header-only answers (`serve.rs:573`).
- **Exit codes and signals** (0013 open item): `--watch` exits 2 when any account ended failing and `--serve` exits 2 when no account was healthy (`crates/cuotai-tui/src/headless.rs:108-122`; `main.rs:157-159`, `:177-178`), so a normal `systemctl stop` marks the unit failed; `SIGHUP` ends `--watch` (`headless.rs:128-158`) instead of reloading.
- **Status line** (moved from PRD 0025 RF8): a one-line text for tmux/SketchyBar from `tray::summary`.
- **Owner decision P:** `/v1/stats` (PRD 0026 statistics) is included in the local server under explicit security rules.

## 2. Problem

- Integrators guess formats, ports and meanings; one contract change can break every bar script.
- Status bars need a small summary and the provider status without parsing the whole snapshot.
- Supervisors treat normal stops as failures, and configuration changes need a restart.
- Some events never fire or fire twice.

## 3. Goals

- Ship tested recipes, completions and a status-line command.
- Make `/v1/limits` enough for a status bar: worst state, tightest account, next reset, provider status.
- Write down and enforce the contract policy (support window, open enums, advertised schemas, one meaning per "stale").
- Serve from a per-generation cache with `ETag`/`304`, accept `HEAD`, bind an exact port everywhere.
- Fire the missing events once per provider, and stop treating a signal stop as a failure.
- Offer `/v1/stats` without widening what a local reader can learn beyond the export.

## 4. Non-goals

- Remote access, multi-user or TLS; the server stays loopback-only (PRD 0012 §4).
- Writing endpoints (`POST`, refresh on demand); the server stays read-only and never queries a provider per request.
- Maintaining plugins inside third-party projects; recipes live in this repository.
- An `export::v2` in this PRD; RF5 only defines how one would be introduced.

## 5. Functional requirements

### RF1 — Tested recipes (medium priority; INT-01)

- **RF1.1** `docs/integrations/` with one page per tool: Waybar (custom module, `/v1/limits` or `--status-line`), SketchyBar (plugin script), tmux (`status-right` with `--status-line`), i3/sway `i3status`-style JSON; each with the minimum Cuotai version and the contract it reads.
- **RF1.2** Scripts under `examples/integrations/` run in CI against fixture exports (`docs/export-v1.example.json`) and a fake `--status-line`, checking the produced text; a contract change that breaks a recipe fails CI.
- **Acceptance criterion:** CI job green on Linux and macOS; each page has a screenshot or literal output.

### RF2 — `cuotai --status-line` (medium priority; moved from PRD 0025 RF8)

- **RF2.1** `cuotai --status-line [--style tightest_percent|logos_percent] [--account ID…]` prints one line from `tray::summary` after one refresh and exits 0/2/1 like `--once`; default selection is every account in TOML order; with `--serve` running it can read `/v1/limits` instead (`--from-server [URL]`), avoiding a second query cycle.
- **RF2.2** Plain text without ANSI by default; `--color` adds ANSI using theme roles when stdout is a TTY and `NO_COLOR` is unset.
- **Acceptance criterion:** a binary test with a fake provider and a text snapshot per style; a `NO_COLOR` test.

### RF3 — Shell completions (low priority; INT-02)

- **RF3.1** `cuotai --completions bash|zsh|fish|powershell` prints the script generated from the `clap` definition (`clap_complete`); the Homebrew formula and the AUR/Scoop packages (PRD 0021) install it.
- **Acceptance criterion:** a test that each shell's script is generated and mentions `--status-line`; packaging smoke installs it.

### RF4 — Status-bar summary in `/v1/limits` (medium priority; INT-03)

- **RF4.1** `AccountLimit` gains `incident` (`state`, `stale`; the `IncidentExport` subset) and `id` (PRD 0029).
- **RF4.2** `Limits` gains `summary`: `worst_class` (most severe `AccountClass`), `tightest` (`id`, `name`, binding `used_ratio`, `resets_at`), `next_reset_at` (earliest future `resets_at` of any plan limit), `worst_incident` (most severe non-stale provider state), and, when PRD 0024 decision C lands, `best`. All additive.
- **Acceptance criterion:** table tests over fixture exports (all ready, one blocked, one stale, an outage, no reading); schema validation.

### RF5 — Contract policy (high priority; INT-05)

- **RF5.1** Policy in `docs/export-v1.md` and `docs/events.md`: additive fields and enum values within a schema number; a removed or redefined field needs a new number; the previous schema is served **at least 90 days** after the next one ships (`/v1/…` stays while `/v2/…` exists; `--json-v1` stays).
- **RF5.2** Every enum in `export-v1.schema.json` and the events schema becomes open (`anyOf` of the known values and `string`, with the known values documented); consumers are told to treat unknown values as "other".
- **RF5.3** `/health` adds `schemas: [1]` (keeping `schema`), and the events stream's first line under `--jsonl` is unchanged (events already carry `schema`).
- **RF5.4** The three "stale" fields are documented in one table with their exact meaning; `AccountExport.stale` gains the alias `showing_last_good` (additive), and `incident.stale` keeps its name; no meaning changes.
- **Acceptance criterion:** schema tests that an unknown enum value validates; a `/health` test; documentation review.

### RF6 — Per-generation cache and `ETag` (low priority; INT-06)

- **RF6.1** The `Source` publishes an `Arc<Export>` with a generation number once per state change (the desktop already publishes a `Feed` on every `Backend::emit`); requests reuse it until the next generation, and ages are recomputed from `generated_at` at answer time.
- **RF6.2** Responses carry `ETag: "<generation>-<route>"`; `If-None-Match` with the same value answers `304` without a body; `Cache-Control: no-store` stays.
- **Acceptance criterion:** server tests: two requests in one generation build one export (counter); `304` on a matching `ETag`; a new generation changes it.

### RF7 — New events (medium priority; INT-07)

- **RF7.1** `session_expiring`: emitted by `events::diff` when PRD 0016's near-expiry alert would fire (`alerts::session_expiring`), once per account and expiry date; payload `expires_at`, `days_left`; never the session value.
- **RF7.2** `quota_reset_soon`: emitted at the PRD 0003 lead time (`--reset-lead MIN`, default 15) for the windows `resets::upcoming` lists, once per window instance (the `DRIFT_TOLERANCE` identity); payload `resets_at`, `window`, `label`.
- **RF7.3** The pace event is **PRD 0024 RF5** (`quota_exhaustion_projected`); this PRD only lists it in the event table.
- **RF7.4** Additive to events schema 1; `--events` and `hooks.toml` accept both names; the no-leak test covers them.
- **Acceptance criterion:** reading-sequence tests for each event (once per instance, restart without repetition when the history gives the previous reading), a `--watch --jsonl` binary test.

### RF8 — Provider-incident events per provider (high priority; FIX-15)

- **RF8.1** `EventState` tracks incident state **per provider** (and status source), not per account; one `provider_incident` event per change, with `accounts: [id…]` listing the affected accounts (additive) and `account_*` fields naming the first in configuration order.
- **RF8.2** `Unknown` is a tracked state: a change from a known state to `unknown` (status source lost) and back emits nothing for `unknown` itself but resolves correctly, so a provider that only reaches `unknown` after an outage reading emits `provider_incident` with `current.state = unknown` once as the resolution. Stale readings are still ignored.
- **Acceptance criterion:** tests with two accounts of one provider (one event), outage → unknown → operational, AWS/xAI fixtures that never have a status source (no event at all).

### RF9 — Exact port and `HEAD` (high priority; 0012 open item)

- **RF9.1** `cuotai --serve` binds exactly `--port` or 6738; `port_fallback` is removed from the CLI path (the core keeps the field for compatibility, default changed to `false`). A taken port is a local error (exit 1) that names a Cuotai server on that port (`serve::is_cuotai_server`) and suggests `--port`.
- **RF9.2** `HEAD` is accepted on every route with the same status and headers as `GET` and no body; `Allow: GET, HEAD` on `405`.
- **Acceptance criterion:** server tests for `HEAD` on each route and on `401`/`404`; a CLI test that a taken port fails with exit 1 and the message; the desktop is unchanged (it already binds exactly).

### RF10 — Exit codes on signal stop (high priority; 0013 open item)

- **RF10.1** `--watch` and `--serve` exit **0** when stopped by `SIGINT`/`SIGTERM` (Ctrl+C/Ctrl+Break on Windows), whatever the accounts' state; 1 stays for local errors.
- **RF10.2** `--strict-exit` restores today's behaviour (2 when an account ended failing, or no account was healthy for `--serve`) for supervisors that want it.
- **RF10.3** AGENTS.md, `docs/events.md`, `docs/local-server.md` and the `--help` texts are updated in the same delivery.
- **Acceptance criterion:** binary tests sending `SIGTERM` with a failing fake account (0, and 2 with `--strict-exit`); a Windows test with Ctrl+Break where supported.

### RF11 — `/v1/stats` (medium priority; owner decision P)

- **RF11.1** `GET /v1/stats?range=7d|30d` answers PRD 0026's `stats::summarize` for the configured accounts: per account `id`, display `name`, `provider`, limit hits, average tightest peak, peak per day, coverage; never raw samples, notes or provider text.
- **RF11.2** Security rules, all required:
  1. **Token required**: the route answers `401` without a valid bearer token even when the server runs with `--no-token` (it is then `404`, not exposed);
  2. **Loopback only**: refused (`403`) whenever `allow_non_loopback` is set, and subject to the existing `Host`, `Origin` and `Sec-Fetch-Site` checks (`serve.rs:13-20`);
  3. **Rate limit**: at most 6 requests per minute per peer address, `429` with `Retry-After`;
  4. **Redaction**: the export's `Scrub` rules (`export/v1.rs:293-305`): no e-mails (even with `include_identity`), no URLs, no credential variable names, no account params;
  5. **Same consent model**: off by default; a separate opt-in (`--serve-stats`, desktop Settings → Integration "Share usage statistics") on top of the server switch; history recording off means `503 history_off`;
  6. The access log records the route and status only (as today).
- **Acceptance criterion:** server tests for each rule (no token, `--no-token`, non-loopback bind, `Origin`, 7th request in a minute, fixture with an e-mail in a note and a URL in a label never leaks), and a schema for the response.

### RF12 — `SIGHUP` reloads (low priority; later)

- **RF12.1** On Unix, `SIGHUP` to `--watch`/`--serve` rereads the TOML through `ConfigFile` and `hooks.toml` (with the existing consent for changed hooks: changed rules are suspended until confirmed on a TTY, or refused without one), applies the edit to the running cycle (`RefreshCycle::apply_edit`), and logs "configuration reloaded"; an invalid file keeps the old configuration and logs the error. Ctrl+Break keeps ending the process on Windows.
- **Acceptance criterion:** binary tests: add an account and send `SIGHUP` (new account appears in `/v1/limits` and events), invalid TOML keeps serving.

### RF13 — Pace in the export uses history (medium priority; DOC-05)

- **RF13.1** `Export::from_cycle` accepts an optional history handle; when given, `pace::assess` uses the same recent samples as the app (PRD 0006 RF2), and `pace.source` tells `average` or `recent`. Without history (`--json-v1` with recording off), the field says `average`.
- **RF13.2** `docs/export-v1.md` documents the rule.
- **Acceptance criterion:** a test that the desktop's view and `/v1/snapshot` give the same pace for one fixture with history.

## 6. Non-functional requirements

- **Security, regression NFRs (delivered by fix commits; keep as regression tests):**
  - **INT-04:** `cuotai --serve` creates or reuses a random token in a `0600` file under the data directory and requires it by default; `--no-token` opts out with a warning; a world- or group-readable `--token-file` is refused or warned; `docs/local-server.md:101` no longer says "same user" without qualification. Regression: another local user gets `401`.
  - **FIX-01:** a hook's timeout kills its whole process tree (process group on Unix, Job Object on Windows), not only the child (`hooks.rs:1348` `kill_on_drop`); regression: a grandchild that sleeps is gone after the timeout.
  - **FIX-02:** hooks run in a private `0700` working directory, not the shared `temp_dir()` (`hooks.rs:1344`); regression: the directory mode and that nothing else is created in `/tmp`.
- **Privacy:** no route, event or recipe exposes tokens, e-mails (unless `include_identity` on `/v1/snapshot`), URLs with query, notes or account params; `/v1/stats` never exposes e-mails at all.
- **Compatibility:** every contract change is additive within schema 1, except the documented CLI behaviour changes of RF9.1 and RF10.1 (minor bump, `Changed` in the changelog; not a TOML/`--json`/`export::v1`/`desktop.json` break).
- **Cost:** no route or event queries a provider; the cache (RF6) reduces work, never increases query frequency.
- **i18n:** `--status-line` uses the core short texts in the active locale; JSON stays language-neutral except `Localized` objects.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `/v1/limits` | `incident`, `id` per account; `summary` | Additive (schema 1) |
| `/health` | `schemas: [1]` | Additive |
| `export::v1` schema | open enums; `showing_last_good` alias; pace with history | Additive |
| HTTP | `HEAD`, `ETag`/`304`, `/v1/stats`, `429` | Additive |
| Events schema 1 | `session_expiring`, `quota_reset_soon`; `accounts` on `provider_incident` | Additive |
| CLI | `--status-line`, `--completions`, `--strict-exit`, `--serve-stats`, `--reset-lead`; exact port; exit 0 on signal | New flags; behaviour change documented |
| `desktop.json` | `serve.stats` | Additive |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | `Limits::summary`, policy enforcement in schema tests, cache and `ETag`, `HEAD`, `/v1/stats` and its rules, events, per-provider incident state |
| TUI/CLI | `--status-line`, `--completions`, exit codes, `--strict-exit`, exact port, `SIGHUP` reload |
| Desktop | "Share usage statistics" switch; nothing else changes |
| Docs/CI | recipes, policy pages, recipe CI job |

## 9. Dependencies and sequencing

RF8, RF9, RF10 are corrections and go first. RF5 (policy) before any further contract change. RF2 needs PRD 0025 RF1 (`tray::summary`). RF11 needs PRD 0026 RF1 and the INT-04 token default. RF4's `best` waits for PRD 0024 decision C; `id` fields wait for PRD 0029 RF4. RF12 last. Wave 5e of `0000_control.md`.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Exact port breaks scripts that relied on fallback | the error names the server on the port and suggests `--port`; changelog entry |
| Exit 0 hides real failures from supervisors | `--strict-exit`; failures still logged and emitted as events |
| `/v1/stats` reveals usage patterns to local processes | token always required, separate opt-in, rate limit, no e-mails |
| Open enums weaken validation | documented "treat unknown as other"; known values listed |
| Recipes rot | CI runs them against fixtures |

## 11. Order of execution and verification

1. **RF8, RF9, RF10** (corrections).
2. **RF5** policy, then **RF4**, **RF13**, **RF6**.
3. **RF7** events; **RF2**, **RF3**, **RF1** recipes.
4. **RF11** after PRD 0026 RF1; **RF12** last.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke with `curl` against `cuotai --serve` and a `--watch --jsonl` run. The requirement is only delivered when a `systemctl stop` of a unit with a failing account exits 0, `/v1/stats` refuses every request without a token, and two accounts of one provider produce one incident event.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Should `--status-line` read from a running server by default | No; explicit `--from-server`, so it works without a server. |
| `quota_reset_soon` lead: shared with the desktop's PRD 0003 lead | Separate flag for `--watch`; hooks configure their own. |

## 13. Decisions (owner, 2026-10-23)

| Topic | Decision |
|---|---|
| Port | **Exact port**, no fallback and no `--port-fallback` opt-in, in the CLI and the desktop (RF9.1). |
| Exit code on signal stop | **0** for `--watch` and `--serve`, with `--strict-exit` for the old behaviour (RF10); documented as a behaviour change with a minor bump, no `BREAKING CHANGE:` footer. |
| `/v1/stats` rate limit | **6 requests per minute per peer** (RF11.2.3). |
