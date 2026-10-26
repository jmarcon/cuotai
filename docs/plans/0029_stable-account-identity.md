# PRD 0029 — Stable account identity

| Field | Value |
|---|---|
| ID | 0029 |
| Status | Implemented |
| Owner | `cuotai-core` (`Account::id`, `ConfigFile`, `account_edit`, state migrations, vault index), desktop and TUI |
| Consumers | `apps/desktop` (account form, bells, mutes, tray selection), `crates/cuotai-tui` (wizard, `--notify`), `export::v1`/`--serve`/`--watch` consumers, hooks, PRDs 0024, 0025, 0026 |
| Depends on | PRD 0001 (`ConfigFile`, `account_edit`), PRD 0016 (RF1, RF4: `[accounts.session]`, `stage_forget`) |
| Blocks | PRD 0025 RF2 (`tray.accounts` keyed by account), PRD 0026 RF3 (journal keyed by account digest), PRD 0024 RF7 (`[[groups]]` and suggestions name accounts), PRD 0030 RF1 (per-account plan prices), PRD 0032 RF1.3 (bells and mutes in the shared preferences), PRD 0036 RF5 (sub-account dedup and preferences); all of them persist or match account-keyed state and must key it by `id` |
| Extends | PRD 0003 RF2 (ledger keys), PRD 0006 RF1 (series identity), PRD 0008 RF2 (alert marks), PRD 0012 RF1/RF2 (`id` field, `/v1/accounts/{id}`), PRD 0013 RF1/RF3 (event `account_id`, hook filter) |
| Date | 2026-10-23 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. The **legacy key** is today's `Account::key` (`<provider slug>:<name>`, or `extension:<id>:<name>`). The **display name** is the configured name or, when there is none, the provider's name. Code references are to `origin/main` at `0ba9c9b` (0.75+); the 2026-10-21 review's line numbers moved.

## Implementation status (2026-10-23)

| RF | Version | Delivery |
|---|---|---|
| RF0 | 0.80.0 | Implemented: account TOML accepts the future `id` key and a missing `name` while rejecting other unknown keys; validation permits an unnamed account only when it has no provider peer; `ConfigFile` preserves existing ids without generating them. |
| RF1 | vNEXT | Implemented: `AccountId` (`acc_` + 12 Crockford base32, or hand-written `[a-z0-9_-]{4,40}`); effective ids derived from the legacy key and `ConfigIdentity` on load, written as each table's first key on the next save; random ids for new accounts; `ConfigFile` matches tables by id; `ConfigError::AccountIds`; reports carry `account_id`. |
| RF2 | vNEXT | Implemented: `Account::state_key`/`Report::state_key` key the reset ledger, alert marks and prefs, desktop bells and mutes, widget, watch/event tracks and history series (a digest of the id). Legacy state migrates once: `desktop.json` with the configuration, `notified.json` with the readings in hand (`Ledger::adopt_ids`, at load from the restored readings and before judging each new one), history rows on first access into an `aliases` table created without a `user_version` step so older releases still open the file. `RefreshCycle::apply_edit` was already positional, so a rename stays an edit. |
| RF3 | vNEXT | Implemented: `validate_name(name, provider, config, editing)` requires a name only beside another account of the provider (`edit_error.name_required_same_provider`); `name` is omitted from the TOML when empty; `Account::display_name`/`Report::display_name` fall back to the provider (an extension account to its manifest name, then its extension id); `account_edit::unnamed_peer` + `Config::rename_account` + `refresh::Edit::Rename` let the TUI wizard and the desktop dialog name the existing unnamed account in the same save; the export `name` carries the display name. Extension accounts get the exact per-extension rule from `Config::validate` only (the form does not know the extension id). |
| RF4 | vNEXT | `id` and `named` on `AccountExport` and `AccountLimit`, `id` on the widget snapshot, `account_id` on events and `--json` reports, `/v1/accounts/{id_or_key}`; hooks receive `CUOTAI_ACCOUNT_ID` and `hooks.toml` rules accept an `account_id` filter (validated as an `AccountId`); docs and schema updated. |
| RF5 | 0.77.1, vNEXT | RF5.1 and the session half of RF5.2 delivered as PRD 0016 FIX-06 (`d265960`): removing an account or switching its provider deletes its vault web session, restored when the save fails. vNEXT: `account_edit::switched_oauth`/`switched_oauth_notice`/`purge_switched_oauth` purge the old OAuth token after a successful save of a provider switch (shared-key guard applies) and the TUI summary says so before saving (the desktop dialog does not allow a provider switch while editing); `RemovedAccount::sessions_removed` reports the session apart from the OAuth credential in the TUI result and the desktop notice (RF5.3). Decision kept: "keep credentials" still keeps only the OAuth token; the session goes with the account, as FIX-06 decided, since no other account can read it. |
| RF6 | vNEXT | Implemented: `oauth::vault_index::IndexedStore` wraps the OS vault (`OAuthSession::system`) and the development SQLite store (its own `<database>.vault-index.json`), recording each saved key and its creation time in `vault-index.json` (`0600`, never a value; missing or corrupt reads as empty, write failures ignored); `vault_index::audit`/`prune` resolve kind, provider, account id and display name from the configuration at audit time (the store only sees opaque keys) and mark orphans; `cuotai --vault-audit [--json]` (exit 2 with orphans) and `--vault-prune` (terminal confirmation); desktop Settings › Advanced › Files › Stored credentials with Remove unused. Not indexed: `serve_token` (kept in a file, not the vault) and entries written before this release. |
| RF7 | vNEXT | `cuotai.example.toml` (optional `name`, the `id` line, copying blocks, the 0.80.0 downgrade floor), `README.md` (adding accounts), `docs/architecture.md` ("Account identity"), `docs/export-v1.md`/schema, `docs/events.md`, `docs/tui.md`, `docs/oauth.md` and `docs/widgets.md`; the TOML compatibility note is in the RF7 commit's `Changelog:` trailer. |

## 1. Context

Every piece of per-account state Cuotai keeps is keyed by the account **name**, directly or through a digest of it:

- `Account::key` is `provider:name` (`crates/cuotai-core/src/model.rs:293-305`); its doc calls it "stable", but a rename changes it.
- Reset notices and usage alerts in `notified.json` key their marks by `key_digest(account_key, label)` (`resets.rs:94-100`, `alerts.rs:241-250`, `alerts.rs:531-533`); `AlertState::retain_accounts` prunes by key (`alerts.rs:447`).
- The desktop's bells and per-metric mutes are `Account::key` sets in `desktop.json` (`apps/desktop/src-tauri/src/settings.rs:146-156`, `:259`).
- The history series identity (`models::account_key`, `models.rs:322-337`) is a digest of provider, **name, `credential_env`, `oauth` and every non-display param**, used as `account_digest` (`history.rs:93-113`, `:480`, `:618`). Renaming an account, or changing any parameter, silently starts a new series.
- `export::v1` exposes `key` (`export/v1.rs:67-69`), `/v1/accounts/{key}` looks it up (`serve.rs:654-667`), events carry `account_key` and `account_name` (`events.rs:190-199`), hooks filter by `account` name (`hooks.rs:87-108`), the widget snapshot keys accounts by `key` (`widget_snapshot.rs:83`).
- `ConfigFile` matches `[[accounts]]` tables **by name** when accounts move and positionally on renames (`config_file.rs:543-612`).

Account names are also **mandatory and globally unique**: `Config::validate` rejects an empty or repeated name (`lib.rs:117-123`, case-sensitive) and `account_edit::validate_name` rejects an empty name or one used by any other account of any provider (`account_edit.rs:1610-1634`, case-insensitive). Most people have one account per provider and type "Claude" into a field that adds nothing.

Two removal defects found in the review belong here, since they are about the lifecycle of an account's identity and credentials:

- **FIX-06.** `remove_account_save_only` (`account_edit.rs:2421-2515`) computes and purges only the OAuth storage key (`:2443-2455`); the vault entries of `[accounts.session]` (PRD 0016) are left behind. A provider switch drops the session reference without deleting its entries (`account_edit.rs:2123-2127`: "a provider switch starts without one"). `user_session::stage_forget` (`user_session.rs:1248-1300`) already deletes entries with a rollback, but only on explicit "forget".
- **FIX-07.** `TokenStore` has `load`, `save` and `delete` only (`oauth/store.rs:53-61`); Cuotai cannot list what it stored, so orphaned entries are invisible and cannot be cleaned.

## 2. Problem

- Renaming an account loses its bell, mutes, alert marks, reset ledger and history; editing its credential variable or a parameter loses its history too.
- Front ends and integrations have no identifier that survives a rename; `key` changes with the name.
- The name is required even when it is redundant, and the uniqueness rule spans providers, so "Work" cannot exist for both Claude and ChatGPT unless the person invents a second label.
- Removing an account, or switching its provider, can leave web-session secrets in the OS vault with nothing pointing at them.

## 3. Goals

- Give every `[[accounts]]` table an opaque, stable `id` that survives renames, reordering and parameter edits.
- Key all account-keyed state by `id`, migrating today's state forward once without loss.
- Make the account name optional: required only when two or more accounts share a provider; the provider name is shown otherwise.
- Expose `id` additively in every contract while keeping `key` (INT-05 policy, PRD 0031 RF5).
- Leave no Cuotai-created secret behind when an account is removed or changes provider, and make what Cuotai stored auditable.

## 4. Non-goals

- Account groups (PRD 0024 decision C); this PRD only gives groups a stable member identity.
- Syncing identities across machines or profiles; the `id` lives in the TOML and travels with it.
- Renaming or removing accounts from the CLI (the wizard and the desktop form remain the editors).
- Deleting vault entries Cuotai did not create (other apps' Keychain items, environment variables).
- Changing what the history stores (PRD 0006 still applies); only its identity column changes.

## 5. Functional requirements

### RF0 — Tolerant TOML reader, shipped one release earlier (high priority; precursor, owner decision 2026-10-23)

- **RF0.1** A release **before** RF1–RF3 stops rejecting what the identity change writes: `Account` drops `deny_unknown_fields` (`model.rs:252-253`) for the `id` key (unknown account keys other than `id` keep being rejected, so typos are still caught), and a missing `name` reads as empty.
- **RF0.2** In that release `Config::validate` already applies the RF3.1 rule (an empty name is valid when no other account shares the provider); the TUI and the desktop forms do not change yet.
- **RF0.3** `ConfigFile` keeps an `id` line it does not understand when it edits a table (the `toml_edit` path already preserves unknown content); when a table is rewritten from scratch, the id is lost, which is harmless because RF1.3 derives the same id again from the legacy key.
- **RF0.4** The changelog of that release (its `Changelog:` trailer, PRD 0039) says it is the oldest version that can open a configuration written after PRD 0029; downgrading to it after RF1 keeps working.
- **Acceptance criterion:** a fixture TOML with `id` lines and one unnamed account loads in the RF0 release; a typo key is still rejected; an edit in that release keeps the `id` lines.

### RF1 — `id` per account (high priority; prerequisite)

- **RF1.1** `Account::id: Option<AccountId>` in the TOML as `id = "acc_xxxxxxxxxxxx"`: the prefix `acc_` and 12 characters of lowercase Crockford base32 (60 bits), valid for URL paths, file names and digests. `AccountId::parse` accepts `[a-z0-9_-]{4,40}` so a hand-written id ("work-claude") is also valid.
- **RF1.2** `Config::validate` rejects two accounts with the same `id` with a new `ConfigError::AccountIds` that names the field (a copied `[[accounts]]` block is the usual cause) and suggests deleting the duplicated `id` line.
- **RF1.3** New accounts created through `account_edit` (`build_account`, `upsert_account`) get a random id. Existing accounts without `id` get an **effective id** computed in memory from the legacy key and the config identity (`ConfigIdentity`), so every process that reads the same file without writing it (`--json`, `--watch`, `--serve`, the widget) agrees on it. The first `ConfigFile::save` of an interactive front end writes those effective ids into the file (ID-01: "an old config gains ids on save without any other change").
- **RF1.4** `ConfigFile` inserts the `id` line with `toml_edit` as the first key of each `[[accounts]]` table, preserving comments, order and formatting, and matches tables **by `id`** (then by name, then by position, as today) for moves and renames (`config_file.rs:543-612`).
- **RF1.5** The id is never derived from a secret; it carries no information an export does not already carry.
- **Acceptance criterion:** an old fixture config gains exactly one `id` line per account on its next save and nothing else changes (byte comparison outside those lines); two read-only processes compute the same effective ids; a duplicated id is rejected with `ConfigError::AccountIds`; moving and renaming accounts in one save keeps comments attached to the right table.

### RF2 — Account-keyed state migrates to `id` (high priority)

- **RF2.1** `Account::state_key() -> String` returns the id; every consumer listed in §1 switches to it: `resets::Ledger` and `AlertState` digests (`key_digest(id, label)`), `AlertState::retain_accounts`, the desktop `off_accounts` and `muted` maps, `widget_snapshot` accounts, the `EventState` per-account tracks, and PRD 0024/0025/0026 state when they land.
- **RF2.2** The history identity becomes the `id` alone: a forward migration of PRD 0006's versioned schema adds an `aliases (config_key, legacy_digest, account_id)` table; on open, each configured account maps its current `models::account_key` digest to its id, and reads follow the alias, so old samples join the new series. Parameter edits no longer split a series.
- **RF2.3** `notified.json` and `desktop.json` migrate once on load with the configuration in hand: a mark or entry keyed by a legacy key (or its digest) of a configured account is rewritten with the id; an entry matching no account is kept until the existing pruning removes it. The files stay readable by the current release (additive keys).
- **RF2.4** `RefreshCycle::apply_edit` and the desktop/TUI prune paths compare by id, so a rename is an edit of the same account, not a removal plus an addition.
- **Acceptance criterion:** rename an account with a bell off, a muted metric, a notified 90 % mark, a pending reset notice and 30 days of history: after the save all five survive (desktop, TUI and core tests); an edited `credential_env` keeps the history series; a removed account's state is pruned as today.

### RF3 — Optional account name (high priority)

- **RF3.1** `name` becomes optional in the TOML (`#[serde(default)]`, omitted when empty). It is **required only when two or more accounts share a provider** (for extension accounts, the same `extension_id`); every account in such a set needs a non-empty name. Names that are present stay unique case-insensitively across the configuration (the rule of `validate_name`, now also enforced by `Config::validate`, which today compares case-sensitively).
- **RF3.2** `account_edit::validate_name(name, provider, config, editing)` (signature gains `provider`) returns `edit_error.name_required_same_provider` ("Name this account: you have another {provider} account") when the rule applies, keeps `edit_error.duplicate_name`, and accepts an empty name otherwise. `Config::validate` returns `ConfigError::AccountNames` with the provider for hand-edited TOML.
- **RF3.3** Adding a second account of a provider whose first account has no name: the form asks for a name for the new account **and** shows an inline field to name the existing one (prefilled with the provider name, editable); both are saved in one `ConfigFile::save`. Removing accounts down to one never clears a name.
- **RF3.4** `Account::display_name() -> Cow<str>`: the name, else the provider's localized name (`provider_l10n`), else for an extension account the manifest's display name, else its id. Every surface (cards, tray, notifications, `--once`, TUI, events `account_name`, export `name`) uses it.
- **RF3.5** The TUI wizard's `Step::Name` (`crates/cuotai-tui/src/wizard.rs:1501`) and the desktop account dialog (`apps/desktop/ui/js/account-dialog.js:102`) show the field as optional with the provider name as placeholder and the hint "Optional; needed when you add another {provider} account"; the desktop demo mock (`apps/desktop/ui/js/lib.js:1202`) follows the core rule.
- **RF3.6** Existing configurations need no migration: every current name stays as it is, nothing is cleared automatically (a name equal to the provider name displays exactly like an unnamed account).
- **Acceptance criterion:** table tests of the rule (one unnamed account per provider; two of one provider with one unnamed rejected in the form and in TOML; same name in two providers rejected case-insensitively; extension accounts per extension id); a form test adding a second Claude account names both; snapshot tests of display names in the four languages.

### RF4 — Contracts carry `id` (medium priority)

- **RF4.1** `AccountExport` and `AccountLimit` gain `id` (additive, `export::v1` schema 1); `key` is kept unchanged in value and format and documented as "changes on rename; prefer `id`" (INT-05: kept at least 90 days after a v2). `name` stays a non-empty string carrying `display_name()`, and a new `named: bool` tells whether it was configured.
- **RF4.2** `/v1/accounts/{id_or_key}`: an id match is tried first, then the legacy key.
- **RF4.3** `Report` in `--json` gains `account_id` (additive, skipped when absent for byte compatibility of old fixtures); events schema 1 gains `account_id`; hooks receive `CUOTAI_ACCOUNT_ID` and `hooks.toml` rules accept `account_id = "…"` beside the existing `account` (name) filter.
- **RF4.4** `docs/export-v1.md`, `export-v1.schema.json`, `docs/events.md` and `docs/local-server.md` updated.
- **Acceptance criterion:** schema validation of the example export with `id`; a server test answering by id and by key; a `--watch --jsonl` test with `account_id`; a hook rule filtering by id.

### RF5 — Credentials removed with the account or its provider (high priority; FIX-06)

- **RF5.1** `remove_account_save_only` also stages the account's `[accounts.session]` entries: the core deletes them through the `user_session` path, saves the TOML, and on a failed save restores them (the `stage_forget`/`rollback` pattern and the PRD 0016 criterion, `apps/desktop/src-tauri/src/backend.rs:5157`). The shared-reference guard applies to sessions as to OAuth (a copied `credential_ref` keeps the entry).
- **RF5.2** A provider switch in `build_account` (`account_edit.rs:2123-2127`) stages the old provider's session entries and, when the OAuth storage key changes, the old OAuth token for purge after the save, with the same guard and rollback; the form says what will be deleted before saving.
- **RF5.3** The removal result (`StagedRemoval`) reports sessions and OAuth separately so the desktop and the TUI (`crates/cuotai-tui/src/ui.rs:635`) show "Removed the account and its stored session"; "keep credentials" keeps both.
- **Acceptance criterion:** core tests with a fake `TokenStore`: removal deletes session entries; a failing save restores them; a shared reference keeps them; a provider switch purges the old session and token only after a successful save; desktop and TUI tests of the messages.

### RF6 — Index of vault entries Cuotai created (low priority; FIX-07)

- **RF6.1** An `IndexedStore` wrapper over every `TokenStore` keeps `<OS local data dir>/cuotai/vault-index.json` (`0600`): per entry the storage key, kind (`oauth`, `session`, `serve_token`, later `extension_secret` from PRD 0028), provider slug, account id and creation time. Never a value. Written on `save`, removed on `delete`; a missing or corrupt index is rebuilt empty and never blocks a credential operation.
- **RF6.2** `cuotai --vault-audit [--json]` lists indexed entries and marks orphans (no configured account references them, checked with `load` without printing anything); `--vault-prune` deletes orphans after a terminal confirmation (no terminal: error). Desktop: Settings → Advanced → "Stored credentials" shows the same list with a "Remove unused" action.
- **Acceptance criterion:** tests with the debug SQLite store: index follows save/delete, an orphan after a manual TOML edit is listed and pruned, the index never contains values, a corrupt index does not break login.

### RF7 — Documentation (low priority)

- **RF7.1** `cuotai.example.toml` (optional `name`, `id`), `README.md` (accounts section), `docs/architecture.md` (identity and migrations), the docs of RF4.4, and the TOML compatibility note of §7 in the delivering commit's `Changelog:` trailer (PRD 0039).
- **Acceptance criterion:** documentation reviewed with the delivery; the changelog check passes.

## 6. Non-functional requirements

- **Compatibility:** a new release reads every old TOML; state migrations are forward-only and idempotent (running twice changes nothing).
- **Determinism:** effective ids depend only on the legacy key and the config identity; migrations are pure functions of the files and the configuration.
- **Privacy and security:** ids carry no secret and no e-mail; the vault index never holds values; removal never deletes an entry another account references; nothing is printed from a vault entry.
- **Atomicity:** every TOML write goes through `ConfigFile` (conflict detection, atomic rename); credential deletion is rolled back when the save fails.
- **i18n and accessibility:** new texts in pt-BR, en-US, es and it (plus the languages of PRD 0035 when present); the optional field is labelled "optional" in text, not only by placeholder.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| TOML `[[accounts]]` | RF0: `id` tolerated, `name` may be missing. RF1–RF3: `id` added on first save; `name` optional | Old files read unchanged. Because RF0 ships first, the identity release is **additive, not breaking** (minor bump for the `feat`, no `BREAKING CHANGE:` footer): downgrading to the RF0 release or later keeps working. Releases before RF0 still reject the file (`deny_unknown_fields`, `model.rs:252-253`); the changelog names the RF0 version as the oldest supported downgrade target and says "to go further back, delete the `id` lines and name every account" |
| `export::v1` | `id`, `named` on accounts and limits; `/v1/accounts/{id}` | Additive (schema stays 1) |
| `--json` `Report` | `account_id` | Additive |
| Events schema 1, hooks | `account_id`, `CUOTAI_ACCOUNT_ID`, rule `account_id` | Additive |
| `notified.json`, `desktop.json` | keys rewritten to ids | Forward migration, lenient read |
| `history.sqlite3` | `aliases` table | Forward schema migration |
| `vault-index.json` | new file | New |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | `AccountId`, effective ids, validation, `display_name`, `ConfigFile` matching, all migrations, removal staging, vault index, contract fields |
| Desktop | optional name field, inline "name the existing account", removal messages, Stored credentials page |
| TUI/CLI | wizard name step, removal messages, `--vault-audit`, `--vault-prune` |
| Widgets | read `id` from the widget snapshot (no UI change) |

## 9. Dependencies and sequencing

RF0 ships in an earlier release than RF1–RF4. RF1 next; RF2 needs RF1. RF3 needs RF1 (the id, not the name, identifies the account). RF4 needs RF1. RF5 and RF6 are independent of RF1–RF4 and can ship first as fixes. **PRD 0025 RF2 and PRD 0026 RF3 must not ship before RF2**, or they persist name-keyed state that would need its own migration; PRD 0024 RF7 and decision C groups should reference ids. Wave 5b of `0000_control.md`.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Downgrade after ids are written fails to read the TOML | RF0 precursor release reads it; the changelog names the oldest supported downgrade target |
| Copy-pasted `[[accounts]]` blocks duplicate ids | `ConfigError::AccountIds` names the field and the fix |
| Migration maps state to the wrong account | maps only exact legacy keys of currently configured accounts; table tests for renames, reorders and removals |
| Unnamed accounts confuse hook rules by name | `account_id` filter; display name documented as not unique across providers |
| Vault deletion during a failing save loses a session | stage, save, roll back (PRD 0016 pattern) |

## 11. Order of execution and verification

0. **RF0:** tolerant reader, in its own earlier release.
1. **RF5, RF6:** removal and audit fixes.
2. **RF1:** ids and `ConfigFile` matching.
3. **RF2:** state migrations.
4. **RF3:** optional names in core, TUI and desktop.
5. **RF4:** contracts; **RF7** with each step.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface. The requirement is only delivered when renaming an account in the desktop keeps its bell, mutes, alert marks and history, and removing an account with a pasted session leaves no vault entry.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Write ids at startup of the desktop/TUI (one save) or only on the next edit | On the next save only; the effective id already makes state stable. |

## 13. Decisions (owner, 2026-10-23)

| Topic | Decision |
|---|---|
| Ids for existing accounts | **Derived deterministically** from the legacy key and the config identity (RF1.3); random ids only for new accounts. Supersedes the review's "random slug" for legacy accounts. |
| Downgrade safety | Ship **RF0** (tolerant reader) in an earlier release, then the identity change; the identity release is additive, without a `BREAKING CHANGE:` footer (§7). |
| Name uniqueness | Keep **global, case-insensitive** uniqueness of non-empty names (RF3.1). |
