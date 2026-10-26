# PRD 0034 — English source locale

| Field | Value |
|---|---|
| ID | 0034 |
| Status | Implemented (RF1–RF7 in vNEXT; the raw metric identifiers stay as the adapters emit them, with additive localized `label_text`/`unit_text`/`period_text` beside them) |
| Owner | `cuotai-core` (`text`, catalogs, every `Localized` fallback), TUI/CLI (chrome texts, help), desktop (catalog loading) |
| Consumers | every front end, `--json`/`--json-v1`/`--serve`/`--watch` consumers reading `fallback` or `Report.notes`, translators, PRD 0035 (additional languages) |
| Depends on | PRD 0001 RF8 (the `Localized` contract and core catalogs) |
| Benefits from | PRD 0031 (RF5: contract policy and support window) |
| Blocks | PRD 0035 (additional UI languages build on the English source and the completeness tests of RF4) |
| Extends | PRD 0001 RF8 (source language), PRD 0010 RF5 (TUI age text), PRD 0012 RF1 (fallback values) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. The **source locale** is the language of the `fallback` text written in code; a **catalog** is a JSON file mapping ids to texts for one locale. **Chrome** is the TUI's own interface text (frames, shortcuts, captions). Code references are to `origin/main` at `0ba9c9b`.

## Implementation status (2026-10-25)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | Every core `Localized` fallback is English: constructors take the English template (the en-US catalog text for the id) and interpolate the params into `fallback`; `Report.notes` and the `Display` texts of the core errors are English. `crates/cuotai-core/locales/pt-BR.json` was generated from the old Portuguese fallbacks (call-site templates recovered mechanically and checked against a capture of every `Localized` the test suite builds: the pt-BR rendering equals the old fallback); `en-US.json` is removed; `core_catalog("pt-*")` returns pt-BR and `en-US` none. The TUI renders status labels, wizard field labels/help/options and login texts through their ids, so `CUOTAI_LANG=pt-BR` output is unchanged; the desktop demo fixtures and UI smokes moved to English fallbacks. |
| RF2 | vNEXT | The TUI's own texts (dashboard, details, wizard, login dialog, about, models, stats, extensions, deletion dialogs, `--once`, `--history`, `--cost*`, `--vault-*`, `--extension-check`, `--serve`/`--watch` stderr) go through `crates/cuotai-tui/src/l10n.rs` (`tr`/`trf`, English in code, ids in `crates/cuotai-tui/locales/{pt-BR,es,it}.json`). The age text is the core's `freshness::age_label` (`freshness.age.*`). Login progress lines, the prompt instructions and the Xiaomi invalid-code message carry a `Localized` (`LoginEvent::*::l10n`, additive and optional in JSON). Provider names render through `provider.<slug>` (Alibaba Cloud). Metric labels, units and periods render through `metric_text::metric_text` (display only, see decisions). Tests: every `tr` id is in every TUI catalog with the same placeholders, English sources and es/it texts carry no Portuguese marker, a dashboard/about/age render in en-US/es/it has no marker, and the pt-BR suite (the TUI tests run in pt-BR) is unchanged. |
| RF3 | vNEXT | `--help` (clap doc comments and value names) is English; CLI argument errors and prompts use the RF2 catalogs. Binary tests: `tests/help.rs` (English help in the four locales, a localized argument error per locale). |
| RF4 | vNEXT | `text` tests: pt-BR, es and it carry the same ids and the same `{placeholder}` names (paths with literal braces allow-listed); `text::portuguese_markers` (ã, õ, ç and " não ", " já ", " está ", " conta ", " cota ", with an allow-list for proper names) runs on every `Localized` built in the core's tests, so a Portuguese fallback fails the suite; a seeded fallback is caught. The per-module "every id is in every catalog" tests now walk pt-BR, es and it. |
| RF5 | vNEXT | `fallback` and `notes` of `--json`, `--json-v1`, `/v1/*` and `--watch --jsonl` are English. `--fallback-locale pt-BR` (and `CUOTAI_FALLBACK_LOCALE`) renders them through a catalog (`text::localize_json_fallbacks`, `ServeConfig::fallback_locale`); shapes, ids, params and `schema` are unchanged (export, server and binary tests in both modes; the schema validation runs on the rendered export). `docs/export-v1.md` and `docs/events.md` updated. |
| RF6 | vNEXT | `AGENTS.md` "Documentation Language", `text.rs` module docs, `docs/architecture.md` (Internationalization and "Adding a provider"), `CONTRIBUTING.md`, `README.md`, `docs/tui.md`, `docs/advanced.md` and `apps/desktop/ui/locales/README.md` updated. |
| RF7 | vNEXT | The desktop backend's own errors (`Configuração não carregada.`, the sign-in session errors, the account lookup, the vault and the config-folder errors) are `desktop.error.*` ids with English fallbacks (`src-tauri/src/desk_text.rs`) in the four UI catalogs, with a parity test; `edit_error.refresh_cycle_diff`, `edit_error.index_out_of_range` and `edit_error.login_partial_failure` carry the `detail`/`count` params their catalog texts use, and `extensions.action.rollback_failed` combines English fallbacks. The browser demo's fixtures (notes, hooks, cost, Kiro data, incident states, update notes, extension texts, account names such as `Account 2` and `Personal` through `demo.account.*`) and `cost.js` use English fallbacks and catalog ids. The `en-XA` pseudo-locale derives the core's texts from their English fallbacks again; the English UI catalog gained the ids the UI reads from the core (`status.*`, `pace.chip.*`) and no longer shadows a core translation. The app's local server reads `CUOTAI_FALLBACK_LOCALE`. `apps/desktop/tools/ui-shots/locale-smoke.mjs` renders the tray, main window, settings pages and dialogs in en-US, es and it with the core catalogs and fails on any Portuguese word or catalog key; `check.mjs` stays at 0 overflows in all five locales. |

## 1. Context

Every text the core delivers is a `Localized { id, params, fallback }` whose `fallback` is **Portuguese** (`crates/cuotai-core/src/text.rs:1-20`): about 480 `Localized::fixed`/`with_param(s)` call sites in the core outside tests. `core_catalog` returns no catalog for `pt-*` (`text.rs:325-332`) because the fallback is the Portuguese text; `en-US`, `es` and `it` are catalogs of 1,219 lines each (`crates/cuotai-core/locales/`). Unknown locales already resolve to `en-US` (`text.rs:255-276`), and the desktop's UI fallback is already `en-US` (`apps/desktop/ui/js/i18n.js:27`).

What is still Portuguese for a non-Portuguese user:

- every `fallback` in `--json`, `export::v1` (documented as "the Portuguese `fallback`", `docs/export-v1.md:65`), events and the server;
- `Report.notes`, kept Portuguese "for compatibility with CLI/JSON consumers" (`docs/architecture.md:100-103`);
- error `Display` texts (for example `ThresholdError`, `alerts.rs:46-48`), CLI messages on stderr (`crates/cuotai-tui/src/headless.rs:110-121`) and the `--help` texts (clap doc comments in `crates/cuotai-tui/src/main.rs`);
- the TUI chrome, documented as "outside the scope of RF8 and remain in Portuguese" (`docs/architecture.md:96-99`), including the age text "há 3 min" (`crates/cuotai-tui/src/ui.rs:2822-2835`, `dashboard.rs:156`).

AGENTS.md states the rule as "core `Localized` fallbacks stay in Portuguese by design". Owner decision E (2026-10-23) reverses it: English becomes the source locale, Portuguese a catalog like the others, and AGENTS.md changes in the same delivery (I18N-01).

## 2. Problem

- A Spanish, Italian or English user still sees Portuguese whenever an id is missing from a catalog, in JSON consumers that show `fallback`, in notes, errors, help and the TUI chrome.
- Contributors write new texts in Portuguese first, which is not the language of the repository (AGENTS.md "Documentation Language").
- New languages (PRD 0035) would have to translate from Portuguese.

## 3. Goals

- English (`en-US`) is the text written in code for every core `Localized`, error and CLI message.
- `pt-BR` becomes a complete catalog with today's Portuguese texts, unchanged for Portuguese users.
- The TUI chrome and the age text are localized through the same mechanism.
- Catalog completeness is enforced by tests for every shipped locale.
- JSON consumers get a documented transition, with a way to keep Portuguese fallbacks during the support window.

## 4. Non-goals

- New languages (PRD 0035).
- Re-wording texts beyond translating them; the Portuguese wording stays byte-identical in the `pt-BR` catalog.
- Translating code comments, commit history or identifiers.
- Localizing provider-supplied text (incident titles, provider error codes) beyond what is done today.

## 5. Functional requirements

### RF1 — English fallbacks in the core (high priority; prerequisite)

- **RF1.1** Every `Localized` fallback in `cuotai-core` becomes the current `en-US` catalog text for its id (a mechanical replacement driven by `locales/en-US.json`, not hand-retyped); ids and params are unchanged.
- **RF1.2** `crates/cuotai-core/locales/pt-BR.json` is generated from the old Portuguese fallbacks before they are replaced (a one-off `xtask` or test-mode extractor), so Portuguese texts stay byte-identical.
- **RF1.3** `core_catalog`: `pt-*` returns the `pt-BR` catalog; `en-US` uses the fallback directly (the `en-US.json` file is removed, §13); resolution order becomes exact locale → same language → fallback (English).
- **RF1.4** `Report.notes` (plain strings) are produced in English; `notes_l10n` is unchanged.
- **RF1.5** `Display` texts of core errors (`thiserror` messages) become English; front ends keep showing the `Localized` of each error where one exists.
- **Acceptance criterion:** a test that every id resolves in `pt-BR` to the old Portuguese text (the generated snapshot); a test that every `Localized` built in the core has an English fallback (the RF4 heuristic); `CUOTAI_LANG=pt-BR` snapshot tests of the TUI and the desktop unchanged.

### RF2 — TUI chrome and age text (high priority)

- **RF2.1** The TUI's own texts (frames, shortcuts, captions, dialogs, `--once` labels, stderr messages) move to ids in a TUI catalog set (`crates/cuotai-tui/locales/{pt-BR,es,it}.json`, English in code) resolved with the active locale (`tui_locale`, `ui.rs:2778-2800`, which already reads `CUOTAI_LANG` and the system locale).
- **RF2.2** The age text comes from a core `freshness::age_label(age) -> Localized` (`freshness.age.now`, `.seconds`, `.minutes`, `.hours`, `.days`, with plural forms), replacing the hard-coded `format!("há {secs} s")` family (`ui.rs:2822-2835`, `dashboard.rs:156`); the desktop's `fmtAge` uses the same ids.
- **Acceptance criterion:** with `CUOTAI_LANG=es` and `CUOTAI_LANG=en-US`, a render of every TUI screen and of `--once` contains no Portuguese text (RF4 detector); `pt-BR` output is unchanged.

### RF3 — CLI help and messages (medium priority)

- **RF3.1** `--help` texts are written in English in the clap definitions. Localized help is not provided (clap has no runtime catalog); the README links each flag's description.
- **RF3.2** Interactive prompts and stderr messages of the CLI use the RF2 catalogs.
- **Acceptance criterion:** a binary test of `--help` in English; prompts in the four languages.

### RF4 — Catalog completeness and Portuguese detector (high priority)

- **RF4.1** Tests that every id emitted by the core and by the TUI exists in every shipped catalog (`pt-BR`, `es`, `it`), with the same placeholders (`{name}` sets equal) and plural categories required by the locale.
- **RF4.2** A detector test over English fallbacks and English-rendered output that fails on Portuguese markers (characters `ã`, `õ`, `ç` outside proper names, and a short stop-word list: " não ", " já ", " está ", " conta ", " cota "), with an allow-list for proper names.
- **RF4.3** The same framework is what PRD 0035 extends for its languages (length budgets, scripts).
- **Acceptance criterion:** the tests fail when an id is removed from a catalog or a placeholder is renamed; the detector catches a seeded Portuguese fallback.

### RF5 — Contract transition (high priority)

- **RF5.1** `fallback` values in `--json`, `--json-v1`, `/v1/*`, events and widget snapshots, and `Report.notes`, change from Portuguese to English. Shapes, ids and params do not change.
- **RF5.2** `CUOTAI_FALLBACK_LOCALE=pt-BR` (and `--fallback-locale pt-BR` on `--json`, `--json-v1`, `--serve`, `--watch`) renders `fallback` and `notes` through the `pt-BR` catalog, for consumers that display them verbatim. It is supported for at least the PRD 0031 RF5 window (90 days) and then kept or removed by an explicit decision.
- **RF5.3** `docs/export-v1.md` and `docs/events.md` say "the English `fallback`; resolve `id` in a catalog, Cuotai's are `crates/cuotai-core/locales/*.json`".
- **Acceptance criterion:** export and event tests in both modes; schema validation unchanged.

### RF6 — Repository rules and documentation (high priority; same delivery as RF1)

- **RF6.1** AGENTS.md "Documentation Language": "UI strings live in the English source and the Portuguese, Spanish and Italian catalogs; core `Localized` fallbacks are English; new texts are written in English first and added to every catalog."
- **RF6.2** `text.rs` module docs (`:1-25`), `docs/architecture.md` "Internationalization" (`:77-103`, including the TUI chrome sentence), `CONTRIBUTING.md` (how to add a text) and `README.md` (languages) updated.
- **Acceptance criterion:** documentation reviewed with the delivery; the changelog check passes.

### RF7 — Desktop (medium priority)

- **RF7.1** The desktop merges the core `pt-BR` catalog like the others (`core_catalog(locale)` Tauri command); its own `apps/desktop/ui/locales/pt-BR.json` is unchanged; any UI text still read from a core `fallback` without an id is fixed to use an id.
- **Acceptance criterion:** a browser-demo smoke in the four languages; no Portuguese text in `en-US`, `es`, `it` (RF4 detector run over the rendered demo).

## 6. Non-functional requirements

- **No visible change for Portuguese users:** the `pt-BR` output of the TUI, `--once`, the desktop and notifications is byte-identical before and after (snapshot tests).
- **Mechanical safety:** the replacement of fallbacks is generated from the catalogs, reviewed as a diff of literals only; no id, param or code path changes in the same commit.
- **Security invariant unchanged:** parameters carry only non-secret data (`text.rs:22-25`).
- **Accessibility:** the language of rendered text is declared (`lang` attribute in the desktop already follows the locale); the TUI writes no mixed-language lines.
- **Performance:** one more embedded catalog (≈ 1,200 entries) loaded lazily like the others.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `Localized.fallback` in `--json`, `export::v1`, events, `/v1/*`, widget snapshot | English instead of Portuguese | Shape unchanged. Treated as **breaking** per AGENTS.md (minor bump, `BREAKING CHANGE:` footer) because the docs promised Portuguese fallbacks and Portuguese `notes`; migration: resolve `id` in a catalog, or set `CUOTAI_FALLBACK_LOCALE=pt-BR` |
| `Report.notes` | English | Same as above |
| Core catalogs | `pt-BR.json` added; `en-US.json` removed | Internal |
| TUI catalogs | new | New |
| CLI | `--fallback-locale`, `CUOTAI_FALLBACK_LOCALE` | New flag |
| TOML, `desktop.json` | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | English fallbacks, `pt-BR` catalog, resolution order, `freshness::age_label`, completeness and detector tests, fallback-locale option for producers |
| TUI/CLI | chrome catalogs, English help, `--fallback-locale` |
| Desktop | loading the `pt-BR` core catalog; no UI text from bare fallbacks |
| Docs | AGENTS.md, `text.rs`, architecture, contributing, export and events docs |

## 9. Dependencies and sequencing

RF1, RF4, RF5 and RF6 ship together (one delivery, one version, with the breaking footer). RF2 and RF3 can follow in a second delivery. PRD 0035 starts after RF4. Wave 5e of `0000_control.md`.

## 10. Risks

| Risk | Mitigation |
|---|---|
| A Portuguese text changes by accident | generated `pt-BR.json` from the old fallbacks and byte-identical snapshot tests |
| JSON consumers display English unexpectedly | breaking footer, `CUOTAI_FALLBACK_LOCALE`, changelog migration |
| Large mechanical diff hides a code change | the commit touches literals and catalogs only; reviewers diff with `--word-diff` |
| Detector false positives on proper names | allow-list; the detector runs only on English output |

## 11. Order of execution and verification

1. Generate `pt-BR.json` from the current fallbacks (RF1.2) and add RF4's completeness tests against it.
2. Replace fallbacks (RF1.1), switch resolution (RF1.3), notes and errors (RF1.4, RF1.5), RF5 and RF6 — one delivery.
3. RF2 and RF3; RF7.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and smokes with `CUOTAI_LANG=pt-BR`, `es` and `en-US`. The requirement is only delivered when `CUOTAI_LANG=es` and `en-US` show no Portuguese string in the TUI, `--once`, the desktop and the export, and the `pt-BR` output is unchanged.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Keep `CUOTAI_FALLBACK_LOCALE` after the 90-day window | Decide at the end of the window from issues; it is cheap to keep. |

## 13. Decisions (owner, 2026-10-23)

| Topic | Decision |
|---|---|
| English fallbacks in `export::v1`, `--json`, events | **Breaking**: minor bump with a `BREAKING CHANGE:` footer, migration through `CUOTAI_FALLBACK_LOCALE=pt-BR` (RF5.2); shape and ids unchanged, so `schema` stays 1. |
| `en-US.json` | **Removed** once English is the fallback (RF1.3); no generated copy. |
| `--help` | **English only** (RF3.1). |

### Implementation decisions (RF1, RF4–RF6 delivery, 2026-10-25)

| Topic | Decision |
|---|---|
| English text per id | The en-US catalog text, except where it did not match the code: placeholder names that no parameter carried (`{line_count}`, `{plan_name}`, `{timestamp}`, `{url}`, `{value}`, …) were renamed to the parameters in the English text and in es/it, and texts whose meaning differed from the Portuguese (Alibaba billing notes, Grok and Perplexity notes, Cloudflare and Qwen reasons, the Xiaomi credential note) were rewritten from the Portuguese and retranslated in es/it. |
| One text for different messages | Where one id carried several Portuguese messages (`models.unsupported` with a per-provider reason, the MiniMax balance alert state, the inference endpoint hint, the purge-vault wrappers) the variable part became a nested `Localized` parameter with its own id, so pt-BR stays identical and es/it get the translated part. |
| Shared field/native descriptors | A native-login option borrowed from the account form has one English text (the form's); the `native.*` ids keep their own translations in the catalogs. |
| Not `Localized` today | Metric labels, units and periods (provider data), the login progress lines (`LoginEvent::Progress`), the Xiaomi code-rejection message and the TUI's own captions keep their wording; they belong to RF2/RF7 or a later change. |
| Stale ids | Catalog ids that no code emits (`edit_error.save`, `query.oauth`, `models.unsupported.admin_key`, …) were dropped from es/it with `en-US.json`. |

### Implementation decisions (RF2/RF3 delivery, 2026-10-25)

| Topic | Decision |
|---|---|
| Metric labels, units and periods | `Metric::label`/`unit`/`period` keep the adapters' strings: they are part of the metric identity in the history (`history::metric_key`), the reset ledger, the journal digests and the `metrics = [...]` alert selectors, so changing them would orphan existing users' history and rules. Front ends render them through the core's `metric_text::metric_text` (exact texts and `{slot}` templates mapped to `metric.*` ids with English fallbacks; pt-BR = the adapter text, unchanged). Changing the raw values in `--json`/`export::v1` needs a history/alert migration and is left to an owner decision (decided on 2026-10-25, next table). |
| TUI catalogs | Separate from the core catalogs (`crates/cuotai-tui/locales`), English in code; the pt-BR texts are the previous literals byte for byte. |
| Login events | `LoginEvent::{Prompt,Progress,CodeRejected,Failed}` gain an optional `Localized` next to the English text; the desktop (lane O3) should render it when present. |
| Detection consent (0027 RF6.5) | The TUI detection prompt now names folders, environment variables (presence only) and the local 9router check on 127.0.0.1, in all catalogs. |

### Owner decision: display texts beside the raw metric identifiers (2026-10-25)

| Topic | Decision |
|---|---|
| Raw values stay | `label`, `unit` and `period` of a metric (and `binding.label`, the events' `metric.label`/`unit`) keep the adapters' strings, so the history `metric_key`, the reset ledger, the journal digests and the alert `metrics` selectors do not change. No migration. |
| New fields | Additive `label_text`, `unit_text` and `period_text` (a `Localized`: `metric.*` id, English `fallback`; `period_text` is `null` with `period`) on every metric of `--json`, `export::v1` (`report.metrics`, `last_good.metrics`), `/v1/*`; `label_text` on `binding`; `label_text` on the events' `metric` and `metrics[]`, `unit_text` on `metrics[]` and `current`/`previous`. Pattern: the existing `Localized` sidecars (`notes_l10n`). `--fallback-locale`/`CUOTAI_FALLBACK_LOCALE` render their `fallback`, like every other `Localized`. |
| Schema | `docs/export-v1.schema.json` describes them (not required); `schema` stays 1; the reference example is regenerated. `Metric` deserialization ignores them. |
| Tests | Every label, unit and period of the adapter fixtures (`label_snapshot`) maps to a `metric.*` id and renders without Portuguese in English; model, event and export tests cover the shape and the pt-BR rendering. |
