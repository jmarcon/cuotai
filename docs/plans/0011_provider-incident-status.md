# PRD 0011 — Provider incident status

| Field | Value |
|---|---|
| ID | 0011 |
| Status | In progress |
| Owner | `cuotai-core`, desktop and TUI |
| Consumers | `apps/desktop` (cards, tray), `crates/cuotai-tui`, PRDs 0012 and 0013 |
| Depends on | PRD 0005 (RF1) to tell "provider incident" from "interface changed"; not required |
| Date | 2026-10-19 |

## Implementation status

| Requirement | Status | Version | Delivery |
|---|---|---|---|
| RF1 | Implemented | 0.46.0 | Allocation-free `incidents::source` exhaustively maps 36 providers: 17 have verified public sources with exact component selectors; 19 explicitly return `None`. Statuspage v2, native AWS RSS, Google Cloud JSON, incident.io Widget JSON and xAI RSS verified with real network reads and public-use documentation; anonymized local captures and scope-boundary tests; [source audit and limitations](../providers.md#public-incident-source-registry). |
| RF2 | Implemented | 0.47.0 | `IncidentMonitor` (off by default, `Monitor::incidents().set_enabled`) driven by the `Monitor`'s HTTP client and query semaphore: identified User-Agent, no credentials/cookies/query parameters, no redirects, 5 s timeout, 1 MiB cap, nothing about a failure reaches a report. One cached reading per URL with a 5-minute TTL and single-flight coalescing across accounts and products; a failure keeps the last reading marked `stale`. Only configured providers with a source are queried, never while off. All five formats normalized per scope to `Operational`/`Degraded`/`PartialOutage`/`MajorOutage`/`Maintenance`/`Unknown` with a sanitized title, start and same-host public link; [client contract](../providers.md#status-client-since-0470). |
| RF3 | Implemented | 0.47.0 | A query ending in `Error`, `RateLimited` or `AuthenticationError` while the provider is `Degraded` or worse gains `note.incident.ongoing`; `AuthenticationError` also gains `note.incident.auth_wait` (wait before signing in again, nothing blocked). `Status` unchanged; the note never names the incident as the cause; the failed query reuses the cycle's single status request. |
| RF4 | Pending | — | Desktop display and privacy settings. |
| RF5 | Pending | — | TUI/CLI display and override. |

**Owner decision (2026-10-20):** status queries are opt-in and off until enabled; `--no-status` remains an override. RF1 does not poll, add preferences, change reports or modify either frontend. RF2/RF3 implement the opt-in gate in the core (`IncidentMonitor` starts off); no frontend enables it until RF4 (Settings → Privacy) and RF5 (TUI/CLI) land, so the TUI, desktop, `--once`, `--json` and `export::v1` are unchanged in 0.47.0. The final end-to-end badge/tray acceptance below applies when RF4–RF5 land.

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

## 7. Order of execution and verification

1. **RF1:** verified mapping and table tests.
2. **RF2 and RF3:** client, cache and correlation.
3. **RF4:** desktop.
4. **RF5:** TUI and CLI.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when a simulated provider outage appears as a badge on the card and in the tray, the matching query failure gets the explanatory note without changing its `Status`, providers without a verified source never appear as "operational", and the feature turned off makes no network request.
