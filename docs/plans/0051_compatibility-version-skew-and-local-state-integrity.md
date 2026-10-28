# PRD 0051 — Compatibility, version skew and local-state integrity

| Field | Value |
|---|---|
| ID | 0051 |
| Status | Proposed (groundwork only: RF1.5, RF1.6 and COMP-02 partial) |
| Owner | `cuotai-core` (`config_file`, `private_fs`, state codecs, `ConfigIdentity`, data-directory resolution), desktop and TUI/CLI |
| Consumers | `apps/desktop`, `crates/cuotai-tui`, package-managed and auto-updated Cuotai installations, widgets and hook authors |
| Depends on | PRD 0001 RF4 (`ConfigFile`, typed edit errors), PRD 0003 RF2 (`notified.json`), PRD 0006 RF1 (versioned history), PRD 0013 RF1 (`hooks.toml`), PRD 0014 RF1 (`widget.json`), PRD 0015 RF2 (`cost.sqlite3`); PRD 0029 RF1 for RF5.4 |
| Benefits from | PRD 0034 (English source fallback), PRD 0039 (release traceability) |
| Extends | PRD 0001 RF4, PRD 0003 RF2, PRD 0006 RF1, PRD 0014 RF1, PRD 0015 RF2, PRD 0018 RF2/RF3, PRD 0024 RF7, PRD 0029 RF0/RF2, PRD 0032 RF1/RF4, PRD 0036 RF2 |
| Date | 2026-10-27 |

> **Note on terminology:** “RF” (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **tolerant reader** preserves and isolates syntax it does not understand; a **strict reader** rejects it as a likely typo. A **schema major** changes old-reader meaning; a **schema revision** is additive. **N** is a writer release and **N-1** is its immediately preceding compatibility-precursor release, not a numeric schema value. Code references are to `origin/main` at `7a1d93c3`.

## Implementation status (2026-10-28)

Written on 2026-10-27 as PRD 0041 by the 2026-10-25 PRD audit against `origin/main` (0.80.0) and renumbered 0051 when it moved onto `integration/next`, whose 0040–0049 are other plans. This table was re-evaluated on 2026-10-28 against `integration/next` at `18009a04`; the §1 code references still point at `origin/main` at `7a1d93c3`, and the current location is named here where it moved.

| RF | Version | Delivery |
|---|---|---|
| RF1 | — | Proposed. RF1.5 partial: `ConfigFile::render` edits the previous `DocumentMut` and keeps comments and known keys, but still falls back to fresh serialization. RF1.6 partial: `47ac1bd1` accepts and preserves exactly `budgets`, `floors` and `breakdown` on accounts (plus `id`, `group`/`[[groups]]`, `[updates]` and the 9router keys as known keys); any other unknown account key, unknown top-level key or unknown provider slug still rejects the whole file, and `--check-config` does not exist. |
| RF2 | — | Proposed. `ConfigError::Parse` still has no line, column or path; `ConfigFile::open`/`reload` still map to `edit_error.config_toml_invalid`. |
| RF3 | — | Proposed. No `StateVersion`/`state_meta`; SQLite stores use `user_version` only and a newer one is a generic storage error. **Raised priority:** `eae69857` moved `history.sqlite3` to schema version 7 with no `.bak` and no N-1 policy, so a 0.80.0 reader now rejects the file after an upgrade (COMP-03, COMP-04). |
| RF4 | — | Proposed. `desktop.json` is still read/modify/written in `apps/desktop/src-tauri/src/settings.rs` with a fixed temporary name, no lock, fsync, mode or symlink check; corrupt content becomes defaults. |
| RF5 | — | Proposed. Reset marks are keyed by account key and label only; no `CUOTAI_DATA_DIR` (only `CUOTAI_USER_DIR`); no relocation or adoption. |
| RF6 | — | Proposed. No `compat/` fixtures, N-1 readers or `xtask compat-check`. |
| RF7 | — | Proposed. No `docs/local-state.md` or configuration tolerance documentation. |

## 1. Context

Cuotai’s independently updated front ends share one TOML and several local-state files, but their readers do not yet form a compatibility boundary:

- `Provider` is a closed serde enum (`crates/cuotai-core/src/model.rs:5-49`), while `AccountConfig` and `Config` use `deny_unknown_fields` (`model.rs:278-300`, `:401-410`). PRD 0029 RF0 allow-lists only `id`; its tests still require every other account typo to reject the whole document (`crates/cuotai-core/src/lib.rs:1015-1046`).
- `Config::from_toml` maps every TOML decoder error to a fieldless `ConfigError::Parse` (`crates/cuotai-core/src/lib.rs:70-74`, `:116-121`). `ConfigFile::open` then replaces almost every parse detail with one generic edit error (`crates/cuotai-core/src/config_file.rs:193-213`).
- `ConfigFile::render` edits a `toml_edit::DocumentMut`, but can fall back to fresh serialization (`config_file.rs:464-490`); `edit_account` overwrites known keys and leaves other table items in place (`:673-713`). No test protects arbitrary future tables and keys through every save shape.
- `history.sqlite3` has two forward migrations and rejects a higher `user_version` as generic storage failure (`crates/cuotai-core/src/history.rs:85-122`, `:285-305`); `models.sqlite3` similarly accepts only version 0 or exactly 1 (`crates/cuotai-core/src/models.rs:273-288`, `:408-421`). `cost.sqlite3` reuses the history migrator (`crates/cuotai-core/src/cost/index.rs:25-70`, `:168-177`).
- `desktop.json` has no schema or unknown-field bag (`apps/desktop/src-tauri/src/settings.rs:17-79`). A corrupt file becomes `Stored::default()` and the next write replaces it; every write shares `.desktop.json.tmp`, without a lock or fsync (`settings.rs:734-755`). The updater writes the same preferences after a background check (`apps/desktop/src-tauri/src/updater.rs:575-598`).
- `Ledger::load` treats unreadable or invalid `notified.json` as empty (`crates/cuotai-core/src/resets.rs:343-374`), despite PRD 0032’s claim that corruption is already quarantined. Its writer is atomic and syncs the temporary file, but has no schema or parent-directory fsync (`resets.rs:376-387`).
- History and model rows are isolated by `ConfigIdentity::digest()` (`crates/cuotai-core/src/config_file.rs:105-142`; `history.rs:422-440`; `models.rs:387-405`). Reset marks use one global `notified.json` (`resets.rs:362-366`), while desktop bells and mutes are account-key maps (`apps/desktop/src-tauri/src/settings.rs:332-360`).
- `ConfigIdentity::File` derives from the canonical TOML path (`config_file.rs:114-141`), so moving the file changes its digest. `LastConfig` remembers earlier paths (`crates/cuotai-core/src/last_config.rs:94-147`), but does not adopt the old history key.
- Local stores call `dirs::data_local_dir()` independently, including history, models, reset marks, costs, hooks, extensions, the serve token and last-config records; for example `history.rs:432-440`, `models.rs:396-405`, `cost/index.rs:151-177`, and `last_config.rs:94-97`.
- `widget.json` alone declares `schema = 1` (`crates/cuotai-core/src/widget_snapshot.rs:36-42`); its atomic writer syncs the temporary file but not the parent directory (`widget_snapshot.rs:203-224`). `hooks.toml` has no schema field and reports only a line on parse failure (`crates/cuotai-core/src/hooks.rs:55-62`, `:408-430`).

## 2. Problem

- A newer provider slug or account key can make an older TUI reject every otherwise usable account in the shared TOML.
- A typo and a genuinely newer field look identical, while the user receives neither the responsible path nor a safe way to inspect the file.
- A downgrade can report a healthy newer state file as damaged storage, or an older writer can erase fields it did not understand.
- [INFERENCE] Concurrent desktop preference writes can lose consent, server, alert or updater changes; corruption is demonstrably converted into defaults.
- Two TOMLs with identical account keys share reset marks and desktop alert preferences, while a renamed TOML loses access to its history.
- Hard-coded OS data paths prevent a private portable profile and [INFERENCE] a single hermetic end-to-end profile spanning every store.

## 3. Goals

- Keep known accounts usable when one account requires a newer Cuotai, without silently accepting typos on validation or save.
- Give every frontend one redacted, localized configuration diagnostic with line, column and key path.
- Establish one downgrade and migration policy for every named local-state contract.
- Make `desktop.json` writes serialized, durable, private and recoverable.
- Scope account-keyed local state to the active TOML and support an explicit history carry after a config move.
- Keep compatibility promises executable through frozen fixtures and N-1 readers.

## 4. Non-goals

- Guessing the meaning of an unknown provider, field or schema-major version.
- Querying a provider while validating configuration, or probing credentials for `--check-config`.
- Synchronizing state between computers or placing credentials in the data directory.
- Automatically merging two unrelated histories merely because account names or provider slugs match.
- Making `hooks.toml` part of `cuotai.toml`; it remains beside the selected configuration (PRD 0013 RF1).
- Changing `export::v1`, events or `--json` report semantics beyond the new `--check-config --json` output.

## 5. Functional requirements

### RF1 — Forward-tolerant TOML with strict authoring (high priority; compatibility precursor)

- **RF1.1** Replace direct `AccountConfig`/`Config` deserialization with `config_file::parse(text, ConfigParseMode::{Tolerant, Strict}) -> Result<ParsedConfig, ConfigError>`; `Config::from_toml` delegates to its tolerant path. `ParsedConfig` preserves account-table order and exposes `ConfigEntry::Ready(Account)` or `ConfigEntry::NeedsConfiguration(ConfigAccountIssue)`; all runtime callers migrate to the entries instead of reconstructing indices.
- **RF1.2** In `Tolerant`, an unknown `provider` slug or unknown key inside one `[[accounts]]` table disables only that table. `ConfigAccountIssue` carries the zero-based table index, safe provider slug when present, key path and `Localized` reason `config.account.requires_newer`; it retains no value, credential, URL or raw TOML. Known accounts still refresh in document order and the unavailable entry is presented in the same position.
- **RF1.3** Unknown top-level keys and tables (including future `[[groups]]`) are retained and produce one non-fatal `ConfigWarning`; syntax errors, duplicate keys and wrong types remain `ConfigError::Parse`. The reader never maps a missing numeric value to zero.
- **RF1.4** `Strict` rejects every unknown provider, top-level key and account key with RF2’s diagnostic. `cuotai --check-config` uses it. TUI and desktop editors apply it to newly created or changed fields before save; opaque fields that were present at load may remain only when byte-identical.
- **RF1.5** `ConfigFile` keeps its parsed `toml_edit::DocumentMut` and an opaque-item manifest. `save`, `reload`, account move/rename/delete and first-save id insertion may alter known items only; every unknown scalar, array, inline table, table, comment and whitespace slice stays byte-for-byte. If a save cannot map a changed account without touching opaque content, it returns `EditError::Unsupported` and writes nothing; it never falls back to `serialize_fresh` for such a document.
- **RF1.6** This generalizes PRD 0029 RF0: `id` remains a known compatibility key, while PRD 0024 RF7’s `group`/`[[groups]]`, PRD 0028 RF8.2’s `[updates]`, and PRD 0036 RF2.7’s account keys can cross a version boundary without rejecting unrelated accounts.
- **Acceptance criterion:** pure unit tests load a four-account TOML whose second account has an unknown provider and whose third has an unknown key; the first and fourth remain usable, while the second/third reports are ordered and localized. Golden round-trip tests edit, move, rename and delete known accounts while byte-comparing every opaque range; strict-mode typo tests fail with RF2 diagnostics.

### RF2 — Redacted configuration diagnostics and `--check-config` (high priority)

- **RF2.1** `ConfigError::Parse` becomes `Parse { line: usize, column: usize, path: Option<ConfigPath> }`; `Config::from_toml` retains the decoder span and serde key path when constructing it. Positions are one-based and point to the first offending token. `ConfigPath` renders forms such as `accounts[2].provider` and never includes the value. UTF-8 failure uses path `None` and byte-safe position metadata.
- **RF2.2** `config_error_note` maps the variant to `config.error.parse` in every core catalog, with line, column and optional path as typed parameters. `ConfigFile::open` and `reload` preserve that note instead of replacing it with `edit_error.config_toml_invalid`; no error contains TOML text, a URL, token, environment-variable value or response body.
- **RF2.3** Add `cuotai --check-config [--json]`. It follows normal `--config`/`CUOTAI_CONFIG`/discovery precedence, requires an existing UTF-8 file, runs the strict parser and semantic `Config::validate`, performs no HTTP, vault, model, status or update access, and exits 0 only when valid or 1 on any local/configuration error.
- **RF2.4** Human output prints one localized diagnostic per line. JSON is a stable object `{ "valid": bool, "diagnostics": [{ "kind", "line", "column", "path" }] }`; it contains no message fallback or input value. `--check-config` conflicts with query, history, cost, watch, serve, notify and editor modes; `--json` is its only modifier.
- **RF2.5** TUI load/reload and desktop file selection/account save render the same core `Localized` diagnostic and focus or link to the named field when one exists. An invalid reload keeps the current live `Config` and readings, as `ConfigFile::reload` does today.
- **RF2.6** `cargo xtask check-example-config` materializes an uncommented temporary copy of every explicitly marked example block in `cuotai.example.toml`, runs the same strict parser without credentials or network, and CI invokes it before the TUI smoke.
- **Acceptance criterion:** CLI smoke tests assert exits 0/1, zero `wiremock` requests and redacted human/JSON output for syntax, unknown-key, unknown-provider and semantic errors; a browser-free desktop UI script and a `TestBackend` TUI render test point to the same path without pinning sentence wording; an xtask test covers every example block.

### RF3 — Versioned local-state contracts and downgrade policy (high priority; two-release rollout)

- **RF3.1** `private_fs::StateVersion { major, revision }` and `StateCompatibility::{Current, Older, NewerRevision, NewerMajor}` govern `desktop.json`, `notified.json`, `history.sqlite3`, `models.sqlite3`, `cost.sqlite3`, `hooks.toml` and `widget.json`. JSON/TOML use integer `schema` (major) plus additive `schema_revision` (absent means 0); the existing `widget.json schema = 1` is major 1. SQLite adds a one-row `state_meta` table while `PRAGMA user_version` remains the append-only migration count.
- **RF3.2** Rollout is mandatory: release N-1 first learns the envelope, ignores unknown keys/columns for the same major and still writes the old revision; release N may then emit the new revision. A writer must not remove/rename a field, narrow a value or change meaning inside a major. `NewerRevision` is readable and writable only through unknown-preserving codecs; `NewerMajor` is read-only refusal with localized `state.error.newer_major { file }`, never generic storage failure.
- **RF3.3** Before N performs any N-1 migration, it creates or atomically replaces sibling `<filename>.bak` from the exact pre-migration state, mode `0600`, flushes and fsyncs it, and fsyncs the directory. SQLite uses the SQLite backup API after a WAL checkpoint, never a raw main-file copy. Migration is transactional/idempotent; failure leaves the source and prior usable backup intact.
- **RF3.4** `desktop.json` and `notified.json` preserve unknown object members through writes. `hooks.toml` preserves unknown same-major fields and comments through the editor. `widget.json` remains a generated snapshot, but the publisher retains the previous-major/revision file as `widget.json.bak` before its first schema-changing replacement. Secrets remain forbidden in every file, snapshot and backup.
- **RF3.5** `history::migrate`, `ModelCache::open` and `CostIndex::open` return typed `NewerMajor`/`Migration` errors with localized notes; same-major databases with later additive tables or columns continue serving all known queries and writes. SQLite migrations may add nullable/defaulted columns, tables and indexes only.
- **RF3.6** `docs/local-state.md` owns a generated matrix: file, location, owner, schema major/revision, sensitive-data class, permission, backup, corruption action, downgrade behavior and erase path. A drift test enumerates the seven contracts above so adding a store without policy fails.
- **Acceptance criterion:** a core compatibility matrix opens N fixtures with frozen N-1 readers and N-1 fixtures with N migrators for all seven files; pure tests cover same-major unknown fields, clear newer-major refusal, `.bak` bytes/mode, interrupted migration and SQLite WAL backup consistency. An xtask test regenerates the matrix and fails on drift.

### RF4 — `desktop.json` integrity and serialized writes (high priority)

- **RF4.1** Public `cuotai_core::private_fs::update_json<T>(path, mutate)` is the only `desktop.json` writer. Under a per-file advisory lock it re-reads, decodes, applies one mutation and encodes once; `settings.rs::update`, all public settings writers and the updater’s scheduled/manual `last_check` write delegate to it. No lock is held across UI or network work.
- **RF4.2** `Stored` gains RF3’s schema fields and a flattened unknown-member map. Read errors distinguish `NotFound`, `Corrupt`, `NewerMajor`, permissions and I/O; only `NotFound` yields defaults. A corrupt file is atomically renamed to `desktop.json.corrupt-<UTC basic timestamp>`, a `desktop.json.recovery-required` marker is durably created, and the operation returns localized `settings.error.corrupt_quarantined`; every writer refuses the marker until the UI explicitly acknowledges recovery, so an updater cannot silently recreate defaults first.
- **RF4.3** The writer uses a same-directory unique temporary name, creates/narrows the directory to `0700` and file to `0600`, refuses symlinks/foreign ownership, writes all bytes, flushes and `sync_all`s, atomically renames, then fsyncs the parent directory. Lock acquisition has a bounded wait and a typed busy error; it never deletes another process’s lock by age alone.
- **RF4.4** Concurrent mutations are serialized as read-modify-write transactions: two settings changes and an updater completion preserve all three. A process crash before rename leaves the previous file valid; orphan unique temp files are ignored and bounded cleanup removes only Cuotai-named temps older than 24 hours.
- **Acceptance criterion:** pure/filesystem tests run 32 concurrent distinct mutations plus updater writes and observe every key; fault-injection tests at write, sync and rename preserve the previous JSON; corrupt input is quarantined byte-for-byte and the updater is refused until recovery is acknowledged; Unix tests assert `0700`/`0600`; a browser-free desktop smoke changes settings while a fake update check completes.

### RF5 — Config identity, history relocation and data-directory override (high priority)

- **RF5.1** `notified.json` becomes a map keyed first by `ConfigIdentity::digest()`; reset marks and `AlertState` for one TOML never suppress another. The desktop’s bells and metric mutes in `desktop.json` use the same first-level config identity before PRD 0029 RF2’s account id. Legacy unscoped entries migrate into the currently loaded identity with RF3’s backup.
- **RF5.2** Add core `data_dir::DataDir`. Precedence is explicit `DataDir` argument, non-empty `CUOTAI_DATA_DIR`, desktop `desktop.json.data_dir`, then `<dirs::data_local_dir()>/cuotai`; the CLI/TUI have no implicit desktop dependency, so a shared custom location uses the environment variable. An invalid higher-precedence override is a localized hard error, never a silent fallback.
- **RF5.3** An override must be absolute. `DataDir::open` creates or validates the root through `private_fs`: real directory, no symlink, current-user ownership, `0700` on Unix and writable. Every direct `dirs::data_local_dir()` caller migrates to it: history, models, resets, cost index/settings/prices, hook consent/log/work, extensions, serve token, last-config records and desktop widget fallback.
- **RF5.4** After PRD 0029 RF1 supplies stable account ids, `history::relocation_candidate(previous, current)` offers adoption only when the prior path no longer exists and the stable-id sets match exactly. Desktop file selection and interactive TUI show source, destination and sample count; acceptance calls transactional `HistoryStore::adopt_config_identity(old, new)`, rejection changes nothing. Non-interactive modes never adopt automatically.
- **RF5.5** Adoption updates only history `config_key` rows, handles destination collisions idempotently and leaves a pre-migration `.bak`; models may refetch and notification marks/preferences remain scoped to their original identity unless separately migrated by their owning PRD. No path or account name is written to history.
- **Acceptance criterion:** core tests prove two TOMLs with equal account names have independent ledgers/bells, canonical aliases share one identity, every store resolves under an injected `DataDir`, insecure overrides fail, and accepted/rejected relocation preserves exact sample counts. A `TestBackend` prompt and browser-free desktop UI script cover the move offer; a CLI smoke uses `CUOTAI_DATA_DIR` without touching the OS directory.

### RF6 — Compatibility fixtures and executable policy (high priority)

- **RF6.1** Put immutable fixtures under `crates/cuotai-core/tests/fixtures/compat/<contract>/major-<M>-revision-<R>/`; each directory has `manifest.json` with writer release token `vNEXT`, expected compatibility and SHA-256. Never edit a shipped fixture; add a revision.
- **RF6.2** Test-only `compat_readers::n_minus_1` modules freeze the immediately preceding public fields and queries. `compat_matrix` proves each N-1 reader accepts the N same-major fixture, ignores only unknown material and returns every old known value unchanged; N readers migrate N-1 fixtures and compare backups.
- **RF6.3** `config/proposed.toml` contains current syntax plus PRD 0024 RF7 `group` and `[[groups]]`, PRD 0029 RF1 `id`, and PRD 0036 RF2.7 `base_url`, `password_env`, `params.include/exclude`, `tls_ca_file`, `tls_pin_sha256`, `allow_private_http` and `dedup`. Today’s tolerant reader must parse it without rejecting the document; unsupported account entries may become RF1 issues, while known accounts remain ready.
- **RF6.4** `cargo xtask compat-check` enumerates every manifest, verifies hashes, runs the matrix and rejects a schema-changing fixture unless its N-1 reader and `.bak` expectation are present. CI runs it on Linux, macOS and Windows with no network.
- **Acceptance criterion:** xtask tests intentionally add an undeclared fixture, mutate a shipped fixture and introduce an N account key rejected by the frozen N-1 reader; all three fail. The real proposed-key fixture passes the tolerant reader and the strict reader names its first unsupported path.

### RF7 — Documentation and release discipline (medium priority)

- **RF7.1** `docs/configuration.md` documents tolerant runtime versus strict checking, `--check-config`, unknown-account behavior, editor preservation and safe remediation; `docs/advanced.md` documents `CUOTAI_DATA_DIR`, desktop precedence and history relocation.
- **RF7.2** `docs/local-state.md` publishes RF3’s generated state matrix and the downgrade rule: install N-1 before opening N state; same-major downgrade works, newer major refuses clearly, and `.bak` is recovery input rather than an automatic rollback.
- **RF7.3** Every schema-changing commit cites this PRD and the owning PRD, adds N/N-1 fixtures, and uses `BREAKING CHANGE:` only for a schema-major bump or a contract break under PRD 0039; app versions in plans remain `vNEXT`.
- **Acceptance criterion:** xtask documentation and fixture drift tests pass; docs review can follow one invalid-TOML diagnosis, one same-major downgrade and one history relocation without repository knowledge.

## 6. Non-functional requirements

- **Privacy:** diagnostics contain paths, positions and safe slugs, never values; state, backups and quarantine files contain no new secrets and retain private permissions.
- **Durability:** successful atomic replacement means file and directory metadata reached stable storage where the platform supports it; unsupported directory fsync is handled explicitly and tested per OS.
- **Honesty:** unknown is never zero/default success; corruption, incompatible major versions and migration failures remain distinct visible states.
- **Compatibility:** N-1 reads N only after the precursor shipped; no requirement pretends an already published strict binary can be made tolerant retroactively.
- **UI independence:** parsing, compatibility, identity, relocation decisions and filesystem safety live in `cuotai-core`; front ends only present diagnostics and consent.
- **Performance:** known-provider parsing and ordinary reads add no per-refresh allocation; locks and backups occur only on local-state reads/writes or migrations, never provider query paths.
- **Accessibility and i18n:** diagnostics and relocation/corruption prompts are keyboard reachable and resolve through every core/desktop catalog present at delivery.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `cuotai.toml` | tolerant runtime reader; strict checker/editor; opaque-item preservation | Existing files unchanged; future account syntax isolated |
| CLI | `--check-config [--json]`; `CUOTAI_DATA_DIR` | Additive; exits 0/1, no network |
| `desktop.json`, `notified.json` | schema/revision, unknown-member preservation, config scope | Two-release additive migration with `.bak` |
| `history.sqlite3`, `models.sqlite3`, `cost.sqlite3` | `state_meta`, typed compatibility, backups | Same-major additive; newer major refused |
| `hooks.toml` | schema/revision reservation and tolerant same-major reader | Precursor before any writer emits fields |
| `widget.json` | additive `schema_revision`; backup before schema change | Major remains `schema = 1` |
| Data root | `DataDir`, `CUOTAI_DATA_DIR`, desktop `data_dir` | Existing OS path remains default |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | both config modes, diagnostics, `ConfigEntry`, state versions/codecs, atomic writer, locks, backups, `DataDir`, identity scoping and history adoption |
| Desktop | diagnostic and corruption UI, data-directory picker, relocation consent; no parsing or filesystem policy |
| TUI/CLI | `--check-config`, diagnostic render, relocation prompt, environment override; no duplicate parsing policy |
| Docs/xtask | example checker, compatibility manifests/matrix, generated local-state inventory and CI entry points |

## 9. Dependencies and sequencing

RF1 and RF2 ship first as the compatibility precursor and before any proposed PRD writes new TOML keys. RF3’s tolerant readers ship without new revisions; only the following release may migrate/write them. RF4 then adopts the writer. RF5.1–RF5.3 can follow RF3; RF5.4 waits for PRD 0029 RF1. RF6 lands with RF1 and grows with every state revision. RF7 accompanies each delivery. Suggested wave: **5b, compatibility foundation lane**, before the identity writer and every 5d–5f TOML/state extension.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Tolerant parsing hides a typo | disable only the affected account, show a warning, and keep strict checker/save validation |
| An old writer drops future data | unknown-preserving codecs plus N-1 fixture gate before N writes |
| Additive SQLite migration still breaks old writes | nullable/defaulted additions only and frozen old read/write tests |
| Locking stalls the desktop | bounded lock wait, no network/UI work under lock, visible retry state |
| Backup doubles a large database briefly | one sibling backup only, SQLite backup API, report required free-space failure before migration |
| History is adopted into the wrong config | missing old path plus exact stable-id set and explicit consent |
| Custom data directory weakens privacy | fail closed on ownership, symlink, mode or absolute-path checks |

## 11. Order of execution and verification

1. **RF1, RF2, RF6.3:** tolerant/strict parser, diagnostics, CLI and proposed-key fixture.
2. **RF3 precursor, RF6.1–RF6.2:** envelopes and frozen readers without new writer revisions.
3. **RF4:** core atomic updater, desktop cutover, corruption quarantine and concurrency faults.
4. **RF3 writer/migrations:** `.bak`, seven contracts and generated matrix in the next release.
5. **RF5.1–RF5.3:** config-scoped state and `DataDir`; **RF5.4–RF5.5** after PRD 0029 RF1.
6. **RF7** with every step; **RF6.4** gates each schema change.

For each delivery run workspace and desktop formatting, Clippy and tests; `node --check` and browser-free scripts for touched JS; `cargo xtask check-example-config`, `cargo xtask compat-check`, and a real CLI smoke with an unknown provider plus one healthy account. The RF3 writer step is not delivered until an N-1 binary opens and updates the N same-major fixtures without losing old or unknown fields.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Should tolerant mode query an account with an unknown account key | No; isolate it as `NeedsConfiguration`, because the field may alter credentials, endpoint or cost. |
| Should a desktop-selected data directory silently apply to CLI/TUI | No; use `CUOTAI_DATA_DIR` for cross-frontend sharing and show that instruction in the picker. |
| How long should `.bak` and `.corrupt-*` files remain | Keep one `.bak` until the next migration; keep quarantine until explicit user deletion or erase-all policy. |
| Can a same-major SQLite revision alter an existing column | No; additions only. Any semantic or destructive change is a major bump. |
| Should config relocation carry model cache and notification marks too | No in this PRD; models refetch, and scoped marks/preferences stay with the original identity. |

## 13. Decisions needed from the owner

| Topic | Recommendation |
|---|---|
| Unknown account fields at runtime | Accept RF1’s fail-local policy: keep other accounts live, disable only the affected account. |
| Data-directory precedence | Accept environment over desktop setting over OS default; invalid explicit choices fail closed. |
| History relocation | Require exact stable-id match, missing old path and explicit consent; never infer from names. |
| Major-version refusal | Keep the file untouched and show recovery/downgrade guidance; never auto-restore `.bak`. |

These recommendations are the implementation defaults; an owner decision changes the PRD before delivery rather than leaving an implementer-time choice.

## 14. Integration notes (for 0000_control.md)

**Proposed §2 row:**

| ID | Plan | Status | Delivered | Depends on (strong) | Depends on (weak) | Notes |
|---|---|---|---|---|---|---|
| [0051](0051_compatibility-version-skew-and-local-state-integrity.md) | Compatibility, version skew and local-state integrity | Proposed | — | 0001, 0003, 0006, 0013, 0014, 0015; 0029 RF1 for history relocation | 0034, 0039 | Tolerant TOML plus strict checking, redacted diagnostics, N/N-1 state policy, durable desktop settings, config-scoped marks, movable history and a private data-directory override. Must precede proposed TOML/state extensions. |

**Suggested wave:** 5b, as a new compatibility foundation lane. Deliver RF1/RF2 and the RF3 reader precursor before 0029 RF1 and before waves 5d–5f emit new TOML/state fields; RF5.4 follows 0029 RF1 in the same wave.

**Existing PRDs extended:**

- **PRD 0001 RF4:** adds tolerant/strict parsing and field-position diagnostics; no non-goal superseded.
- **PRD 0003 RF2:** scopes the ledger by config identity and defines corruption/schema handling.
- **PRD 0006 RF1:** adds readable same-major downgrade, typed newer-major failure, backup migration and explicit path-relocation adoption; supersedes path changes orphaning history as an accepted consequence.
- **PRD 0014 RF1:** adds `widget.json` revision fixtures and backup policy; no widget UI change.
- **PRD 0015 RF2:** applies the common SQLite downgrade/backup policy to `cost.sqlite3`.
- **PRD 0018 RF2/RF3:** serializes updater background preference writes through the shared `desktop.json` writer.
- **PRD 0024 RF7:** makes `group` and `[[groups]]` survivable before older readers understand them.
- **PRD 0029 RF0/RF2:** generalizes the one-key precursor and config-scopes state before account-id migration.
- **PRD 0032 RF1/RF4:** supplies the state codec and schema policy beside RF4's delivered `LedgerLock`, and strengthens the corruption set-aside (COMP-02).
- **PRD 0036 RF2:** makes the aggregator TOML keys fail locally and survive older-editor saves.

**Fix requirements that belong in existing plans (decision J):**

- **PRD 0001 COMP-01 (RF4):** preserve `ConfigError::Parse` position/path through `ConfigFile::open` and `reload` instead of returning the generic TOML error.
- **PRD 0003 COMP-02 (RF2):** quarantine corrupt `notified.json` instead of treating it as empty and overwriting it; PRD 0032’s “as today” statement is false at `7a1d93c3`. Re-evaluated on 2026-10-28: on `integration/next`, PRD 0032's delivery (`0f778f1d`) renames a corrupt file to a fixed `notified.json.corrupt` (each new corruption overwrites the last) and still reads it as empty, so the statement now holds; COMP-02 remains open for a non-overwriting, timestamped quarantine and a visible recovery marker.
- **PRD 0006 COMP-03 (RF1):** replace generic `HistoryError::Storage` for a newer schema with the shared compatibility result and same-major read policy.
- **PRD 0029 COMP-04 (RF0):** correct RF0.3/RF0.4 and §7: lost ids are not harmless, arbitrary future keys are still rejected, and a newer history schema makes the stated downgrade unsafe; use RF1/RF3 here instead of a one-key promise.
