# Local state: files, versions and downgrades

Cuotai keeps a few files of its own next to your accounts file. This page lists each one, how it is versioned, what happens when it is damaged, and what an older Cuotai does with a file a newer one wrote (PRD 0051). No file here holds a credential: secrets stay in the system vault (see [`oauth.md`](oauth.md)).

`<data>` is `CUOTAI_DATA_DIR` when it is set, otherwise `<OS local data dir>/cuotai` (`~/Library/Application Support/cuotai` on macOS, `~/.local/share/cuotai` on Linux, `%LOCALAPPDATA%\cuotai` on Windows). `<config>` is `<OS config dir>/cuotai`. See [`advanced.md`](advanced.md#environment-variables) for `CUOTAI_DATA_DIR`.

## The state contracts

| File | Location | Owner | Version | Holds | Permissions | Backup | Damaged file | Written by a newer Cuotai | Erase |
|---|---|---|---|---|---|---|---|---|---|
| `cuotai.toml` | `--config`, `CUOTAI_CONFIG`, `./cuotai.toml` or `<config>/config.toml` | core `config_file` | none (tolerant reader) | accounts, variable **names**, vault references | `0600` when Cuotai creates it | edit history is yours; `cuotai --backup` | the file is rejected with its line, column and key, never ignored | an unknown provider or account key sets that account aside; unknown top-level keys are ignored and kept; a save keeps both byte for byte | delete the file |
| `desktop.json` | `<config>/desktop.json` | desktop, through core `private_fs::update_locked` | `schema` (absent = 1) | preferences and window state | `0600` in a `0700` folder | `desktop.json.bak[.N]` before a damaged file is replaced | kept as `.bak`, the app starts with the defaults and says so | unknown members are kept; a later `schema` major is read but never rewritten | delete the file |
| `history.sqlite3` | `<data>/history/` | core `history` | `user_version` (migrations) and `state_meta` major 1 | readings: numbers, metric labels, digests | `0600` in `0700` | `history.sqlite3.bak` (consistent copy) before each migration | the history is reported unavailable, nothing is deleted | same major: readable and writable; later major or no `state_meta`: refused untouched | Settings › Data & privacy, or delete the folder |
| `cost.sqlite3` | `<data>/cost/` | core `cost` | as the history | per-request token counts, model names, digests | `0600` in `0700` | `cost.sqlite3.bak` before each migration | the index is reported unavailable | as the history | `cuotai --cost-erase` |
| `models.sqlite3` | `<data>/models/` | core `models` | `user_version` 1 and `state_meta` major 1 | model lists per account digest | `0600` in `0700` | none (refetched) | reported as a storage error | same major: usable; otherwise refused | delete the folder |
| `notified.json` | `<data>/notified.json` | core `resets` | none yet | digests and dates of sent notices | `0600` | none | set aside as `notified.json.corrupt` and read as empty | not versioned yet | delete the file |
| `hooks.toml` | beside `cuotai.toml` | core `hooks` | none yet | your hook rules (commands) | as you created it | `cuotai --backup` | the hooks are refused with the line | not versioned yet | delete the file |
| `widget.json` | the widget folder or `<data>` | core `widget_snapshot` | `schema = 1` | the published readings | `0600` | none (regenerated) | rewritten on the next publish | major 1 only | turn widgets off and delete it |

Other files under `<data>`: `serve/token` (the local server token, `0600`; `cuotai --doctor` warns when others can read it), `hooks/` (consent, run log, work folder), `extensions/`, `metered/`, `canary/`, `last-config-*.json`, `vault-index.json` and `update-check.json`. None holds a secret value except the server token, which never leaves this computer (it is not part of a backup).

## Downgrading

- Install the older version **before** opening the files with it. From this release on, the SQLite stores record their schema major, so a later release that only **adds** tables or columns stays readable and writable by this one. A release that changes the meaning of a file raises the major, and an older Cuotai then refuses that file without touching it (`cuotai --doctor` names it).
- Releases before this one (0.82.3 and older) refuse any history written by a newer release, and 0.80.0 also refuses the TOML once it uses `[[groups]]` or an account `group`.
- A `.bak` file is the state **before** the last migration. It is recovery input, never restored automatically: quit Cuotai, move the current file aside, rename the `.bak` copy (for example `history.sqlite3.bak` to `history.sqlite3`) and start the older version.
- `cuotai --check-config` names any account or key the running version does not know; `cuotai --doctor` reports every file above with its state.
