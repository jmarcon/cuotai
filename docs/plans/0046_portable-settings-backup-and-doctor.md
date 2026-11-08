# PRD 0046 — Portable settings, backup and `cuotai --doctor`

| Field | Value |
|---|---|
| ID | 0046 |
| Status | Awaiting owner verification (RF1–RF6: core, CLI and the desktop Check setup in 0.83.0; the rest in 0.84.0; packaged-app and two-machine smokes in `0000_control.md` §2.1) |
| Owner | `cuotai-core` (`bundle` module: export/import with manifest and conflict plan; `doctor` module: checks and report), `cuotai-tui` (`--backup`, `--restore`, `--doctor`), desktop (Settings › Advanced › Files actions, first-run restore) |
| Consumers | People moving to a new computer or running Cuotai on two machines; support (PRD 0028 "Report a problem" gains a redacted doctor report) |
| Depends on | PRD 0029 (stable account ids, so a restored account keeps its history and preferences), PRD 0032 (core-owned preference file), PRD 0012/0031 (server token file to **exclude**) |
| Benefits from | PRD 0028 (report draft), PRD 0027 (credential detection, to say which variables are missing after a restore), PRD 0041/0044/0045 (their TOML and preference keys travel in the bundle) |
| Extends | PRD 0028 RF2 (the report gains the doctor section), PRD 0019/0021 (`--install-source` reused by the doctor) |
| Date | 2026-10-25 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **bundle** is one archive of the person's configuration and preferences **without any secret**; the **doctor** is a local check of the installation that explains what is missing or wrong and how to fix it, without changing anything.

## Implementation status (2026-11-07)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.83.0, 0.84.0 | Core `bundle`: `.cuotai-backup.zip` written by a small deterministic ZIP writer (deflate through `flate2`, fixed 1980-01-01 timestamps, so the same tree gives the same bytes) with `manifest.json` `{schema: 1, created_at, cuotai_version, os, files: [{path, sha256, kind}], excluded: [vault, serve_token, cost_index, extensions, logs]}`. Allow-list: `config/cuotai.toml` (verbatim), `config/hooks.toml`, `prefs/desktop.json` (window geometry, one-time records and `settings` dropped; `config_file`/`user_dir` become `@config`/`@user` markers; `serve` keeps its port, `stats` and token switch, never a token; the notification, alert and quiet preferences travel inside it), `prefs/cost-settings.json` (per-machine `sources` and their folders dropped), `prefs/pricing.json`, `user/themes/*.css`, `user/locales/*.json`. A file holding a token-shaped string (known key prefixes, PEM private keys, JWTs) stops the backup, and the development SQLite store is refused. The reader accepts only that ZIP subset (no ZIP64, encryption or data descriptors; 32 MiB per entry, 128 MiB in all, CRC checked), refuses any path outside the allow-list (absolute, `..`, unknown folders), unlisted or duplicated entries, digest mismatches and other schemas. 0.84.0: `prefs/cost-labels.json` (`CostIndex::labels`: the labelled or hidden projects and the session labels, never a folder name) and, with `--with-history`, `history/<account id>.csv` (`HistoryStore::export_account_csv`: the PRD 0006 CSV sample table of the last 90 days, the journal left behind; the id is checked as an `AccountId`). The PRD 0044 channel definitions (`id`, `kind`, `label`, `host`, `filters`, never the address or signing secret, which stay in the vault) travel inside `prefs/desktop.json`; a restore merges them by id (this computer's stay) and lists one step per channel to type its address again (`bundle.need.channel`, next step `channel`). |
| RF2 | 0.83.0, 0.84.0 | `cuotai --backup FILE [--json]` writes the archive (`0600`, atomic) and prints what went in and each exclusion with its reason; exit 0, 1 on a refusal or an I/O error. 0.84.0: `--with-history` (90 days, `bundle::HISTORY_DAYS`) with a standard-error warning above 32 MiB (`HISTORY_WARN_BYTES`); the desktop **Back up…** (Settings › Advanced › Files › Backup and restore) shows the same list (files in, the core's exclusion reasons, the history checkbox and its size warning) before the native save dialog, which suggests `cuotai-backup-YYYY-MM-DD.cuotai-backup.zip`; both front ends call the same `bundle::collect_with`, so the manifests are the same for the same tree (only `created_at` differs per run). |
| RF3 | 0.83.0, 0.84.0 | `cuotai --restore FILE [--check] [--yes] [--only config\|prefs\|user\|history]`: verifies the archive, then plans per file `add`, `replace`, `keep` or `skip` (the local file was modified after the backup's `created_at`), with the TOML's accounts added/changed/removed by stable id, and lists the variables to set and the sign-ins to redo (`needs`, now with one `next_steps` entry each: kind, account position, provider). `--check` prints the plan (exit 0; 2 for a damaged or newer backup); otherwise additions are written and each replacement is asked `y/N` in a terminal (`--yes` takes all; without a terminal it stops when a file would be replaced). Desktop settings are merged over the local ones (markers and this machine's members stay, through `private_fs::update_locked`); every other write is atomic and refused when the file changed since the plan. The restore has no vault code path. Re-restoring plans `keep` everywhere. 0.84.0: history files and cost labels are `merge` items (rows in the backup, rows new here): `HistoryStore::import_csv` merges by series and time (the local sample wins, rows more than five minutes ahead are dropped, the account found by id and else by provider and name), `CostIndex::import_labels` takes a label only where this computer has none and only for projects and sessions it knows; a dry run counts without creating the store. The desktop **Restore…** opens a backup, shows the plan as a dialog with a checkbox per replacement (replacements checked, newer local edits unchecked; additions and merges applied without a choice), restores (`restore_apply` plans again and writes only the chosen replacements, then reloads the accounts), and shows **Next steps** with a button per step that opens the account (only when the backup's accounts file is the one in use) and **Restart Cuotai** when desktop preferences were restored. The first-run empty state offers **Restore from a backup** before **Add account**. The desktop never reads variables, so every key the backup's accounts read from one is listed as a key to type again, in the desktop's words (PRD 0047 G1). |
| RF4 | 0.83.0, 0.84.0 | Core `doctor::run(Inputs) -> Report { schema: 1, checks: [{id, status: ok\|warn\|fail\|skip, title, detail, fix}] }`, read-only: version/OS/architecture/install source; the configuration file (path and the rule that chose it, `--check-config`'s strict result, which also covers `query_mode`, `breakdown` and the metered floors, and the set-aside accounts of the tolerant reader); each account's credential (variable set or not, never its value); `CUOTAI_DATA_DIR`; history, model and cost databases opened read-only (size, version, `quick_check`, newer format); `hooks.toml`; desktop settings; the server token's permissions; whether port 6738 is free. `cuotai --doctor [--json]` prints it grouped by status with the fixes; exit 0 without a failure, 2 with one, 1 when it fails. Accounts are named by position and provider and home paths start with `~` (redaction tests seed values and names). **Settings › Advanced › Files › Check setup** runs the same report through `doctor_report`. 0.84.0: the system vault's availability when an account signs in through it (`SystemTokenStore::available`, no entry opened), each extension account's approval (`Manager::is_enabled`), the local cost sources that are on (can a folder be listed), the clock (`network::ClockSkew` now records whether any server date was compared: a warning, agreement, or else a stored reading more than five minutes ahead), the language and time zone, and in the desktop the notification permission (`notify::permission_granted`, never asked). Each PRD 0044 notification channel reports its last delivery from the delivery log (failing, accepted, nothing sent, or off), named by position and kind only. RF4.3: `cuotai --doctor --probe` (`doctor::probe`): one `HEAD` per fixed provider host the accounts use (never an account's base URL; loopback aggregators, AWS Bedrock, Kiro and extensions have none) and, with `--status`, per public status page, without credentials, cookies or body; any HTTP answer is reachable; the answers' `Date` feed the clock check, which runs after the probes; tested with `wiremock` (headers and body checked). RF4.4: a **Needs setup** card links **Check setup**, which opens the Files page and runs it on arrival; `IssueDraft::with_setup_check` appends the `fail`/`warn` checks to `--report-problem`, the TUI report and the desktop **Report a problem** by their ids only. In the desktop the account checks read in its own words (no variable, PRD 0047 G1). Not done by decision: the token expiry class (below). |
| RF5 | 0.83.0, 0.84.0 | RF5.1 through `cuotai --restore FILE --check` (the plan is the difference; `--only config` aligns the accounts alone) rather than a separate `--backup --diff`; RF5.2 recipes in `docs/advanced.md` (linked from `docs/installing.md`). 0.84.0: the Files page links them (**Move to a new computer**, allow-listed `BACKUP_GUIDE`). |
| RF6 | 0.83.0, 0.84.0 | `docs/installing.md`, `docs/advanced.md`, `docs/tui.md`, `SECURITY.md`, `docs/architecture.md` and `docs/gui-features.md` (Check setup) updated; 0.84.0: the desktop backup and restore, `--with-history`, `--only history`, `--probe`, the new checks and the report section; `docs/privacy.md` lists `--doctor --probe` under the provider and status flows; `docs/design-system.md` DS-CMP-BAK-1..3 and DS-CMP-DOC-1. |

**Decisions and deviations (2026-11-07, taken without the owner as the brief asked, the most conservative option each time):**

- **No token-expiry class in the doctor (RF4.1).** Reading a native sign-in's expiry means opening its vault entry; the doctor stays a tool that never reads a secret, so it checks only that the vault answers, and the next refresh reports an expired sign-in as before.
- **Channels merge by id (RF1.2, RF3.1).** PRD 0044 landed during this work with its definitions in `desktop.json`; a restore keeps this computer's channels (their addresses are in its vault) and adds the backup's others, each listed as an address to type again.
- **RF5.1 stays `--restore --check`** (as delivered in 0.83.0) instead of a second `--backup --diff` that would print the same plan.
- **History is matched by account id, then provider and name.** An id derived from the old computer's file path differs on the new one; the CSV rows name the account and provider, so a history file still finds its account. A sample already here wins.
- **Desktop restores never read environment variables** (PRD 0047): every `credential_env` of the backup is listed as a key to type again.
- **Restored desktop preferences apply on restart**: the dialog offers **Restart Cuotai** rather than reloading every window's in-memory state; the accounts file is reloaded at once.
- **Owner verification** (packaged app, native dialogs, notification permission, a real two-computer move and `--probe` on the owner's network) is in `0000_control.md` §2.1.

## 1. Context

Cuotai spreads its state deliberately: the accounts TOML (shared by TUI and desktop), `desktop.json`, the core-owned notification preferences (PRD 0032), `hooks.toml`, user themes and locales (`CUOTAI_USER_DIR`), `pricing.json` and the cost settings (PRD 0030), the history database (PRD 0006), the cost index (PRD 0015), extensions (PRD 0017), the server token (PRD 0031), and the vault entries. That is right for privacy and wrong for the person who buys a new laptop, keeps a desktop and a notebook, or asks "why does this account say Needs setup?".

No competitor in the survey offers a secret-free backup or a self-check: CodexBar and Fetchy read credentials "already on your Mac" and keep settings in the app ([CodexBar docs](https://mintlify.wiki/steipete/codexbar); [Fetchy listing, read 2026-10-25](https://apps.apple.com/app/id6786673900)); the CLI tools are stateless. A 40-provider monitor with budgets, channels, groups and themes is worth carrying over, and a doctor is the cheapest way to cut support threads (PRD 0028 decision O: GitHub only, 3-day triage).

## 2. Problem

- Moving to another machine means re-adding every account, re-choosing every setting and losing the history's continuity (ids are stable, but nothing exports them together).
- Two machines drift: a budget set on one is absent on the other.
- "Needs setup", "Stale", "Channel failing" and "Server port taken" each have a cause the app knows but no single place tells the whole story; a problem report (PRD 0028) describes one account, not the installation.
- People back up the TOML by hand and, with the best intentions, might also copy the server token or the dev SQLite store.

## 3. Goals

- `cuotai --backup <file>` / Settings › Files › **Back up…**: one archive with the accounts TOML, preferences, hooks rules, user themes and locales, `pricing.json`, cost settings and labels (PRD 0042), groups, budgets, floors, channels **definitions** (no URLs), and optionally the history as CSV; **never** vault entries, tokens, the server token, the cost index, extension folders, logs or `.env`.
- `cuotai --restore <file>` / **Restore…**: a preview of what would change (added, replaced, kept), conflict choices, and a list of **what you still need**: the environment variables and native sign-ins to redo, because secrets did not travel.
- `cuotai --doctor [--json]` / **Check setup**: read-only checks with plain sentences and fixes, redacted so the output can be pasted into an issue.
- Deterministic, versioned bundle format with a manifest and digests, verifiable by `--restore --check`.

## 4. Non-goals

- Carrying secrets in any form (encrypted or not): a secret never leaves the vault through Cuotai; the restore tells the person what to sign in to again.
- Cloud sync, automatic sync between machines, or a background backup schedule (people can run `--backup` from their own scheduler).
- Repairing anything automatically in the doctor: it explains and offers the command or the setting; the person acts (the restore is the only write path, and it is explicit).
- Exporting the cost index (per-request rows; large and derived) or the extensions' code (the person reviews and approves extensions per machine by design, PRD 0017).

## 5. Functional requirements

### RF1 — Bundle format (high priority; prerequisite)

- **RF1.1** A `.cuotai-backup.zip` (deflate) with `manifest.json` `{ "schema": 1, "created_at", "cuotai_version", "os", "files": [ { "path", "sha256", "kind" } ], "excluded": [ "vault", "serve_token", "cost_index", "extensions", "logs" ] }` and the files under `config/` (accounts TOML, `hooks.toml`), `prefs/` (`desktop.json` scrubbed, the core notification preferences, cost settings, `pricing.json`, cost labels export as JSON), `user/` (themes, locales), optional `history/` (per-account CSV as PRD 0006 exports). Paths are relative; absolute paths and `..` are refused on restore.
- **RF1.2** Scrubbing on export: `desktop.json` loses window geometry and machine-specific paths (the chosen config path becomes a relative marker), `serve.*` keeps the port and the `stats` flag but never the token; channel entries keep `id`, `kind`, `label`, `host`, `filters` (PRD 0044) and are marked `needs_secret`; the accounts TOML is copied verbatim (it holds no secret by design; a `credential_env` is a variable **name**).
- **RF1.3** Refusals: the export stops with exit `1` if any candidate file contains a string matching the vault-reference or token patterns the privacy tests already use (defence in depth), or if the dev SQLite store is the active credential store (`--dev-credentials-sqlite`); release builds never bundle it.
- **Acceptance criterion:** golden manifest for a fixture tree, the scrub rules, the refusal cases, a privacy test that greps the whole archive for token-like strings, path traversal entries rejected.

### RF2 — Export (high priority)

- **RF2.1** `cuotai --backup <file> [--with-history] [--json]` writes the bundle and prints what went in and what was excluded and why (one line each); exit `0`, `1` on refusal or I/O error. The desktop **Back up…** shows the same list before writing and a file picker (default name `cuotai-backup-YYYY-MM-DD.cuotai-backup.zip`).
- **RF2.2** Size: `--with-history` includes at most 90 days (the retention) and warns above 32 MiB.
- **Acceptance criterion:** CLI and desktop produce byte-identical manifests for the same tree (deterministic ordering and timestamps), the exclusion list printed, the size warning.

### RF3 — Restore with preview (high priority)

- **RF3.1** `cuotai --restore <file> [--check] [--yes] [--only config|prefs|user|history]` verifies the manifest digests, then prints a **plan**: per file, `add`, `replace` (with a diff summary for the TOML: accounts added/changed/removed by stable id), `keep` (identical), `skip` (newer local edit, unless `--yes`); for accounts, the **secrets you need to provide**: each `credential_env` not set in the current environment, each native sign-in to redo, each channel needing its URL. Without `--yes` on a terminal it asks per conflict; `--check` prints the plan and exits (`0` plan ok, `2` digest mismatch or unsupported schema).
- **RF3.2** Writes are atomic through the existing `ConfigFile` and preference writers (they preserve unrelated content and refuse concurrent external edits); history CSVs import through PRD 0006's importer, merging by account id and timestamp (duplicates dropped, the local sample wins). Nothing is written to the vault.
- **RF3.3** Desktop **Restore…**: the same plan as a dialog (DS-CMP-DLG with a list and per-item choices), then a **Next steps** page listing the sign-ins and variables, each with the action that opens the right dialog. The first-run screen (PRD 0025 RF6) offers **Restore from a backup** before **Add account**.
- **Acceptance criterion:** plans for the fixture (fresh machine, same machine, conflicting edits, older/newer schema), idempotent re-restore (`keep` everywhere), the history merge, the next-steps list, and a test that the restore never calls the vault.

### RF4 — Doctor (high priority)

- **RF4.1** `doctor::run(config, env, platform) -> Report` with checks, each `{ id, status: Ok | Warn | Fail | Skip, title, detail, fix }`: the configuration file (which path was chosen and why, parse and validation result), each account (credential kind; env variable **set or not**, never its value; vault entry present or not; native-login token expiry class; `query_mode`/`breakdown` validity; metered cadence), the vault backend availability (Keychain / Credential Manager / Secret Service reachable), notification permission (desktop), the local server (configured port free or taken by whom when the OS tells, token file mode), history and cost databases (openable, size, retention, integrity check), local cost sources (directories readable), extensions (approved digest still valid), hooks file (parse), channels (last outcome), update channel and install source (PRD 0021), version and OS, locale, time zone sanity (clock skew against `resets_at` of the last readings above 5 minutes).
- **RF4.2** Output: text grouped by status with the fix lines ("Set `OPENAI_ADMIN_KEY` in the environment that starts Cuotai, or switch the account to the vault"), or `--json`; exit `0` all ok, `2` any `Fail`, `1` the doctor itself failed. Redaction: no values, paths only under the user's home shown with `~`, account names as in the export scrubber, so the output is safe to paste in an issue.
- **RF4.3** The doctor is **read-only** and **offline** by default; `--doctor --probe` adds one HTTPS connectivity check per provider host (no credential sent) and the public status page reachability (PRD 0011) when status is on.
- **RF4.4** Desktop **Check setup** (Settings › Advanced › Files, and a link from every "Needs setup" card) shows the report with the fixes as actions; PRD 0028's **Report a problem** draft appends the doctor's `Fail` and `Warn` lines.
- **Acceptance criterion:** checks tested with fixture environments (missing variable, vault unavailable, port taken, stale clock, broken hooks file), the redaction suite over every line, the exit codes, the `--probe` gate with `wiremock`.

### RF5 — Two-machine workflow (medium priority)

- **RF5.1** `cuotai --backup --diff <file>` prints what differs between the bundle and this machine (the restore plan without applying), so a person can keep two machines aligned by hand.
- **RF5.2** Documentation recipe: "Move Cuotai to a new computer" and "Keep two computers aligned" in `docs/advanced.md` and `docs/installing.md`.
- **Acceptance criterion:** the diff output golden, the docs linked from the Files page.

### RF6 — Documentation (low priority)

- `docs/installing.md` (back up before uninstalling; what a backup contains and never contains), `docs/advanced.md` (commands), `docs/gui-features.md` (Files actions), `SECURITY.md` (the bundle holds no secret; the doctor output is redacted), `docs/architecture.md` (the `bundle` and `doctor` modules).

## 6. Non-functional requirements

- **Privacy:** no secret in a bundle, ever; defence in depth (pattern refusal); the doctor prints no values; both outputs are safe to attach to an issue.
- **Determinism:** identical trees give identical manifests; restore is idempotent.
- **Safety:** path traversal refused; writes atomic through the existing writers; `--check` and the desktop preview before any write; the vault is never touched.
- **Portability:** a bundle from macOS restores on Windows and Linux (paths relative, OS-specific keys dropped with a note).

## 7. Data and contract changes

- **New files:** none persistent (the bundle is the person's file).
- **CLI:** `--backup`, `--restore`, `--doctor`, with the flags above.
- **PRD 0028 report:** gains a doctor section.
- **Preferences:** none; the first-run gains a restore entry point.

## 8. Surfaces

| Surface | Change |
|---|---|
| `cuotai-core` | `bundle` (export, scrub, plan, import), `doctor` (checks, redaction, probe) |
| TUI/CLI | three commands and their flags |
| Desktop | Files page actions, restore dialog, next steps, first-run entry, Check setup from cards |
| Docs | installing, advanced, gui-features, SECURITY, architecture |

## 9. Dependencies and sequencing

RF1 and RF4 are independent and can run as two lanes; RF2/RF3 after RF1; RF5/RF6 last. After PRD 0029 (ids) and 0032 (preference file). PRD 0041/0044/0045 keys travel automatically once they live in the TOML/preference files.

## 10. Risks

| Risk | Mitigation |
|---|---|
| A future file holds a secret and gets bundled | Allow-list of files (not a directory copy), pattern refusal, privacy test over the archive in CI |
| Restore overwrites a newer local edit | Plan with `skip` by default on newer local edits; `--yes` explicit; atomic writers |
| The doctor becomes a support crutch that reveals too much | Redaction suite; values never printed; `--probe` opt-in |
| Platform-specific preferences break on another OS | Dropped with a note; tested on the three OSes in CI |

## 11. Order of execution and verification

1. **RF4** (doctor; immediate support value) and **RF1** (format).
2. **RF2**, **RF3**.
3. **RF5**, **RF6**.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests; a smoke: back up on the owner's Mac, restore on a fresh Linux VM, the plan lists two sign-ins and three variables to provide, every account appears as "Needs setup" with the doctor naming each cause, and `unzip -l` plus a token grep over the archive show nothing sensitive.

## 12. Open questions and decisions to take

| Question | Recommendation |
|---|---|
| Encrypted bundle with secrets as an opt-in? | No: secrets stay in the vault by principle; revisit only with a strong user request and a security review. |
| Include the cost index? | No (derived, large); include its settings and labels. |
| Should the desktop offer a periodic backup reminder? | A gentle one every 90 days in Settings › Files, off by default. |
| Doctor from the tray menu? | Later, after PRD 0025's menu settles; the Settings action and the card link first. |
