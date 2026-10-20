# PRD 0012 — Local HTTP server and export contract

| Field | Value |
|---|---|
| ID | 0012 |
| Status | In progress |
| Owner | `cuotai-core` (contract and server), TUI/CLI and desktop |
| Consumers | `crates/cuotai-tui` (`--serve`), `apps/desktop` (embedded server), third-party integrations, PRDs 0013 and 0014 |
| Depends on | none; it incorporates the results of PRDs 0005, 0007, 0009, 0010 and 0011 as they are delivered |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-20)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.36.0 | `cuotai_core::export::v1`: `Export::from_cycle(config, &ConfigIdentity, &RefreshCycle, &[ProviderStatus], now, Options) -> Export { schema: 1, generated_at, config_identity, accounts }`, pure (reads the cycle and the given statuses, never queries). `config_identity` is `ConfigIdentity::digest()` (the SHA-256 `models` already keyed by), never the path. `AccountExport` carries `index`, `key` (`Account::key`), `name`, `provider`, `class` (`classify_report`), `report` (the current report, `null` while pending), `age_seconds`, `freshness`, `clock_adjusted`, `stale` and `last_good` (PRD 0010: `{checked_at, age_seconds, metrics}` from `last_good_report`, `stale` exactly when present; it never feeds class, binding or pace), `binding` (PRD 0009 RF3.2), `pace` per metric, parallel to `report.metrics` (PRD 0007 RF4.2), `access` (PRD 0005 RF4.3) and, since 0.49.0, `incident` (PRD 0011 RF5: the provider's status from `Monitor::incident_statuses`, `null` while status queries are off or without a verified source). RF1.2: `docs/export-v1.md` (fields, additive policy, deprecation), `docs/export-v1.schema.json` (maintained by hand; a test validates the exports against it and fails on any undeclared field) and the reference `docs/export-v1.example.json` (exact comparison; `CUOTAI_REGENERATE_DOCS=1` regenerates it). RF1.3: notes, `notes_l10n` (text and parameters) and `source` replace the account's `credential_env`, `*_env` parameters, the provider's default variable and any `UPPER_SNAKE` name with `[redacted]`, and strip URL user, password, query and fragment; `Options::include_identity` (off by default) adds `email` (from the account's `email` parameter, `null` when unknown) and otherwise redacts every e-mail address from notes, including those a provider reported. RF1.4: missing and non-finite values serialize as `null`. Core only: `--json-v1`, the server and the desktop arrive with RF2 to RF5. |
| RF2 to RF5 | — | Not delivered. |

**Owner decisions (2026-10-20):** the local server is off by default and bound to loopback only; the suggested port is 6738, subject to a conflict check when RF2 is implemented.

## 1. Context

Today the only output for machines is `cuotai --json`, which serializes the `Snapshot` (`started_at`, `finished_at`, `reports`) and requires a new full query on every run (`crates/cuotai-tui/src/main.rs`). Status bars and scripts that call it every few seconds would repeat provider queries, with cost and rate-limit risk. There is no versioned schema: any new field is additive and silent.

In the 2026-10-19 market analysis: CodexBar has `codexbar serve` (routes `/usage`, `/health`, a web panel and a token-protected snapshot), OpenUsage exposes `127.0.0.1:6736/v1/limits` and Quotio has a REST API on loopback. CodexBar's ecosystem (Waybar, KDE Plasma, GNOME, SketchyBar, tmux, Stream Deck, physical displays) was born on top of that stable surface.

## 2. Problem

- Integrations need fresh data without triggering queries on every read.
- Without a versioned contract, any change to the JSON can break third-party scripts.
- A badly designed server becomes a leak vector: OpenUsage documents that web pages can also read the local endpoint, which needs a conscious decision.

## 3. Goals

- Define a versioned export contract (`v1`), independent of transport, without secrets.
- Offer a read-only HTTP server on loopback that serves the last known state, without triggering queries per request.
- Provide the same server in the TUI/CLI and in the desktop app, with the logic in the core.

## 4. Non-goals

- Writing: no route changes accounts, credentials or configuration. There is no login nor editing over HTTP.
- Exposure on the local network or the internet (out of scope; see RF3.5).
- Replacing `--json`, which stays the same and compatible.
- A full web panel (may become a future PRD).

## 5. Functional requirements

### RF1 — Export contract v1 (high priority; prerequisite)

- **RF1.1** Create `export::v1` in `cuotai-core` with `Export { schema: 1, generated_at, config_identity, accounts: Vec<AccountExport> }`. `AccountExport` carries: `index`, `key` (stable identity, the same as in PRD 0003, RF1.3), `name`, `provider`, `class` (`AccountClass`), `report` (the current `Report`, without credentials), `age_seconds`, `freshness`, `stale: bool` and, when the corresponding PRDs exist, `binding` (0009), `pace` per metric (0007), `access` (0005) and `incident` (0011).
- **RF1.2** The schema is additive within version 1 and documented in `docs/export-v1.md`, with a JSON Schema generated or maintained alongside the code. Removing a field or changing its meaning requires `schema: 2`. A test compares the output with a reference file to prevent accidental breakage.
- **RF1.3** No secrets: no credentials, no names of credential environment variables, no authenticated URLs and no notes containing a response body. Account e-mails only appear if the user enables "include identity" (default: omitted).
- **RF1.4** Missing values stay `null`; they never become zero.
- **Acceptance criterion:** contract tests (reference file), a no-secrets test (a scan for token patterns on a fixture account) and `null` in missing fields.

### RF2 — Server in the core (high priority)

- **RF2.1** `serve::LocalServer` in `cuotai-core`, behind a `serve` feature (off by default in the core and on in the binaries that use it), on `tokio` and a minimal HTTP library chosen at implementation (through Cargo, with no JavaScript dependencies). It listens on `127.0.0.1` by default, with a configurable port (suggested default 6738, subject to checking for conflicts), and picks an alternative port if it is taken.
- **RF2.2** `GET`-only routes: `/health` (`{"status":"ok","version":"...","schema":1}`), `/v1/snapshot` (the full `Export`), `/v1/accounts/{key}` and `/v1/limits` (a summary version: per account, the binding metric and the age). Any other method answers `405`; unknown routes, `404`.
- **RF2.3** The server reads from the state already maintained by the `RefreshCycle`; it **never** triggers a provider query per request. If there is no data yet, it answers `503` with `Retry-After`. An optional `?refresh=1` parameter does not exist in this version.
- **RF2.4** Defensive limits: header and URL size, number of simultaneous connections, time per request, and the response is always `Content-Type: application/json` and `Cache-Control: no-store`.
- **Acceptance criterion:** integration tests with `reqwest` against an ephemeral port: routes, forbidden methods, `503` with no data, no provider queries during 100 requests, respect for the connection limit.

### RF3 — Server security (high priority)

- **RF3.1** Bound only to loopback (`127.0.0.1` and `::1`). Requesting another bind address requires an explicit flag and results in a configuration error, not in silent exposure.
- **RF3.2** An optional token (`Authorization: Bearer`) generated by Cuotai and stored in the OS vault, not in the TOML. Without a token, the server only accepts requests whose origin is loopback and that **do not** carry a browser `Origin` header.
- **RF3.3** Protection against reads by web pages: no `Access-Control-Allow-Origin` by default (blocks reads from other origins) and a check of the `Host` header against `127.0.0.1`/`localhost` to mitigate *DNS rebinding*. An "allow browsers" mode can be turned on explicitly, with a warning of what that means.
- **RF3.4** No route returns credentials, and the access log does not record authorization headers or parameters.
- **RF3.5** Access over the local network is out of scope. Whoever needs it must do so through their own tunnel.
- **Acceptance criterion:** *DNS rebinding* tests (a strange `Host` refused), a browser `Origin` refused, an invalid token (`401`), a non-loopback bind refused without the flag and no secrets in responses and logs.

### RF4 — TUI/CLI (high priority)

- **RF4.1** `cuotai --serve [--port N] [--token-file FILE]` runs in the foreground without opening the TUI, keeps the core's refresh cycle and serves the data. It shuts down cleanly on `SIGINT`/`SIGTERM`. Running without an interactive terminal is allowed.
- **RF4.2** Exit codes: 0 normal shutdown; 1 local error (port, configuration, token); 2 if the cycle ended up with no healthy account until shutdown.
- **RF4.3** `--once`/`--json` stay identical to the current ones; a new `--json-v1` emits the full `Export`.
- **Acceptance criterion:** a smoke test with `curl` against `--serve`, clean shutdown and exit codes.

### RF5 — Desktop (medium priority)

- **RF5.1** In **Settings → Integration**: turn the server on and off (off by default), port, token (generate, copy, revoke), "allow browsers", and the effective address. The server uses the app's same refresh cycle; it does not create a second one.
- **RF5.2** Server state (on, port, bind error) shown clearly, including a port conflict with a `cuotai --serve` instance.
- **Acceptance criterion:** a smoke test on the packaged app: turn it on, query with `curl`, revoke the token, turn it off and confirm that the port closes.

## 6. Non-functional requirements

- **Privacy:** the server serves only what `--json` already exposes, plus derived fields, and no credentials. The documentation makes clear that any local process of the same user can read the data.
- **Stability:** a versioned contract, reference tests and a documented deprecation policy.
- **Cost:** no request to the server causes a provider query.
- **Dependencies:** the HTTP library comes in through Cargo, behind the `serve` feature, and is recorded in `THIRD_PARTY_NOTICES` and in the About screen.

## 7. Order of execution and verification

1. **RF1:** the contract and reference tests, without a server.
2. **RF2 and RF3:** the server with security and integration tests.
3. **RF4:** `--serve` and `--json-v1`.
4. **RF5:** desktop.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when a script reads `/v1/snapshot` repeatedly without causing any additional provider query, a hostile web page cannot read the data, the `v1` schema is protected by a reference test and no response or log contains credentials.
