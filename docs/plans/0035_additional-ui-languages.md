# PRD 0035 — Additional UI languages (zh-CN, fr, de, ja, ko, hi-IN, id-ID, ru, tr, vi)

| Field | Value |
|---|---|
| ID | 0035 |
| Status | In progress |
| Owner | `cuotai-core` (locale registry, resolution, catalogs, plural rules), desktop (`apps/desktop/ui`: catalogs, `i18n.js`, `flags.js`, typography), TUI (`crates/cuotai-tui`: locale selection, chrome catalog, terminal width) |
| Consumers | every front end and every text the backend emits: cards, details, tray, native tray menu, notifications (PRD 0003, 0008), `--once`/`--watch` text, "What's new" (PRD 0019 RF7.7), user language packs |
| Depends on | PRD 0034 (English source locale: en-US becomes the source and fallback, pt-BR becomes a catalog) for RF2 and RF3; none for RF1, RF4 and RF5 (they can land first) |
| Benefits from | PRD 0022 (UI refresh) and `docs/design-system.md` (DS-TOK-TYP-7..10, DS-CMP-SEL-3, DS-MSG-12, DS-MSG-14..16 already describe this PRD's targets); PRD 0020 RF7 (preferences move to `desktop.json`) |
| Extends | PRD 0001 RF8 (core-owned catalogs and the lookup order), PRD 0003 RF2.2 (notices in the app's language), PRD 0019 RF7.7 (changelog labels), PRD 0017 (user language packs keep working) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **locale** is a BCP 47 code with a catalog (`pt-BR`, `zh-CN`); a **catalog** is one JSON file of message ids; the **core catalog** holds the texts the backend emits (`Localized`), the **UI catalog** holds desktop chrome, and the **TUI chrome** is the terminal interface's own captions. "Critical strings" are defined in RF11.3.

## Implementation status (2026-10-25)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | Delivered: `text::LOCALES` (fourteen rows, the ten new ones `in_progress`), `text::normalize` (alias table of RF1.4), `Locale` as a registry handle, one `include_str!` per catalog from a macro with a registry/file test in the core and the TUI, the `locale_registry` Tauri command with a generated static copy and generated pre-paint copies, the same algorithm in `i18n.js`, `CUOTAI_LANG` normalized with one warning, `set_locale` canonical for built-in codes (decision D11). |
| RF2 | vNEXT | Delivered: `zh-CN`, `fr`, `de`, `ja`, `ko`, `hi-IN`, `id-ID`, `ru`, `tr` and `vi` core catalogs (13 files with `pt-BR`, `es` and `it`, 14 languages with the embedded `en-US`), every row `Shipped`, with the parity, placeholder and plural tests walking all of them. They are machine drafts: `_meta.status = "machine-translated, needs native review"` and `reviewed = false`; native review (RF11.2, RF13.5) is owner work. |
| RF3 | vNEXT | Delivered: ten desktop catalogs next to `pt-BR`, `es`, `it` and `en-US`, merged under the core's as before; the generated pre-paint copies carry every language; the same machine-draft status. |
| RF4 | vNEXT | Delivered: plural objects in every catalog layer (`text::Catalog`, `text::Message`), `text::plural_category` cross-checked against a recorded `Intl.PluralRules` fixture, `Localized::plural`, the nine core count texts as plural objects in pt-BR, es and it, plural objects of core texts in the desktop `tl()`. |
| RF5 | vNEXT | Delivered: `crates/cuotai-tui/locales` holds the chrome catalogs of the thirteen files (PRD 0034 RF2 extracted them), `CUOTAI_LANG` is normalized through the registry with one warning, and `freshness::age_label` and the duration patterns are catalog texts. |
| RF6 | vNEXT | RF6.3 and RF6.7 delivered: a last pass over every TUI frame (`glyphs.rs`, called from `palette::finish`) replaces, from one table, the East Asian Ambiguous glyphs (pace and status shapes, bullets, quotes, box drawing, gauge blocks) with one-cell ASCII when they are wide (zh-CN, ja, ko, or `CUOTAI_AMBIGUOUS_WIDTH=wide`; `narrow` turns it off), and every table glyph when the locale is not UTF-8 (or `CUOTAI_GLYPHS=ascii`); a test renders every locale at 80 columns under both widths. RF6.1, RF6.2, RF6.4 and RF6.5 delivered: column padding (`pad_end`/`pad_start`) and the incident badge cut count cells, so the price table, the statistics lines and the badge stay aligned with CJK text; Backspace removes a whole grapheme in every text field (`unicode-segmentation`, already in the tree); headings are capitalized by `text::uppercase` (Turkish and Azerbaijani `i` to `İ`, caseless scripts unchanged); the models filter uses `text::fold`; `docs/tui.md` states the terminal and font requirement. |
| RF7 | vNEXT | Delivered: `flags.js` draws the six new flags, the selector lists the fourteen languages with flag and endonym (each name carries its `lang`), the active option is announced in its own language, and DS-CMP-SEL-3 records the 1 px hairline for the white-field flags (Japan, South Korea, Indonesia). |
| RF8 | vNEXT (partial) | RF8.3 to RF8.5 code delivered in `themes.css`: `--line-body` 1.5 (CJK), 1.6 (Devanagari), 1.45 (Vietnamese); `font-synthesis: none`, no letter-spacing or forced capitals under CJK and Devanagari (kept for tr); strict CJK line breaking; German hyphenation on paragraphs only. A core test keeps the rules. RF8.1 and RF8.2 were delivered earlier. Open: RF8.6–RF8.7. |
| RF9 | vNEXT | RF9.4 delivered: `text::pseudo_text` (the twin of the desktop `pseudoText`, cross-checked on the same samples), `text::registered_locales` with the `en-XA` row only under `cfg(debug_assertions)` (a test pins fifteen rows in debug and fourteen in release), `Localized::resolve("en-XA")` and the TUI chrome render it, and `CUOTAI_LANG=en-XA` selects it for TUI and `--once` smoke runs in development builds only. RF9.2 and RF9.6 delivered: `text/budget_tests.rs` measures every shipped catalog per component class (pace and time chips, buttons, tabs, state chips, sort options) in display columns with placeholders at their widest sample, fails on a new label over budget, lists today's offenders for rewording (a list that only shrinks: de, fr, ru, tr `pace.chip.lasts`; id-ID percent chips; ru, id-ID, vi time chips) and generates `docs/i18n-length-budgets.md`. |
| RF10 | vNEXT | Partly delivered: RF10.3 (desktop countdowns use the core's `text.duration.*` patterns and two new UI patterns), RF10.5 (`fold()` in `util.js` and its Rust twin `text::fold` for the TUI filter), RF10.6 (explicit `localeCompare` locales). RF10.1, RF10.2 and RF10.4 need no code (the registry code already reaches `Intl`); their per-language screenshot checks belong to the language lanes (RF13). |
| RF11 | vNEXT (partial) | The process is in `apps/desktop/ui/locales/README.md` (the registry, the four steps that ship a language, the plural categories and the parity rules); each catalog states its provenance in `_meta`. Native reviewers and the critical-string review are owner work. |
| RF12 | vNEXT (partial) | User language packs keep working and a pack with the code of a language overrides the built-in catalog. The community contribution guide beyond the locales README is not written. |
| RF13 | — | Open: the per-language screenshot evidence, the cross-OS font checks and the reviewers' sign-off are owner work. |
| RF14 | vNEXT (partial) | `docs/tui.md` (`CUOTAI_LANG` normalization), the locales README and `AGENTS.md` describe the registry; the README states 14 languages. |

## 1. Context

The owner wants ten more languages next to the four that ship (pt-BR, en-US, es, it), because together they cover about 80 % of internet users. This section records how locales work **today** (verified against the worktree at 0.75.0) because several mechanisms are closed to exactly four languages.

### 1.1 What exists

Refreshed on 2026-10-25 against the tree after PRD 0034 (English source locale) was fully delivered; the 0.75.0 reading of the first draft is superseded.

| Area | Today (after PRD 0034) | Where |
|---|---|---|
| Desktop UI catalogs | `apps/desktop/ui/locales/{en-US,pt-BR,es,it}.json`: 1 205 ids in en-US, 1 198 in pt-BR, 1 191 in es and it (about 41 000 to 47 000 characters of values per language). en-US also carries nine `pace.chip.*` and five `status.*` core ids and pt-BR two (`status.ready`, `edit_error.login_cleanup_failed`), the leftovers of the old pt-BR source. Eleven ids are plural objects (`quota.count`, `card.modelCount`, `settings.search.count`, `tray.allOkTitle`, `trend.table.resets`, `models.count`, `settings.hooks.state.on`, `stats.accounts.hits`, `stats.footer.unknown`, `tabs.provider`, `card.limits`). | `apps/desktop/ui/locales/*.json` |
| Core catalogs | `crates/cuotai-core/locales/{pt-BR,es,it}.json`: 1 686 ids each, about 111 000 to 117 000 characters, **flat strings only**. There is **no en-US file**: the English text is the embedded `fallback` of each `Localized`, written next to its id in the code (PRD 0034 RF1). | `text.rs` (`parse_catalog` panics on any non-string value) |
| TUI catalogs | `crates/cuotai-tui/locales/{pt-BR,es,it}.json`: 472 ids each, flat strings; the English source is in the code (`l10n::tr(id, "English")`), as in the core. The chrome extraction of RF5.2/RF5.3 was delivered by PRD 0034 RF2/RF3. | `crates/cuotai-tui/src/l10n.rs` (its own parser and three `LazyLock` statics) |
| Desktop language list | A hard-coded array of four entries in `i18n.js` (`LOCALES`), `FALLBACK = "en-US"`, and a second copy (`supportedLocales`) in the pre-paint script of `index.html`, `tray.html` and `settings.html`. The choice lives in `desktop.json` with a `localStorage` render mirror (`cuotai.locale`, PRD 0020 RF7.2) and reaches the backend with `set_locale`. | `js/i18n.js`, `*.html`, `lib.rs` (`set_locale`), `notify.rs` (`set_locale`) |
| Desktop detection | `pickLocale` and the pre-paint copy: saved choice, then each `navigator.languages` entry by exact code, then by **language base**, then en-US. `<html lang>` is set before the first paint (`scripts/test-language-first-paint-ui.mjs` keeps the copies equal). | `js/i18n.js`, `*.html` |
| Desktop plurals | `Intl.PluralRules(current)` cached per locale, then `v[rule] ?? v.other ?? first`; `tl()` (core texts) has no plural branch. User packs may carry plural objects. | `js/i18n.js`, `js/usercontent.js` |
| Core locale resolution | A **closed enum** `Locale { Pt, EnUs, Es, It }` resolved with `starts_with` prefixes; every other code, including `fr-FR`, renders the English fallback. The TUI (`l10n::catalog`, `ui::tui_locale_for`) matches on the same enum. | `text.rs` (`Locale::resolve`, `catalog_for`, `resolve_as_locale`) |
| Core lookup order | Catalog of the locale's language -> the embedded English `fallback`. The PRD 0034 special case "pt-* never consults a catalog" is gone from the core; the desktop `tl()` still skips the en-US UI catalog for pt-*. | `text.rs` (`Localized::resolve`), `js/i18n.js` (`tl`) |
| Core parity tests | `catalog_keys_match_between_pt_es_and_it` (key sets) and `catalog_placeholders_match_between_catalogs` (placeholder names) compare pt-BR with es and it, with literals; `every_descriptor_id_is_translated_in_every_catalog` and a dozen module tests loop `["pt-BR", "es", "it"]`. The TUI has its own source-id and parity tests. **No test compares the desktop UI catalogs with each other**; `desk_text.rs` only checks the `desktop.error.*` ids. | `text.rs`, `access.rs`, `crates/cuotai-tui/src/l10n.rs`, `apps/desktop/src-tauri/src/desk_text.rs` |
| Core plurals | None. Nine ids carry a noun `{count}` (`alerts.title.accounts`, `alerts.title.many`, `resets.title.reset_many`, `resets.title.soon_many`, `quiet.digest.title`, `quiet.digest.more`, `extensions.output.invalid`, `history.warning.dropped`, `hooks.warning.dropped`); two of them read "(s)". `edit_error.index_out_of_range` carries a length, not a noun count. | `alerts.rs`, `resets.rs`, `quiet.rs`, `extensions/output.rs`, `history/recorder.rs`, `hooks.rs` |
| Durations | The core words spans (`text::duration`, `text::countdown`: `text.duration.*`, PRD 0024 RF1.4) for the TUI and the desktop's core texts; the desktop's local countdowns (`legacyCountdown`, `fmtNext`) still concatenate `d`, `h`, `m`, `s`. | `text.rs`, `js/lib.js` |
| Flags | `flags.js` draws br, us, es, pt, fr, it, de, nl, ru, pl, ar, mx, jp; `LANG_FLAG` already maps ko, vi, tr, zh, hi and id ahead of their artwork, so they show the globe; jp, kr and id are marked as white-field flags with a hairline (RF7.5). | `js/flags.js`, `scripts/test-flags-ui.mjs` |
| TUI locale | `CUOTAI_LANG` verbatim when set (no validation), else `sys_locale::get_locale()` mapped through the closed core enum to `pt-BR`, `es`, `it` or `en-US`. | `ui.rs` (`tui_locale_for`) |
| TUI date and time | Language-neutral numeric formats (`HH:MM`, `YYYY-MM-DD HH:MM`), 24 h. | `ui.rs`, `dashboard.rs`, `main.rs` |
| TUI width | Most layout uses `Span::width()`/`Line::width()`, but several places still count `chars()` (`dashboard.rs`, `wizard.rs`), so CJK would misalign. | as listed |
| Desktop formatting | `Intl.NumberFormat(currentLocale())`, `toLocaleTimeString`, `toLocaleDateString`, `Intl.RelativeTimeFormat`; durations as above. | `js/lib.js` |
| Casing and search | Desktop account search folds with NFD and lower-case (`main.js` `norm`); the model search only lower-cases (`detail.js`); three `localeCompare()` calls have no locale (`lib.js`). | as listed |
| Typed consent answers | `hooks.confirm.answer`, `cost.confirm.answer`, `extensions.consent.answer`, compared with `to_lowercase()`. | core catalogs; `crates/cuotai-tui/src/cost.rs`, `extension_check.rs` |
| User language packs | One JSON per code in the user directory overrides a built-in catalog of the same code. The shipped example is a **Dutch** (`nl`, not built in) partial pack; it replaced the French one once `fr` became built in (RF12.3). | `apps/desktop/ui/locales/README.md`, `apps/desktop/examples/user/locales/nl.json` |

### 1.2 Flags: which ones exist

| Target | Country | Artwork in `flags.js` today |
|---|---|---|
| zh-CN | `cn` | **missing** |
| fr | `fr` | exists |
| de | `de` | exists |
| ja | `jp` (via `LANG_FLAG.ja`) | exists |
| ko | `kr` | **missing** (and `LANG_FLAG` has no `ko`) |
| hi-IN | `in` | **missing** |
| id-ID | `id` | **missing** |
| ru | `ru` | exists |
| tr | `tr` | **missing** |
| vi | `vn` | **missing** (and `LANG_FLAG` has no `vi`) |

Six flags are missing (cn, kr, in, id, tr, vn); fr, de, jp and ru exist. `LANG_FLAG` also needs `zh`, `ko`, `hi`, `id`, `tr` and `vi` for codes without a region. Region wins: `zh-CN` -> cn, `hi-IN` -> in, `id-ID` -> id, as `flagCountry()` already does.

### 1.3 Traps found while reading the code

- **Prefix matching.** `lower.starts_with("es")` (`text.rs:268`) also matches `est`, `esu` and any code that merely begins with those letters; ten more `starts_with` arms would multiply the problem. Matching must be on the parsed language subtag.
- **Chinese script.** `pickLocale` falls back to the language base (`i18n.js:113-115`), so a Traditional Chinese system (`zh-TW`, `zh-Hant`) would silently receive Simplified.
- **Legacy codes.** Java, Android and old browsers report Indonesian as `in`; `sys_locale` on Linux can report `zh_CN.UTF-8`, `hi_IN` or `id_ID.UTF-8`.
- **Plurals.** The core has no plural mechanism and its parser rejects objects; Russian needs four forms.
- **TUI chrome is not a catalog.** Adding ten core catalogs alone would translate cards' notes but leave the header, footer and wizard Portuguese in every language, which is the TUI's state for es and it today.
- **Fixed-size glyph assumptions.** `--text-2xs` is 10 px with line-height 1.1 (DS-TOK-TYP-3), which clips Devanagari and Vietnamese diacritics and is unreadable for Han characters; `letter-spacing` on micro labels breaks Devanagari conjuncts; Rust's `to_uppercase()` is not locale-aware (Turkish dotted I).

## 2. Problem

- About 80 % of internet users cannot read the interface in their language; Cuotai's audience (developers who pay for AI subscriptions) is heavily concentrated in China, Japan, Korea, India, Russia, Türkiye, Indonesia, Vietnam, France and Germany.
- Adding a language today means editing a closed enum, two prefix chains, a hard-coded JS array, three test loops and a flag table, in five places that must agree; there is no single source of truth.
- No tooling guarantees that a new catalog is complete in the places that matter: placeholders, plural forms, and the desktop UI catalogs have no parity test at all.
- Scripts and languages Cuotai has never rendered (Han, Hangul, Devanagari, stacked Vietnamese diacritics, Cyrillic) need font, size, line-height and width handling that the current CSS and the TUI do not have.
- Machine translation of 1 215 + 623 ids per language is cheap; translation that a native speaker would trust for credential, consent and error texts is not, and nothing in the repository defines who reviews what.

## 3. Goals

- One **locale registry** (code, endonym, flag, aliases, plural categories, script) that core, desktop and TUI consume; adding a language means one catalog per layer plus one registry row.
- Fourteen built-in languages with **full key, placeholder and plural parity**, enforced by tests that run on every push.
- Correct resolution of system and saved languages for every alias family (`zh-Hans`, `zh-CN`, `zh`, `hi`, `hi-IN`, `id`, `in`, `id-ID`), without ever mapping Traditional Chinese to Simplified.
- Typography, terminal width, casing, plural and number/date handling that behave correctly in each script.
- A documented translation process with a glossary, a critical-string review, honest provenance per language and a path for the community to improve any catalog.
- A phased rollout (infrastructure, then five languages, then five more) where each phase ships only when its gates pass.

## 4. Non-goals

- **Right-to-left languages** (Arabic, Hebrew, Persian, Urdu). None of the ten is RTL; no mirroring, bidi isolation or logical-property work is done here. The registry carries a `dir` field fixed to `ltr` so a later PRD can extend it.
- Traditional Chinese (`zh-TW`, `zh-HK`) as a built-in catalog (section 12, decision 2), Brazilian/European Portuguese variants, English variants, or regional variants of any language beyond what resolution already folds (es-AR -> es).
- Translating `CHANGELOG.md` entries (the "What's new" screen keeps entries in English and says so, `changelog.entries_language`, `text.rs:225-229`), `docs/`, the README, GitHub templates, provider-side content, installer wizards, store listings or the website.
- Downloading translations at run time, a hosted translation service, crowd-sourcing platforms or a runtime machine-translation call (NFR). Catalogs are files in the repository.
- Changing `export::v1`, `--json`, events or the TOML: those carry no localized prose except the Portuguese compatibility `notes`/`fallback` (PRD 0034 owns their fallback change).
- Locale-specific number systems (all fourteen use Latin digits by default, DS-TOK-TYP-10), an app-level 12/24-hour switch, or a measurement-unit setting.

## 5. Functional requirements

### RF1 — Locale registry and resolution (high priority; prerequisite)

- **RF1.1 Registry.** Replace the closed `Locale` enum and its `starts_with` chain (`text.rs:246-277`, `text.rs:393-401`, `text.rs:325-333`) with a data table `text::LOCALES` of rows `{ code, endonym, flag, script, dir, aliases, plural_rule, critical_reviewed }` (code in BCP 47 as listed below). `Locale::resolve`, `core_catalog`, `Localized::resolve` and nested-parameter resolution become table lookups; the three `LazyLock` catalog statics become one `LazyLock` per row created by a macro or a generated `match` over the table, so a new language cannot be added to the table without its `include_str!`.
- **RF1.2 Built-in codes.** `pt-BR`, `en-US`, `es`, `it` (unchanged) and `zh-CN`, `fr`, `de`, `ja`, `ko`, `hi-IN`, `id-ID`, `ru`, `tr`, `vi`. Regional codes only where the flag and the formatting need the region (zh-CN, hi-IN, id-ID, as the owner decided); bare codes elsewhere, following es and it.
- **RF1.3 Normalization.** A single `text::normalize(raw) -> Option<&'static LocaleRow>`: strip an encoding (`.UTF-8`) and a modifier (`@euro`), turn `_` into `-`, compare case-insensitively, split into language, optional script and optional region, then match in this order: exact code or alias; language + script (`zh-Hans-*` -> zh-CN); language + region for the languages with a regional row (`zh-CN`, `zh-SG` -> zh-CN, `hi-IN`, `id-ID`); the **language subtag alone** (`fr-CA`, `fr-BE`, `de-AT`, `es-AR`, `pt-PT` -> the language row). Matching is on the parsed subtag, never on a string prefix. Empty or unparsable input yields `None` (callers then use en-US).
- **RF1.4 Alias table (normative).**

| Input (case-insensitive) | Resolves to |
|---|---|
| `zh`, `zh-CN`, `zh-SG`, `zh-Hans`, `zh-Hans-CN`, `zh-Hans-SG`, `cmn-Hans-CN`, `zh_CN.UTF-8` | `zh-CN` |
| `zh-TW`, `zh-HK`, `zh-MO`, `zh-Hant`, `zh-Hant-*` | **none** (en-US), never `zh-CN` |
| `hi`, `hi-IN`, `hi_IN.UTF-8` | `hi-IN` |
| `id`, `id-ID`, `in`, `in-ID` (legacy Java/Android code) | `id-ID` |
| `ms`, `ms-MY` (Malay, mutually intelligible with Indonesian only partly) | none (en-US) |
| `fr`, `fr-FR`, `fr-CA`, `fr-BE`, `fr-CH` | `fr` |
| `de`, `de-DE`, `de-AT`, `de-CH` | `de` |
| `ja`, `ja-JP` | `ja` |
| `ko`, `ko-KR` | `ko` |
| `ru`, `ru-RU`, `ru-UA` | `ru` |
| `tr`, `tr-TR` | `tr` |
| `vi`, `vi-VN` | `vi` |
| `pt`, `pt-BR`, `pt-PT` | `pt-BR` (unchanged) |
| `es-*`, `it-*` | `es`, `it` (unchanged) |
| `en`, `en-*` | `en-US` (unchanged) |

- **RF1.5 One resolution, three consumers.** The desktop `pickLocale` (`i18n.js:106-118`) is replaced by the same algorithm: the registry (code, aliases, endonym, flag, scripts) reaches the UI through a Tauri command `locale_registry()` that serializes `text::LOCALES`; `LOCALES` in `i18n.js:21-26` is built from it, with a static copy used only when the command is unavailable (browser preview). The TUI's `tui_locale_for` (`ui.rs:2793-2805`) calls `text::normalize` and returns the row's canonical code; `CUOTAI_LANG` is normalized too, and an unrecognized value falls back to en-US with one stderr warning instead of being passed through verbatim (`ui.rs:2794-2796`). Saved choices that are not in the registry (downgrade) fall through to detection, as `i18n.js:108` does today.
- **RF1.6 Fallback chain.** Locale catalog -> en-US -> the id. After PRD 0034 the en-US step is the embedded English `fallback` of each `Localized` in the core and the TUI (they have no en-US file), and the en-US UI catalog on the desktop. The pt-* special case is already gone from the core (PRD 0034); the desktop `tl()` drops its remaining `startsWith("pt")` branch here. A registered language without a catalog yet ("translation in progress") resolves through the same chain, so it reads English until its catalog lands. Missing keys in a built-in locale fail the parity tests (RF2, RF3), so the chain only matters for user packs, languages in progress and ids added later.
- **RF1.7 Backend notices.** `set_locale` (`notify.rs:286-303`) keeps validating the character set but now also requires `normalize(locale).is_some()` to store the code; unknown codes are stored as en-US.
- **Acceptance criterion:** table tests of `normalize` for every row of RF1.4, for `est`, `ita`, `deu`, `kor`, `zh-Hant-TW`, `zh_CN.UTF-8`, `in`, empty and garbage; a test that the Rust registry, the catalogs on disk and the `locale_registry` output list the same codes; the TUI test at `ui.rs:2862-2867` extended to every row; no `starts_with` on a locale string remains in `text.rs`.

### RF2 — Core catalogs for ten languages (high priority)

- **RF2.1** Add `crates/cuotai-core/locales/{zh-CN,fr,de,ja,ko,hi-IN,id-ID,ru,tr,vi}.json` with every id of `en-US.json` (1 215 today; `_meta`, `access.*`, `field.*`, `native.*`, `login.*`, `credential_help.*`, `oauth*`, `edit_error.*`, `query.*`, `note.*`, `remove*`, `status.*`, `provider.*`, `binding.*`, `metric_window.*`, `pace.*`, `alerts.*`, `incident.*`, `resets.*`, `events.*`, `hooks.*`, `cost.*`, `extensions.*`, `changelog.*`, `serve.*`, `user_session.*`, `history.*`, `watch.*`, `identity.*`, `models.*`). Ids added by PRDs 0023 to 0033 and 0036 to 0038 reach all fourteen catalogs in the commit that adds them (RF11.6).
- **RF2.2 Parity test (replaces `text.rs:725-743`).** One test iterates `text::LOCALES` and fails when, for any locale and any id: the key set differs from en-US; the **set of `{placeholders}`** differs from en-US (names only, any order, so word order may change but no placeholder may be dropped, renamed or invented); a value contains markup (`<`, `>`) that en-US does not; a plural object lacks a required category (RF4); `_meta.name` is missing or differs from the registry endonym; a value is empty; a Vietnamese or any other value is not NFC; a value contains a control character other than `\n`. The listed locale loops at `text.rs:603` and `access.rs:895` iterate the registry instead of literals.
- **RF2.3 Proper nouns.** Cuotai, every provider and product name (`Provider::label`), model ids, environment-variable names, CLI flags and file names are never translated or transliterated (RF11.4 lists the rule); a test checks that `provider.*` values contain the provider's label.
- **RF2.4 Consent words.** `hooks.confirm.answer`, `cost.confirm.answer`, `extensions.consent.answer` are one short word in each language (at most 12 characters, no spaces, not a punctuation-only token). The comparison at `cost.rs:276-281` and `extension_check.rs:49-54`, `82` normalizes both sides with NFKC and case folding (so full-width Latin typed through a CJK input method matches) instead of `to_lowercase()`. A test proves each answer matches itself after NFKC and case folding and does not match the other two languages' answers by accident (an empty line never consents).
- **RF2.5 Size.** Per-catalog budget in NFR; `include_str!` of fourteen catalogs stays lazy per locale (the existing `LazyLock` pattern), so startup pays only for the locale in use.
- **Acceptance criterion:** `cargo test -p cuotai-core` passes with fourteen locales; deleting any one id from any catalog, renaming a placeholder, or adding a plural without `other` fails a named test; `core_catalog("ja")` returns the ja map; `Localized::resolve("ru")` of nested parameters resolves the nested id in Russian.

### RF3 — Desktop UI catalogs and their parity test (high priority)

- **RF3.1** Add `apps/desktop/ui/locales/<code>.json` for the ten languages with all 623 ids (`common`, `dialog`, `tray`, `settings`, `theme`, `account`, `card`, `delete`, `filter`, `lang`, `metric`, `quota`, `sort`, `time`, `update`, `cost`, `about`, `trend`, `extensions`, `models`, `userContent`, and every id added meanwhile), plural objects with the categories each language requires (RF4). `_meta` carries `name` (endonym), `flag` (only when the code has no region or the region is not the flag, e.g. none of the ten) and the provenance block of RF11.5.
- **RF3.2 Desktop parity test.** A new Rust test in `cuotai-core` (the repository has no JavaScript test runner, and `access.rs` already reads this directory from Rust) reads every `apps/desktop/ui/locales/*.json` and asserts the same conditions as RF2.2 against **`en-US.json`, the UI source** (PRD 0034), plus: every file is a registry locale with a UI catalog and every such locale has a file, the plural ids of en-US are plural objects in every locale with the categories of its rule, and no `access.*` id appears (existing rule). Ids that a catalog lacks relative to en-US are allowed only for the core ids en-US carries because the core has no en-US catalog (`pace.chip.*`, `status.*`), which the other languages take from their core catalog; ids a catalog has beyond en-US are allowed only for the two pt-BR leftovers (`status.ready`, `edit_error.login_cleanup_failed`). Both lists are explicit in the test and only shrink.
- **RF3.3 Language list.** `LOCALES` is built from the registry (RF1.5); `langMenuHtml` (`lib.js:566-574`) shows fourteen options. At fourteen entries the selector no longer fits a short menu: the Settings list (`lang-list`, radiogroup) scrolls inside its container and the popup menu, if kept anywhere, gets `max-height` with scrolling and type-ahead by endonym (DS-CMP-SEL-2 menu rules). The order is fixed: pt-BR, en-US, es, it, then the ten by phase 1 then phase 2 in the order of RF1.2 (no alphabetization across scripts).
- **RF3.4** Tray window and native tray menu use the same catalogs (`tray-menu.js` builds the menu in JS), so no separate work beyond the smoke test of RF13.
- **Acceptance criterion:** the new test fails on a missing id, an extra id, a placeholder mismatch, a malformed plural, an unregistered file and an unregistered locale; the app starts, switches among all fourteen languages without a console error and keeps the choice after restart.

### RF4 — Plural mechanism (high priority)

Today plurals exist only in the desktop (`Intl.PluralRules`, `i18n.js:154-161`); the core cannot express them. Required forms (CLDR categories used by the fourteen languages):

| Language | Categories | Notes |
|---|---|---|
| ja, zh-CN, ko, id-ID, vi | `other` | no grammatical plural |
| tr | `one`, `other` | the noun stays singular after a numeral; both forms are normally identical |
| en-US, de, es, it, pt-BR, fr, hi-IN | `one`, `other` | `one` is 1 in en, de, es, it; 0 and 1 in pt-BR, fr and hi-IN; `many` (millions) for es, it, pt-BR, fr is ignored and falls to `other` |
| ru | `one`, `few`, `many`, `other` | `one` 1, 21, 31 (not 11); `few` 2-4, 22-24 (not 12-14); `many` 0, 5-20, 25-30; `other` fractions |

- **RF4.1 Core.** `parse_catalog` accepts a catalog value that is either a string or an object of plural categories; `Localized::resolve` selects the form from the parameter named `count` (parsed as an integer; a non-numeric or missing `count` selects `other`), using `text::plural_category(row.plural_rule, n)`, a pure function with the rules above hard-coded per `plural_rule` (no ICU dependency). A form missing in a locale falls to `other`, then to the en-US form. Strings stay the default: only ids that need a number get an object.
- **RF4.2 Which ids.** The eight core ids listed in section 1.1 become plural objects in all fourteen catalogs (including en-US, `{count} problem(s)` becomes "1 problem" / "{count} problems"), plus any later id that embeds a count. Fixed phrasing such as "(s)" is removed from sources.
- **RF4.3 Desktop.** Keeps `Intl.PluralRules(current)` (browsers already implement the CLDR rules for all fourteen) and the existing `v[rule] ?? v.other ?? first` fallback; user packs unchanged.
- **RF4.4 Test.** A table-driven test of `plural_category` for each rule (1, 2, 5, 11, 12, 21, 22, 100, 101, 0, 1.5) that is **cross-checked against a fixture generated once from `Intl.PluralRules`** (a recorded JSON of expected categories per locale and number, so the Rust rules cannot drift from what the WebView does); the parity test of RF2.2/RF3.2 requires exactly the categories of the table above for each locale, no others.
- **Acceptance criterion:** "1 alert", "2 alerts", "5 алертов", "21 алерт", "22 алерта", "11 алертов" (ru) render correctly in notices, tray and `--once`; ja renders one form for any count.

### RF5 — TUI: locale selection and chrome catalog (high priority; scope decision)

- **RF5.1 Selection.** Per RF1.5: `tui_locale()` resolves through the registry, `CUOTAI_LANG` and `sys_locale` accept every alias of RF1.4, and `docs/tui.md:20-21` lists the fourteen codes. The in-TUI language is the same for core texts and chrome.
- **RF5.2 Chrome is currently not translatable.** The TUI's header, footers, hints, empty states, wizard prompts and ages are Portuguese literals (`dashboard.rs:649`, `729`, `738`, `811`; `ui.rs:2824-2845`; about 190 literals with diacritics plus those without). Fourteen core catalogs alone leave the dashboard mixed-language. This PRD therefore includes the extraction: every user-visible TUI literal becomes a `tui.*` id resolved through the same engine (RF1, RF4) from **TUI-owned catalogs** `crates/cuotai-tui/locales/<code>.json` (the strings are chrome, not backend messages, which keeps "do not define ... in TUI" for rules while text stays with its front end), exposing a small public core helper (`text::Catalog::parse`, `resolve`, `plural_category`, registry) so there is exactly one parser, one plural implementation and one fallback chain. Decided by the owner (section 12, decision 1).
- **RF5.3 Order of work.** Phase 0 extracts the Portuguese literals into `tui/locales/pt-BR.json` and creates `en-US`, `es`, `it` from them (so the extraction is verifiable against today's behavior), with a test that scans the TUI sources for string literals containing Portuguese diacritics and fails when one remains outside the catalog or an allow-list of identifiers; the ten new TUI catalogs follow with each phase. Key parity, placeholder and plural tests are the same as RF2.2.
- **RF5.4 Ages and durations.** `age_text` ("há 3 min", `ui.rs:2824-2845`) and the pace `margin` durations use catalog patterns with `{n}` placeholders and plural forms, not concatenation (DS-MSG-12). The same `duration.*` ids are used by the desktop (RF10.3).
- **RF5.5 Consent prompts** follow RF2.4; the prompt shows the word the user must type.
- **Acceptance criterion:** `CUOTAI_LANG=ja cargo run -p cuotai-tui -- --once` prints a fully Japanese report (chrome and core text); the source scan is green; `CUOTAI_LANG=xx` prints the warning and uses en-US.

### RF6 — TUI terminal width, glyphs and text handling (high priority)

- **RF6.1 Display width everywhere.** Replace every `chars().count()` used for layout (`dashboard.rs:344`, `1133`, `1162`, `1926`, and the instances in `motion.rs` and `wizard.rs`) with display width from `unicode-width` (add it as a direct dependency of `cuotai-tui` at the version already in `Cargo.lock`, so no new crate enters the tree). Truncation with an ellipsis is by display columns and never splits a double-width character or a combining sequence: if a wide character does not fit in the last column the column is left blank.
- **RF6.2 Wide input.** The wizard's and filter's text cursor moves by grapheme and renders at display width; CJK account names typed through an input method (including pre-edit text that the terminal composes) must not desynchronize the cursor.
- **RF6.3 Ambiguous-width glyphs.** CJK terminals commonly render East Asian Ambiguous characters (`↓ → ↑ ✕ ◆ ▲ ·`, used by pace and status shapes) as two cells. For `zh-CN`, `ja` and `ko` the width function uses the CJK (ambiguous = 2) table; for the others the narrow table. A switch `CUOTAI_AMBIGUOUS_WIDTH=narrow|wide` overrides it. A test renders the card and header of each locale with `TestBackend` at 80 columns under both widths and asserts no line exceeds the area and no border is displaced.
- **RF6.4 Casing.** `to_uppercase()` for headings (`changelog_view.rs:162`, `dashboard.rs:1883`) is removed: headings come from the catalog already in their final casing (DS-TOK-TYP-5 applies uppercase only through CSS on the desktop; the TUI uses weight/underline for the same role in scripts without case). TUI filter matching (`dashboard.rs:115-122`, `models_view.rs:72`) uses the same folding as the desktop (RF10.5) rather than `to_lowercase()`.
- **RF6.5 Terminals.** `docs/tui.md` states the requirement: a UTF-8 terminal and a font with the script (Windows legacy `conhost` with a bitmap font cannot show CJK or Devanagari; Windows Terminal, iTerm2, Terminal.app, GNOME Terminal and Konsole do with a system fallback). Devanagari conjuncts and Vietnamese stacked diacritics are terminal-dependent; the TUI cannot fix them and says so.
- **RF6.7 ASCII fallbacks (owner decision 7).** When ambiguous-width glyphs are wide (CJK locales or `CUOTAI_AMBIGUOUS_WIDTH=wide`), the pace and status shapes `↓ → ↑ ✕ ◆ ▲` are replaced automatically by ASCII equivalents (for example `v > ^ x * !`) so columns stay aligned; the mapping is one table and the shape still carries the state, color only repeating it.
- **Acceptance criterion:** snapshot tests for each CJK locale at 80 x 24 and 120 x 40; an account named `日本語のアカウント` aligns in the card, the list and the filter; the footer never wraps because of width miscounting.

### RF7 — Flags, endonyms and the language selector (medium priority)

- **RF7.1 Artwork (DS-CMP-SEL-3, DS-COL-1).** Add to `FLAGS` (`flags.js:15-29`) simplified 30 x 20 SVGs for `cn`, `kr`, `in`, `id`, `tr`, `vn` (flat shapes, no gradients, recognizable at 20 x 13 px; `kr` reduces the taegeuk and trigrams to the circle plus four bar groups; `in` draws the chakra as a ring with a hub and 24 spokes only if legible, otherwise a ring with a dot). Add to `LANG_FLAG` (`flags.js:31`): `zh: "cn"`, `ko: "kr"`, `hi: "in"`, `id: "id"`, `tr: "tr"`, `vi: "vn"`. fr, de, jp and ru reuse existing artwork. This is the only multicolor artwork besides provider tiles and the logo (DS-COL-1).
- **RF7.2 Resolution.** `flagCountry()` is unchanged in behavior: explicit `_meta.flag`, then region (`zh-CN` -> cn, `hi-IN` -> in, `id-ID` -> id), then language. A test (Node-free, run as a Rust test that parses the two maps, or a `<script type=module>` check in the existing smoke page) asserts that every registry locale resolves to a drawn flag, not the globe.
- **RF7.3 Endonyms (`_meta.name`, DS-CMP-SEL-3).** `中文（简体）` (full-width parentheses), `Français`, `Deutsch`, `日本語`, `한국어`, `हिन्दी`, `Bahasa Indonesia`, `Русский`, `Türkçe`, `Tiếng Việt`, next to the existing `Português`, `English`, `Español`, `Italiano`. Never a translated name, never in Latin transliteration.
- **RF7.4 Accessibility and fonts in the selector.** Every option carries `lang="<code>"` on its name so a screen reader pronounces it and the engine picks the right glyph variant (`中文（简体）` as zh-Hans, `日本語` as ja share Han glyphs: without `lang` they render with the wrong regional forms); the flag is `aria-hidden="true"`; the option text uses the script stack of RF8 regardless of the current UI language.
- **RF7.5 White fields (owner decision 6).** jp, kr and id have large white areas that disappear on light surfaces, so these three flags get a 1 px inner hairline in `--border`. This amends DS-CMP-SEL-3 ("hairline-free"), which the design-system owner updates in the same delivery. The RF13 review still checks all fourteen flags on the four reference themes.
- **Acceptance criterion:** the selector lists fourteen options with flag and endonym in every reference theme; none shows the globe; the active option is announced in its own language.

### RF8 — Typography per script (high priority; implements DS-TOK-TYP-7..10)

- **RF8.1 Language attribute (DS-TOK-TYP-8).** `<html lang>` is set before first paint from the saved or detected locale, not left as `pt-BR` (`index.html:2`, `tray.html:2`) until `initI18n` finishes (`i18n.js:126`); it uses the **registry code** (`zh-CN`, not `zh`), so the engine selects the Simplified Han glyphs and Turkish casing. Text in another language (endonyms, provider names that are not translated) carries its own `lang` (DS-TOK-TYP-8).
- **RF8.2 Font stacks (DS-TOK-TYP-7, DS-TOK-TYP-1, DS-GOV-3).** `themes.css:20-21` keeps the theme's `--font-ui`/`--font-mono` first; text is set as `font-family: var(--font-ui), var(--font-script)` and `var(--font-mono), var(--font-script)`, with `--font-script` defined once under `:root:lang(zh)`, `:lang(ja)`, `:lang(ko)`, `:lang(hi)`, `:lang(ru)` and `:lang(vi)` using the system stacks of DS-TOK-TYP-7 (zh-CN: PingFang SC / Microsoft YaHei UI / Noto Sans CJK SC; ja: Hiragino Sans / Yu Gothic UI / Noto Sans CJK JP; ko: Apple SD Gothic Neo / Malgun Gothic / Noto Sans CJK KR; hi-IN: Kohinoor Devanagari / Nirmala UI / Noto Sans Devanagari). No web fonts, no `@font-face` download (the app is offline, DS-TOK-TYP-1). Themes do not define `--font-script` (DS-GOV-3).
- **RF8.3 Sizes and line-height (DS-TOK-TYP-9, DS-TOK-TYP-3, DS-CMP-ROW-1b).** CJK body 1.5, Devanagari 1.6, Vietnamese 1.45 (Vietnamese strings are stored NFC, enforced by RF2.2); `--text-2xs` (the unit word under a figure, 10 px today) resolves to 11 px under CJK and Devanagari locales and the two-line figure block of DS-CMP-ROW-1b is re-measured at that size; the figure line keeps 1.1 (digits are Latin). Components with a fixed height that contains text (rows, chips, buttons, segmented controls, sidebar items) are checked at the larger line-height and either grow or use `min-height`; none may clip a diacritic or a matra (`ệ`, `ữ`, `ि`, `ी`, `ं`).
- **RF8.4 No faux styles (DS-TOK-TYP-9, DS-TOK-TYP-4, DS-TOK-TYP-5).** Under CJK and Devanagari locales `font-synthesis: none`; no italics for emphasis in any catalog (use weight or `--fg-muted`); weights 650 and 700 map to the family's bold (Microsoft YaHei UI, Yu Gothic UI, Malgun Gothic and Nirmala UI have only regular and bold); `text-transform: uppercase` and `letter-spacing` of micro labels are dropped for CJK and Devanagari (tracking breaks Devanagari conjuncts), but **kept** for tr, where `lang="tr"` yields `İ`/`ı` correctly.
- **RF8.5 Line breaking (DS-TOK-TYP-9).** CJK uses `line-break: strict` and no hyphenation; Latin/Cyrillic languages keep `overflow-wrap: anywhere` only on long identifiers; German allows `hyphens: auto` on description paragraphs (never on buttons, chips, numbers or labels) given `lang="de"`.
- **RF8.6 Numbers stay Latin and tabular** (DS-TOK-TYP-2, DS-TOK-TYP-10, DS-MSG-7) in every locale; verify that `hi-IN`, `ja` and `zh-CN` do not substitute full-width or Devanagari digits in `Intl` output.
- **RF8.7 Mixed content.** Latin account names, provider names, model ids and file paths inside a CJK or Devanagari sentence render with the theme's Latin font; a long unbreakable Latin token wraps (`overflow-wrap`), it does not widen the container.
- **Acceptance criterion:** the main window, tray, details, settings and account dialog render in the reference themes (Dracula, GitHub light, Win98, CRT) with zh-CN, ja, ko, hi-IN, ru, vi and tr without clipped marks, without synthetic bold, and with visible glyphs on macOS, Windows and Linux (Noto packages present or documented); `docs/platforms.md` lists the Linux font packages to install for CJK and Devanagari.

### RF9 — Length budgets and the pseudo-locale (medium priority; DS-MSG-14..16)

- **RF9.1 Worst case.** German is expected to be the longest for labels and compounds (for example `Einstellungen`, `Aktualisieren`, `Kontingent`), then Russian (about +30 % with long words that cannot break) and French (+20-30 %); Vietnamese and Indonesian run longer than English in words but short per word; Chinese and Japanese are the shortest in characters but about 1 em wide per glyph, so budgets are in rendered pixels at the target font size (DS-MSG-14).
- **RF9.2 Budget table.** The first phase produces, from the real catalogs, a table of the longest rendered width per component class (button, chip, tab, sidebar item, row title, figure unit word, status line) across all fourteen languages, stored with the review evidence (RF13); components are laid out against that maximum and verified at the minimum window size and the 380 px tray (DS-MSG-15 policies unchanged). Characters are never the unit: a `max_render_width` check uses a headless render of the strings in the theme font.
- **RF9.3 Translators' guidance.** Each translation carries the rule "shorter is better for labels": a label key whose draft is more than 30 % longer than en-US or longer than the class limit in RF9.2 is flagged for rewording during review, not truncated by the UI. Descriptions and callouts may wrap (DS-MSG-15).
- **RF9.4 `en-XA` pseudo-locale (DS-MSG-16).** Review also renders each changed screen in de, in one of ja or zh-CN and in hi-IN (DS-MSG-16). Development builds register `en-XA`, generated at load from en-US (expand +100 % up to 10 characters, +50 % up to 20, +40 % beyond, accented and stacked-diacritic letters, ⟦ ⟧ around each string), and never in release builds (`cfg(debug_assertions)` in the registry and a test that the release registry has exactly fourteen rows). The core exposes the same transform for `--once` smoke runs (`CUOTAI_LANG=en-XA`, debug only).
- **RF9.5 Fixed-width audit.** The CSS audit lists every fixed `width`/`max-width`/`height` that contains text (sidebar, chips, segments, plan badge, unit block) and records the result for de, ru, zh-CN, ja and hi-IN; failures are fixed by the component, not by shortening the translation.
- **RF9.6 Chip and label budgets in every catalog (DS-CMP-TIME-1, DS-MSG-14).** Time and pace chips come from core keys and have hard character budgets: at most 18 characters including "≈" and the time, and at most 22 for `pace.chip.ahead_pct` and `pace.chip.reserve_pct` (the percent-mode chips, "{p}% faster than pace" / "{p}% slower than pace"). A core test checks, for every locale and every `pace.chip.*` and time-chip id, the budget on the **substituted** text with placeholders replaced by their widest sample (`{d}` = 99, `{p}` = 100, a 5-character time); for zh-CN, ja and ko a CJK character counts as two characters, so the budget is a display width, consistent with the pixel rule above. A catalog that exceeds a budget is a failing test, and the translator rewords (RF9.3).
- **RF9.7 Ids from sibling PRDs.** The labels introduced by the other 2026-10 plans must be in all fourteen catalogs when they ship (RF11.6), and are budgeted like any control label: the pace chip keys (PRD 0024 and DS-CMP-TIME-1..5), the dashboard density presets and custom-display switches (Detailed, Standard, Minimal, Custom and one label per row element, DS-CMP-DENS-1..5; labels at most two words, DS-CMP-SEG-2 parallel wording, segment widths fit the longest catalog), the points/percent deviation setting (PRD owner decision M), the overage texts (`overage.*`, PRD 0023 RF3.3) and the menu-bar style options (PRD 0025). The `en-XA` and longest-locale review (DS-MSG-16) covers these screens in de, ja or zh-CN, and hi-IN.
- **Acceptance criterion:** the checklist item of DS-GOV-5 (longest locale or pseudo-locale at the minimum size) passes in de, ru, ja and hi-IN for every screen changed in the phase; `en-XA` shows no unwrapped hard-coded text.

### RF10 — Numbers, dates, times, durations, casing and search (medium priority)

- **RF10.1 Numbers and currency** stay `Intl.NumberFormat(currentLocale())` (DS-MSG-7); the language code passed to `Intl` is the registry code, so `hi-IN` uses lakh/crore grouping only when the locale's `Intl` data does (it does for `hi-IN`) and `de`, `fr`, `ru`, `tr`, `vi`, `id-ID` use a decimal comma. Tests of figures (`82 %`, `$6.25`, `1,2 M`) render per locale. Cuotai never concatenates a number to a unit outside a translated pattern.
- **RF10.2 Clock.** Desktop follows `Intl` defaults for the locale (en-US and ko typically 12 h with day period, hi-IN 12 h, the rest 24 h; id-ID uses a dot separator). The TUI and CLI keep their language-neutral 24-hour numeric formats (`HH:MM`, `YYYY-MM-DD`; no month or weekday names, no chrono locale feature), which is intentional and documented, so `--once` output is parseable and identical across languages. The desktop offers no 12/24 h override for now (section 12, decision 5).
- **RF10.3 Durations.** `fmtCountdown`, `fmtNext` (`lib.js:306-325`) and the TUI's pace margin and ages stop concatenating `d`, `h`, `m`, `s` and use catalog patterns `duration.dhm`, `duration.hm`, `duration.m`, `duration.dh`, `duration.ms` with `{d}`, `{h}`, `{m}`, `{s}` placeholders (for example ja `{h}時間{m}分`, ru `{h} ч {m} мин`, en `{h}h {m}m`), in the **core** catalog so desktop and TUI read the same words; ages use `Intl.RelativeTimeFormat` on the desktop and `age.*` plural patterns in the TUI chrome. DS-MSG-8's example text stays valid for English.
- **RF10.4 Dates** without a time use `toLocaleDateString` inside translated sentences ("Released {date}"), so word order and the era/day markers (`2026年10月22日`) belong to the translation (DS-MSG-8, DS-MSG-12). The ISO dates in the TUI are not translated.
- **RF10.5 Casing and folding.** All user-visible uppercase is CSS (`lang`-aware); JavaScript uses `toLocaleLowerCase(currentLocale())` only where the language needs it and otherwise the shared `fold()` helper: NFD, strip combining marks `U+0300-036F`, then map `đ -> d`, `ı -> i`, `ß -> ss`, `ł -> l` (letters that do not decompose), lower-case; used by the account filter (`main.js:164`), the model search (`detail.js:102-103`) and, as the same table in Rust, by the TUI filter. Searching `dong` finds `Đồng`, `istanbul` finds `İstanbul`, `strasse` finds `Straße`. CJK and Devanagari text is compared as typed (NFC).
- **RF10.6 Sorting** of provider labels and lists uses `Intl.Collator(currentLocale())` (account dialog already does, `account-dialog.js:41`); other `localeCompare()` calls without a locale (`lib.js:549`, `876`, `1088`) become explicit. Account order is the TOML order and is not sorted.
- **Acceptance criterion:** unit tests of `fold()` and the Rust twin with the examples above; durations, ages, dates and money screenshot-checked in all fourteen locales; `--once` output has the same shape in every language.

### RF11 — Translation process, glossary and quality (high priority)

- **RF11.1 Source and workflow.** en-US is the source (PRD 0034). A new language is produced in four steps: (1) **machine draft** of both catalogs and the TUI chrome from en-US by a translation model run by the maintainer with the glossary (RF11.4) and the placeholder rules as instructions; the raw output is committed only after the parity tests pass; (2) **placeholder and length checks** (RF2.2, RF9.3) automatically; (3) **native review** of the critical strings (RF11.3) by a named reviewer before the language ships in a release; (4) **community review** of the remaining strings through pull requests (RF12). The committed catalogs are the product; no translation memory service or tool is introduced.
- **RF11.2 Who translates.** The maintainer produces drafts; native reviewers are volunteers recruited through GitHub Discussions (a "Looking for reviewers" thread per language) and the owner's network; reviewers are credited by GitHub handle in the status table. A language without a reviewer for its critical strings is **not shipped** (section 12, decision 3: non-critical text may ship machine-translated and labelled).
- **RF11.3 Critical strings.** Reviewed first and by a human, because a mistranslation causes harm or a wrong consent: `credential_help.*`, `edit_error.*`, `remove_error.*`, `oauth_error.*`, `query.*`, `user_session.*`, `access.risk.*`, `access.credential_hint.*`, `serve.*`, every `*.confirm.*` and `*.consent.*` id (including the typed answers), alert and reset notice bodies (`alerts.*`, `resets.*`), and the desktop texts for deleting credentials, privacy, the updater and the account dialog (`dialog.delete*`, `update.*`). That is roughly 400 of 1 215 core ids and about 90 UI ids per language. Everything else (labels, hints, notes about provider behavior) is draft quality at first.
- **RF11.4 Glossary.** A glossary table lives in `apps/desktop/ui/locales/README.md` (next to the process, RF14) and is the first input of every draft. Rules: **never translate** Cuotai, provider and product names, plan names as the provider spells them, model ids, `CUOTAI_*`/environment-variable names, CLI flags, file names, TOML keys, the words `OAuth`, `API`, `token`, `URL`, `JSON`, `TOML`, `CLI`, `TUI` where the language uses them in English. **One term per concept** across core, UI and TUI (DS-MSG-13: account, provider, limit/quota, binding limit, pace, reset, reading, fresh/aging/stale, incident, access level, usage alert, reset notice, extension, history). Seed proposals for the first draft (reviewers decide and may overturn them in a pull request):

| Term | zh-CN | fr | de | ja | ko | hi-IN | id-ID | ru | tr | vi |
|---|---|---|---|---|---|---|---|---|---|---|
| account | 账户 | compte | Konto | アカウント | 계정 | खाता | akun | учётная запись | hesap | tài khoản |
| quota | 额度 | quota | Kontingent | クォータ | 할당량 | कोटा | kuota | квота | kota | hạn mức |
| reset | 重置 | réinitialisation | Zurücksetzung | リセット | 초기화 | रीसेट | reset | сброс | sıfırlama | đặt lại |
| pace | 节奏 | rythme | Tempo | ペース | 페이스 | गति | laju | темп | hız | nhịp độ |

  The existing es and it catalogs are the precedent for tone (informal "tú" in es, "tu" in it, no honorific mismatch): the glossary also fixes **register** per language (formal polite: ja です/ます, ko 해요체/합쇼체 consistently, de Sie, fr vous, ru вы, tr siz, vi bạn, id Anda, zh-CN 您 only in error and consent text, otherwise omitted, hi-IN आप), chosen once and stated in the README.
- **RF11.5 Provenance and status.** Each catalog's `_meta` gains optional `status` (`machine` or `reviewed`), `reviewed` (list of `{ handle, date, scope: "critical" | "full" }`) and `source` (the Cuotai version of en-US it was synchronized with). The parity test validates the shape; the registry exposes `status`. Settings › General shows, under the language list, one muted line for the active language when its status is `machine`: "Machine-translated, partly reviewed. Help improve it" linking to CONTRIBUTING (DS-MSG-5 honesty: the product says what it does not know). The line disappears when `full` review is recorded.
- **RF11.6 Keeping catalogs in sync.** A PR that adds or changes an en-US id must add or update it in **all** built-in catalogs in the same commit (the parity tests enforce presence). The contributor adds a machine draft for languages they cannot write and marks the id in `_meta.pending_review` (a list of ids that the next reviewer sees first); the status table (RF14) shows the pending count per language. A changed en-US wording whose placeholders are unchanged is re-translated only if the meaning changed (reviewer's judgment).
- **RF11.7 Review checklist** (in the locales README, used by reviewers and by PRs touching a catalog): placeholders intact and order natural; consent word typable; no brand or provider name translated; glossary terms used; register consistent; critical texts say what happened and what to do (DS-MSG-9) without blame; units and numbers outside the sentence; no markup, no exclamation marks (DS-MSG-1), sentence case where the language has case (DS-MSG-2); lengths within RF9.3; for ja/zh-CN/ko, no machine-translation artifacts (a Han word used with the wrong reading, mixed width punctuation); the native reviewer signs in the PR.
- **Acceptance criterion:** the README contains the glossary, the register table, the checklist and the status table; a PR template section for catalog changes exists; `_meta` shape is tested; the first shipped language has a recorded reviewer for its critical strings.

### RF12 — Community contributions and user packs (medium priority)

- **RF12.1 CONTRIBUTING** gains a "Translations" section: how to fix a string (edit the three catalogs of one language, run `cargo test --workspace`, read the checklist), how to propose a new language (open an issue "Language: xx", the registry row, three catalogs, the flag, reviewers), how reviewers are credited, and the English-only rule for code comments and docs (the product is multilingual, the repository is not, AGENTS.md "Documentation Language"). Its "Documentation language" paragraph (`CONTRIBUTING.md:98-100`) and README line 31 list all fourteen languages.
- **RF12.2 Issue template** "Translation fix": language, id, current text, suggested text, context screenshot, native-speaker confirmation.
- **RF12.3 User language packs keep working** (PRD 0001, `usercontent.js`): a pack whose code equals a built-in overrides that catalog key by key, and a pack for a language not built in (`nl`, `pl`, `ar`, `mx` flags already exist, `flags.js:23-27`) still registers with `registerLocale` (`i18n.js:45-52`). Because `fr` is now built in, the shipped example `apps/desktop/examples/user/locales/fr.json` is replaced by a partial `nl` pack (the flag exists) and the README of the user directory says that a pack with a built-in code overrides it. A user pack for `zh-TW` is the documented way to add Traditional Chinese until a Traditional Chinese catalog exists (section 12, decision 2).
- **RF12.4 Translation fixes are ordinary `fix(i18n)` commits** (a patch at the next release, with a `Changelog:` trailer under **Fixed** or **Changed**); a new language is a `feat(i18n)` (a minor at the next release), per PRD 0039 and AGENTS.md "Versioning, Commits & Changelog".
- **Acceptance criterion:** a contributor following only CONTRIBUTING can correct a Japanese string and see CI fail if a placeholder was lost; the example pack loads without a console error.

### RF13 — Validation per language: screenshots and smoke (medium priority)

- **RF13.1 Evidence set.** For every language of a phase, screenshots stored under `docs/plans/assets/0035/<code>/` (review evidence, not shipped): main window (cards, one account in each of the stale, exhausted and overage states), tray popover and native tray menu, details, Settings › General (language selector) and Notifications, the account dialog, a delete confirmation, and the updater card, in two themes (dark and light of a reference theme) plus Win98 and CRT once, at the minimum window size. The mockup generator used for PRD 0022 (`docs/plans/assets/0022/after`, its locale layer `src/i18n.mjs`) is extended with the new codes so the mockups resolve strings from the real catalogs.
- **RF13.2 TUI evidence.** `--once` text and a 120 x 40 `TestBackend` snapshot per language; a real terminal capture in Windows Terminal or iTerm2 for zh-CN, ja, ko and hi-IN.
- **RF13.3 Plural and number checks** per language with counts 0, 1, 2, 5, 11, 21, 22 and 1.5 on the six plural ids of the desktop and the eight core ids.
- **RF13.4 Cross-platform fonts.** The glyph check (no tofu, right Han variant, right diacritics) is done on macOS, Windows and one Linux with and without Noto CJK installed; the Linux fallback behavior is documented.
- **RF13.5 Review gate.** A phase is released only when the DS-GOV-5 checklist is ticked for each of its languages, the native reviewer has signed the screenshots (not only the text), and the evidence folder exists.
- **Acceptance criterion:** the folder for each shipped language exists and the PR description links it.

### RF14 — Documentation, changelog and status table (low priority)

- **RF14.1** `apps/desktop/ui/locales/README.md` is rewritten: fourteen languages, the registry, "Adding a built-in language" (registry row, three catalogs, flag, `LOCALES` generated from the registry, tests), plural rules, glossary, register table, checklist, provenance, and the **translation status table** (below); the line "Missing keys fall back to en-US" stays and states that built-in catalogs may not miss keys.
- **RF14.2** `docs/architecture.md` (the RF8 catalog section) and `docs/tui.md:20-21` list the fourteen codes, the registry and the TUI chrome catalog; `docs/platforms.md` lists the font and terminal requirements; `README.md:31` and `CONTRIBUTING.md:100` list the languages; `docs/design-system.md` gets a one-line status update on the planned rows (DS-MSG-12, DS-CMP-SEL-3) when each phase ships. `AGENTS.md` wording about catalogs is owned by PRD 0034; this PRD updates it again to name the registry.
- **RF14.3** Changelog per phase under **Added**: "The interface and messages are now available in ... (PRD 0035 RF...)", with the reviewed/machine status stated honestly, and **Changed** for the plural and duration pattern changes.
- **Acceptance criterion:** documentation-drift tests (existing) pass; the README table matches the registry (a test reads the table's codes).

#### Translation status table (initial state; maintained in the locales README)

| Locale | Phase | Core catalog | UI catalog | TUI chrome | Critical review | Full review | Flag | Status |
|---|---|---|---|---|---|---|---|---|
| pt-BR | shipped | embedded fallback, catalog by PRD 0034 | 630 ids | extract (RF5.3) | n/a (source of product tone) | maintainer | br | shipped |
| en-US | shipped | 1 215 | 623 | extract | source | maintainer | us | shipped |
| es | shipped | 1 215 | 623 | extract | pending | pending | es | shipped |
| it | shipped | 1 215 | 623 | extract | pending | pending | it | shipped |
| zh-CN | 1 | — | — | — | — | — | cn (add) | Proposed |
| fr | 1 | — | — | — | — | — | fr | Proposed |
| de | 1 | — | — | — | — | — | de | Proposed |
| ja | 1 | — | — | — | — | — | jp | Proposed |
| ru | 1 | — | — | — | — | — | ru | Proposed |
| ko | 2 | — | — | — | — | — | kr (add) | Proposed |
| hi-IN | 2 | — | — | — | — | — | in (add) | Proposed |
| id-ID | 2 | — | — | — | — | — | id (add) | Proposed |
| tr | 2 | — | — | — | — | — | tr (add) | Proposed |
| vi | 2 | — | — | — | — | — | vn (add) | Proposed |

## 6. Non-functional requirements

- **No runtime download.** All catalogs are files in the repository, embedded (core, TUI) or bundled (desktop UI); the app makes no network call to obtain a translation, a font or a locale database. User packs remain local files.
- **Size.** Raw size per core catalog at most 350 KB (en-US is 131 KB; Devanagari and Cyrillic cost 2-3 bytes per character) and per UI catalog at most 120 KB (en-US is 42 KB); the fourteen core catalogs together stay under 3.5 MB raw and add at most 3.5 MB to a release binary and 1.5 MB to the compressed installer. The numbers are measured in phase 0 and revised once, in this PRD, if the measurement disagrees. The desktop fetches only the active UI catalog and en-US (`i18n.js:54-67`), and core catalogs parse lazily per locale (`LazyLock`, `text.rs:304-319`), so memory and startup depend on the active language only.
- **Accessibility.** `<html lang>` equals the active registry code from first paint (RF8.1); endonyms in the selector carry their own `lang`; flags are decorative; the state of a language (machine, reviewed) is text, not color; focus order and roles of the language list are unchanged (DS-A11Y); screen-reader checks with VoiceOver (macOS), Narrator (Windows) and Orca (Linux) read the selector options in their languages at least once per phase.
- **Honesty.** Machine-translated catalogs are labelled as such (RF11.5); no catalog claims review it does not have; an unknown value is "—" in every language, never a translated "0".
- **Right-to-left is out of scope.** None of the fourteen languages is RTL; layout uses no `direction` handling; the registry's `dir` stays `ltr` and a test fails if a registry row declares `rtl` without a PRD.
- **Compatibility.** `--json`, `export::v1`, events and TOML are unchanged (no localized prose, Portuguese compatibility `notes` unchanged); `CUOTAI_LANG` values that worked keep working (`pt-BR`, `es`, `it`, `en-US`, `es-AR`), and a previously accepted unknown value now warns and uses en-US instead of resolving every `Localized` through the Portuguese fallback; this is noted in the changelog. User packs keep overriding built-ins.
- **Security.** Catalog text is plain text escaped before rendering (`util.js`, `te()`/`tle()`); the parity test forbids markup. Translations never carry secrets, URLs with credentials or response bodies (the invariant of `text.rs:20-24` applies to params). The consent words are not spoofable by another catalog (RF2.4).
- **Performance.** `normalize` is a table scan of fourteen rows; the desktop switch between languages keeps the cross-fade (`motion.js`) and remains instantaneous; the `Intl.PluralRules` object is cached per locale instead of created per call (`i18n.js:157`).
- **Maintainability.** A new built-in language is one registry row, three catalogs (core, UI, TUI), one flag if missing, and passes without editing a test loop, a prefix chain or a hard-coded array.
- **Privacy.** The detected system language is used locally; it is never sent anywhere. The saved choice stays in `desktop.json` and local storage as today.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `crates/cuotai-core` public API | `text::LOCALES`, `text::normalize`, `text::plural_category`, `text::Catalog`; `Locale` enum removed or replaced by a registry handle | Pre-1.0 library API change, noted in the changelog; `core_catalog(locale)` and `Localized::resolve(locale)` keep their signatures |
| Core catalogs | ten new files; eight ids become plural objects in all catalogs; `duration.*` ids added; `_meta` gains `status`, `reviewed`, `source`, `pending_review` | Additive; plural objects are new on the wire to the desktop through `core_catalog` (`lib.rs:308-317` returns `BTreeMap<String, String>` and changes to a map of string or plural object) |
| Desktop catalogs | ten new files; `_meta` additions; `duration.*` removed from nothing (new in core) | Additive |
| Tauri commands | new `locale_registry()`; `core_catalog` return type widened | Internal to the app |
| TUI | `crates/cuotai-tui/locales/*.json` (new); `CUOTAI_LANG` normalized | Documented; unknown values warn |
| `desktop.json` | none (`locale` already stored, `settings.rs:46`) | — |
| TOML, `--json`, `export::v1`, events | none | — |
| Docs | locales README, CONTRIBUTING, architecture, tui, platforms, README | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | registry, normalization, plural rules, catalog parsing, core catalogs, parity tests of core and desktop catalogs, pseudo-locale transform |
| Desktop | UI catalogs, `locale_registry` consumption, language selector, flags, `lang` attribute, script typography, Intl formatting, folding helper |
| TUI | locale selection through the registry, chrome catalogs, display-width layout, width-aware input, ASCII/numeric time formats |
| Docs and process | glossary, register, checklist, status table, contributor workflow, evidence folders |

## 9. Dependencies, sequencing and phased rollout

**Dependencies.** PRD 0034 makes en-US the source and fallback and turns pt-BR into a catalog; RF2 and RF3 add catalogs against that source (adding them against today's Portuguese-fallback model would repeat the work). RF1, RF4, RF5 (extraction part) and RF6 do not depend on it and can ship first. New ids introduced by PRDs 0023 to 0033 and 0036 to 0038 must reach every catalog by RF11.6; the more languages exist, the more expensive a late id is, so phase 0 also adds a CI check that a PR touching `en-US.json` touches all catalogs.

**Phase 0 (infrastructure, no new language visible).** RF1, RF4, RF5.1-RF5.4 (extraction into pt-BR/en-US/es/it), RF6, RF7.1-RF7.2 (flags), RF8, RF9.4 (pseudo-locale), RF10.3, RF10.5, the parity tests (RF2.2, RF3.2), the size measurement. Delivered as one or more `feat`/`refactor` commits; the user-visible gains are correct language detection, plural-aware core texts and duration patterns.

**Phase 1: zh-CN, fr, de, ja, ru.** Rationale: these five exercise every distinct technical risk once, so the infrastructure is proven before volume grows: **zh-CN and ja** (largest unserved and high-value audiences; Han glyph variants, no plural, no spaces, CJK terminal width, CJK fonts on three OSes), **ru** (the only four-form plural, Cyrillic, long words), **de** (the longest strings, so it sets the length budgets of RF9 for all later languages), **fr** (lowest risk, a large audience, close to es and it, a good control that the pipeline works). Flag work: only `cn` is new. One `feat(i18n)` minor version for the phase (or one per language if reviewers arrive at different times); a language ships when RF11.2/RF13.5 gates pass.

**Phase 2: ko, hi-IN, id-ID, tr, vi.** Rationale: they reuse the machinery and add one script-specific hazard each, better tackled when the base is stable: **ko** (Hangul on the CJK paths already built, honorific register), **hi-IN** (Devanagari shaping, 1.6 line-height, no italics, Linux font availability, terminal rendering of conjuncts), **tr** (dotted/dotless I casing, `lang`-dependent `text-transform`, suffix-heavy strings that make placeholders awkward), **vi** (stacked diacritics, NFC, `đ` folding, tight line-heights), **id-ID** (Latin, no plural, easiest of the five, long affixed words). Flags: kr, in, id, tr, vn are new. Same gates as phase 1.

Within a phase the order is: registry row and flag, core catalog, UI catalog, TUI chrome, evidence, review, release. Order of languages inside a phase follows reviewer availability, not the list.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Mistranslated credential, consent or error text causes a wrong action | Critical-string native review before shipping (RF11.3), typed consent words normalized (RF2.4), "Machine-translated" label until `full` review (RF11.5) |
| No volunteer native reviewer for a language | Language not shipped (RF11.2); non-critical text may ship machine-translated and labelled (section 12, decision 3); recruit through Discussions before starting the draft |
| TUI chrome extraction is larger than expected and delays all TUI languages | It is its own sub-step with a source-scan test (RF5.3); core-text translation does not depend on it, and the TUI chrome for a language can ship one release later, labelled in the status table |
| CJK or Devanagari fonts missing on some Linux systems | Documented package list (RF8, RF14.2); system stacks only; no tofu check on a clean Linux in RF13.4 |
| Ambiguous-width glyphs misalign in CJK terminals | Per-locale width table and an override variable (RF6.3); ASCII fallbacks for pace shapes if tests show breakage |
| German or Russian strings break layouts | Budgets from real catalogs, `en-XA`, layout fixes in the component not the translation (RF9) |
| Every new PRD id (0023 to 0038) now costs fourteen catalog edits | CI rule that en-US changes touch all catalogs, machine-draft convention with `pending_review` (RF11.6) |
| Rust plural rules drift from the WebView's `Intl.PluralRules` | Fixture recorded from `Intl` and cross-checked in tests (RF4.4) |
| Traditional-Chinese users get English | Documented; user pack path (section 12, decision 2) |
| Binary size grows | Budgets measured in phase 0 (NFR); lazy parsing; compression of the installer; revisit by trimming `note.*` verbosity only with owner approval |
| White flags (jp, kr, id) blend into light themes | Review item RF7.5; reopen DS-CMP-SEL-3 for a hairline if needed |
| Unreviewed community PRs degrade a catalog | CODEOWNERS-style rule: PRs touching a catalog require a second native-speaker approval when a reviewer is on record |

## 11. Order of execution and verification

1. **RF1 and RF4:** registry, normalization, plural objects and rules, their tests (core builds with the four current languages).
2. **RF2.2 and RF3.2:** the generalized parity tests, run green against the four current languages first.
3. **RF5 (extraction) and RF6:** TUI locale selection, chrome catalogs for the four current languages, width handling.
4. **RF7.1, RF8, RF9.4, RF10:** flags, typography, pseudo-locale, durations, folding.
5. **Phase 1 languages:** drafts, parity, critical review, RF13 evidence, release; **RF11, RF12, RF14** documentation lands with the first language.
6. **Phase 2 languages** with the same gates.

At each step: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, the desktop workspace tests, a smoke of the changed interface (`CUOTAI_LANG=<code> cargo run -p cuotai-tui -- --once`, the desktop app switched through every shipped language, Settings › General). The requirement is only delivered when: every shipped language passes the parity tests; a Chinese, a Japanese and a Russian system start in their language with the right flag and font; `zh-TW` does not start in Simplified; a ru plural, a ja duration and a de long label render correctly in the desktop and in `--once`; and a native reviewer has signed the critical strings and the screenshots.

## 12. Decisions (owner)

The owner accepted every recommendation on 2026-10-23. The questions that were open are closed as follows.

| # | Question | Decision (2026-10-23) | Effect on the requirements |
|---|---|---|---|
| 1 | Where do TUI chrome strings live? | TUI-owned catalogs (`crates/cuotai-tui/locales/<code>.json`) with a shared core helper (`text::Catalog`). | RF5.2 stands as written; no `tui.*` ids in the core catalogs. |
| 2 | Traditional Chinese (`zh-TW`, `zh-HK`) | Falls back to English; a user pack is the interim path. Never silently Simplified. | RF1.4 and RF12.3 stand; a separate PRD only if demand appears. |
| 3 | Ship machine-translated languages? | Yes. A language ships with a machine draft, a native review of the critical strings (about 490 in total across core and UI) and a visible "machine-translated" label. No language ships without a reviewer of the critical strings. | RF11.2, RF11.3 and RF11.5 stand; the critical set is about 490 strings (roughly 400 core plus 90 UI). |
| 4 | Register and honorifics | The RF11.4 register table is the starting point; the first native reviewer of each language confirms or changes it in the shipping PR. | RF11.4 stands. |
| 5 | Desktop 12/24-hour override | No for now; follow the locale's `Intl` defaults. | RF10.2 stands; revisit only on user reports. |
| 6 | Hairline on white-field flags | Yes, a 1 px inner hairline on jp, kr and id. **This amends DS-CMP-SEL-3**, which says "hairline-free". | RF7.5 becomes a requirement: `flags.js` draws the hairline in `--border` on those three flags (or on every flag, if the review prefers uniformity); `docs/design-system.md` DS-CMP-SEL-3 and its decision log are updated by the design-system owner in the same delivery. |
| 7 | ASCII fallbacks for pace/status shapes in CJK terminals | Yes, automatic: when the terminal renders East Asian Ambiguous glyphs as double width, the TUI replaces `↓ → ↑ ✕ ◆ ▲` with ASCII equivalents. | RF6.3 gains the fallback; detection uses the per-locale width table plus `CUOTAI_AMBIGUOUS_WIDTH`, and a `TestBackend` test covers both modes. |
| 8 | hi-IN and id-ID number grouping | Yes, one screenshot-backed check per language; no custom formatting code. | RF13.3 includes it. |
| 9 | Group the language list | No grouping; fixed order with scrolling. Revisit above twenty languages. | RF3.3 stands. |
| 10 | Reviewer roster | The owner maintains it through GitHub Discussions ("Looking for reviewers" per language); the status table is the public record. | RF11.2 and RF14.1 stand. |

### Implementation decisions (2026-10-25, phase 0 infrastructure lane)

| # | Decision | Reason |
|---|---|---|
| D11 | `set_locale` stores a built-in language by its canonical code and keeps any other valid code as chosen, instead of storing unknown codes as en-US (RF1.7). | `desktop.json` is the source of the UI language (PRD 0020 RF7); a user pack's code (`nl`, `zh-TW`) must survive a restart. Its notices still read English, the core having no catalog for it. |
| D12 | The ten new languages are registry rows with the status `in_progress` until their three catalogs land: they resolve, read the English source, are never detected or selectable, and the language list shows them with a "Translation in progress" caption. A user pack with the same code makes one selectable. | Lets the infrastructure ship before the translations (phase 0) without a half-translated language reaching users. |
| D13 | The core parity reference is the first translated core catalog (pt-BR), not en-US. | After PRD 0034 the English core and TUI texts live in the code (no en-US file); the descriptor and source-id tests already prove every English id is in every catalog. |
| D14 | The NFC rule of RF2.2 is checked by a proxy: no combining diacritic (U+0300..U+036F) in a Latin or Cyrillic text; the Rust `text::fold` maps precomposed letters by table. | The tree has no Unicode normalization crate and RF6.1 forbids adding crates for this. |
| D15 | `edit_error.index_out_of_range` stays a plain text. | Its `{count}` is a length printed in parentheses, not a counted noun. |
| D16 | Italian `quiet.digest.more` reads "e un altro" for one; the plural parity compares the union of placeholders across forms. | A singular form may name the number in words. |
