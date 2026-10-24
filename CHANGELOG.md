# Changelog

All notable changes to Cuotai are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/) (still `0.x`: a minor bump may carry a breaking change, marked **Breaking** with its migration).

Each entry says what changed for the user, followed by the plan requirement it meets (`PRD 0006 RF2`, see [`docs/plans/0000_control.md`](docs/plans/0000_control.md)) and the short hash of the commit that delivered it.

How to read the history:

- `0.80.0` is the first published version. Each release section is written once, by `cargo xtask release prepare`, from the `Changelog:` trailers of the commits since the previous release (PRD 0039). Its version is the one that release publishes, and its footer link compares release tags.
- Versions `0.1.0` to `0.79.0` were **internal builds**. Each was raised by the commit that delivered it, under the per-commit rules used before PRD 0039, and none was tagged or published. They are summarized in one **pre-release history** at the bottom of this file, grouped by area, instead of one section per version. `cuotai --changelog 0.5.0`, or any other internal version, prints that history.
- The per-version detail of the internal builds lives in the Git history: the commits that raised each version (`git log -G '^version = ' -- Cargo.toml`), the full log up to [`2bbb39a`](https://github.com/jmarcon/cuotai/commits/2bbb39a), the plans in [`docs/plans/`](docs/plans/), and the "Delivered by version" table in §3 of [`docs/plans/0000_control.md`](docs/plans/0000_control.md). A version that a plan cites from that period means "the tree state at the commit that raised it".
- Commits with no effect for the user (`refactor`, `test`, `style`, `docs`, `chore`) do not appear, except security fixes in dependencies.

## [Unreleased]

## [0.80.0] - 2026-10-24

### Added

- Cuotai opens configurations that carry stable account ids or omit account names, so you can return to this version after a later one writes them (PRD 0029 RF0; e534fa2).
- The What's new screen and `cuotai --changelog` show the internal builds 0.1.0–0.79.0 as one pre-release history, and `cuotai --changelog 0.5.0` prints that history instead of failing (1afacb9).
- Metered accounts now refresh hourly by default, and shorter compatible intervals are raised to 15 minutes with a visible note (PRD 0033 RF5.1, RF5.2; f71d389).
- Cuotai asks before checking for updates and sends no update request until you answer (PRD 0018 RF2.6; b6fcb9d).
- Settings opens in its own window beside the dashboard, from the gear, the tray, the tray menu or Cmd+, (Ctrl+, on Windows and Linux), and remembers its size, position and last page (PRD 0020 RF1, RF2; 5c65747).
- Settings is organized in pages with a sidebar: General, Dashboard, Alerts, Data & privacy, Advanced (Integrations, Local cost, Files) and About (PRD 0020 RF1, RF2; 5c65747).

### Fixed

- The menu-bar popover now opens on top of full-screen apps on macOS (85fe9e1).
- Metered accounts with their own refresh interval no longer force the global interval above the metered floor (PRD 0001 FIX-08; fb47c70).
- Config loading now names the account and field when a metered refresh interval is below 300 seconds (PRD 0033 RF5.2; 2594a4c).
- English quota figures now say how much is left instead of how much is available (PRD 0022 RF17.7; a1303ca).
- Settings displays the Updates heading only once (PRD 0022 RF17.4; 9d0cf1b).
- The update banner now shows release dates in the selected language instead of raw ISO timestamps (PRD 0022 RF17.2; e64e330).
- Local-cost charts and price tables now show dates in the selected language instead of raw ISO values (PRD 0022 RF17.2; a36fe2b).
- The window and tray now open in the stored or detected language from the first painted frame (PRD 0022 RF17.6; b7159f9).
- The tray no longer opens blank when an updater status is already cached (PRD 0022 RF17.1; 9cc16f2).
- Update consent now describes the public manifest request accurately and preserves newer update notices after an answer (PRD 0018 RF2.6, RF3.1; 2bc2d82, 364240b, cbd10db).
- Gemini API status no longer reports Cloud Monitoring health as Gemini API health (PRD 0011 GEM-01; dc679dc, 0d3357a).
- History now rejects readings more than five minutes in the future without replacing the last good reading (PRD 0033 FIX-10; 6936672).
- History exports now omit readings more than five minutes in the future (PRD 0033 FIX-10; 3b1f76f).
- History restores the last valid reading when legacy future metadata remains in the store (PRD 0033 FIX-10; b4434ea).
- Old provider outages now expire to an unknown status after 30 minutes while preserving the last-check age (PRD 0033 FIX-09; d8608c3).
- Stale or aging readings no longer show a pace, and the pace of a fresh reading is computed at the time it was measured (PRD 0024 FIX-11; e160b91).
- A web session the provider rejected now offers "Paste new session", and a pasted session is saved only after a successful test query, or with an explicit "Save anyway" when the provider could not be reached (PRD 0016 SES-01, SES-02; 1027d8e).
- The notice that the TUI uses another config file ignores temporary or deleted files, has a close button, and shows only once for the same pair of files (PRD 0002 RF1.2; dd1a572, f98e6cf).

## Pre-release history (0.1.0–0.79.0, internal builds, never published)

Cuotai went through its internal builds 0.1.0 to 0.79.0 between 2026-10-01 and 2026-10-23. None of them was tagged or published, so what they delivered is summarized here by area. The first published version, `0.80.0`, contains all of it.

### Added

- **Monitor and providers:** a read-only monitor for the usage, quotas and costs of 36 AI providers, with a terminal dashboard (TUI), text and JSON snapshots (`--once`, `--json`), an account wizard with native sign-in flows, the OS credential vault for secrets, and a documented example configuration (`cuotai.example.toml`).
- **Desktop app:** a Tauri 2 app with a main window and a tray popover, account management with OAuth sign-in and reordering, incremental refresh, provider logos, a theme catalog that includes the built-in Cuotai theme, and animations that respect the system's reduced-motion setting (PRD 0004 RF1, RF2, RF3).
- **One core for every front end:** sign-in modes, account options, metric kinds and windows, translations and the account identity come from `cuotai-core`, so the TUI and the desktop app render any provider the same way and can share one configuration file (PRD 0001 RF2, RF7; PRD 0002 RF1).
- **Model catalogs:** on-demand model lists per account, cached locally, with search, source and date in the TUI and in the desktop app (PRD 0002 RF2, RF3, RF5, RF6).
- **Reading the quotas:** each account's most restrictive limit, the consumption pace until the reset, how old each reading is, how each provider's usage is read (official, undocumented, derived or unavailable), and the providers' public status, off until you turn it on (PRD 0009 RF1; PRD 0007 RF3; PRD 0010 RF1; PRD 0005 RF1; PRD 0011 RF1).
- **History and notifications:** a local measurement history, shown as sparklines in the TUI and as charts in the desktop app, plus quota reset notices and usage alerts in the desktop app and with `cuotai --notify` (PRD 0006 RF1; PRD 0003 RF1; PRD 0008 RF1).
- **Automation:** the versioned `export::v1` contract, the read-only local server (`cuotai --serve` and the desktop's Settings → Integration), `cuotai --watch` with event hooks, a widget snapshot for macOS widgets and desktop panels, and script extensions that run only after your explicit consent (PRD 0012 RF1; PRD 0013 RF1; PRD 0014 RF1; PRD 0017 RF1).
- **Costs and web sessions:** an estimated cost at API prices from the local logs of Claude Code and Codex CLI, off until you turn a source on, and pasted web sessions kept in the system vault for the providers that need one, with their risk spelled out (PRD 0015 RF1; PRD 0016 RF1).
- **Languages:** the interface in Portuguese, English, Spanish and Italian, with the TUI starting in the system's language when it is supported.
- **Distribution:** the `MIT OR Apache-2.0` license, the What's new screen and `cuotai --changelog`, a release pipeline with signed artifacts, checksums, an SBOM and provenance, Homebrew, winget, Scoop and AUR manifests, a daily update check in the desktop app, and `cuotai --install-source` (PRD 0019 RF1, RF7.7; PRD 0021 RF2; PRD 0018 RF1.1).

### Fixed

- Many provider, sign-in and display fixes found while the builds were used internally, for example the Ollama Cloud usage report, the Bailian sign-in profile, OAuth re-authentication of an unchanged account, and the retry backoff of providers billed per request (PRD 0001 RF6).

### Security

- Credentials travel only over HTTPS, OAuth client secrets are not kept in memory while a browser login waits, `cuotai --serve` requires a bearer token by default, hooks and extensions run in private directories with checked permissions, removing an account also deletes its web session, and dependency advisories were fixed as they appeared (PRD 0012 INT-04; PRD 0013 FIX-01, FIX-02; PRD 0017 FIX-04; PRD 0016 FIX-06).

[Unreleased]: https://github.com/jmarcon/cuotai/compare/v0.80.0...HEAD
[0.80.0]: https://github.com/jmarcon/cuotai/compare/2bbb39a...v0.80.0
