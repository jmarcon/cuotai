# PRD 0011 — Provider incident status

| Field | Value |
|---|---|
| ID | 0011 |
| Status | Implemented |
| Owner | `cuotai-core`, desktop and TUI |
| Consumers | `apps/desktop` (cards, tray), `crates/cuotai-tui`, PRDs 0012 and 0013 |
| Depends on | PRD 0005 (RF1) to tell "provider incident" from "interface changed"; not required |
| Extended by | PRD 0022 (RF17.8, with PRD 0025 RF3.1: the RF4.2 outage dot becomes a template-compatible cut-out badge on a monochrome menu-bar icon), PRD 0028 (RF4.5: the local canary checks that fixed status components still exist), PRD 0031 (`incident.state` per account and the worst case in `/v1/limits`), PRD 0033 (stale status readings ignored for the tray dot and expired to `unknown` after about 30 minutes), PRD 0037 and PRD 0038 (one more incident source each) |
| Date | 2026-10-19 |

## Implementation status

| Requirement | Status | Version | Delivery |
|---|---|---|---|
| RF1 | Implemented | 0.46.0 | Allocation-free `incidents::source` exhaustively maps 36 providers: 16 have verified public sources with exact component selectors; 20 explicitly return `None`. Statuspage v2, native AWS RSS, Google Cloud JSON, incident.io Widget JSON and xAI RSS verified with real network reads and public-use documentation; anonymized local captures and scope-boundary tests; [source audit and limitations](../providers.md#public-incident-source-registry). |
| RF2 | Implemented | 0.47.0 | `IncidentMonitor` (off by default, `Monitor::incidents().set_enabled`) driven by the `Monitor`'s HTTP client and query semaphore: identified User-Agent, no credentials/cookies/query parameters, no redirects, 5 s timeout, 1 MiB cap, nothing about a failure reaches a report. One cached reading per URL with a 5-minute TTL and single-flight coalescing across accounts and products; a failure keeps the last reading marked `stale`. Only configured providers with a source are queried, never while off. All five formats normalized per scope to `Operational`/`Degraded`/`PartialOutage`/`MajorOutage`/`Maintenance`/`Unknown` with a sanitized title, start and same-host public link; [client contract](../providers.md#status-client-since-0470). |
| RF3 | Implemented | 0.47.0 | A query ending in `Error`, `RateLimited` or `AuthenticationError` while the provider is `Degraded` or worse gains `note.incident.ongoing`; `AuthenticationError` also gains `note.incident.auth_wait` (wait before signing in again, nothing blocked). `Status` unchanged; the note never names the incident as the cause; the failed query reuses the cycle's single status request. |
| RF4 | Implemented | 0.48.0 | Settings → Privacy switch (off by default, `status_queries` in `desktop.json`, only `true` stored) explaining the public, credential-free requests and listing the queried domains of the configured providers (`Monitor::status_domains`, also while off); the backend shares one `IncidentMonitor` across reloads and runs a status loop beside the scheduler (the core reads a due source at most every 5 minutes). Cards and tray rows/tiles show a badge with tone, icon and the core's `incident.state.*` text for degraded, outage, maintenance or an unreadable (stale) page, with "Open status page" through `open_external`, now also allowing `incidents::is_status_link` URLs (https, verified source host, no port/credentials/query/fragment); details list the full status, start and read time; a tray footer chip lists the providers; the tray icon gets a red dot for a partial outage or worse. Unknown readings show no badge and are never shown as operational. |
| RF5 | Implemented | 0.49.0 | Opt-in `--status` flag or `CUOTAI_STATUS=1` (any non-empty value except `0`), off by default, for the TUI, `--once` and `--json`; `--no-status` always wins. The TUI shares one `IncidentMonitor` across monitor rebuilds and runs a status read every 30 s off the event loop (the core reads a due source at most every 5 minutes). Cards show the RF4 badge (shape, color and the core's `incident.state.*` label, "última leitura" when stale, "Página de status indisponível" for an unreadable page) on the top border, cut so it never covers the name; the details add a `PROVEDOR` block with the full state (operational and unknown too), incident title, start, reading time or failure and the page. `--once` prints `Status do provedor: …` under a flagged account plus the RF3 notes; `--json` keeps the `Snapshot` shape; `export::v1` gains `incident` (state, label, event, page, read time, stale; `null` while off or without a verified source) through `Export::from_cycle(…, &monitor.incident_statuses(), …)`. |
| GEM-01 | Implemented | vNEXT | Removed the Google Cloud status mapping for `gemini_api`: Cloud Monitoring covers only Cuotai's measurement dependency and cannot establish Gemini API health. Google's official AI Studio status page was checked, but its data source is an undocumented internal positional RPC requiring the site's embedded API key, with no verified stable public schema or programmatic-use policy; the registry therefore honestly returns `None`. The exhaustive registry test, status-query test and domain-listing test keep it absent rather than reporting `Operational`. |

**Owner decision (2026-10-20):** status queries are opt-in and off until enabled; `--no-status` remains an override. RF1 does not poll, add preferences, change reports or modify either frontend. RF2/RF3 implement the opt-in gate in the core (`IncidentMonitor` starts off); no frontend enables it until RF4 (Settings → Privacy) and RF5 (TUI/CLI) land, so the TUI, desktop, `--once`, `--json` and `export::v1` are unchanged in 0.47.0. In 0.48.0 (RF4) only the desktop's Settings → Privacy switch turns it on. In 0.49.0 (RF5) the TUI and CLI opt in with `--status` or `CUOTAI_STATUS=1`, and `--no-status` overrides both.

**RF5 decisions:** the TUI/CLI opt-in is a flag or an environment variable, not a TOML key: the TOML is shared with the desktop, whose preference lives in `desktop.json`, and the other TUI-only options (`--notify`, `--no-animations`/`CUOTAI_ANIMATIONS`) follow the same pattern. `--no-status` also applies to the interactive TUI. The card and `--once` follow the RF4 badge rule; operational and plain unknown states appear only in the details. Shapes carry the state (`!` degraded, `▲` partial outage, `✕` major outage, `i` maintenance, `?` unknown or unreadable, `✓` operational in the details); color repeats it. The TUI chrome stays Portuguese and the state label follows `CUOTAI_LANG`. `--json` stays the `Snapshot` (only the existing notes may carry the RF3 note); the provider state enters PRD 0012's contract as `export::v1` `incident`. The status page link is printed in the details and `--once`, not made clickable.

**RF5 validation (2026-10-20):** new tests: `--status`/`CUOTAI_STATUS` opt-in with `--no-status` winning (empty and `0` stay off); a `--once` text snapshot of a failed query during a major outage (status line with title, start and incident page, then the core notes) plus the stale-maintenance and unreadable-page lines and no line for operational or plain unknown; a card text snapshot (badge on the top border right of the name, nothing for an operational provider, "Página de status indisponível" for an unreadable page, nothing at all while off), the stale "última leitura" mark, a narrow card cutting the badge but never the name, and the details block for each state (incident, reading age, failed read with and without a previous reading, nothing while off); `export::v1` carrying only the account's own provider status, `null` while off or unread, with the schema and the regenerated reference. CLI smoke with an isolated HOME and 12 accounts without credentials through a logging HTTPS proxy: default and `--status --no-status` (also with `CUOTAI_STATUS=1`) made no connection and printed the unchanged output; `--status` connected only to the 10 status hosts of the configured providers (OpenAI read once for two products, none for Mistral) and printed no status line while every page was operational; with a refusing proxy every sourced account printed "Status do provedor: Página de status indisponível"; `--json` with status on and off had the same keys and reports. TUI smoke in tmux (120 × 40, temporary HOME): live pages showed no card badge and `PROVEDOR ✓ Operacional`, "lido há 9 s" and the page in the details, Mistral nothing; unreachable pages showed "? Página de status indisponível" on the cards and the failed read in the details; `CUOTAI_STATUS=1 --no-status` and the default made no connection and showed no status. A live incident could not be produced against the real binary; incident badges, the RF3 note and the `--once` incident line are proven by the rendering tests and snapshots above. Formatting, Clippy with `-D warnings`, workspace tests (874) and desktop tests (54) passed for both workspaces; `node --check` passed for all 17 desktop scripts and JSON validation for all three catalogs. No new dependency.

**RF4 decisions:** the card badge appears only for `Degraded`/`PartialOutage`/`MajorOutage`/`Maintenance` (marked "last reading" when stale) or when the latest read of an otherwise operational or unknown page failed ("Status page unavailable"); `Operational` and a plain `Unknown` (e.g. AWS/xAI without an active event) show nothing on the card and are spelled out in the details. State labels come from the core (`IncidentState::label`, `incident.state.*` in the core catalogs) for RF5 to reuse. Only the tray icon indicator needs `PartialOutage` or worse; the popover chip lists every flagged provider. Turning the switch off stops the loop's requests at once and hides every badge and the dot; the cache stays in memory.

**RF4 validation (2026-10-20):** new tests: the status-link allow-list (accepted pages; look-alike hosts, other schemes, ports, credentials, queries, fragments and non-normalized forms refused) and every link the client reports passing it, `status_domains` deduplicated per host and listed while off with no request, `desktop.json` off by default and round-trip, the view attaching only the account's own provider status with the outage flag needing a partial outage or worse, the preference surviving a reinstall, the outage dot only in the icon's corner, and `open_external` refusing look-alikes. Browser smoke (`apps/desktop/ui` with the demo backend, `?status` and `?status=offline`) on the cards in pt-BR, en-US and es: off (no badge), simulated incidents (partial/major outage in red, degraded in amber, maintenance in blue, each with "Open status page") and no network (every source stale: "Status page unavailable" in a muted tone, no outage). In en-US also: the failed Cursor query with the RF3 note and the details' status block (start and read time), both links opening the incident pages, Settings → Privacy with the explanation and the nine demo domains, its switch hiding and restoring every badge, and the tray popover rows, tiles and footer chip. Tauri debug binary with a temporary HOME and one Groq account: off made no HTTPS connection in 12 s; with `status_queries: true` its only connection was to `groqstatus.com`. In the browser demo the core state labels show their Portuguese fallbacks (the core catalog only reaches the Tauri app); the real tray icon with a live outage was not captured on screen, only through the icon-painting test. Formatting, Clippy with `-D warnings`, workspace tests (867) and desktop tests (54) passed for both workspaces; `node --check` passed for all 17 desktop scripts and JSON validation for all three catalogs; CLI `--json`/`--once` with an isolated empty TOML exited 0 with unchanged output. No new dependency.

**Verification decisions:** share URLs across products but filter exact stable IDs (Google explicitly documents product IDs as stable). `AtomRss` and `JsonCustom` carry a dialect so consumers do not guess how to extract component selectors. AWS's published RSS has a five-minute TTL and a documented format-change/completeness caveat; it is not an account health guarantee. OpenAI/Groq/Perplexity use their enabled documented Widget API, not compatibility summaries with null incidents. Unverified/blocked or unscopable sources stay `None`; no HTML scraping or challenge bypass. No new dependency.

**RF1 validation (2026-10-20):** five fixture-backed scope tests passed; a throwaway library consumer enumerated all 36 providers and exercised the shared-source/Codex isolation without a runtime, credentials or network. All 17 JSON/RSS fixtures parse locally. Both workspace `cargo check`, formatting checks, Clippy with `-D warnings`, workspace tests (853) and desktop tests (49) passed; `node --check` passed for all 17 desktop scripts and JSON validation for all three catalogs. CLI `--json` and `--once` with an isolated empty TOML both exited 0 with unchanged output. There is no changed visual surface in RF1; desktop/TUI incident smokes belong to RF4/RF5.

**RF2/RF3 decisions:** the AWS and xAI feeds stay `Unknown` without an active scoped event (their history cannot prove health, per the RF1 audit); Statuspage needs a scoped component in the summary; the incident.io widget and Google Cloud read as `Operational` when no active event touches the scope. An event that lists the scoped components as operational is `Unknown`, so a page-wide impact never leaks into the scope. AWS events are grouped by the GUID's service/region key and decided by the newest update. Statuspage `shortlink`s on other hosts are replaced by `/incidents/<id>` on the source host; any other link must be `https` on the source host (query and fragment dropped) or falls back to the status page. Maintenance is not an incident for RF3. A failed status query waits a full TTL before retrying. No new dependency (RSS is read by a minimal bounded reader).

**RF2/RF3 validation (2026-10-20):** 12 new core tests: `wiremock` sources for every format with scoped active incidents, all captured fixtures through HTTP (quiet pages operational, AWS/xAI history unknown, Cloudflare's page-wide outage kept out of Workers AI), invalid/non-2xx/redirect responses, the 1 MiB boundary (exactly the limit reads, one byte more is refused), a timeout keeping the last reading as stale, cache/coalescing (two accounts plus a second product on one URL and two racing callers make one request; a rebuilt monitor reuses it; a new request only after the TTL), no request while off or for unconfigured/unverified providers, identified credential-free requests, a 500 status source leaving ready and failed quota reports identical to the feature-off run, the end-to-end note on 401/429/500 Groq failures with one status request, the status × state table and text sanitization. A throwaway library consumer with all 36 providers got no status while off and, once enabled, read all 17 real sources (14 URLs) in one pass: 15 operational, AWS Bedrock and xAI unknown, none stale; the repeat call within the TTL was served from the cache. Formatting, Clippy with `-D warnings`, workspace tests (865) and desktop tests (49) passed for both workspaces; `node --check` passed for all 17 desktop scripts and JSON validation for all three catalogs. CLI `--json`/`--once` with an isolated empty TOML exited 0 and with one unconfigured account exited 2, both with unchanged output. No visual surface changed in RF2/RF3; desktop/TUI smokes belong to RF4/RF5.

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## 1. Context

When a query fails, the `Report` only gives a `Status` (`AuthenticationError`, `RateLimited`, `Error`…) and a note. The user does not know whether the problem is their credential, the provider's interface or a public outage. This generates useless support work, such as needlessly swapping credentials during a service outage.

CodexBar polls the providers' status pages and shows incident badges in the menu and an overlay on the icon. Those pages are public and need no credential, which fits the project's rule of being read-only and spending no tokens.

## 2. Problem

- Query failures caused by provider incidents look like failures of Cuotai or of the account.
- There is no context to decide between reauthenticating, waiting or ignoring.

## 3. Goals

- Query public status sources per provider, lightly and cached, and show the current state next to the account.
- Relate a query failure to an ongoing incident, without asserting a cause.
- Work without credentials and at no cost.

## 4. Non-goals

- Monitoring the health of providers that do not publish status in a machine-readable format.
- Scraping HTML pages or using endpoints that require login.
- E-mail or push alerts (PRD 0013 allows a `provider_incident` event for anyone who wants to integrate).
- Changing the report's `Status`: the incident is additional information, not a replacement for the query state.

## 5. Functional requirements

### RF1 — Verified sources and mapping (high priority; prerequisite)

- **RF1.1** Create `incidents::source(provider) -> Option<StatusSource>` with an exhaustive `match` on `Provider`. `StatusSource` carries: the format (`StatuspageV2`, `AtomRss`, `JsonCustom`), a public URL, the scope (components relevant to the API or to the product) and the date of the last verification.
- **RF1.2** Only sources with a public URL **verified at implementation** (format, stability and terms of use) enter. Providers without a reliable source return `None` and do not appear as "no incidents". The initial mapping must cover the providers with a status page in the Statuspage v2 format (`/api/v2/summary.json`), as is common among AI vendors, plus the native AWS and Google Cloud formats, when verified.
- **RF1.3** Providers that share a platform (for example, several products of the same company) share the same source, with a component filter so as not to alarm for unrelated services.
- **Acceptance criterion:** a table test guarantees every `Provider` has a verified source or an explicit `None`; fixtures with anonymized real responses of each format; the `docs/providers.md` document lists the sources.

### RF2 — Status client in the core (high priority)

- **RF2.1** `IncidentMonitor` in `cuotai-core` reuses the `Monitor`'s HTTP client and semaphore, with a short timeout, a maximum response size and an identified `User-Agent`. It sends no credentials, cookies or account data; the log never records the URL with sensitive parameters.
- **RF2.2** Cache per source with a 5-minute TTL and a single query per source per cycle, even with several accounts of the same provider. A status failure does not affect the quota query nor the account's `Status` and uses the last known result, marked as old.
- **RF2.3** Normalized states: `Operational`, `Degraded`, `PartialOutage`, `MajorOutage`, `Maintenance`, `Unknown`. The active incident carries a title, a start and a public link to the status page. Free text from the provider goes through truncation and markup removal before reaching the interfaces.
- **RF2.4** Query only when there are accounts of the provider configured and the feature is on.
- **Acceptance criterion:** `wiremock` tests for each format, an invalid response, the size limit, a timeout, cache and per-source coalescing, and a failure with no effect on the quota query.

### RF3 — Correlation with failures (medium priority)

- **RF3.1** If an account's query fails (`Error`, `RateLimited` or `AuthenticationError`) and the provider has a `Degraded` or worse incident, the core attaches a localized note: "The provider reports an ongoing incident; the failure may not be from your account." The message does not change the `Status` and does not say the incident is the cause.
- **RF3.2** An `AuthenticationError` with an active incident suggests waiting before reauthenticating, without preventing the action.
- **Acceptance criterion:** table tests by status and incident state.

### RF4 — Display on the desktop (medium priority)

- **RF4.1** An incident badge on the card (color, icon and text) with an "Open status page" link, opened through the existing allow-listed `open_external`.
- **RF4.2** The tray icon gets an indicator when any configured provider has an incident of `PartialOutage` level or worse.
- **RF4.3** In **Settings → Privacy**: turn the status query on and off (off by default on first use, with an explanation that the app will make public requests to those domains) and the list of domains queried.
- **Acceptance criterion:** a smoke test in three languages with a simulated incident, turned off and with no network.

### RF5 — TUI and CLI (medium priority)

- **RF5.1** The account row shows the badge; the details list the incident. `--once` prints the incident note; `--json` stays the same, and the provider's state enters PRD 0012's contract.
- **RF5.2** The `--no-status` flag turns the status query off in non-interactive mode.
- **Acceptance criterion:** rendering tests and a text snapshot.

## 6. Non-functional requirements

- **Privacy:** public requests to providers' domains reveal the user's IP and the fact that they use that provider; for that reason the feature is opt-in and documented.
- **Robustness:** the `Unknown` state is the right answer to any doubt; "operational" is never asserted without a valid reading.
- **Cost:** light, cached requests with no credential; no effect on quota scheduling.
- **Rights:** respect the pages' terms of use and `robots`/rate limits; no HTML scraping.

### Fix requirements (2026-10-21 review)

Added on 2026-10-23 (owner decision J): defects found by the 2026-10-21 requirements review in this delivered plan, in an area no new PRD covers. They are non-functional requirements of this plan, delivered as `fix` commits.

| ID | Requirement | Acceptance |
|---|---|---|
| GEM-01 | `gemini_api` status scope (open item, owner accepted on 2026-10-23): the Google Cloud status source for `gemini_api` may assert "operational" from a product-wide Cloud Monitoring component that does not cover the Gemini API. Its scope maps to `None` (no source) or, if a source stays, its state is limited to `unknown`, never `operational`; `docs/providers.md` records the reason. | The per-provider source table test expects `None` (or an `unknown`-only scope) for `gemini_api`; no fixture yields "operational" for it. |

## 7. Order of execution and verification

1. **RF1:** verified mapping and table tests.
2. **RF2 and RF3:** client, cache and correlation.
3. **RF4:** desktop.
4. **RF5:** TUI and CLI.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when a simulated provider outage appears as a badge on the card and in the tray, the matching query failure gets the explanatory note without changing its `Status`, providers without a verified source never appear as "operational", and the feature turned off makes no network request.
