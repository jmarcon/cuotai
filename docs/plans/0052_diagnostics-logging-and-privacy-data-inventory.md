# PRD 0052 — Diagnostics, logging and privacy data inventory

| Field | Value |
|---|---|
| ID | 0052 |
| Status | In progress (RF3.6 delivered in vNEXT; the rest of RF3 overlaps PRD 0046 RF4, see §13 and `0000_control.md` §6) |
| Owner | `cuotai-core` (`diagnostics`, `logging`, `privacy`, `network`, erasure orchestration), desktop and TUI/CLI |
| Consumers | maintainers and security reporters; desktop Settings/About; `cuotai` CLI and TUI; every provider, OAuth flow, public-status reader, local store and extension integration |
| Depends on | PRD 0005 RF1 (access descriptors), PRD 0006 RF4 (history export/erase), PRD 0012 RF3 (serve token), PRD 0013 RF3 (hook log), PRD 0021 RF8.1 (install-source detector) |
| Benefits from | PRD 0020 (Settings rows), PRD 0022 (desktop refresh and dialogs), PRD 0027 RF1 (provider catalog modes), PRD 0028 RF1/RF2 (schema diagnostics and issue drafts), PRD 0046 RF4 (`cuotai --doctor`, which overlaps RF3; see §13) |
| Extends | PRD 0006 RF4.4 (TUI/CLI export and erase parity), PRD 0010 RF1 (server-clock skew), PRD 0012 RF3 (diagnostics and total erasure), PRD 0019 RF1 (security-report evidence), PRD 0021 RF8.2 (one-command purge), PRD 0028 RF2.2 (`Report.source` privacy) |
| Date | 2026-10-27 |

> **Note on terminology:** “RF” (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **diagnostic** is an offline, user-requested description of this installation, never telemetry. A **data artifact** is a Cuotai-owned file, directory or vault namespace. Code references are to `origin/main` at `7a1d93c3`.

## Implementation status (2026-10-28)

Written on 2026-10-27 as PRD 0042 by the 2026-10-25 PRD audit against `origin/main` (0.80.0) and renumbered 0052 when it moved onto `integration/next`, whose 0040–0049 are other plans. This table was re-evaluated on 2026-10-28 against `integration/next` at `18009a04`; the §1 code references still point at `origin/main` at `7a1d93c3`, and the current location is named here where it moved.

| RF | Version | Delivery |
|---|---|---|
| RF1 | — | Proposed. No logging facade or dependency; product messages are still `eprintln!`. |
| RF2 | — | Proposed. No desktop panic hook; startup still ends in `.expect("erro ao iniciar o Cuotai")`. |
| RF3 | RF3.6 vNEXT | RF3.6 delivered: `providers::Data::new` takes the source as `&'static str`, so no value from the params or a response can reach `Report.source`; Cline reports `https://api.cline.bot/api/v1/users/{user_id}/balance` or `…/organizations/{organization_id}/balance` (its note names the same template), Xiaomi MiMo's session source names the cluster as `{region}`, and the sources built from an injected base (OpenRouter, ElevenLabs, Kimchi, the MiMo platform) are now literals. The audit also took runtime identifiers out of notes: Alibaba Cloud billing no longer echoes the `AccountID`, Cursor neither the device id nor the `/api/auth/me` e-mail and name, Azure OpenAI neither the resource path nor the subscription id, Perplexity not the member e-mail (the Perplexity note moved to the id-free `note.inference.perplexity_member_filtered`). Seeded `wiremock` tests prove the ids still reach the request but no report text (`Data::assert_id_free`); `export::v1` keeps the placeholders. The rest is proposed: no `--diagnostics`, "Copy diagnostics" or `--list-providers --json`. RF3.1, RF3.2, RF3.4 and part of RF3.3 overlap PRD 0046 RF4 (`cuotai --doctor [--json]`, **Check setup**), planned for the next version and undelivered; the owner decides whether one plan absorbs the other before either is built. |
| RF4 | — | Proposed. No `privacy` module or `docs/privacy.md`. |
| RF5 | — | Proposed. The core exporter and eraser exist (PRD 0006 RF4.4, `history/retention.rs`); the CLI and TUI still lack export/erase (only `--cost-erase`), and there is no `--erase-all-data` or vault enumeration. |
| RF6 | — | Proposed. RF6.2 partial: proxies from the environment follow `reqwest`'s defaults, untested and undocumented; bundled webpki roots remain (9router's per-account `tls_ca_file` from `882bc4eb` is not OS trust). |
| RF7 | — | Proposed. No HTTP `Date` comparison. |

## 1. Context

- No logging facade or logging dependency exists in the workspace; `Cargo.toml:16-24` has `reqwest` but no `tracing` or `log`. Product messages are ad-hoc `eprintln!`: the desktop development-store warning and widget no-op are at `apps/desktop/src-tauri/src/backend.rs:991-999,1292-1300`, and a Settings-window failure is at `apps/desktop/src-tauri/src/lib.rs:1012-1019`.
- Desktop startup ends in `.expect("erro ao iniciar o Cuotai")` (`apps/desktop/src-tauri/src/lib.rs:1614-1615`). The TUI alone restores the terminal in a panic hook (`crates/cuotai-tui/src/ui.rs:242-255`).
- The reusable rotation pattern already exists: `hooks::RunLog` writes a private file, moves it to `.1` at 256 KiB and keeps one generation (`crates/cuotai-core/src/hooks.rs:1049-1103`).
- `--list-providers` prints localized columns before any `--json` handling (`crates/cuotai-tui/src/main.rs:430-438`); `--json` is not a conflict of that flag (`:65-75`), so the combination silently remains text.
- History export and erasure exist as desktop commands (`apps/desktop/src-tauri/src/lib.rs:520-555`), while PRD 0006 RF6 gives the CLI only `--history` display. The core already owns `HistoryStore::erase_all` and export (PRD 0006 RF4.4).
- Local state is split across `history/history.sqlite3` (`history.rs:432-440`), `models/models.sqlite3` (`models.rs:396-405`), `cost/*` (`cost/index.rs:156-161`, `cost/settings.rs:261-265`, `cost/prices.rs:413-418`), `notified.json` (`resets.rs:364-366`), hooks, extension consent and desktop files. There is no exhaustive inventory.
- Privacy statements are scattered: updater traffic in `docs/updater-security.md:321-332`, public status traffic in `docs/gui-features.md:55-59`, event payloads in `docs/events.md:145-159`, and widget data in `docs/widgets.md:80-86`.
- Workspace HTTP uses `reqwest` with `default-features = false` and `rustls-tls` (`Cargo.toml:21`). Provider/status traffic shares the client built at `crates/cuotai-core/src/lib.rs:267-273`; OAuth builds another at `oauth/session.rs:33-42`. No response path compares the HTTP `Date` header with local time.
- Cline currently places an organization id or returned user id in `Data.source` (`crates/cuotai-core/src/providers/coding.rs:86-119,121-160`), and `Report.source` is part of the current JSON contract (`model.rs:597-598`).

## 2. Problem

- GUI failures disappear into stderr or panic, and support reports lack one safe, reproducible diagnostic.
- Debug output can accidentally become a second, unreviewed data export.
- People cannot see every network flow and retained local artifact in one maintained document, nor erase all of them reliably.
- Proxy, corporate-CA and headless-vault behavior is implicit, so connection failures are difficult to distinguish from provider failures.
- A wrong local clock corrupts age, reset and retention decisions without a warning from an independent server clock.

## 3. Goals

- One structured, levelled and redacted logging facade owned by the core, with bounded private storage.
- Offline diagnostics shared by CLI and desktop, including install, network and local-data health but no account or metric data.
- One generated privacy/data-flow inventory and one exhaustive artifact registry reused by diagnostics and erasure.
- Complete history-management parity and an explicit, idempotent “erase all data” flow.
- Defined native-root, proxy and headless credential behavior, plus a non-correcting clock-skew warning.

## 4. Non-goals

- Telemetry, automatic crash upload, remote support bundles or sending diagnostics without the user copying them.
- Logging provider response bodies, headers, credentials, metric values or full account snapshots at any level.
- Packet capture, connectivity tests that contact providers, or changing the current `--json`/`export::v1` schemas.
- Automatically correcting the system clock, provider timestamps, readings, history or reset dates.
- Deleting an account TOML, external user-content directory or user-supplied hooks file unless that exact path is separately selected.

## 5. Functional requirements

### RF1 — Core logging facade and redaction (high priority; prerequisite)

- **RF1.1** Add `cuotai_core::diagnostics::logging` over `tracing` and `tracing-subscriber` (with `env-filter`), not `log`: async spans and typed fields are required, and their permissive licenses are allowed by `deny.toml:10-33`. Frontends call `logging::init(AppKind, data_dir)` once; library code emits only through this facade. `CUOTAI_LOG` wins over `RUST_LOG`; valid `EnvFilter` syntax is accepted, invalid syntax falls back to `info` and records one fixed warning, and `off` disables writes.
- **RF1.2** `AppLog` writes UTF-8 lines to `<OS local data dir>/cuotai/logs/cuotai.log`, mode `0600` on Unix, with the `RunLog` policy: at 256 KiB rename to `cuotai.log.1`, replace the prior backup, and never retain more than 512 KiB. Extract the private rotating writer so hooks and app logging use one implementation; logging failure never crashes or blocks a refresh and is exposed by RF3.
- **RF1.3** The facade accepts stable event ids and typed safe fields. Its final formatter rejects or redacts bearer/API/session tokens, cookies, authorization headers, response/request bodies, URL query/fragment/userinfo, e-mail-shaped strings and control characters. Provider slugs, status classes, durations and sanitized path classes are allowed. Account names are omitted below `debug`; at `debug` they are allowed only after e-mail redaction. No `Debug` formatting of a request, response, config, `Report`, token or arbitrary error is accepted.
- **RF1.4** Replace product `eprintln!` by an explicit presenter plus a log event: development SQLite warnings (`main::run`, desktop `Inner::install`) are `WARN`; startup/window failures are `ERROR`; an absent widget file is `DEBUG`; serve access/start/stop lines are `INFO` (port fallback `WARN`); watch start/stop is `INFO`; dropped hooks, failed hooks, history degradation and non-ready shutdown are `WARN`; top-level CLI failures are `ERROR`. Human lifecycle text that is part of the CLI contract still goes to stderr through `cli::message`, never stdout; `xtask` output is outside the runtime facade.
- **Acceptance criterion:** pure unit tests run a seeded token, cookie, query URL, body, e-mail and account name through every level/field type and find none in either rotated file; a CLI smoke proves machine stdout is unchanged and the mapped stderr messages still appear.

### RF2 — Crash capture and native startup errors (high priority)

- **RF2.1** `logging::install_panic_hook(AppKind)` chains the prior hook after writing one bounded panic event: app kind, version, thread name, source file basename/line and a redacted payload of at most 2 KiB. It never writes a backtrace, environment, full path or process memory. The TUI installs it before its existing terminal-restoration hook; the desktop installs it before constructing `Backend` or Tauri.
- **RF2.2** Replace desktop `.expect` with explicit handling of `Builder::run`: log the sanitized failure and call `startup_error::show`, a platform-native modal independent of the webview, saying that Cuotai could not start and naming the app-log path. The dialog never includes the raw Tauri error. If the dialog itself fails, stderr receives the same fixed message and path.
- **RF2.3** Panics in spawned desktop tasks use the same hook; recoverable task errors remain normal `ERROR` events and do not show a crash dialog. A startup failure returns a non-zero process status rather than unwinding.
- **Acceptance criterion:** a desktop subprocess smoke with a test-only injected setup error observes non-zero exit, one redacted log record and the fake native-dialog adapter call; a pure hook test proves the TUI restoration hook is still chained.

### RF3 — Offline diagnostics and machine-readable provider catalog (high priority)

- **RF3.1** `diagnostics::collect(Context) -> DiagnosticReport` performs no network request and reads no secret value. `cuotai --diagnostics [--json]` prints it and exits `0` when all checks are available, `2` when one or more checks are unavailable/degraded, and `1` only for invalid arguments or inability to render. It conflicts with query, watch, serve, history/cost actions and `--list-providers`. Human output is localized; JSON is stable `{ "schema": 1, ... }`.
- **RF3.2** Fields are version/build target/profile, PRD 0021 RF8.1 `InstallSource`, OS/version/architecture, effective locale, home-abbreviated config path and `ConfigIdentity::digest`, provider slugs grouped with account count, access level and status-class counts only (`not_checked` without a live desktop cycle), vault availability, sanitized effective proxy state, log health, and server/hooks/extensions state (`off`, `on`, `error`, `unavailable`; counts only). It never carries account names/e-mails, credential or environment-variable names, notes, URLs with paths/query, incident text or metric labels/values.
- **RF3.3** Every `privacy::artifacts()` entry appears with logical id, home-abbreviated path, existence, regular-file/directory kind, byte size, native permission summary (octal mode on Unix; owner/other ACL summary on Windows) and schema version (`PRAGMA user_version`, JSON `schema`, or explicit `unversioned`/`not_applicable`). The registry covers the selected TOML; `desktop.json`; history/models/cost SQLite and sidecars; cost settings/pricing; `notified.json`; `widget.json`; `last-config-{tui,desktop}.json`; extension tree and `consent/extensions.json`; hook consent/work/run logs; serve token file; app logs; and the `cuotai.oauth`/`cuotai.serve` vault namespaces as metadata-only artifacts.
- **RF3.4** Desktop Settings/About offers **Copy diagnostics**, built from the same report plus its current status-class counts. The exact preview is visible before clipboard write. Clipboard failure is shown locally; nothing opens a browser or sends data.
- **RF3.5** `cuotai --list-providers --json` becomes real JSON schema 1: each entry carries stable `slug`, `label_key`, connection `modes`, PRD 0005 access `level`, `metered`, and model-list/incident-support classes. Plain `--list-providers` stays human text; both are offline and exhaustive over `Provider::ALL`.
- **RF3.6** Provider `Report.source` must be a static public origin/path template with runtime identifiers replaced by named placeholders. Cline uses `/organizations/{organization_id}/balance` or `/users/{user_id}/balance`; no configured organization id or returned user id reaches `--json`, details, widgets or diagnostics. Add the invariant to PRD 0028 RF2.2’s seeded privacy test.
- **Acceptance criterion:** pure serialization tests seed every forbidden value and scan both formats; a CLI smoke covers exit `0/2/1` and `--list-providers --json`; a browser-free desktop UI script previews/copies the same JSON; Cline `wiremock` tests prove request ids work but output contains neither id.

### RF4 — Generated privacy and data-flow inventory (high priority)

- **RF4.1** `cuotai_core::privacy::inventory()` is the single typed manifest. Network rows contain host class (provider API, OAuth, public status, updater, user-approved extension/browser), trigger, data sent, default on/off and owning PRD. Local rows contain artifact id/path template, content class, expected mode, schema, retention and exact erase command/desktop action. Extensions and hooks explicitly state that approved child programs may use the user’s network independently.
- **RF4.2** A core drift test renders `docs/privacy.md` between generated markers, compares normalized LF text, and regenerates only with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core privacy_doc_matrix`, matching `access::tests::providers_doc_matrix_matches_the_descriptors` at `access.rs:1285-1310`. Tests also prove every `privacy::artifacts()` entry has one local row and every core HTTP client purpose has one network row.
- **RF4.3** `README.md` links `docs/privacy.md`. The duplicate inventories in `docs/updater-security.md`, `docs/gui-features.md`, `docs/events.md` and `docs/widgets.md` become short feature-specific statements plus links; they retain only details unique to those features. `SECURITY.md` asks reporters to attach `cuotai --diagnostics --json` output after reviewing it, while continuing to forbid secrets and raw config.
- **Acceptance criterion:** the core drift test fails after adding an artifact or network purpose and passes only after regeneration; an xtask link test resolves every PRD/doc link and confirms the four old documents point to the generated inventory.

### RF5 — History parity and erase all data (high priority)

- **RF5.1** Extend PRD 0006 RF4.4 with CLI `--history-export PATH --history-format csv|json [--history ACCOUNT] [--history-days N]` and `--history-erase ACCOUNT|all [--yes]`, using the existing core exporter/eraser and exit `0/2/1`. The TUI History/details flow exposes Export and Erase, prompts for format/path, lists the selected account or all history, and requires the localized typed confirmation; it contains no storage rule.
- **RF5.2** `privacy::erase_all(EraseOptions) -> EraseReport` consumes the artifact registry, stops/locks writers, preflights vault and filesystem access, then removes history, models, all cost state, notifications, widget, last-config records, extension consent/default extension tree, hook consent/work/run logs, serve token file, app logs, `desktop.json`, and all entries in vault services `cuotai.oauth` and `cuotai.serve`. Missing items are success; every failure is a per-id fixed error, never a path containing a secret. A successful desktop erase exits before workers recreate files.
- **RF5.3** `cuotai --erase-all-data [--yes] [--include-config]` and desktop **Erase all Cuotai data** first show the same ordered inventory. Without `--yes`, a terminal requires the exact localized confirmation word; without a terminal it refuses. Desktop requires typing that word. `--include-config`/an off-by-default desktop checkbox adds only the selected accounts TOML and adjacent `hooks.toml`; absent it, no TOML or external user-content directory is touched.
- **RF5.4** OAuth service-wide erasure is not inferred only from the current TOML: `SystemTokenStore::delete_all()` maintains/migrates an opaque-id index and uses the platform vault’s service search so orphaned `cuotai.oauth` entries are removed too. No id or token is printed. If service enumeration is unavailable, preflight refuses before deleting files and points to the platform vault remedy; it never reports a partial erase as complete.
- **Acceptance criterion:** pure core tests over a temporary complete artifact tree and fake vault prove idempotence, config preservation/opt-in deletion and retry after a partial failure; CLI smokes cover TTY/non-TTY and `--yes`; a `TestBackend` TUI render and browser-free desktop UI script cover the complete typed list and confirmation.

### RF6 — Native trust, proxies and headless credentials (medium priority)

- **RF6.1** Replace workspace `reqwest` feature `rustls-tls` with `rustls-tls-native-roots`: provider, OAuth and public-status clients trust the OS root store, including administrator-installed corporate roots, and do not silently fall back to bundled webpki roots. `network::client(ClientPurpose, timeout)` centralizes redirect-off, timeouts, user agent, proxy handling and diagnostics; startup reports a fixed trust-store error when no usable roots load.
- **RF6.2** Preserve reqwest’s environment behavior for `HTTPS_PROXY` and `NO_PROXY` (and lowercase equivalents) across all three client purposes. Proxy credentials/userinfo are accepted by reqwest but never logged or diagnosed. Diagnostics reports only source variable, sanitized scheme/host/port and `proxied`/`bypassed`/`invalid`; it never emits raw `NO_PROXY` entries. `docs/platforms.md` documents CONNECT visibility, TLS interception through the native store and process-restart requirements after env changes.
- **RF6.3** `docs/platforms.md` states support explicitly: native macOS/Windows/Linux with an available system vault is supported; SSH, headless Linux and WSL without a D-Bus Secret Service session support `credential_env` accounts (and `--serve --token-file`) only; OAuth/user-session vault flows and desktop UI are unavailable. The existing Linux vault error (`docs/platforms.md:28-39`) gains a direct `credential_env` remedy. There is no plaintext fallback.
- **Acceptance criterion:** `wiremock`/local-proxy integration tests prove provider, OAuth and status requests use `HTTPS_PROXY`, matching `NO_PROXY` bypasses it, proxy userinfo is absent from logs/diagnostics, and a test native CA is accepted only through the configured native-root fixture; platform error tests name the environment-variable route.

### RF7 — HTTP server-clock skew warning (medium priority)

- **RF7.1** `network::ClockSkew::observe(headers, received_at, purpose)` parses a valid RFC 7231 `Date` from provider, OAuth and status responses. Samples more than 5 minutes from local UTC produce one process-level warning with direction and rounded magnitude; absent/invalid headers are ignored. Multiple hosts are coalesced, and a later sample within 5 minutes clears the warning after two agreeing responses.
- **RF7.2** The warning appears in desktop/TUI banners, `--once` stderr and RF3 diagnostics. It never changes `Utc::now`, `checked_at`, `resets_at`, history cutoffs, pace, freshness or notification scheduling; `--json` and `export::v1` remain unchanged. Logs carry only purpose, direction and rounded seconds, never the response URL/header.
- **RF7.3** This extends PRD 0010 RF1.2: its existing “clock adjusted” result for a future reading remains, while server evidence warns before or after a reading. Text advises enabling automatic system time and retrying; it never claims which clock is authoritative from one server.
- **Acceptance criterion:** `wiremock` tests with injected local time cover exact ±5-minute boundaries, malformed/missing dates, coalescing and clearing; `TestBackend` and browser-free desktop scripts show the warning; tests assert every reading/export value is byte-for-byte unchanged.

## 6. Non-functional requirements

- **Privacy:** redaction is fail-closed; diagnostics and logs contain no secret, e-mail, account name below debug, metric value, body or authenticated/query URL. Paths abbreviate the home directory.
- **Boundedness:** logs keep at most two 256 KiB files; diagnostic strings and lists have explicit limits; inventory rendering is deterministic.
- **Availability:** logging, diagnostics and privacy-document generation make no network request. A logging failure cannot fail a refresh; an erasure failure is visible and retryable.
- **Security:** private artifacts use `private_fs` (`0700` directories, `0600` files on Unix); native trust keeps HTTPS and the existing `transport::is_secure` fail-closed guard.
- **Compatibility:** current `--json`, `export::v1`, events and TOML do not change. New diagnostic/provider JSON starts at schema 1. New CLI modes conflict explicitly rather than being silently ignored.
- **Accessibility and i18n:** desktop confirmations/dialogs are keyboard-operable and screen-reader labelled; user-facing text resolves from core catalogs in every supported locale; JSON keys and confirmation semantics remain stable.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| Environment | `CUOTAI_LOG`, `RUST_LOG`, documented `HTTPS_PROXY`/`NO_PROXY` | Additive; `CUOTAI_LOG` has precedence |
| Files | `logs/cuotai.log[.1]`; generated `docs/privacy.md`; opaque vault index | New, private and bounded |
| CLI | `--diagnostics`, diagnostic JSON schema 1, provider catalog JSON schema 1, history export/erase, erase-all | Additive flags; explicit conflicts |
| Desktop/TUI | Copy diagnostics, log/startup failure UI, history parity, erase-all, clock warning | Additive presentation |
| `Report.source` | Runtime ids replaced by placeholders | Privacy fix; same string field/type |
| HTTP trust | Native OS roots replace bundled webpki roots; environment proxy contract documented | Connection behavior change for machines whose stores differ |
| Existing exports/config | no shape change; TOML preserved by default | Compatible |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | tracing facade/redactor, rotating writer, panic record, diagnostic/artifact/network manifests, erasure orchestration, proxy/trust client, clock-skew rule |
| Desktop | native startup dialog, Copy diagnostics, current status input, typed erase UI, history actions and warning presentation |
| TUI/CLI | explicit stderr presenter, diagnostic/catalog/history/erase flags, TUI history flow and clock warning |
| Documentation | generated `docs/privacy.md`; README, SECURITY and feature-doc links; proxy/headless matrix in `docs/platforms.md` |

## 9. Dependencies and sequencing

RF1 is first because RF2, RF3 and RF7 use its safe sink. RF3’s artifact registry precedes RF4 and RF5. RF6’s shared client precedes RF7. History parity can land with RF5 after PRD 0006 RF4.4 (already delivered). Desktop presentation follows PRD 0020’s Settings component patterns and PRD 0022’s refresh patterns but does not wait for them. Suggested delivery is wave 5b for core/CLI and the matching desktop lane in 5c.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Redaction gives false confidence around arbitrary strings | typed fields, final fail-closed formatter, seeded-secret tests and no body/error `Debug` |
| Logging recursively fails or blocks async work | bounded non-blocking sink, one fixed health state, no logging from its writer |
| Native roots break a machine that relied on bundled roots | explicit trust diagnostic and documented OS-store repair; no unsafe fallback |
| Erasure races active writers or leaves vault data | stop/lock writers, preflight, service-wide vault deletion, per-artifact report and exit on success |
| Proxy diagnostics expose internal topology | sanitized active endpoint only; never raw bypass list or userinfo |
| One malicious/incorrect server `Date` creates noise | warning only, 5-minute threshold, coalescing and two-sample clear; never clock correction |

## 11. Order of execution and verification

1. **RF1**, then **RF2**: facade, redaction, rotation, call-site migration and crash/startup handling.
2. **RF3**: artifact registry, diagnostics, provider JSON and Cline source fix.
3. **RF4**: generated inventory and documentation cutover.
4. **RF5**: history parity, erasure orchestration and all three presentations.
5. **RF6**, then **RF7**: shared network client, trust/proxy contract and clock observation.

At each step run workspace format, Clippy and tests plus desktop equivalents when touched. Run `node --check` and the browser-free scripts for changed desktop JavaScript, CLI subprocess smokes for each flag, `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core privacy_doc_matrix` followed by the plain drift test, a real proxy smoke, and a real desktop startup-failure/erase preview. Delivery requires a copied diagnostic and both log generations to pass the seeded-secret scanner, a complete erase to leave only an unselected TOML, and a skewed `Date` to warn without changing a reading.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Should diagnostics include exact home paths? | No. Preserve the path structure but replace the home prefix with `~`/`%USERPROFILE%`; the identity digest distinguishes configs. |
| Should a user-provided external extensions or user-content directory be erased recursively? | No. Remove Cuotai’s consent/reference only and list the external path as preserved; require separate manual deletion. |
| Should native-root failure fall back to webpki? | No. Fail with the trust diagnostic; a fallback would make corporate behavior differ silently. |
| Should `--yes` imply deleting the TOML? | No. Only `--include-config` or the desktop checkbox does so. |

## 13. Decisions needed from the owner

No owner decision blocks implementation. This PRD chooses `tracing`, a default `info` file filter, one 256 KiB backup, OS-native roots without bundled fallback, a 5-minute clock threshold, home-abbreviated diagnostic paths, and config preservation by default. The owner may revise those policy choices before implementation; otherwise they are the delivery contract.

**Overlap with PRD 0046 (added on 2026-10-28, when this PRD moved onto `integration/next`):** PRD 0046 RF4 (`cuotai --doctor [--json]`, offline and redacted, and the desktop **Check setup**) covers most of RF3.1, RF3.2 and RF3.4 and part of RF3.3. Neither is delivered. Before either is built, the owner picks one of:

| Option | Effect |
|---|---|
| **A. 0046 RF4 owns the command** | `--doctor` is the single diagnostic command; RF3.1/RF3.2/RF3.4 become additions to it (log health, proxy, artifact registry, "Copy diagnostics"), and `--diagnostics` is not added. |
| **B. This PRD owns it** | RF3 delivers `--diagnostics`; 0046 RF4 keeps only its setup checks and reuses RF3's report. |

Recommendation: **A**, because 0046 is already planned for the next version and one command is easier to document.

## 14. Integration notes (for 0000_control.md)

**Proposed §2 row:** `0052 | Diagnostics, logging and privacy data inventory | Proposed | — | Strong: 0005 RF1, 0006 RF4, 0012 RF3, 0013 RF3, 0021 RF8.1 | Weak: 0020, 0022, 0027 RF1, 0028 RF1/RF2 | One redacted diagnostic/logging contract, generated data-flow inventory, total erasure, network-environment support and clock-skew warning across every frontend.`

**Suggested wave:** 5b as a new diagnostics/privacy lane for core and CLI; desktop bindings in wave 5c.

**Existing PRDs extended:**
- **PRD 0006 RF4.4:** adds concrete TUI/CLI export and erasure flows; no non-goal is superseded.
- **PRD 0010 RF1:** adds independent HTTP `Date` skew evidence while preserving the no-schedule-change and no-clock-correction rules.
- **PRD 0012 RF3:** diagnoses and erases the serve token/file and reports local-server state without exposing the token.
- **PRD 0019 RF1:** adds reviewed diagnostics to the private security-report template; telemetry remains a non-goal.
- **PRD 0021 RF8.2:** adds the explicit all-data purge path while preserving uninstall data by default.
- **PRD 0028 RF2.2:** extends seeded privacy to diagnostics and removes runtime ids from `Report.source`; automatic report sending remains a non-goal.

**Fix requirements for delivered plans (decision J):** none left outside this PRD. The Cline `Report.source` identifier leak is explicitly owned by RF3.6 and recorded as an extension of PRD 0028 RF2.2; the desktop tray-without-AppIndicator fallback remains a separate PRD 0025/0019 lifecycle requirement, not diagnostics scope.
