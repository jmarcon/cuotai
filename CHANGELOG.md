# Changelog

All notable changes to Cuotai are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/) (still `0.x`: a minor bump may carry a breaking change, marked **Breaking** with its migration).

Each entry says what changed for the user, followed by the plan requirement it meets (`PRD 0006 RF2`, see [`docs/plans/0000_control.md`](docs/plans/0000_control.md)) and the short hash of the commit that delivered it.

How to read the history before this file existed:

- The versions up to `0.20.0` were **reconstructed** (PRD 0019, RF7.6) from the commits that change the version in `Cargo.toml` and from the status tables of the plans. Nothing here is invented: every entry cites its commit.
- A commit belongs to the version its tree declares. The date is the UTC date of the commit that raised the version, so a version that gathered more commits afterwards (for example `0.14.3`) may include entries committed a few hours later.
- Commits with no effect for the user (`refactor`, `test`, `style`, `docs`, `chore`) do not appear, except security fixes in dependencies. A version made only of such commits says so.
- No Git tags exist yet, so the comparison links at the bottom point at commits. They become tag comparisons when the release pipeline (PRD 0019, RF3) starts tagging.

## [Unreleased]

## [0.21.0] - 2026-10-19

### Added

- `cuotai-core` exposes `freshness::age` and `freshness::assess`, which tell how old a measurement is and classify it as fresh, aging or stale against the account's effective refresh interval; a system clock that went backwards reads as age zero instead of an error (PRD 0010 RF1).

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

[Unreleased]: https://github.com/jmarcon/cuotai/compare/fd1e299...main
[0.21.0]: https://github.com/jmarcon/cuotai/compare/fd1e299...main
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
