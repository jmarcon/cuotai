# PRD 0028 — Provider contract health and support

| Field | Value |
|---|---|
| ID | 0028 |
| Status | In progress |
| Owner | `cuotai-core` (diagnostics, report drafts, local canary, update detector), repository and CI (`.github/`, `xtask`), desktop and TUI |
| Consumers | users reporting problems, maintainers (the owner's local canary), `export::v1` consumers, extension authors |
| Depends on | PRD 0005 (RF1: `access::describe` and `verified_on`) for RF1–RF3 and RF6; PRD 0017 (RF1–RF5: extensions) for RF7; PRD 0021 (RF8.1: install-source detector) for RF8 |
| Benefits from | PRD 0011 (RF1: status sources, for RF4.5), PRD 0012 (RF1: `export::v1`), PRD 0013 (RF1.3: failure categories), PRD 0018 (update manifest), PRD 0019 (RF1: community files; RF6: release policy), PRD 0022 (UI refresh: details and About visuals), PRD 0036 (9router accounts, for RF10.2) |
| Extends | PRD 0002 RF4 (model listing gated on evidence; static lists removed), PRD 0005 RF1 and RF4 (per-interface `verified_on`, `access.risk.subscription_token`, a redacted path in the schema-mismatch explanation), PRD 0006 RF2 (label snapshot test against broken series), PRD 0011 RF1 (status component check), PRD 0012 RF1 (additive `failure` object), PRD 0017 RF1 and RF3 (manifest `overrides`, `secrets`, `min_cuotai`, ignored `x-*` tables), PRD 0019 RF1 (`SUPPORT.md`), PRD 0021 RF8.1 (detector moved into the core and used by the CLI) |
| Date | 2026-10-22; amended 2026-10-23 (owner decisions A, K, O; review items CAN-01..05, CAT-01, CAT-02, EXT-01, DIST-12 and the open items of PRDs 0002, 0005, 0006 and 0017) |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. **Schema drift** means a provider response that no longer matches the shape an adapter decodes. A **structural fingerprint** lists keys and JSON types only, never values. The **local canary** is a run on the owner's own machine, with the owner's own accounts, that records response shapes and never values.

## Implementation status (2026-10-23)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | Delivered for the shared JSON decoder (`providers::json`): `diagnostic::Diagnostic`, `Report::diagnostic`, the details note and the additive `export::v1` `failure` object. Adapters that read a `Value` field by field still report no path. |
| RF2 | vNEXT | Delivered: core `support::issue_draft`, `cuotai --report-problem ACCOUNT`, the TUI details key `b` (RF2.4), the desktop preview dialog in a non-ready account's details and in Settings › About (RF2.3, the link opens only through the project's issue-form allow-list), and the canary's drafts with the shape diff (RF2.5). The install channel is the detected one; the incident flag stays `unknown` until PRD 0011 feeds it. |
| RF3 | vNEXT | Delivered: generated provider dropdown with a drift test and the new form fields. |
| RF4 | vNEXT | Delivered in part: `cuotai --canary [--out DIR] [--dry-run] [--canary-open] [--canary-include-metered]`, the pure `cuotai_core::canary` (shape records, drift, report, drafts, evidence list, 30 private runs) with seeded-secret tests, and `docs/canary.md` (RF4.1–RF4.4, RF4.6, RF4.7). Pending: the response fingerprint of a successful answer (RF4.1), the status-component check (RF4.5, needs PRD 0011 sources), `cargo xtask access-verified --apply` for the evidence list, and the dedicated CI dry-run job (RF4.8; the unit tests of the same logic run in the normal CI jobs). |
| RF5 | vNEXT | Delivered: `cargo xtask anonymize-fixture --provider SLUG [--perturb F] IN.json`, with a secret scan that refuses the output. The "decodes with its adapter" check is covered with a local type, since xtask does not depend on the core. |
| RF6 | vNEXT | Delivered: per-interface `verified_on`, `access-staleness` xtask with the CI warning and the non-required 180-day job, `access.risk.subscription_token`, and the core age text. The core still exposes the verification text (`UsageAccess::verification`), but by the owner decision of 2026-10-25 no app surface shows integration status, so the desktop "Verified N months ago" chip (DS-CMP-VER-1) is not rendered; the age lives in the `docs/providers.md` matrix. |
| RF7 | — | Proposed. |
| RF8 | — | Proposed. |
| RF9 | — | Proposed. |
| RF10 | vNEXT | Delivered: evidence-gated listings with a generated matrix, static lists and the Copilot token exchange removed. |
| RF11 | — | Proposed (added 2026-10-23). |

## 1. Context

Most of Cuotai's integrations use interfaces the vendors do not document (`Level::Undocumented`, PRD 0005), so they break without notice. Today:

- A decode failure becomes `QueryError::schema` with the `schema_changed` failure category (`events.rs` `FailureCategory::SchemaChanged`) and, for undocumented interfaces, "the interface may have changed" with its reference (PRD 0005 RF4). Nothing records **which** field broke, so a bug report says "Claude stopped working" and the maintainer has to reproduce it with a real account.
- `access::describe` carries `verified_on`, but it comes from one constant, `VERIFIED_ON = "2026-10-20"` (`crates/cuotai-core/src/access.rs` line 15), shared by every provider; it cannot tell which contract is overdue for a check.
- `.github/ISSUE_TEMPLATE/provider-problem.yml` lists the 36 provider slugs by hand; a new provider can be forgotten.
- There is no scheduled check against real accounts; regressions are found by users.
- PRD 0011's status sources use fixed component identifiers; a renamed component silently becomes `Unknown`.
- After PRD 0002, every provider except Alibaba Cloud tries to list models, some without evidence that the listing interface exists; `providers/builtin_models.rs` embeds static model lists copied from 9router, contradicting PRD 0002 §4 ("inventing models from … static tables"); and `providers/catalog.rs` (about line 323) calls `/copilot_internal/v2/token`, the Copilot token exchange that `oauth/reference_device.rs` states is "intentionally NOT performed".
- When an adapter changes a metric label or unit, the PRD 0006 history series of that metric silently breaks into two.
- Extensions (PRD 0017) can add new sources, but a user cannot replace a broken built-in adapter until a release ships; an extension that needs a secret has to read it from the environment the user exported; the manifest is strict (`deny_unknown_fields`, closed enums), so an extension written for a newer Cuotai fails on an older one without saying why.
- The access risk texts do not say that using a subscription token outside the vendor's own client may go against the vendor's terms.
- The desktop has an updater with an install-source detector (since 0.77.0 one typed core function, `cuotai_core::install_source::detect_install_source`, shared with the offline `cuotai --install-source` report; PRD 0021 RF8.1); the CLI never says a newer version exists.
- `CONTRIBUTING.md` and `SECURITY.md` exist (PRD 0019 RF1); there is no `SUPPORT.md` stating channels and response targets.

**Market expectation.** A paid macOS monitor (WhatsMyUsage) promises to be "maintained like a product: vendor API changes fixed fast, support by email". An open-source project cannot sell a support contract, but it can make breakage visible early, make reports precise and private, ship fixes quickly through the updater and package managers, and state honest best-effort targets.

## 2. Problem

- Breakage is discovered late and reported vaguely.
- Precise reports risk leaking secrets: people paste responses with tokens, e-mails or organization names.
- Contract evidence ages silently, and some model listings have no evidence at all.
- Users are stuck between a broken adapter and the next release.
- Support expectations are undefined.

## 3. Goals

- A redacted, value-free description of what drifted, shown in details and exported.
- A one-click, secret-free problem report prefilled with the facts a maintainer needs.
- Early detection: a **local** canary on the owner's machine with the owner's own accounts (shapes only), a CI dry run against fixtures, and a CI warning for stale evidence.
- Model listings only where evidence exists; no embedded static lists.
- Tooling that turns a captured response into an anonymized `wiremock` fixture, and a label snapshot that protects history series.
- A consented extension override as a stopgap for a broken built-in adapter, extension secrets in the vault, and a manifest that can evolve.
- A CLI update notice and a `SUPPORT.md` with best-effort targets, through GitHub only.

## 4. Non-goals

- Telemetry, crash reporting or automatic sending of anything: every report is opened by the user.
- An e-mail support desk or paid support. **Decided on 2026-10-23 (decision O): support is through GitHub only** (Issues and Discussions).
- Running the canary in CI or any hosted service with real credentials, or keeping provider tokens anywhere but the owner's OS vault (decision A).
- Uploading raw responses anywhere, even redacted, from the app or the canary.
- Updating the CLI in place: RF8 only notifies; PRD 0018 §4 ("Updating the TUI/CLI" is a non-goal) stands.
- Refunds or licensing (Cuotai is free software).

## 5. Functional requirements

### RF1 — Redacted schema-drift diagnostic (high priority; prerequisite)

- **RF1.1** When an adapter's JSON decode fails, the core records a `SchemaDrift` beside the `QueryError::schema`: `paths` (at most 8 JSON paths of the failing fields, array indices normalized to `[]`, e.g. `five_hour.utilization`), `expected` and `found` JSON types per path, and a `fingerprint` (sorted `path → type` pairs, depth ≤ 4, ≤ 64 entries) with its SHA-256 digest. Path extraction uses `serde_path_to_error` (a new dependency, checked by `cargo deny`) or an equivalent wrapper in `providers::json`.
- **RF1.2** Redaction: object keys that are not identifier-like (`^[A-Za-z_][A-Za-z0-9_]{0,63}$`), and objects with more than 32 keys (maps keyed by model names, e-mails or ids), are collapsed to `{*}`; no value, URL, header or status text is ever kept.
- **RF1.3** `Report` gains `diagnostic: Option<Diagnostic>` (not serialized in `--json`); details (TUI and desktop) show "The provider changed `five_hour.utilization` (number → string)" from a core `Localized` text (DS-CMP-DIAG-1); `export::v1` gains an additive `failure { category, paths }` object on failing accounts.
- **RF1.4** The same mechanism applies to extension outputs (`invalid_output` keeps its own field-only violations, PRD 0017 RF2).
- **Acceptance criterion:** tests per adapter family with a mutated fixture (type change, removed field, renamed field, map of e-mails as keys) asserting the paths, the collapse rules and that no fixture value appears in the diagnostic, the texts or the export.

### RF2 — "Report a problem" (high priority)

- **RF2.1** Pure `support::issue_draft(account: &AccountView, app: &AppInfo) -> IssueDraft` with `title` ("[Provider] claude: schema_changed"), `body` (Markdown) and `url` (a `https://github.com/<repo>/issues/new?template=provider-problem.yml&…` URL with the template's fields prefilled).
- **RF2.2** Fields: Cuotai version, OS and architecture, install channel (RF8), provider slug, `auth_mode`, access level and `verified_on`, failure category, drift paths and fingerprint digest (RF1), freshness of the last good reading and whether a provider incident was active (PRD 0011). Never the account name, e-mail, organization, TOML content, environment-variable names, notes, URLs or tokens.
- **RF2.3** Desktop: "Report a problem" in a failing account's details and in About opens a preview of the exact text with **Open on GitHub** (through the allow-listed `open_external`) and **Copy** (DS-CMP-RPT-1); nothing is sent by Cuotai. When the URL would exceed 8 000 characters, only **Copy** is offered.
- **RF2.4** TUI: a details key prints the draft and the URL; `cuotai --report-problem ACCOUNT` prints them to stdout (exit 0/1).
- **RF2.5** The local canary (RF4.3) builds its drift issue drafts with the same function, adding the shape diff of RF4.2 (paths and types only) (review item CAN-03).
- **Acceptance criterion:** a property test feeding reports seeded with tokens, e-mails, URLs, account names and notes in every field proves none reaches the draft; snapshot tests of the body in English (issue text is English per `AGENTS.md`), with the UI around it localized.

### RF3 — Generated issue template (medium priority)

- **RF3.1** The provider dropdown of `.github/ISSUE_TEMPLATE/provider-problem.yml` is generated from `Provider` by a core test (`issue_template_providers`) that fails on drift and regenerates with `CUOTAI_REGENERATE_DOCS=1`, like `providers_doc_matrix`.
- **RF3.2** The template gains optional fields for the connection mode, the Cuotai version, the failure category, the drift paths and the shape diff, matching RF2's prefill keys.
- **Acceptance criterion:** adding a provider without regenerating fails the test; the regenerated file validates as a GitHub issue form.

### RF4 — Local contract canary (medium priority; rewritten 2026-10-23, decisions A and K; review items CAN-01, CAN-02, CAN-04)

The canary runs on the **owner's machine**, as a scheduled local task, with the owner's own accounts and the tokens already in the OS vault. CI never holds a provider secret.

- **RF4.1** `cuotai --canary [--config FILE] [--out DIR]` runs one refresh of the configured accounts (the same path as `--json-v1`, with the normal per-account deadlines) and writes a **shape record** per provider and connection mode: the response fingerprint of RF1.1 (keys and JSON types only, RF1.2 collapse rules), the metrics present (label key, kind, window, unit and `billing`, never values), the status, and the failure category and drift paths when it failed. Records live in `<data dir>/cuotai/canary/` (`0600`, the last 30 runs kept). No value, account name, e-mail, organization, URL with query, header or body is written.
- **RF4.2** **Drift detection** compares each record with the previous run of the same provider and mode: a field missing, a type changed, a metric new or absent, a new `schema_changed` or `auth_required` failure. Undocumented interfaces (PRD 0005) and the model listings that remain after RF10 are compared first and flagged with priority. Two runs with nothing changed produce no alert.
- **RF4.3** **Output:** a value-free Markdown report on stdout and in the canary directory; on drift, one issue draft per provider and category built by RF2.5, offered through `--canary-open` (opens the GitHub URL) or printed. The canary never files anything by itself.
- **RF4.4** **Evidence report (decision K):** for every provider and mode that answered with no drift, the canary emits a reviewable patch (`canary/verified_on.patch`, or a JSON list consumed by `cargo xtask access-verified --apply FILE`) that raises its RF6.1 `verified_on` to the run date. The owner reviews and commits it; the canary never writes to the repository (review item CAN-05).
- **RF4.5** **Status components (review item CAN-04):** when PRD 0011 status sources exist for the configured providers, the canary also checks that each fixed component identifier still exists in the public status page; a missing or renamed component is a drift entry ("status component renamed"), instead of the silent `Unknown` the app shows today.
- **RF4.6** **Cost and limits:** metered providers (AWS Cost Explorer, `access::describe(...).metered`) are skipped unless `--canary-include-metered`; one refresh per run; the run honors each provider's minimum interval and the reading cadence of PRD 0033.
- **RF4.7** **Scheduling:** `docs/canary.md` gives a launchd agent, a systemd user timer and a Windows Task Scheduler entry (daily), all running as the owner's user so the vault is reachable; nothing is installed by Cuotai.
- **RF4.8** **CI keeps only a dry run:** a workflow job runs `cuotai --canary --dry-run` against the `wiremock` fixtures, with one fixture mutated, and asserts that the drift detector, the report, the issue draft and the evidence patch behave; the job has no secret and no `canary` environment.
- **Acceptance criterion:** two canary runs over unchanged fixtures produce no alert; a mutated fixture yields exactly one drift entry, one issue draft and no evidence entry for that provider; a seeded-secret test (tokens, e-mails, account names, query URLs and bodies in every response) finds nothing in any record, report, draft or patch; the CI dry run passes with no secret configured.

### RF5 — Fixture anonymizer (medium priority)

- **RF5.1** `cargo xtask anonymize-fixture --provider SLUG IN.json > OUT.json`: keeps structure and JSON types, replaces strings with type-preserving placeholders (dates stay valid RFC 3339 at a fixed epoch, e-mails become `user@example.com`, ids become stable fake ids, JWT-looking strings become a fake JWT), keeps numbers by default with `--perturb` to scale them, and collapses RF1.2 map keys.
- **RF5.2** It refuses input that still matches secret patterns after anonymization and prints a review checklist; the output is meant for `wiremock` tests next to the adapter.
- **Acceptance criterion:** tests with seeded secrets (tokens, cookies, e-mails, organization names) prove none survives; the output decodes with the adapter it came from.

### RF6 — Per-interface evidence dates, staleness warning and subscription-token risk (medium priority)

- **RF6.1** `verified_on` moves from the single `VERIFIED_ON` constant to one date per provider and mode in the `access::classify` table; `docs/providers.md` shows it (already generated). The local canary's evidence patch (RF4.4) is the normal way to raise it.
- **RF6.2** A CI step (`cargo xtask access-staleness --days 90`) lists `Undocumented` and `UserSession` interfaces whose `verified_on` is older than 90 days as a warning annotation, without failing the build; older than 180 days fails a separate, non-required job (**decided on 2026-10-23, decision O**). The details show "Verified N months ago" and, past 90 days, "Not verified since {date}" (DS-CMP-VER-1).
- **RF6.3** `access.risk.subscription_token` (open item of PRD 0005, owner accepted): every interface whose credential class is a subscription session gains a localized risk line saying that the token belongs to the vendor's own client and that using it from another tool may go against the vendor's terms; it is shown with the other risk texts of PRD 0005 RF3 (editor) and RF4 (details), and in the generated matrix.
- **Acceptance criterion:** tests of the xtask with a fixed date; the CI annotation appears on a fixture with an old date; an exhaustive test that every subscription-session interface carries the new risk key in every catalog.

### RF7 — Extension overrides, secrets and manifest evolution (low priority)

- **RF7.1** Manifest schema 1 gains an optional `overrides = "<provider slug>"` (additive). An override extension does not replace anything by itself: the user chooses, per account of that provider, "Use extension override" in the editor.
- **RF7.2** Consent (PRD 0017's disclosure) states that the extension replaces the built-in query for that account; the built-in credentials are **never** passed to the extension (it receives only its own declared `env` and the secrets of RF7.4); revocation, a changed digest or disabling the extension returns the account to the built-in adapter.
- **RF7.3** Cards and details show the access level "Extension (override)" with the extension name; `export::v1` `access.level` is `extension` with `overrides` set; problem reports (RF2) say an override is active.
- **RF7.4** **Extension secrets in the vault (review item EXT-01):** the manifest may declare `[[secrets]]` entries (`name`, `env` variable to receive it, one-line description). Consent lists them; the user enters each value once in the TUI or desktop extension dialog, and it is stored in the OS vault (`TokenStore`, keyed by extension id, consent digest and name). At run time the runner adds it to the reduced environment of PRD 0017 RF3 only. A secret never appears in the TOML, the manifest, logs, `runs.log`, reports, exports or problem drafts; revoking consent or removing the extension deletes the vault entries, with rollback when the save fails.
- **RF7.5** **Manifest evolution (open item of PRD 0017, owner accepted):** an optional `min_cuotai = "X.Y.Z"`; a manifest that requires a newer Cuotai is rejected with a localized reason naming the version, before any other validation error. Top-level tables whose names start with `x-` are ignored (reserved for extension authors and future fields); every other unknown key is still rejected (`deny_unknown_fields` stays).
- **Acceptance criterion:** tests that an override never receives the account's credentials, that revocation restores the built-in adapter, and that the card and export show the override; a seeded-secret test that an extension secret appears only in the child's environment and nowhere else (TOML, manifest, logs, reports, exports, drafts); a manifest with a higher `min_cuotai` is refused with its version; an `x-vendor` table is ignored while another unknown key is still refused; `docs/extensions.md` documents the three additions.

### RF8 — Install source in the core and CLI update notice (low priority)

- **RF8.1** (already delivered by PRD 0021 RF8.1 in 0.77.0; kept for traceability of review item DIST-12) The install-source detector lives in `cuotai-core` (`cuotai_core::install_source::detect_install_source`): it recognizes Homebrew (Caskroom and Cellar), winget, Scoop, AUR or a distribution package, and a Cargo install; the desktop updater (PRD 0018) uses it to turn itself off and show the manager's own command, and the offline `cuotai --install-source` prints the same result. This PRD adds nothing here: RF8.2 and RF8.3 reuse it.
- **RF8.2** Opt-in check (`CUOTAI_UPDATE_CHECK=1` or `[updates] check = true` in the TOML; off by default) against the same signed manifest the desktop updater reads (PRD 0018), sending only what PRD 0018 sends (version, OS, architecture), at most once per 24 h with the answer cached in the data directory.
- **RF8.3** The notice ("Cuotai 0.80.0 is available: brew upgrade cuotai") goes to stderr only in the interactive TUI and `--once` on a terminal; never in `--json`, `--json-v1`, `--watch --jsonl`, `--serve` or when stdout is not a terminal. It names the channel's own command from the detector.
- **Acceptance criterion:** the detector's path-fixture tests for every channel already exist and are shared by the desktop and the CLI (0.77.0); tests with a local manifest server proving zero requests while off, one per 24 h while on, no output in machine modes, and the per-channel command.

### RF9 — `SUPPORT.md` and support targets (low priority; decision O)

- **RF9.1** `SUPPORT.md` at the repository root (linked from `README.md`, `CONTRIBUTING.md` and `.github/ISSUE_TEMPLATE/config.yml`): support is **through GitHub only** — Issues with the templates for bugs and provider breakage, Discussions for questions; no e-mail address. It says what to include ("Report a problem", RF2), that security reports go through `SECURITY.md` (GitHub private vulnerability reporting), and what is not supported (paid support, account recovery with a provider).
- **RF9.2** Best-effort targets, stated as goals and not guarantees (decided on 2026-10-23): provider breakage triaged within **3 days**; a patch release as soon as a fix is merged, shipped through the in-app updater (PRD 0018) and the package managers (PRD 0021); evidence considered stale after **90 days** and failing the non-required job after **180 days** (RF6.2); release cadence and supported versions as in `docs/release-policy.md` (PRD 0019 RF6).
- **RF9.3 (owner action)** GitHub Discussions are enabled on the repository.
- **Acceptance criterion:** the file exists, is linked from the three places, names no e-mail address, and Discussions are enabled.

### RF10 — Model listing gated on evidence (medium priority; added 2026-10-23, review items CAT-01 and CAT-02)

- **RF10.1** A provider lists models (PRD 0002) only when its listing interface is documented or verified live; without evidence the model capability is `Unsupported` (PRD 0002 RF4.2). The `docs/providers.md` matrix shows the evidence of each provider that lists models, generated and drift-tested like the access matrix.
- **RF10.2** The static model lists embedded from 9router (`providers/builtin_models.rs`) are removed, as PRD 0002 §4 already requires. A provider whose only source was a static list becomes `Unsupported` for models. Coordinated with PRD 0036 (9router and aggregator providers), which states that the removal does not affect its 9router accounts (they read the instance's own management routes); the removal can therefore ship independently, and the canary (RF4) gains PRD 0036's optional 9router target on the owner's machine.
- **RF10.3** The Copilot model listing stops calling `/copilot_internal/v2/token` (`providers/catalog.rs`, about line 323), which contradicts `oauth/reference_device.rs` ("intentionally NOT performed"); Copilot models are listed only through an interface that uses the stored credential as is, or the capability becomes `Unsupported`.
- **Acceptance criterion:** an exhaustive test that every provider with a model listing has an evidence entry; a test that no adapter references the embedded static lists; a `wiremock` test proving the Copilot listing never requests the token-exchange path; `THIRD_PARTY_NOTICES` updated when the 9router-derived lists go.

### RF11 — Per-adapter label snapshot (medium priority; added 2026-10-23, open item of PRD 0006)

- **RF11.1** A core snapshot test records, per adapter fixture, every emitted metric's label key, kind, window, unit and `billing`; a change fails with instructions to regenerate (`CUOTAI_REGENERATE_DOCS=1`) and to decide whether history continues.
- **RF11.2** When a label or unit changes on purpose, the change adds an entry to a core alias table (`history::aliases`: old identity digest → new) so PRD 0006 series and PRD 0026 statistics continue across the rename; without an alias the series breaks visibly, never silently merges values of different units.
- **Acceptance criterion:** changing a fixture label without regenerating fails the test; an alias entry keeps one continuous series in `HistoryStore::series`; a unit change without an alias starts a new series.

## 6. Non-functional requirements

- **Privacy:** diagnostics, drafts, canary records and reports, evidence patches and fixtures contain no values, URLs, tokens, e-mails, account or organization names; every path is tested with seeded secrets. Nothing is sent without a user action, except the opt-in update check (RF8). Extension secrets live only in the OS vault and the child's environment.
- **Security:** extension overrides never see built-in credentials; provider tokens never leave the owner's vault for the canary; CI runs the canary only against fixtures.
- **Cost:** no extra provider query in the app; the canary makes one refresh per run, skips metered providers by default and respects intervals.
- **i18n:** UI texts in pt-BR, en-US, es and it (and the languages of PRDs 0034 and 0035 when they land); issue bodies in English (repository rule).
- **Accessibility:** the report preview is a readable text region with copy and open buttons reachable by keyboard.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `export::v1` | `failure { category, paths }` on failing accounts; `access.overrides` | Additive |
| Extension manifest schema 1 | optional `overrides`, `[[secrets]]`, `min_cuotai`; ignored `x-*` tables | Additive |
| TOML | optional `[updates] check` | Additive; unknown-key handling unchanged |
| `--json` | none (`Report::diagnostic` is not serialized there) | — |
| Core catalogs | `access.risk.subscription_token` | Additive |
| Model capability | providers without evidence become `Unsupported`; static lists removed | Behavior change, listed in the changelog |
| Local data | `<data dir>/cuotai/canary/` (owner's machine only) | New |
| Repository | canary dry-run CI job, generated issue template, `SUPPORT.md`, `docs/canary.md`, xtasks (`anonymize-fixture`, `access-staleness`, `access-verified`) | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | drift capture and redaction, texts, issue draft, canary shapes and comparison, install source, update check, override routing, extension secrets, evidence gating, label aliases |
| Desktop | details text, report preview, override and extension-secret entry in the editor, About link |
| TUI/CLI | details text, `--report-problem`, `--canary`, update notice, override and extension-secret entry in the wizard |
| Repository/CI | canary dry run, anonymizer, staleness and evidence xtasks, generated template, `SUPPORT.md` |

## 9. Dependencies and sequencing

RF1 first: RF2, RF4 and RF5 reuse its redaction. RF3, RF6 and RF11 are independent and small. RF4 needs RF1 and RF2 and benefits from RF6.1 (the evidence patch has somewhere to land). RF10 coordinates with PRD 0036 (9router). RF7 needs PRD 0017 (delivered). RF8 completes PRD 0021 RF8.1 and reuses PRD 0018's manifest. RF9 can ship at any time. Details and About visuals follow PRD 0022. Wave 5d of `0000_control.md` puts RF1 first.

## 10. Risks

| Risk | Mitigation |
|---|---|
| A path or fingerprint leaks data through key names | identifier-only keys, map collapse, seeded-secret tests |
| The local canary depends on one machine being on | daily schedule; a missed day only delays detection; the CI dry run keeps the detector honest |
| Canary hits rate limits or costs money | one refresh per run, metered providers excluded, intervals respected |
| Removing static model lists looks like a regression | changelog entry naming the providers that become `Unsupported` for models; evidence can be added provider by provider |
| Overrides or extension secrets used to run untrusted code | PRD 0017 consent and digests, explicit per-account choice, no credential hand-off, secrets only in the child's environment |
| Support targets read as promises | "best effort" wording, owner-approved |

## 11. Order of execution and verification

1. **RF1:** drift capture and redaction.
2. **RF2 and RF3:** problem report and generated template.
3. **RF6 and RF11:** evidence dates, subscription-token risk and the label snapshot.
4. **RF4:** local canary (CI dry run first, then the owner's scheduled task) and **RF5** anonymizer.
5. **RF10** with PRD 0036.
6. **RF7, RF8 and RF9.**

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, `cargo deny`, and a smoke test of the changed interface. The requirement is only delivered when a mutated fixture yields a precise, value-free path in details, export, the problem draft and the canary report, and no test with seeded secrets finds one in any output.

## 12. Open questions

| Question | Recommendation |
|---|---|
| ~~Offer an e-mail support address~~ | **Decided on 2026-10-23: no**; GitHub Issues and Discussions only, `SECURITY.md` for private reports. |
| Which providers the canary covers | The owner's own subscriptions (Claude, ChatGPT / Codex, Cursor first); no metered providers unless explicitly included. |
| ~~Staleness threshold~~ | **Decided on 2026-10-23:** 90 days warning, 180 days failing the non-required job. |
| ~~Triage target~~ | **Decided on 2026-10-23:** 3 days best effort. |
| Keep `--canary` in release builds or behind a feature | Keep it in release builds (the owner runs the installed binary); it only reads what a normal refresh reads. |
