# Cuotai languages

Each built-in language has a core catalog and a desktop UI catalog. The core
catalogs in `crates/cuotai-core/locales/{en-US,es,it}.json` contain the texts
emitted by the backend. The desktop UI catalogs in
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

1. Add `crates/cuotai-core/locales/<code>.json` with every core message ID:
   `field.*`, `native.*`, `login.*`, `credential_help.*`, `oauth.*`,
   `access.*`, `edit_error.*`, `query.*`, `note.*`, `remove_error.*`,
   `remove.kept.*`, `status.*`, `provider.*`, `binding.*`,
   `metric_window.*`, and `pace.*`. Portuguese is the embedded fallback for
   `pt-*`; other languages use their catalog and fall back to `en-US`.
2. Add `apps/desktop/ui/locales/<code>.json` with the UI's `common.*`,
   `dialog.*`, `tray.*`, `settings.*`, `theme.*`, `account.*`, `card.*`,
   `delete.*`, `filter.*`, `lang.*`, `metric.*`, `quota.*`, `sort.*`,
   `time.*`, and `userContent.*` strings.
3. Add `{ code, name }` to `LOCALES` in `js/i18n.js` and add the language to
   the locale coverage tests.

The `en-US` fallback catalogs must contain every core and UI key.

## User-provided languages

Place a single `<config directory>/cuotai/locales/<code>.json` file and restart
the app. It can combine UI and core keys and overrides any built-in catalog
with the same code. The directory is `~/Library/Application Support/cuotai`
on macOS, `%APPDATA%\cuotai` on Windows and `~/.config/cuotai` on Linux, or
the value of `CUOTAI_USER_DIR`. The folder button in the top bar copies its
path. Invalid files are ignored and a warning explains why. A partial example
is available at `apps/desktop/examples/user/locales/fr.json`.
