# Cuotai languages

Each built-in language has a core catalog and a desktop UI catalog. The core
catalogs in `crates/cuotai-core/locales/{pt-BR,es,it}.json` contain the texts
emitted by the backend; English is the text written in the core's code (the
`fallback`), so en-US has no core catalog. The desktop UI catalogs in
`apps/desktop/ui/locales/<code>.json` contain shell labels, dialogs, tray
messages, settings and themes.

```json
{
  "_meta": { "name": "Italiano" },
  "common.add": "Aggiungi account",
  "quota.count": { "one": "{count} quota", "other": "{count} quote" },
  "metric.balance": "Saldo: {amount}"
}
```

- `_meta.name` is required and is shown in the language selector.
- Values are plain text. `{placeholder}` names are substituted and must not
  be changed.
- Plural objects use the language's `Intl.PluralRules` categories and select
  a form with `{count}`.
- Missing keys fall back to `en-US`, then to the key itself.
- Text is escaped before it is rendered; do not add HTML.
- Frontends resolve stable core message IDs (`{ id, params, fallback }`) rather
  than matching translated text. `js/i18n.js` resolves IDs against the merged
  UI and core catalogs, with user language packs taking precedence.

## Adding a built-in language

Every language is one row of the core registry, `cuotai_core::text::LOCALES`
(`crates/cuotai-core/src/text/registry.rs`, PRD 0035 RF1): code, endonym,
flag, script, language subtags and aliases, plural rule and status. The core,
the TUI and the desktop all read it; no test loop, prefix chain or
hard-coded list needs editing. The fourteen rows are pt-BR, en-US, es and it
(shipped) and zh-CN, fr, de, ja, ru, ko, hi-IN, id-ID, tr and vi, registered
with the status `in_progress`: they resolve (a Japanese system is recognized
as Japanese) but read the English source, and the language list shows them
as "Translation in progress", not selectable.

To ship a registered language:

1. Add `crates/cuotai-core/locales/<code>.json` with every core id (the key
   set of `pt-BR.json`), and its line in the `embedded_catalogs!` table of
   `crates/cuotai-core/src/text.rs`.
2. Add `crates/cuotai-tui/locales/<code>.json` with every TUI id, and its
   line in the `tui_catalogs!` table of `crates/cuotai-tui/src/l10n.rs`.
3. Add `apps/desktop/ui/locales/<code>.json` with every id of `en-US.json`
   (the UI source).
4. Set the row's `status` to `shipped`, then regenerate the desktop's copies
   of the registry (the static copy in `js/locale-registry.js` and the
   pre-paint lists of the three HTML pages):
   `CUOTAI_REGENERATE_LOCALES=1 cargo test -p cuotai-core locale_registry_copies`.

`_meta.name` of each catalog is the registry endonym. Values are strings or,
for an id that embeds `{count}`, a plural object with exactly the categories
of the language's rule (`other` for ja, zh-CN, ko, id-ID and vi; `one` and
`other` for en-US, de, es, it, tr, pt-BR, fr and hi-IN; `one`, `few`, `many`
and `other` for ru). The parity tests (`core_catalogs_are_in_parity`,
`desktop_ui_catalogs_are_in_parity` and the TUI's
`the_registry_matches_the_tui_catalogs`) fail on a missing or extra id, a
renamed placeholder, markup, a wrong plural shape, an empty value, a control
character, a decomposed (non-NFC) Latin or Cyrillic text, a `_meta.name`
that is not the endonym, and a file the registry does not list (or the
reverse). A row whose script is not Latin or Cyrillic also needs its
`--font-script` stack in `css/themes.css`, and every row needs drawn flag
artwork in `js/flags.js`; tests check both. A right-to-left row fails a test
until a PRD covers mirroring.

## User-provided languages

Place a single `<config directory>/cuotai/locales/<code>.json` file and restart
the app. It can combine UI and core keys and overrides any built-in catalog
with the same code. The directory is `~/Library/Application Support/cuotai`
on macOS, `%APPDATA%\cuotai` on Windows and `~/.config/cuotai` on Linux, or
the value of `CUOTAI_USER_DIR`. The folder button in the top bar copies its
path. Invalid files are ignored and a warning explains why. A partial example
is available at `apps/desktop/examples/user/locales/fr.json`.
