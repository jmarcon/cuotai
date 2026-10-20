# Changelog

All notable changes to Cuotai are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/) (still `0.x`: a minor bump may carry a breaking change, marked **Breaking** with its migration).

Each entry says what changed for the user, followed by the plan requirement it meets (`PRD 0006 RF2`, see [`docs/plans/0000_control.md`](docs/plans/0000_control.md)) and the short hash of the commit that delivered it.

How to read the history before this file existed:

- The versions up to `0.20.0` were **reconstructed** (PRD 0019, RF7.6) from the commits that change the version in `Cargo.toml` and from the status tables of the plans. Nothing here is invented: every entry cites its commit.
- A commit belongs to the version its tree declares. The date is the UTC date of the commit that raised the version, so a version that gathered more commits afterwards (for example `0.14.3`) may include entries committed a few hours later.
- Commits with no effect for the user (`refactor`, `test`, `style`, `docs`, `chore`) do not appear, except security fixes in dependencies. A version made only of such commits says so.
- No Git tags exist yet, so the comparison links at the bottom point at commits. They become tag comparisons when the release pipeline (PRD 0019, RF3) starts tagging.

## [Unreleased]

## [0.39.0] - 2026-10-20

### Added

- The TUI account details show the recorded history of each metric: a `▁▂▃▄▅▆▇█` sparkline of the last 24 hours, 7 days or 30 days (keys `1`, `2` and `3`) with its minimum, maximum and last value, blanks where nothing was recorded, and only the text summary on a narrow terminal; `cuotai --history [ACCOUNT] [--history-days N] [--json]` prints the series of every account (or one) with the window instances and their peaks, without querying any provider, says there is no data when nothing was recorded, and exits with 0 (data for every account), 2 (an account without data) or 1 (local error) (PRD 0006 RF6).

## [0.38.0] - 2026-10-20

### Added

- Every successful reading is now recorded in the local measurement history by the TUI, the desktop app and `--once`/`--json`, off the refresh's critical path (a bounded queue: a slow disk never delays a query and a full queue drops the reading with a visible warning), skipping a reading identical to the previous one of its metric within 15 minutes unless its window changed; errors and failed logins record nothing. The history keeps 90 days by default (7 to 365 via `--history-days` or Settings → History on the desktop, saved in `desktop.json`), folds samples older than 7 days to one per hour while keeping each window's peak, stays under 64 MiB by deleting the oldest samples with a visible warning, and can be exported to CSV or JSON and erased per account or entirely (with confirmation) from Settings → History. On opening, each account shows its last recorded reading as stale until the first query answers, and the consumption pace projects from the recent slope of the recorded samples when there are enough (PRD 0006 RF2, RF4, e743686).

## [0.37.0] - 2026-10-20

### Added

- `cuotai-core` exposes `history::HistoryStore`, a local measurement history in its own private SQLite file (`<OS local data dir>/cuotai/history/history.sqlite3`, directory `0700` and file `0600` on Unix) shared by every front end pointing at the same TOML: it records per account and metric only numbers and window metadata (never secrets, account names, URLs or notes; a missing value stays `NULL`), keeps an account's series when it is moved and starts a new one when it is renamed or gets another credential, product or region, marks removed accounts for pruning, writes safely from two instances at once, reports every storage failure as an error, and answers `latest` (the last reading per metric with its original time), `series`, `windows` (use per window instance with reset detection) and `bounds` without any network; nothing records into it yet (PRD 0006 RF1, RF3, b89c2d8).

## [0.36.0] - 2026-10-20

### Added

- `cuotai-core` exposes the versioned `export::v1` contract (`Export::from_cycle`), built from the state a `RefreshCycle` already holds without querying any provider: per account its stable key, class, current report, age, freshness, the preserved stale reading, binding metric, pace per metric and usage access descriptor, with credential variable names, URL credentials and (unless "include identity" is on) e-mails redacted and `null` for every missing value, documented in `docs/export-v1.md` with a JSON Schema and a reference output that tests guard against accidental changes (PRD 0012 RF1, ed5f027).

## [0.35.1] - 2026-10-20

### Changed

- `docs/providers.md` gains a usage access matrix with the level, credential class, plan requirement, scope and evidence of every provider and of each mode that changes the queried interface, generated from `cuotai-core`'s `access::describe` and checked by a test that fails with regeneration instructions when they drift, and the new-provider guides list the access descriptor as a mandatory step (PRD 0005 RF5, 40bbc64).

## [0.35.0] - 2026-10-20

### Added

- Dashboard cards and account details in the TUI and the desktop app show, discreetly, how each account's usage is read (its level and summary); an `Unavailable` account shows its reason instead of a generic failure, and an `Undocumented` account whose response no longer matches the known interface says the interface may have changed and points to the public contract it follows, in Portuguese, English and Spanish; `cuotai-core` serializes the descriptor for the `export::v1` contract while the current `--json` output is unchanged (PRD 0005 RF4, 594aa92).

## [0.34.0] - 2026-10-20

### Added

- The TUI wizard and the desktop account dialog show how each provider's usage is read: the picker labels every provider `Official`, `Undocumented`, `Derived` or `Unavailable`; the credential step adds, under the credential help, which credential the query needs, its risk (for example "may change without notice"), any plan requirement and, for `Unavailable`, the reason; the per-query cost warning now comes from the same description; and saving an `Unavailable` account shows the reason first and needs an explicit "Save anyway" (a second Enter in the TUI), in Portuguese, English and Spanish (PRD 0005 RF3, 6e448ea).

## [0.33.0] - 2026-10-20

### Added

- `cuotai-core` exposes `access::describe(&Account) -> UsageAccess`, a pure, network-free description of how each account's usage is read: the level (`Official` with a vendor documentation URL, `Undocumented`, `Derived` or `Unavailable` with a mandatory reason), the credential class, the plan requirement, the scope, whether every query is billed and the evidence with its verification date, for all 36 providers and the product/mode/login variants that change the queried interface, with `access.*` texts in the `en-US` and `es` core catalogs and Portuguese fallbacks (PRD 0005 RF1, RF2, 79445c0).

## [0.32.0] - 2026-10-20

### Added

- The TUI and `--once` show the consumption pace until the reset: each eligible metric on a dashboard card gets a stage icon (`↓` in reserve, `→` on pace, `↑` above the pace, `✕` limit reached), the details add a line per metric with the stage, whether it lasts until the reset or when it runs out, and the expected use, and `--once` prints a `Ritmo:` line under each eligible metric, in `CUOTAI_LANG`; it is a linear estimate, windows that just opened, have no reset or show a stale reading get none, and `--json` is unchanged (PRD 0007 RF4, 91efa8b).

## [0.31.0] - 2026-10-20

### Added

- The desktop app shows the consumption pace until the reset: a short line under the card's bar for the most restrictive metric ("Week: 8 points in reserve · lasts until the reset", or "runs out at 16:40, 2h 10m before the reset"), the pace of every metric in the details with an "Expected 47% used" mark drawn over the bar, and the same outlook in the tray tooltip and list; it is a linear estimate, windows that just opened or have no reset show nothing, and Settings → Appearance can hide it (on by default, saved in `desktop.json`) (PRD 0007 RF3, f64f5ba).

## [0.30.0] - 2026-10-20

### Added

- The desktop app can group accounts whose quota is used up or blocked in a collapsible "Recharging" section, below the others, where each card shows when it comes back ("Returns Today 14:30", or "Return unknown" when the provider gives no reset time); it is an option in Settings → Appearance, off by default, the group opens and closes with the mouse or Enter/Space, and the manual order holds inside each group without ever changing the order saved in `cuotai.toml` (PRD 0010 RF4, 571fac3).

## [0.29.0] - 2026-10-20

### Added

- The TUI and `--once` show how old each reading is: every dashboard card says `há 3 min`, the details add the age plus the measurement time in UTC and in the local time zone, and each `--once` account heading ends with `consultado há 3 min`. When a query fails after a success, the card and the details keep the last valid metrics, still and marked `(desatualizada) há 42 min` with their original age, beside the cause of the failure, until the next successful query; `--json` and the exit codes are unchanged (PRD 0010 RF5, 8f23ed7).

## [0.28.0] - 2026-10-20

### Added

- The desktop app shows how old each reading is: every card, tray tile, list row, details view and tray menu line says "3 min ago" with a Fresh, Aging or Stale label and icon (the core judges it against the account's refresh interval), and the tray icon tooltip names the oldest reading. When a query fails after a success, the card keeps the last valid metrics dimmed and still, with a "Stale · 42 min ago" badge and the cause of the failure, until the next successful query removes the badge; an authentication failure drops the old values (PRD 0010 RF3, 4a87dbf).

## [0.27.1] - 2026-10-19

### Added

- Every push and pull request is now checked on Linux, macOS and Windows (formatting, Clippy and the tests of the TUI/core and desktop workspaces, plus the dependency-license check); the desktop app gained the Windows icon its Windows build requires, and its test executables now carry the Windows manifest they need to start (PRD 0019 RF2, 4943177).

## [0.27.0] - 2026-10-19

### Added

- The TUI and `--once` show each account's most restrictive limit, chosen by the core: a dashboard card leads with that metric instead of the first one (an account blocked by the weekly window and free on the 5-hour one shows the week first), the details mark it with "◆ limitante" and a "LIMITE" summary line while every metric stays listed, and `--once` opens with one summary line per account such as `Claude  bloqueada até 14:30 pela janela semanal` (in `CUOTAI_LANG`, local time); `--json` and the exit codes are unchanged (PRD 0009 RF3, 7b73876).

## [0.26.0] - 2026-10-19

### Added

- The desktop app shows each account's most restrictive limit, chosen by the core: the tray ring uses it (an account blocked by the weekly window and free on the 5-hour one shows the week), the list view, tray menu and ring tooltip say "Blocked until <time>" when the account is blocked, each card shows a "Week · 100%" chip next to its state and highlights that window and that metric in the details while every other metric stays visible, and Settings offers an optional "Most restrictive first" order that never rewrites the manual drag-and-drop order shared with the TUI (PRD 0009 RF2, ff3429f).

## [0.25.0] - 2026-10-19

### Added

- `cuotai-core`'s `RefreshCycle` keeps each account's last successful reading, with its original `checked_at`, when a later query fails, and exposes it through `last_good_report` only while the current report is not `Ready`; a success replaces it, an authentication failure or an edit to the account discards it, moves and removals carry it with the account, and the current report, `snapshot()` and `--json` stay unchanged. It lasts for the session until local history (PRD 0006) persists it (PRD 0010 RF2, c2ebd6d).

## [0.24.0] - 2026-10-19

### Added

- `cuotai-core` exposes `resets::upcoming`, which plans quota reset notices from the readings already taken (no extra query): 5-hour, daily, weekly and monthly quotas (`other` windows on request) with a future reset, filtered by "only if used", "only if exhausted" or "always", with an optional lead time, grouping one account's metrics that reset in the same minute, skipping accounts with notices off, and treating resets that drift by up to 2 minutes as the same one; `Account::key` gives the stable provider-plus-name account key it uses (PRD 0003 RF1, 6d37344).

## [0.23.0] - 2026-10-19

### Added

- `cuotai-core` exposes `pace::assess`, which compares a 5-hour, daily, weekly or monthly quota with a uniform pace until its reset: how far ahead or in reserve the real use is, a stage with a semantic tone, and whether the current pace lasts until the reset or when it runs out, as a linear estimate from recent samples or the window average; it returns nothing for missing data or a window that has just opened, and its labels come in Portuguese, English and Spanish (PRD 0007 RF1, RF2, 51e7f24).

## [0.22.0] - 2026-10-19

### Added

- `cuotai-core` exposes `metrics::binding`, which picks the metric that restricts each account the most right now (the exhausted one with the latest return when the account is blocked, otherwise the one with the least left), plus `binding_ratio`/`cmp_binding` to sort accounts by it and a localized one-line summary in Portuguese, English and Spanish (PRD 0009 RF1, RF4, eddf602).

## [0.21.0] - 2026-10-19

### Added

- `cuotai-core` exposes `freshness::age` and `freshness::assess`, which tell how old a measurement is and classify it as fresh, aging or stale against the account's effective refresh interval; a system clock that went backwards reads as age zero instead of an error (PRD 0010 RF1, 5fc1e40).

## [0.20.0] - 2026-10-19

### Added

- The terminal dashboard animates: gauges and detail bars ease to the new value with 1/8-cell resolution and a level gradient on truecolor terminals, the class label blinks on a change, the account being refreshed shows a spinner, and details, About and Models open and close with a short wipe. `--no-animations` or `CUOTAI_NO_ANIMATIONS` turn it off; it is also off on `TERM=dumb` and over SSH (`CUOTAI_ANIMATIONS=1` re-enables), and an idle terminal does not redraw (PRD 0004 RF4, c84344a).

## [0.19.0] - 2026-10-19

### Added

- Desktop dialogs open and close with a fade and scale over a blurred backdrop, the details dialog grows from its card, the tray window fades in and its usage rings animate from the previous value, and theme and language switches cross-fade when the system WebView supports it. Reduced or disabled motion keeps everything instant (PRD 0004 RF3, 59f9f22).

## [0.18.1] - 2026-10-19

### Security

- Every request that carries a credential is now refused unless it uses HTTPS (plain HTTP is only allowed to the local machine, for sign-in callbacks), for provider queries and sign-in flows alike. The failure is reported without exposing the address (3855c72).

## [0.18.0] - 2026-10-19

### Added

- Desktop dashboard effects: percentages count up to the new value, a level change highlights the card once, exhausting a quota shakes it once, new cards enter in a cascade, reordering slides the cards, removing one shrinks it away, accounts waiting for data show a shimmering skeleton, and hover, focus, the next-query chip and the refresh button react. An idle dashboard runs no animation (PRD 0004 RF2, d89c1b1).

## [0.17.2] - 2026-10-19

### Changed

- Cuotai is licensed under `MIT OR Apache-2.0` (`LICENSE-MIT`, `LICENSE-APACHE`), shown in the About screen. The repository gains `CONTRIBUTING.md`, `SECURITY.md` with private vulnerability reporting, issue templates for defects, provider problems and new providers, and a `cargo deny` policy for licenses, advisories and sources (PRD 0019 RF1, 2509443).

## [0.17.1] - 2026-10-19

### Security

- Updated `rustls` to 0.23.46, which fixes four advisories (RUSTSEC-2026-0346 to RUSTSEC-2026-0349), and replaced the yanked `yoke-derive` with 0.8.4 (8c72af8).

## [0.17.0] - 2026-10-19

### Added

- Motion foundation for the desktop app: duration and easing tokens in the theme contract (Win98 steps, CRT without interpolation), support for the system's reduced-motion setting and a new **Settings → Appearance → Animations** option (System, Always, Reduced, Off) (PRD 0004 RF1, 6725548).

### Changed

- The desktop dashboard now updates card by card instead of redrawing everything, so keyboard focus and scroll position survive a refresh and the usage bars animate to their new value (PRD 0004 RF1, 6725548).

## [0.16.0] - 2026-10-18

### Added

- The About screen of the terminal and desktop apps credits the color palettes behind the themes (Dracula, Catppuccin, GitHub Primer, One Dark, Tokyo Night, Yaru) in a new Themes section and thanks projects created by Brazilians (d407d0d).

## [0.15.0] - 2026-10-18

### Added

- Each provider card shows how many models were synchronized for the account, in the terminal and in the desktop footer, read from the local cache without any network request (c5d91b5).

### Fixed

- Ollama Cloud accounts no longer fail with an incompatible-response error: the current usage report (requests, tokens and spend per period) is shown as usage metrics, with a note that the API no longer exposes plan limits (1769019).

## [0.14.4] - 2026-10-18

### Changed

- The desktop open-source libraries section now thanks the projects instead of listing technical details, in Portuguese, English and Spanish (d049838).

### Fixed

- Re-authenticating an OAuth account whose configuration entry does not change no longer fails with "the new configuration does not match the declared operation" (8716dce).
- The Bailian sign-in now saves the dedicated profile it generates when the profile field is left blank, instead of failing to save the account (b636d01).

## [0.14.3] - 2026-10-17

### Added

- An open-source libraries screen in the terminal (About, `?`) and in the desktop app (Settings → About), published by the core and checked against the manifests (b82de6a).
- Claude and ChatGPT/Codex subscriptions list their models on demand, in the terminal (`m` in the details) and in the desktop Models section (f093d65).

### Changed

- Every provider now tries to list the account's models with the credential it already stores for quotas, following the 9router requests. Providers without such an endpoint show a built-in list labelled as not being a query of the account (PRD 0002 RF4, 2ae1e79).

### Security

- Updated `ratatui` to 0.30, which removes the vulnerable `lru` 0.12 (GHSA-rhfx-m35p-ff5j), and added a test that fails if a vulnerable `lru` returns (c45e6f7).

## [0.14.2] - 2026-10-17

### Added

- Provider logos are vector glyphs painted on a tile in the brand color, sharp at any size and following each theme's corner radius (6d81982).

### Security

- Guard against the unsafe `glib` 0.18 `VariantStrIter` API (GHSA-wrw7-89jp-8q8g): a test fails if the API is used while an affected `glib` is resolved. Nothing in the tree uses it today (fc1ad07).

## [0.14.1] - 2026-10-17

No user-visible changes (documentation only: PRD 0002 marked as implemented, 089d964).

## [0.14.0] - 2026-10-17

### Added

- The terminal and the desktop app warn when they are using different configuration files, showing the path of the other app and how to align them, since accounts and model caches are then separate (PRD 0002 RF1.2, 6aaea88).

## [0.13.0] - 2026-10-17

### Added

- A searchable Models section in the desktop details, tray popover and account dialog, with keyboard navigation, a manual **Sync models** button, and the source and date of every list; a failed sync keeps the previous catalog (PRD 0002 RF6, 7ece4ee).

## [0.12.0] - 2026-10-17

### Added

- A Models section in the terminal (`m` in the account details and in the wizard's provider step) with local search, source, count and date, and a manual sync that does not trigger a quota refresh (PRD 0002 RF5, a399455).

## [0.11.0] - 2026-10-17

### Added

- Model listing for Groq, AWS Bedrock, Antigravity, Gemini AI Studio, Kimi, Poolside and Alibaba Model Studio through their documented endpoints, and a per-provider matrix of the 36 providers with sources in `docs/providers.md`. Providers without a verified contract show why, without any network request (PRD 0002 RF4, 051b1f5).

## [0.10.0] - 2026-10-17

### Added

- Model catalogs are cached in a local SQLite file (private permissions, digests only, no secrets). Moving an account keeps its catalog, editing or renaming it starts a new one, removing it discards it, and several windows or processes make a single request. An interrupted sync is shown as interrupted instead of retried (PRD 0002 RF3, 2d7a585).

## [0.9.0] - 2026-10-17

### Added

- On-demand model catalog per account: the list, its state (not fetched, ready, failed, unsupported) and a manual re-sync, separate from the quota refresh. Groq is the first provider to list models (PRD 0002 RF2, d1a98ca).

## [0.8.0] - 2026-10-17

### Added

- The terminal and the desktop app can share one configuration file: the desktop lets you pick the terminal's TOML (or go back to the default), both show the path and where it came from, and they resolve it the same way (`--config`, `CUOTAI_CONFIG`, `./cuotai.toml`, then the OS directory) (PRD 0002 RF1, ec96aff).

## [0.7.2] - 2026-10-17

### Fixed

- The desktop no longer recomputes the merged translation catalog on every lookup, and no longer shows "0% free" for an available metric that has no measured value (b4df3d1).

## [0.7.1] - 2026-10-17

No user-visible changes (the account identity shown on the cards is now derived in the core, PRD 0001 RF5/RF8, 399fcd3).

## [0.7.0] - 2026-10-17

### Added

- Account options declare which authentication modes they apply to. Switching the mode replaces values that no longer apply and shows a warning in the terminal and in the desktop app, and saving an invalid combination is rejected with a clear message (PRD 0001 RF11, acf2849).

### Changed

- Kiro uses a single `auth_method` option that accepts `google`, `github`, `import`, `api_key` and `external_idp` (default `google`); native sign-in is limited to `builder-id` and `idc`. Existing accounts keep loading and being queried (PRD 0001 RF11, acf2849).

## [0.6.0] - 2026-10-17

### Added

- Every metric carries an explicit `kind` (`quota`, `rate_limit`, `balance`, `credits`, `spend`, `usage`) and `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other`) in the snapshot and in `--json` (additive fields), so the desktop app no longer guesses them from labels (PRD 0001 RF10, 0163ec4).

## [0.5.1] - 2026-10-17

### Fixed

- Reordering or saving accounts no longer writes an empty `params = {}` into accounts that have no parameters (PRD 0001 RF9, 3b483cf).

## [0.5.0] - 2026-10-17

### Added

- The core owns the English and Spanish catalogs of every text it hands to a front end (fields, sign-in modes, notes, edit, query and OAuth errors), with a fixed lookup order: exact locale, same language, English, then the embedded Portuguese text. The terminal follows `CUOTAI_LANG` (default `pt-BR`) and user language packs override the catalogs (PRD 0001 RF8, a235808).

## [0.4.3] - 2026-10-17

### Changed

- Metric states and credential kinds serialize in one documented format straight from the core (for example `{"state":"available","ratio":0.25,"uncapped":false}`), and the desktop app ignores the ratio of uncapped metrics (PRD 0001 RF7, 6722722).

## [0.4.2] - 2026-10-17

### Fixed

- After a rate limit or an authentication error the retry backoff no longer shortens an interval configured above one hour, which could raise the number of queries on providers billed per request (PRD 0001 RF6, 450831b).

## [0.4.1] - 2026-10-17

No user-visible changes (an obsolete credential-mode helper was removed from the core, PRD 0001 RF5, 9070400).

## [0.4.0] - 2026-10-17

### Added

- When saving conflicts with an outside edit, the desktop app offers to reload the configuration without losing the draft, and detects outside changes when the editor opens and when the window regains focus. Validation errors highlight the field that caused them (PRD 0001 RF4, 140236c).

### Fixed

- Closing the account dialog no longer leaves the sign-in listener behind (PRD 0001 RF4, 140236c).

## [0.3.0] - 2026-10-17

### Added

- After removing an account, the desktop app can retry deleting its stored credential, which is protected by the core when another account shares it (PRD 0001 RF3, 6caf668).

## [0.2.0] - 2026-10-17

### Added

- Sign-in modes are described by the core, so the terminal and the desktop app render any provider's login without per-provider code. Re-authenticating reuses the account's credential, removes the replaced key only after the configuration is written, keeps shared keys, and reports a failed cleanup as a non-fatal result (PRD 0001 RF2, 65e1039).

## [0.1.1] - 2026-10-17

No user-visible changes (a test now counts the real HTTP requests made when editing and removing accounts, PRD 0001 RF1, aeeb460).

## [0.1.0] - 2026-10-01

First version. It also covers the work committed up to 2026-10-17 that precedes the `0.1.1` bump (`08c640d` to `5c52723`).

### Added

- A read-only monitor for AI-provider usage, quotas and costs, with a Ratatui terminal dashboard, an account wizard with native sign-in flows and an OS credential vault, and text or JSON snapshots with `--once` and `--json` (1ca290f).
- 36 providers, with a documented example configuration (`cuotai.example.toml`) and provider notes (1ca290f, 57e2510).
- A Tauri 2 desktop app with a main window and a tray popover. It manages accounts with OAuth sign-in and reordering, refreshes them incrementally, has a donut view in the popover and a native tray menu, provider logos and a theme catalog, a user folder with language packs, a provider filter (`/` focuses it, `Esc` clears it), a per-card next-query countdown, removal that offers to delete the stored credential, and a global refresh interval in Settings (e0a6a60, 6813ecb, cdc08a2, b01044c, 6cc54bd, a3bc0d7, 5c52723).
- Providers billed per query, such as AWS Bedrock, are refreshed on a longer floor interval, and configuration writes are atomic, keep comments and reject outside edits (dbe40d6, f61651a).

### Fixed

- A CSS clash that broke the alert styling in the desktop details, and the available percentage is now shown explicitly (85206dd).

[Unreleased]: https://github.com/jmarcon/cuotai/compare/e743686...main
[0.39.0]: https://github.com/jmarcon/cuotai/compare/e743686...main
[0.38.0]: https://github.com/jmarcon/cuotai/commit/e743686
[0.37.0]: https://github.com/jmarcon/cuotai/commit/b89c2d8
[0.36.0]: https://github.com/jmarcon/cuotai/commit/ed5f027
[0.35.1]: https://github.com/jmarcon/cuotai/commit/40bbc64
[0.35.0]: https://github.com/jmarcon/cuotai/commit/594aa92
[0.34.0]: https://github.com/jmarcon/cuotai/commit/6e448ea
[0.33.0]: https://github.com/jmarcon/cuotai/commit/79445c0
[0.32.0]: https://github.com/jmarcon/cuotai/commit/91efa8b
[0.31.0]: https://github.com/jmarcon/cuotai/commit/f64f5ba
[0.30.0]: https://github.com/jmarcon/cuotai/commit/571fac3
[0.29.0]: https://github.com/jmarcon/cuotai/commit/8f23ed7
[0.28.0]: https://github.com/jmarcon/cuotai/commit/4a87dbf
[0.27.1]: https://github.com/jmarcon/cuotai/commit/4943177
[0.27.0]: https://github.com/jmarcon/cuotai/commit/7b73876
[0.26.0]: https://github.com/jmarcon/cuotai/commit/ff3429f
[0.25.0]: https://github.com/jmarcon/cuotai/commit/c2ebd6d
[0.24.0]: https://github.com/jmarcon/cuotai/commit/6d37344
[0.23.0]: https://github.com/jmarcon/cuotai/commit/51e7f24
[0.22.0]: https://github.com/jmarcon/cuotai/commit/eddf602
[0.21.0]: https://github.com/jmarcon/cuotai/commit/5fc1e40
[0.20.0]: https://github.com/jmarcon/cuotai/commit/fd1e299
[0.19.0]: https://github.com/jmarcon/cuotai/commit/59f9f22
[0.18.1]: https://github.com/jmarcon/cuotai/commit/3855c72
[0.18.0]: https://github.com/jmarcon/cuotai/compare/a40c90a...a7f9e26
[0.17.2]: https://github.com/jmarcon/cuotai/compare/8c72af8...a40c90a
[0.17.1]: https://github.com/jmarcon/cuotai/commit/8c72af8
[0.17.0]: https://github.com/jmarcon/cuotai/compare/f678811...fc34f8e
[0.16.0]: https://github.com/jmarcon/cuotai/compare/1769019...f678811
[0.15.0]: https://github.com/jmarcon/cuotai/compare/b636d01...1769019
[0.14.4]: https://github.com/jmarcon/cuotai/compare/2ae1e79...b636d01
[0.14.3]: https://github.com/jmarcon/cuotai/compare/6d81982...2ae1e79
[0.14.2]: https://github.com/jmarcon/cuotai/compare/089d964...6d81982
[0.14.1]: https://github.com/jmarcon/cuotai/commit/089d964
[0.14.0]: https://github.com/jmarcon/cuotai/commit/6aaea88
[0.13.0]: https://github.com/jmarcon/cuotai/commit/7ece4ee
[0.12.0]: https://github.com/jmarcon/cuotai/commit/a399455
[0.11.0]: https://github.com/jmarcon/cuotai/commit/051b1f5
[0.10.0]: https://github.com/jmarcon/cuotai/commit/2d7a585
[0.9.0]: https://github.com/jmarcon/cuotai/commit/d1a98ca
[0.8.0]: https://github.com/jmarcon/cuotai/compare/b4df3d1...ec96aff
[0.7.2]: https://github.com/jmarcon/cuotai/compare/399fcd3...b4df3d1
[0.7.1]: https://github.com/jmarcon/cuotai/commit/399fcd3
[0.7.0]: https://github.com/jmarcon/cuotai/commit/acf2849
[0.6.0]: https://github.com/jmarcon/cuotai/commit/0163ec4
[0.5.1]: https://github.com/jmarcon/cuotai/commit/3b483cf
[0.5.0]: https://github.com/jmarcon/cuotai/commit/a235808
[0.4.3]: https://github.com/jmarcon/cuotai/commit/6722722
[0.4.2]: https://github.com/jmarcon/cuotai/commit/450831b
[0.4.1]: https://github.com/jmarcon/cuotai/commit/9070400
[0.4.0]: https://github.com/jmarcon/cuotai/commit/140236c
[0.3.0]: https://github.com/jmarcon/cuotai/commit/6caf668
[0.2.0]: https://github.com/jmarcon/cuotai/commit/65e1039
[0.1.1]: https://github.com/jmarcon/cuotai/commit/aeeb460
[0.1.0]: https://github.com/jmarcon/cuotai/commits/5c52723
