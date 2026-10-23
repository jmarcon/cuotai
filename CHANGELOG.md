# Changelog

All notable changes to Cuotai are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/) (still `0.x`: a minor bump may carry a breaking change, marked **Breaking** with its migration).

Each entry says what changed for the user, followed by the plan requirement it meets (`PRD 0006 RF2`, see [`docs/plans/0000_control.md`](docs/plans/0000_control.md)) and the short hash of the commit that delivered it.

How to read the history:

- `0.80.0` is the first published version. Each release section is written once, by `cargo xtask release prepare`, from the `Changelog:` trailers of the commits since the previous release (PRD 0039). Its version is the one that release publishes, and its footer link compares release tags.
- Versions `0.1.0` to `0.79.0` were **internal builds**. Each was raised by the commit that delivered it, under the per-commit rules used before PRD 0039, and none was tagged or published. They are summarized in one **pre-release history** at the bottom of this file, grouped by area, instead of one section per version. `cuotai --changelog 0.5.0`, or any other internal version, prints that history.
- The per-version detail of the internal builds lives in the Git history: the commits that raised each version (`git log -G '^version = ' -- Cargo.toml`), the full log up to [`2bbb39a`](https://github.com/jmarcon/cuotai/commits/2bbb39a), the plans in [`docs/plans/`](docs/plans/), and the "Delivered by version" table in §3 of [`docs/plans/0000_control.md`](docs/plans/0000_control.md). A version that a plan cites from that period means "the tree state at the commit that raised it".
- Commits with no effect for the user (`refactor`, `test`, `style`, `docs`, `chore`) do not appear, except security fixes in dependencies.

## [Unreleased]

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

[Unreleased]: https://github.com/jmarcon/cuotai/compare/2bbb39a...HEAD
