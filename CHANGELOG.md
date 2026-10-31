# Changelog

All notable changes to Cuotai are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/) (still `0.x`: a minor bump may carry a breaking change, marked **Breaking** with its migration).

Each entry says what changed for the user, followed by the plan requirement it meets (`PRD 0006 RF2`, see [`docs/plans/0000_control.md`](docs/plans/0000_control.md)) and the short hash of the commit that delivered it.

How to read the history:

- `0.80.0` is the first published version. Each release section is written once, by `cargo xtask release prepare`, from the `Changelog:` trailers of the commits since the previous release (PRD 0039). Its version is the one that release publishes, and its footer link compares release tags.
- Versions `0.1.0` to `0.79.0` were **internal builds**. Each was raised by the commit that delivered it, under the per-commit rules used before PRD 0039, and none was tagged or published. They are summarized in one **pre-release history** at the bottom of this file, grouped by area, instead of one section per version. `cuotai --changelog 0.5.0`, or any other internal version, prints that history.
- The per-version detail of the internal builds lives in the Git history: the commits that raised each version (`git log -G '^version = ' -- Cargo.toml`), the full log up to [`2bbb39a`](https://github.com/jmarcon/cuotai/commits/2bbb39a), the plans in [`docs/plans/`](docs/plans/), and the "Delivered by version" table in §3 of [`docs/plans/0000_control.md`](docs/plans/0000_control.md). A version that a plan cites from that period means "the tree state at the commit that raised it".
- Commits with no effect for the user (`refactor`, `test`, `style`, `docs`, `chore`) do not appear, except security fixes in dependencies.

## [Unreleased]

## [0.82.3] - 2026-10-31

### Fixed

- Accounts refresh right after the computer wakes from sleep instead of showing old data until the rest of their interval runs out (5d11d70).
- The daily update check now runs on time after the computer has been asleep (bc1af10).
- The app no longer asks the update server over and over when it cannot save the date of its last check (fb47fca).
- Choosing a configuration file that cannot be saved as the choice now leaves the app on the file it was using (359ab17).
- A damaged desktop.json is now kept as a backup and reported on the main window, instead of being silently replaced with the defaults (de8846f).
- Opening Cuotai while it is already running now brings the existing window forward instead of starting a second copy (ab42a4e).
- Reloading the configuration no longer repeats the extra query that follows a plan limit reset (636fa45).
- A sign-in the provider refuses with an unreadable answer now asks you to log in again, and a provider outage no longer reads as an invalid response (6922be4).
- A revoked GitHub Copilot or Kimi sign-in now asks you to log in again instead of showing an invalid response (a2fda3c).
- A refused OAuth client now shows as a configuration problem, and Microsoft sign-ins that need new consent ask you to log in again (4b331a6).
- A temporary CodeBuddy outage no longer reports your login as expired (5adc7f9).
- A CodeBuddy token answer without a lifetime no longer loses the rotated login (8d63dd4).
- A brief network failure no longer aborts a Qoder or CodeBuddy sign-in in progress (e8c3691).
- Cleaning up orphaned vault entries no longer forgets a credential it could not read (5cc384c).
- Signing in again now repairs an account whose stored token became unreadable (a7fbd86).
- A revoked Antigravity session now shows as an authentication failure, and a rate limit as a rate limit (8f697af).
- An Antigravity quota without a reset time no longer shows a reset in 1970 (fa3bf79).
- A temporary Qwen Cloud outage no longer reports your Token Plan session as invalid (6ec5bd2).
- A temporary Alibaba console outage no longer asks you to replace a working Coding Plan session (468db7b).
- A Token Plan window without a reset time or reading no longer fails the whole account (3c7d5fc).
- A Token Plan account whose bl profile starts with an underscore no longer fails every refresh (d2696cf).
- Connections of a 9router instance that report only a percentage now show the right usage (d13b371).
- Report a problem and What's new keep keyboard focus inside the dialog after their content loads (b7b7de4).
- The Back to accounts button of the Stats view no longer touches the banner above or the title below (e180488).
- Settings button rows drop below their label in a narrow window instead of squeezing it (b51c027).
- Settings sidebar labels no longer truncate at the default window size in German, French, Russian and Vietnamese (b51c027).

## [0.82.2] - 2026-10-31

### Fixed

- The macOS widgets now show your accounts' data (7ce59d1).
- Adding an account to a configuration with `accounts = []` no longer erases its comments and the `[updates]` table (a69db0e).
- A non-ASCII usage range from Ollama Cloud no longer crashes the refresh (5aa190b).
- Editing or signing in again to an account clears its retry backoff, so the next error no longer delays it by up to an hour (1f895e8).
- Kimi Coding no longer shows a quota as unused when the provider omits the consumption (24777da).
- MiniMax no longer shows a window as unused or exhausted when the provider omits its usage counter (80640f0).
- Settings changed at the same moment no longer overwrite each other or reset your desktop preferences (d9314b3).
- An update that finished downloading stays ready to install when the app checks for updates again (f3c4285).
- A failed update install no longer stops refreshes and notifications or removes the tray icon until the app restarts (2b71ef3).
- Switching or reloading the configuration during a refresh no longer shows another account's usage or stops automatic refreshes (17d8717).
- On macOS, clicking the Dock icon or opening Cuotai again brings back a closed main window (9960430).
- The local cost row of a narrow account card no longer breaks its title over several lines (76106c7).
- Account cards at narrow widths keep their limits aligned, show the extra usage on its own row and no longer cut the account name short beside the plan badge (a857932).
- The Updates group in Settings and the Add account button of the Dashboard page no longer touch the edge of their card (c62abd7).
- Primary buttons, the title bar's Add account and Sort buttons and plan badges are legible again in the Windows 98 and Windows XP themes (c5c93e6).
- The provider filter tabs and their More menu no longer disappear from the main window's top bar at medium widths in long languages (dbfa3fb).
- The Add account catalog scrolls only in its provider list, so its title, search and foot stay visible in short windows (e859282).
- A malformed Groq reset header no longer stops the refresh (88f44bd).
- The group labels of the Add account catalog use the right size (5919ad4).

## [0.82.1] - 2026-10-30

### Fixed

- The macOS widgets now appear in the widget gallery (088716a).
- Alibaba Token Plan and AWS Bedrock now find their command-line tools when the app is opened from the Finder or a desktop launcher (issue #50; 4746c47).
- The account details name the model or group of each limit when several share a window (issue #52; 754b687).
- The account details dialog now has a single scroll bar (issue #49; 686409b).

## [0.82.0] - 2026-10-29

### Added

- The tray shows the shortest usage window as a thick bar and the next longer one as a thin bar, with pace markers and how many points you are ahead of or behind pace (PRD 0024 RF6.1; 7ab8b16).

### Changed

- The Homebrew desktop app is now installed with `brew install --cask jmarcon/cuotai/cuotai-app`, so it no longer shares a name with the terminal formula; existing installs move over on their next `brew upgrade` (3d7bc30).

## [0.81.0] - 2026-10-28

### Added

- ElevenLabs provider: credits used, plan limit, next reset and extra usage, read with an API key (PRD 0037 RF1, RF2, RF3, RF4, RF5, RF6, RF7, RF9, RF10; 95f731a).
- You can read the full changelog in the desktop app from Settings > About with What's new (PRD 0019 RF7.7; b2d8cbd).
- The page the browser shows after signing in is now a clean Cuotai page with the logo, in your language and theme (1db9df9).
- Each account in the configuration gets a stable `id` line on its next save; 0.80.0 is the oldest release that reads such a file, and to go further back delete the `id` lines and name every account (PRD 0029 RF1; b784dfb).
- Renaming an account keeps its bell, muted metrics, alert and reset notice history and usage history, and editing its credential or parameters keeps its history series (PRD 0029 RF2, RF4; a6a760f).
- The export, `/v1/limits`, events and `--json` reports carry the account's stable `id`, and `/v1/accounts/` also accepts it (PRD 0029 RF2, RF4; a6a760f).
- The local server now uses exactly the port you ask for (6738 by default) and says when another Cuotai server holds it, instead of moving to another port (PRD 0031 RF5, RF8, RF9, RF10; cca50ee).
- Stopping `--watch` or `--serve` with Ctrl+C or a termination signal now exits with 0 whatever the accounts' state; add `--strict-exit` to keep exit code 2 for failing accounts (PRD 0031 RF5, RF8, RF9, RF10; cca50ee).
- The local server answers `HEAD` requests, lists the served schema numbers in `/health` and the export schema now accepts new enum values, so integrations keep working as Cuotai adds values (PRD 0031 RF5, RF8, RF9, RF10; cca50ee).
- A provider status incident now produces one `provider_incident` event per provider instead of one per account, and resolves correctly when the status source is lost (PRD 0031 RF5, RF8, RF9, RF10; cca50ee).
- With --notify-projected, the TUI warns 15, 30 or 60 minutes before a quota is projected to run out at the recent pace, before its reset (PRD 0024 RF1, RF2, RF3, RF5; 3dcaf4a).
- The watch mode (cuotai --watch) and hooks can emit the quota_exhaustion_projected event, with its lead set by --projected-lead (PRD 0024 RF1, RF2, RF3, RF5; 3dcaf4a).
- The export v1 pace entries carry delta_ratio, the deviation relative to the expected use (PRD 0024 RF1, RF2, RF3, RF5; 3dcaf4a).
- Model lists are no longer invented from embedded tables: CodeBuddy, Windsurf, Xiaomi MiMo, Qoder, Alibaba Token Plan, Qwen, Kimchi and GitHub Copilot no longer list models, and each access interface now shows when it was last verified and warns that subscription tokens may not be meant for other tools (PRD 0028 RF10, RF6; 16af9a7).
- When a provider changes its response format, the account details now say which field changed and how, without showing any value (PRD 0028 RF1; 9d769e6).
- You can run cuotai --report-problem ACCOUNT to get a ready-to-file GitHub issue that names the failing field and never includes your account name, e-mail or credentials (PRD 0028 RF2; f80a760).
- The macOS app now ships its Summary, Account and History widgets for the Notification Center and desktop (PRD 0014 RF2.1, RF2.3, RF3.1; 0cca674).
- You can add your own 9router instance and see the quotas of every connection inside it, with a pool summary per provider, in the CLI output and JSON (PRD 0036 RF1, RF2, RF3, RF4, RF6, RF7, RF9, RF11; ced75be).
- Blocked and exhausted cards update about 30 seconds after the reset instead of waiting for the next scheduled refresh (PRD 0033 RF1; a01babc).
- Several Cuotai windows or commands monitoring the same AWS Bedrock account now share one Cost Explorer request per interval (PRD 0033 RF5.3; 7c3f2a8).
- Reports from providers that state when their data is from, such as OpenAI costs, now carry that time in the JSON output (PRD 0033 RF3.1, RF3.3; 36ebf60).
- The AWS Bedrock account form explains the hourly default, the 15-minute minimum and the shared Cost Explorer request (PRD 0033 RF5.4; d0afcef).
- Cards say when a renewed usage window is waiting for a reading, show the provider's data time when it lags, and name the metric of each pace line (PRD 0033 RF2, RF3.2, RF4; af445b0).
- Local cost token totals and price tables now include 1-hour cache writes (PRD 0030 RF5, RF7; 529bf57).
- Local cost now says how old the prices are, lists the models without a price and can print a pricing.json template with --cost-prices --template (PRD 0030 RF3, RF4; 9e6f5eb).
- You can type your plan price and a display exchange rate with --cost-plan and --cost-currency, and cuotai --cost compares the API-price estimate with your plan (PRD 0030 RF1, RF6; 1563530).
- Theme, mode, language, animations, account order and grouping now follow you across the app windows at once and are stored in the desktop settings file (PRD 0020 RF7; 0d38866).
- You can search the settings by name or keyword, and the settings window now adapts to narrow sizes and closes with Esc or Ctrl+W (PRD 0020 RF6, RF8, RF9; ec4374b).
- The terminal dashboard can show a secret-free problem report for an account with the b key in its details (PRD 0028 RF2.4; b9d6f97).
- Maintainers can run cuotai --canary to detect provider interface changes on their own machine (PRD 0028 RF4; 2b2f9ce).
- The desktop app can preview and copy a secret-free problem report, and the account details say how recently the access method was verified (PRD 0028 RF2.3, RF6.2; 3e8b929).
- Accounts that pay for extra usage now report an overage state (off, armed, active, budget used up) and the running amount in the export and widget files (PRD 0023 RF2.2, RF2.3, RF2.7, RF3, RF6; 0d78bc0).
- You are notified once per billing cycle when paid extra usage starts, with the amount so far (PRD 0023 RF4; b482eb4).
- Scripts and hooks can react to the new overage_started and overage_ended events (PRD 0023 RF5; 0640141).
- The terminal dashboard and --once now show when an account is paying for extra usage, with the amount so far (PRD 0023 RF8; 18948be).
- The desktop app now shows when an account is paying for extra usage, on the card, in the details and in the tray, without mistaking it for a plan limit (PRD 0023 RF7; 4f449d6).
- You can turn the extra-usage notices on or off in Settings › Alerts (PRD 0023 RF7.4; 98f3360).
- The local server's /v1/limits now includes provider status per account and a summary of the tightest account, next reset and worst incident (PRD 0031 RF4; 815c47f).
- You can generate shell completions with cuotai --completions bash, zsh, fish or powershell (PRD 0031 RF3; 66b2d93).
- The watch command and hooks now report session_expiring and quota_reset_soon events, with --reset-lead to set how early a reset is announced (PRD 0031 RF7; f5a5245).
- The exported pace now uses the recent usage history, matching what the apps show (PRD 0031 RF13; d292ba8).
- The local server answers repeated requests from one cached export and supports ETag and 304 Not Modified (PRD 0031 RF6; febd32b).
- The account name is optional unless you have two accounts of the same provider; adding the second one asks you to name both in the same step (PRD 0029 RF3, RF4; 921586a).
- The export and `/v1/limits` carry `named`, and `name` shows the provider's name for an unnamed account (PRD 0029 RF3, RF4; 921586a).
- Hooks receive the account's stable id in `CUOTAI_ACCOUNT_ID`, and `hooks.toml` rules can filter by `account_id`, which survives renames (PRD 0029 RF4; 0f29618).
- Settings › Advanced › Files › Stored credentials and `cuotai --vault-audit` list the credentials Cuotai stored in the system vault and the account using each one, and Remove unused or `--vault-prune` deletes the ones no account uses (PRD 0029 RF6; 0ebab61).
- A macOS widget pinned to an account keeps showing it after you rename the account (PRD 0029 RF2, RF4; 0c33d71).
- The TUI wizard's provider list filters as you type, shows how each provider connects, opens its recommended way to connect, and can offer AI tools already installed on this device after you agree to the check (PRD 0027 RF4.1, RF5.1, RF6.5, RF7.1; 12f9138).
- In the TUI wizard's provider step, Tab now opens Models (m types into the search) (PRD 0027 RF4.1, RF5.1, RF6.5, RF7.1; 12f9138).
- Add account has a searchable provider catalog grouped by how you sign in, opens each provider's recommended way to connect, and can suggest AI tools already on this device after you allow the check (PRD 0027 RF3.1, RF3.2, RF3.3, RF3.4, RF3.5, RF5.1, RF6.2, RF6.5, RF7.1, RF7.2, RF7.3; 8da2fa1).
- The menu bar shows your tightest remaining share next to the Cuotai icon, following the light or dark menu bar on macOS (PRD 0025 RF2, RF3, RF7.1; 3dc5bbf).
- Settings has a Menu bar page to choose the menu-bar style, color and which accounts it shows in your own order (PRD 0025 RF2.3, RF4, RF5.1, RF5.3, RF6, RF7.2; 01cc390).
- A first-run step offers to turn on the recommended usage alerts and asks the update question in one place (PRD 0025 RF2.3, RF4, RF5.1, RF5.3, RF6, RF7.2; 01cc390).
- The connections of a 9router instance now get their own usage alerts, reset notices, history, freshness and events, with one notice when every account of a provider in it runs out (PRD 0036 RF1, RF4, RF6, RF7, RF11; 5cbcda9).
- The export, the local server's limits, events and widget data list each 9router connection under its instance, with pool summaries and the account with the most room per provider (PRD 0036 RF1, RF4, RF6, RF7, RF11; 5cbcda9).
- Adding or changing a 9router account now tests the connection first, in the TUI wizard and the desktop dialog, and shows one sentence for each outcome (PRD 0036 RF2, RF8, RF9, RF11; 882bc4e).
- A 9router instance with a self-signed HTTPS certificate can be trusted with a CA file or by confirming its certificate fingerprint, and a changed certificate stops the account instead of being trusted silently (PRD 0036 RF2, RF8, RF9, RF11; 882bc4e).
- The TUI dashboard lists each 9router connection as its own card after your direct accounts, with a pool summary on the instance and the g key to fold them (PRD 0036 RF9, RF11; 12dfa69).
- The desktop app shows each 9router instance as a section of its own with its connections grouped by provider, a grouped row in the tray that expands in place, connection details and an Open dashboard action (PRD 0036 RF8, RF6, RF11; 618d58d).
- You can open usage statistics with the S key in the terminal dashboard, and read them, the reset timeline and the event journal with --stats, --timeline and --events-log (PRD 0026 RF6.1, RF6.2; 450b389).
- You can see usage statistics (limit hits, average peak, extra spent, peak per day, recent events) in a new Stats view, and when each account's tightest limit comes back in the tray popover (PRD 0026 RF4.1, RF4.2, RF4.3, RF4.4, RF4.5, RF5.1, RF5.2; 85d0ed2).
- The desktop app can warn you 15, 30 or 60 minutes before a quota is projected to run out, and hooks can react to it (PRD 0024 RF3.2, RF5; 53802ba).
- Settings > Alerts has a "Warn before running out" switch with a 15 min, 30 min or 1 h lead, and cards and details say when a quota is projected to run out before its reset (PRD 0024 RF4; ddae534).
- You can show the consumption pace in percentage points or in percent (Settings > Dashboard, or --pace-unit in the terminal), and reset times read the same on every surface (PRD 0024 RF1.3, RF1.4, RF6; a659393).
- Accounts can be put in a group in the TOML (group = "coding") so Cuotai can tell which of them has the most room right now; the export and /v1/limits list it as best (PRD 0024 RF7.1, RF7.2, RF7.4; 9f45749).
- The desktop app marks the best account of each group of comparable accounts ("Best now") and tells you what it has left when another one runs out; Settings > Dashboard can turn it off (PRD 0024 RF7.3, RF7.5; 8da15ed).
- The account wizard and dialog have an optional Group field, and the terminal dashboard marks the best account of each group (PRD 0024 RF7.1, RF7.5; b8a1042).
- A quota without a reset date now sends a recovery notice once usage falls to 80% or below (PRD 0032 RF3.2, RF8.1, RF9; 9329847).
- Notifications can follow quiet hours with a summary when they end, hide account names by default, and are never sent twice when the app and the terminal dashboard are both open (PRD 0032 RF1, RF2, RF3, RF4, RF7.1, RF8.1; 0b57bfe).
- The terminal dashboard's --notify now shares your desktop notification settings, quiet hours and sent-notice history, so the same crossing is not announced twice (PRD 0032 RF1.2, RF4.1, RF4.3, RF6.1, RF7.2; 8857393).
- A hook rule can set quiet = "skip" to stay silent during your quiet hours (PRD 0032 RF5.1; c34aeb4).
- Settings > Alerts has quiet hours per weekday with an end-of-night summary, a switch to hide account names in notifications, and cards mark quotas that crossed a limit while Cuotai was closed (PRD 0032 RF2.3, RF5.2, RF7.1, RF8.2; f050efc).
- You can monitor OpenRouter: an API key shows its limit, spend and free-model requests, and a management key shows the credit balance (PRD 0038 RF1, RF2, RF3, RF4, RF5, RF6, RF7, RF8, RF9, RF11, RF12; 72ee36f).
- You can sign in to OpenRouter from the browser instead of pasting an API key (PRD 0038 RF1, RF14; 4b40079).
- When you allow device checks, adding an account also offers OpenRouter if OPENROUTER_API_KEY is set; only the variable's name is stored (PRD 0038 RF5; PRD 0027 RF6; 52149a7).
- A provider's Retry-After on a rate limit now delays only that account's next refresh (PRD 0038 RF7.1; 2bffef9).
- The dashboard can now flag an ongoing ElevenLabs API incident in your account's region when status checks are enabled (PRD 0037 RF8; aeafa41).
- You can print one status-bar line with cuotai --status-line, and the Homebrew formula installs shell completions (PRD 0031 RF2, RF1, RF3; bd419a0).
- The tightest account in /v1/limits now carries its stable id (PRD 0031 RF4; adf71af).
- You can serve usage statistics at /v1/stats with cuotai --serve --serve-stats, behind the access token (PRD 0031 RF11; 7788f0c).
- The status line can read a running local server: cuotai --status-line --from-server reads a running cuotai --serve instead of querying the providers again (PRD 0031 RF2; 289dc63).
- The desktop app can share usage statistics on its local server (/v1/stats) from Settings > Advanced > Integrations (PRD 0031 RF11; e586170).
- The terminal dashboard shows your plan price and the amount at your own exchange rate in the local cost of each account (PRD 0030 RF1, RF6; 198062a).
- You can type what you pay for each plan and an exchange rate in Settings to see the local cost next to your plan price and in your own currency (PRD 0030 RF1, RF6; 7c6962d).
- Gemini CLI can now be added as a local cost source, with the same opt-in consent and numbers-only reading as the other tools (PRD 0030 RF2, RF8; ffbbd62).
- When a provider changes the shape of its answer, more providers now say which field changed (PRD 0028 RF1; d31439e).
- The terminal can now tell you when a newer Cuotai is available, only if you turn on the update notice (PRD 0028 RF8; PRD 0021 RF8.2, RF8.3; 94706d7).
- An extension manifest can require a minimum Cuotai version with min_cuotai and carry its own x- tables (PRD 0028 RF7.5; 98e67bf).
- Extensions can declare secrets that you enter once and that Cuotai keeps in the system vault, passing them only to the extension's own process (PRD 0028 RF7.4; da9db36).
- An extension can replace a broken built-in integration for the accounts you choose, without receiving their credentials (PRD 0028 RF7.1, RF7.2, RF7.3; 0cf35a6).
- The add-account picker lists your local extensions as "Extension: name" and opens their approval when you choose one (PRD 0027 RF1.3; 10c7eef).
- The history CSV export now includes the event journal, and events of 9router connections show their aggregator (PRD 0026 RF3, RF5; PRD 0036 RF7; f0813f5).
- A model that reached its own limit is now named on the card, and statistics no longer count it as an account limit hit (PRD 0023 RF10; PRD 0026 RF1; 9391359).
- Builds made between two releases now show that they are development builds next to the version number (PRD 0039 RF10; ed143a3).
- A 9router connection that is also added as a direct account is shown once, as the direct account, and you can hide single connections or rank them in a group (PRD 0036 RF5, RF6, RF2; e38fb75).
- A 9router connection can be refreshed on its own, and its quota is read again 30 seconds after it resets (PRD 0036 RF4; 71da36a).
- You can add your own LiteLLM proxy and see the budget and spend of each provider it routes to (PRD 0036 RF10, RF1; f6cb007).
- With --watch --strict-exit, a failed 9router connection makes the exit code 2 (PRD 0036 RF9; 9db363c).
- With device checks on, the Add account picker offers 9router when it is running on this computer (PRD 0036 RF2, RF8; 18be3c3).
- The desktop app uses one spacing, type and color scale everywhere, so every theme looks consistent (PRD 0022 RF1, RF17.5; f8ae482).
- Quota bars are continuous, show the expected pace as a soft band and draw unknown or old readings honestly (PRD 0022 RF2; 0da9dd3).
- The tray lists accounts as calm rows with one status line, filter tabs in the header and the last refresh in the footer (PRD 0022 RF3, RF4, RF5, RF6, RF12.3, RF13.4; 4a08e60).
- Main-window cards show each figure with its unit below, one status line and fold the local cost into one row (PRD 0022 RF3, RF4, RF5, RF6, RF12.3, RF13.4; 4a08e60).
- The main window's top bar is shorter, filters accounts by provider and keeps every control visible in a narrow window (PRD 0022 RF7, RF8.1, RF17.3; a6d0c16).
- Settings › Dashboard chooses the tray view: List, Rings or Compact (PRD 0022 RF7, RF8.1, RF17.3; a6d0c16).
- Settings mark the chosen language and sort with a check, and Alerts previews the test notification (PRD 0022 RF9.2, RF9.5, RF9.6; d80da30).
- Account details lead with incidents and list each limit on one quiet row, and banners use one consistent look (PRD 0022 RF10.1, RF10.2, RF10.3, RF10.4, RF1.7; e55dd69).
- Charts no longer animate in, and the local-cost bars mark today, unpriced days and zero days honestly (PRD 0022 RF11, RF6.2; 262284c).
- The app is calmer: data changes appear at once and only the refresh spinner moves while work runs (PRD 0022 RF12; 44a7845).
- The terminal dashboard honors NO_COLOR, uses your terminal's own text and background colors, and no longer flashes or wipes (PRD 0022 RF16; 87c920d).
- The menu bar settings, statistics, 9router and new dialogs follow the same calm design as the rest of the app (PRD 0022 RF8.1, RF14, RF18.3; b01922b).
- Account rows and cards show pace stages with time chips, one reset line per limit and extra usage as its own column (PRD 0022 RF3, RF4, RF8.1; 2be2331).
- Settings > General shows labelled mode choices, a theme swatch and a single language field (PRD 0022 RF9; 5e3e4fb).
- The Add-account catalog lists providers grouped by sign-in method (PRD 0022 RF11, RF18.2; 1a132ce).
- Removing an extension account now offers to delete that extension's saved secrets from the vault (PRD 0028 RF7.4; 96fcce4).
- The Extensions dialog and the Add account picker follow the refreshed design, with a link to add a provider with an extension (PRD 0022 RF18; PRD 0028 RF7.4; 118dab0).
- An extension that is not yet approved now reads "Needs approval", and removing its account from the account details also offers to delete its saved secrets (PRD 0022 RF10; PRD 0028 RF7.4; b2701f0).
- The Stats view now follows the redesigned layout, keeps the extra-spent amount on one line, and recent events of an aggregator's connection name the aggregator (PRD 0026 RF4; PRD 0022 RF18; PRD 0036 RF7.3; 300351e).
- The 9router dashboard password and the LiteLLM key can be kept in the system vault instead of an environment variable (PRD 0036 RF2.1, RF2.6, RF10.1; a9bf9d9).
- An SSO-only 9router instance can be added by pasting its auth_token dashboard session (PRD 0036 RF2.1, RF2.6, RF10.1; a9bf9d9).
- With your consent, Add account also finds Qwen Code, Kiro, Kimi CLI, Grok CLI and Zed on this device (PRD 0027 RF6.4; 7cbf851).
- On Windows the tray icon shows your tightest remaining share, and VoiceOver reads the menu-bar summary on macOS (PRD 0025 RF3, RF7.1; PRD 0022 RF17.8; cc2232e).
- The first-run step shows your package manager's update command when it manages this copy of Cuotai (PRD 0025 RF6.5; ca2a423).
- Settings › Menu bar shows a live preview first, each account's state and how many accounts the menu bar shows (PRD 0025 RF4; PRD 0022 RF9.4; ad69942).
- You can add, edit, remove and reorder accounts in Settings › Dashboard (PRD 0025 RF5.2, RF9; PRD 0022 RF8.2, RF8.3; 1100dc9).
- You can choose how much each account row shows, from Detailed to Minimal or element by element (PRD 0025 RF5.2, RF9; PRD 0022 RF8.2, RF8.3; 1100dc9).
- The first-run, What's new and Report a problem dialogs, the update banner and the Settings sidebar now follow the refreshed design (PRD 0022 RF9, RF10; 044ccfb).
- Settings > General has a Start at login switch that starts Cuotai in the tray when you sign in (PRD 0022 RF9, RF10; 778e00d).
- The plan prices and quiet hours settings use the same card rows as the rest of Settings, and a plan converted to your second currency is marked as an estimate (PRD 0022 RF9, RF10; 63a88b2).
- The Add-account catalog lists 9router and the LiteLLM proxy under their own "Self-hosted gateway" group (PRD 0036 RF10; f200a36).
- Rows and cards show the plan the provider reports, such as Max or Plus (PRD 0022 RF4; PRD 0023 RF10.3; PRD 0036 RF4.4, RF8.5; PRD 0039 RF10; dffb7af).
- Cards and details name a model that is at its limit and when it returns (PRD 0022 RF4; PRD 0023 RF10.3; PRD 0036 RF4.4, RF8.5; PRD 0039 RF10; dffb7af).
- The run-out warning in an expanded account has an "Alert me" button (PRD 0022 RF4; PRD 0023 RF10.3; PRD 0036 RF4.4, RF8.5; PRD 0039 RF10; dffb7af).
- Each 9router or LiteLLM connection can be refreshed, hidden or shown beside its direct account (PRD 0022 RF4; PRD 0023 RF10.3; PRD 0036 RF4.4, RF8.5; PRD 0039 RF10; dffb7af).
- On macOS, you can hide Cuotai from the Dock in Settings > General so it lives only in the menu bar (PRD 0022 RF9; 1a9b9fd).
- **Breaking**: Texts that Cuotai's core delivers are written in English, with Portuguese, Spanish and Italian as catalogs, so a missing translation now shows English instead of Portuguese. Migration: the fallback values of --json, --json-v1 (export::v1), the local server and Report.notes are English instead of Portuguese (shapes, ids, params and schema unchanged); resolve id in a catalog (crates/cuotai-core/locales/*.json) or set CUOTAI_FALLBACK_LOCALE=pt-BR (or --fallback-locale pt-BR) to keep Portuguese fallbacks (PRD 0034 RF1, RF4, RF5; 25e0a98).
- **Breaking**: The JSON outputs, the local server and the event stream carry English fallback texts; --fallback-locale pt-BR or CUOTAI_FALLBACK_LOCALE=pt-BR renders them in Portuguese. Migration: the fallback values of --json, --json-v1 (export::v1), the local server and Report.notes are English instead of Portuguese (shapes, ids, params and schema unchanged); resolve id in a catalog (crates/cuotai-core/locales/*.json) or set CUOTAI_FALLBACK_LOCALE=pt-BR (or --fallback-locale pt-BR) to keep Portuguese fallbacks (PRD 0034 RF1, RF4, RF5; 25e0a98).
- The desktop app no longer shows Portuguese error texts, demo data or pseudo-locale gaps when it runs in another language, and its local server renders the fallback texts in the language named by CUOTAI_FALLBACK_LOCALE (PRD 0034 RF7; c084ac9).
- Measurement ages, sign-in progress lines and metric labels, units and periods can now be shown in English, Spanish and Italian (PRD 0034 RF2; bc1a366).
- The terminal dashboard, its dialogs and the command-line messages follow the interface language (English, Portuguese, Spanish or Italian), and --help is in English (PRD 0034 RF2, RF3; PRD 0027 RF6.5; 6be8b18).
- Unnamed extension accounts can show the extension's own display name instead of its id (PRD 0029 RF3.4; a806e16).
- Searching the provider catalog for tts, text to speech, voice or voice agents now finds ElevenLabs (PRD 0037 RF2.3; 475ccad).
- A [[groups]] table can list its member accounts by id (PRD 0024 RF7.1; 174dbe5).
- Cuotai installed with winget now recognizes it and leaves updates to winget (PRD 0018 RF3.2; PRD 0021 RF8.1; ecb8167).
- The JSON outputs add label_text, unit_text and period_text next to each metric's label, unit and period, in English by default or in the fallback locale (PRD 0034 RF2; e7e000e).
- In the terminal dashboard you can refresh a single 9router or LiteLLM connection, filter by aggregator, and see warnings for instances without a password or on unencrypted http (PRD 0036 RF4.4, RF9.1, RF2.3, RF2.4; 16275fb).
- The macOS widgets now speak English, Portuguese, Spanish and Italian (PRD 0014 RF3.4; 98aa4fa).
- Settings > Advanced now has an Extensions page, searchable like every other setting (PRD 0020 RF2.5; bb8c0d2).
- The release notes now open once after you update Cuotai, with a switch in Settings > About to turn that off, and you can set a one or two character monogram for an account (PRD 0019 RF7.7; PRD 0022 MONO-4, RF15.1; 0a931fa).
- A 9router account shows a persistent chip when the instance has no password or uses unencrypted HTTP on your network (PRD 0036 RF2.3, RF2.4, RF8.1; 796d8b1).
- The tray has a tab of its own for each 9router instance (PRD 0036 RF2.3, RF2.4, RF8.1; 796d8b1).
- Local-cost amounts on the cards and in the daily chart show "≈" to mark them as estimates (PRD 0022 RF6.1; e66911f).
- Accounts whose provider bills every query keep a "Billed per query" chip on their card (PRD 0027 RF3.1; 6231bb5).
- The monogram you set for an account now appears on its tiles in the tray and the main window (PRD 0022 RF3; 663e20d).
- Notices and messages now use the right singular or plural form, such as "1 usage alert" instead of "1 usage alerts" (PRD 0035 RF1, RF4; 8d97819).
- The language is detected by its language code only, so systems set to an unsupported language such as Estonian or Traditional Chinese get English instead of a wrong match (PRD 0035 RF1, RF4; 8d97819).
- An unknown CUOTAI_LANG value now prints a warning and uses English (PRD 0035 RF1, RF4; 8d97819).
- The language selector lists the ten upcoming languages, each with its flag and its own name, marked "Translation in progress" until its translation ships (PRD 0035 RF1.5, RF3.3, RF7.1, RF7.2, RF8.2; 4fdbdd8).
- Searching accounts and models ignores accents and special letters in every language, so "dong" finds "Đồng" and "strasse" finds "Straße" (PRD 0035 RF10.3, RF10.5, RF10.6; 038b761).
- Countdowns in the app use the same localized duration wording as the rest of the interface (PRD 0035 RF10.3, RF10.5, RF10.6; 038b761).
- Cuotai is now available in German and Russian (machine-translated, awaiting native review) (PRD 0035 RF2, RF3, RF5, RF11; 0571693).
- The usage history takes about a tenth of the disk it did and is upgraded once, in seconds, on first use; after that upgrade an older version can no longer read it, so do not downgrade to 0.80.0 (PRD 0006 RF4; PRD 0026 RF4.2; eae6985).
- The terminal dashboard footer shows the selected card's position among the visible accounts ("7/50 accounts"), so long lists tell where you are (3baa097).
- Cuotai is now available in Korean (machine-translated, awaiting native review) (PRD 0035 RF2, RF3, RF5, RF11, RF13; ef21be5).
- Cuotai is now available in Hindi (machine-translated, awaiting native review) (PRD 0035 RF2, RF3, RF5, RF11, RF13; a3c7ce4).
- Cuotai can now be used in Indonesian (machine-translated, awaiting native review) (PRD 0035 RF2, RF3, RF5, RF11, RF13; af627e7).
- Cuotai can now be used in Turkish (machine-translated, awaiting native review) (PRD 0035 RF2, RF3, RF5, RF11, RF13; a2bf39b).
- Cuotai can now be used in Vietnamese (machine-translated, awaiting native review) (PRD 0035 RF2, RF3, RF5, RF8, RF11, RF13; 7f55f4d).
- The interface, messages and terminal dashboard are available in Simplified Chinese (machine-translated, pending native review) (PRD 0035 RF2, RF3, RF5; b3de314).
- The interface, messages and terminal dashboard are available in Japanese (machine-translated, pending native review) (PRD 0035 RF2, RF3, RF5; b158d24).
- The interface, messages and terminal dashboard are available in French (machine-translated, pending native review) (PRD 0035 RF2, RF3, RF5; cf0306b).
- Settings > General says when the interface language is a machine translation and links the guide for reviewing it (PRD 0035 RF11.5; 4b06f7b).
- The event journal records how much extra usage was spent, and in which currency, when paid extra usage starts or ends (PRD 0026 RF3.1; 631e1e0).
- On Chinese, Japanese and Korean terminals, and on terminals without UTF-8, the TUI draws its shapes and borders in ASCII so columns stay aligned; CUOTAI_AMBIGUOUS_WIDTH and CUOTAI_GLYPHS override the choice (PRD 0035 RF6.3, RF6.7; e753b02).
- Chinese, Japanese, Korean, Hindi and Vietnamese text in the desktop app gets taller lines and no artificial letter spacing, so accents and vowel marks are never clipped (PRD 0035 RF8.3, RF8.4, RF8.5; 3930a02).
- The terminal dashboard keeps tables aligned with Chinese, Japanese, Korean and Hindi text, deletes whole letters with Backspace and capitalizes Turkish headings correctly (PRD 0035 RF6.1, RF6.2, RF6.4, RF6.5; 1902ec7).
- Long model names and paths wrap inside Chinese, Japanese, Korean and Hindi text instead of widening the window (PRD 0035 RF8.6, RF8.7, RF9.5; 2167205).
- The Add account dialog lists every provider right away, grouped by sign-in method, and the search narrows the list (PRD 0027 RF3; 6cf332a).
- In the desktop app you can type an API key or secret when adding an account, and Cuotai keeps it in the system vault instead of asking for an environment variable (PRD 0047 E1, E2, E3, E4, E5, E6, E7; 3c47863).
- The desktop account dialog asks you to paste the API key itself, keeps it in the system vault and can move a key set up in the terminal into Cuotai (PRD 0047 G1, G2, G3, G4, G5, G6; 658d1a3).
- A second account of the same provider now gets its own suggested environment variable, such as GROQ_API_KEY_WORK (PRD 0047 T1, T2, T3, T4; e2181a8).
- The desktop explains in plain words why an account shows no usage, without naming environment variables, and offers Connect session when a web session is missing (PRD 0047 G1, G7; 15f976f).
- The desktop account dialog has a link that opens the site where you copy a web session (PRD 0016 RF2.1; b7bfd4b).
- You can sign in to the Xiaomi MiMo platform in a private Cuotai window and capture its web session without copying cookies (PRD 0048 RF1, RF2, RF4, RF5, RF6, RF7, RF8; 5e07bb7).
- The Xiaomi session capture window also lets you sign in with Facebook or Google (PRD 0048 RF3; 5f0b729).
- The desktop app can sign in to Qwen Cloud in its own window and capture the Token Plan session, instead of copying the cookie by hand (PRD 0048 RF1.1, RF3.3, RF4.3; 20de84e).
- The account form shows where to create the API key of each provider that needs one (PRD 0047 E2; 899a916).
- In the desktop account dialog, a link next to the API key opens the provider page where that key is created (PRD 0047 G2; 9550ca1).
- You can add a Meta Model API (Muse) account: Cuotai checks the key and shows the team's rate limits when Meta reports them (PRD 0049 RF1, RF2, RF3, RF4, RF5; 69790f1).
- The Meta Model API key input links to the dev.meta.ai page where the key is created (PRD 0049 RF5; 5b7d747).
- The desktop app can sign in to the Alibaba Coding Plan console, international or mainland China, in its own window and capture the session, instead of copying the cookie by hand (PRD 0048 RF1.1, RF3.4, RF3.5, RF4.4; f6b6228).
- The Models view lists the Meta Model API (Muse) models available to your key (PRD 0049 RF1; 55d0bcf).
- You can sign in to dev.meta.ai from a Meta Model API account so Cuotai keeps its console session (PRD 0049 RF7, RF10; PRD 0048 RF3; 5f5e909).
- Meta Model API accounts signed in to dev.meta.ai show the team's console usage (undocumented interface, may change without notice) (PRD 0049 RF8, RF9, RF10; f020c61).

### Changed

- Adding an account now lists providers by name only, without integration-status labels, and no longer offers providers whose usage cannot be read; the cards and details no longer show those labels either, and the documentation explains how each integration works (PRD 0005 RF3, RF4; c012cdb).
- Settings are shorter and apply as you change them, with a "Saved" confirmation, errors next to the setting and details behind info buttons (PRD 0020 RF3, RF4, RF5; 5c8da8c).
- Configurations written by this version (account `id` lines, unnamed accounts) open in Cuotai 0.80.0 or later; to go further back, delete the `id` lines and name every account (PRD 0029 RF7; c5cd9d6).
- On Unix, sending SIGHUP to cuotai --serve or --watch rereads the configuration without restarting; SIGHUP no longer stops --watch (PRD 0031 RF12; 83d786b).
- New installs start in the Cuotai theme; existing users keep the theme they had (PRD 0022 RF15.1, RF15.2, RF15.5; e4c6792).
- The Stats page and the account details read a month of history in a fraction of the time (PRD 0026 RF1; PRD 0006 RF5; c551982).
- The desktop dashboard redraws its cards about three times faster (PRD 0022 RF1.9; 2b5a5bd).
- The example user language pack is now a partial Dutch pack, since French is built in (PRD 0035 RF12.3; 59fe3a8).

### Fixed

- A restored reading whose usage window has already renewed no longer shows as exhausted, and readings older than 24 hours are not restored (PRD 0033 RF2; 897509a).
- Claude Code cost estimates now price 1-hour cache writes at their own rate and count the final output tokens of streamed replies (PRD 0030 RF5, RF7; 3c58e43).
- Claude, Grok, Cursor and Kiro extra usage no longer counts as your binding limit or triggers quota alerts, and one exhausted model no longer blocks the whole account (PRD 0023 RF1, RF2, RF10; d4dc4c3).
- Switching an account to another provider deletes its old OAuth token from the vault after the save, and the summary says so beforehand (PRD 0029 RF5; 31e4449).
- Removing an account says when its stored web session was deleted too (PRD 0029 RF5; 31e4449).
- Every icon button has a name for screen readers, and keyboard focus looks the same everywhere (PRD 0022 RF13; 19e96b4).
- Secondary text and status words reach readable contrast in every built-in theme (PRD 0022 RF15.4; f99aaf7).
- Compact tray rows and card reset lines no longer cut long translations (PRD 0022 RF18.3; ab09599).
- Test connection of a LiteLLM proxy no longer mentions 9router in its result (PRD 0036 RF10.3; 888826f).
- The 9router and LiteLLM Test connection now checks the password, key or session you just pasted, shown in a masked field (PRD 0036 RF2.1; 27c09ae).
- The About list now credits the platform libraries Cuotai uses on macOS and Windows (b5dc144).
- Copilot limits read through 9router show "Premium requests" instead of the raw key premium_interactions (PRD 0022 RF4; 2b038ef).
- The General settings show their helper text, use a themed picker with a color swatch, and call the startup switch Open at login (PRD 0022 RF3; 2ce054b).
- The Add account picker groups providers by sign-in method and no longer highlights the first row on open (edf2913).
- The tray and the main window read closer to the design: clearer status lines, extra usage as its own column and no repeated limit messages (PRD 0022 RF18; 382d63e).
- Problem reports now say whether a provider incident was active when status queries are on (PRD 0028 RF2.2; 88af7d5).
- The terminal dashboard now records projected quota exhaustion in the event journal (PRD 0026 RF3.2; 734c418).
- The device check consent now says it also looks for environment variables (presence only) and a local 9router (PRD 0027 RF6.5; f4e0de6).
- ElevenLabs accounts now show when extra usage is off, reached its cap or cannot be determined (PRD 0037 RF5.5; 0fc186b).
- Extra-usage and projected-exhaustion alerts held by quiet hours now reach the digest, even after a restart (PRD 0032 RF3; ae6383e).
- The device check question now names everything Cuotai looks at: tool folders, whether environment variables are set, and a local 9router (PRD 0027 RF6.5; a5e53c3).
- Sign-in progress lines in the account dialog follow the interface language (PRD 0034 RF2; 37d9803).
- The desktop provider list now uses the same order as the terminal wizard (PRD 0027 RF3; 6c164a8).
- Typing a time in the quiet-hours schedule with the keyboard no longer restarts after each part of the time (PRD 0032 RF2; 77d55d1).
- Screen readers now name every usage meter and announce the provider tabs correctly (PRD 0022 RF13.1, RF13.2; cab337f).
- The extra spent in Stats is now marked as an estimate (≈) (PRD 0022 RF6.1; 82172e1).
- The Stats view uses a shorter Avg peak label, a compact 7d / 30d range toggle and neutral daily bars (PRD 0022 RF1; 0abc52f).
- The Stats header select and the KPI cards are more compact (PRD 0022 RF1; e31a621).
- Card notes, titles and session labels in the terminal dashboard wrap and truncate by display width, so accented, CJK and emoji text no longer clips early or overflows its room (b6ebb0e).
- The terminal dashboard footer wraps its keys onto extra rows at narrow widths, so every binding (including ? about and q quit) stays visible at 80 columns (ccba435).
- On terminals limited to the 16 ANSI colors the terminal dashboard uses the named colors of your scheme instead of RGB sequences the terminal ignores (9a7bd1f).
- The terminal dashboard now colours quotas green from 70% left and red below 30%, matching the desktop app (PRD 0022 RF2; 1f0418b).
- The German translation now follows the English wording in error, consent and provider messages (PRD 0035 RF11; fc44c4d).
- The Russian translation now follows the English wording in error, consent and provider messages (PRD 0035 RF11; 3bde308).
- The Indonesian translation now follows the English wording in error, consent and provider messages (PRD 0035 RF11; 08db552).
- The Turkish translation now follows the English wording in error, consent and provider messages (PRD 0035 RF11; 3c71438).
- The Vietnamese translation now follows the English wording in error, consent and provider messages (PRD 0035 RF11; 900f455).
- French texts use one word for the credential vault and for quota resets, and the updater privacy note is complete (PRD 0035 RF11.3, RF11.4; c0b703b).
- Japanese texts use one word for the credential vault and read more naturally in help and consent texts (PRD 0035 RF11.3, RF11.4; 219a7c0).
- Korean error and credential texts match the English meaning, and quota resets use one word everywhere (PRD 0035 RF11.3, RF11.4; e2fd352).
- Simplified Chinese texts use 您 only in errors and consent texts and one word for the credential vault (PRD 0035 RF11.3, RF11.4; 040dcd0).
- Hindi error and credential texts match the English meaning, and quota resets use one word everywhere (PRD 0035 RF11.3, RF11.4; 2ea64e2).
- Japanese and Hindi delete dialogs distinguish removing an account from deleting its stored credentials (PRD 0035 RF11.3, RF12.3; b3df75b).
- Typed consents in Hindi accept both हाँ and हां (PRD 0035 RF11.7; 3d352ef).
- An unsupported --fallback-locale now lists every language Cuotai ships (PRD 0035 RF2.2; 895324c).
- Pace and return chips in German, French, Turkish, Indonesian, Russian and Vietnamese use shorter wording that fits the chip (PRD 0035 RF9; 03eb1c5).
- The terminal dashboard draws the failure cross as an x in Chinese, Japanese and Korean so columns stay aligned (PRD 0035 RF6; bbf9317).
- Adding an account without a name no longer fails with an "account layout was rejected" error (1d23900).
- The Add-account picker now marks the AI tools found on this device even when you already have an account for that provider (PRD 0027 RF6; afe92b7).
- The Xiaomi account session help no longer points to a website that does not exist and explains where the cookie comes from (PRD 0016 RF2.1; 1d942f6).
- A Xiaomi MiMo account with a live token plan and no prepaid balance no longer shows as exhausted (072bedb).
- The name for your existing unnamed account is prefilled, explained and optional when you add a second account of the same provider (PRD 0029 RF3.3; PRD 0048 RF9, RF10; e2bb81d).
- The account dialog no longer says there is no credential to enter when the provider offers a web session, and presents that session as an alternative (PRD 0029 RF3.3; PRD 0048 RF9, RF10; e2bb81d).
- The Alibaba Token Plan option is called "Bailian CLI profile (bl)" and links the official install instructions when the bl CLI is missing (PRD 0029 RF3.3; PRD 0048 RF9, RF10; e2bb81d).
- Signing in to Qwen Cloud or Alibaba Coding Plan from Cuotai waits until the session is complete instead of saving one that fails as expired (PRD 0048 RF4.3; 77e3fc2).
- The Meta card reads this month's spend, requests and token totals from the dev.meta.ai console session (PRD 0049 RF8, RF9, RF10; 44bb352).
- Signing in to dev.meta.ai from Cuotai also works with Google or GitHub (PRD 0049 RF7; a53eac3).
- A Meta Model API account can save and use a dev.meta.ai session even without an API key (PRD 0049 RF8; e0446ff).
- Clearing part of a quiet-hours time with the keyboard no longer moves the focus away from the field (PRD 0032 RF2; 02b30e7).
- The status-bar recipes show the right status when an account name contains a tab or a line break (PRD 0031 RF1; 5de28e5).
- Adding an Alibaba Cloud account with a typed AccessKey ID and AccessKey Secret no longer fails the test before saving (PRD 0047 E3; 53e763f).
- The statistics no longer shrink earlier daily peaks when a quota limit is raised within the range (a86a386).
- The OpenRouter spending breakdown is no longer shown understated when the activity omits an amount (bacc2d2).
- Leaving an incomplete quiet-hours time now also removes the note that explained why it was not saved (PRD 0032 RF2; be2011d).

### Security

- Extensions are now refused when a folder above them, up to the extensions folder, can be changed by other users (PRD 0017 FIX-05; 10a0bfc).
- Provider and aggregator answers are read up to a fixed size, so an oversized or malicious body can no longer exhaust memory (dc3f8b0).
- The desktop windows now run under a Content Security Policy that blocks remote or injected scripts (e330ba8).
- After a refused 9router password, Cuotai no longer keeps a plain hash of it in memory (b886ca6).
- Account reports no longer include organization, user, device or subscription ids or e-mail addresses in their source or notes; Cursor no longer shows the account e-mail and name (PRD 0052 RF3.6; 69535e5).

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
- Release checksums are signed as a Sigstore bundle (`SHA256SUMS.sigstore.json`); see the updated verification steps (PRD 0021 RF3.2; 04cc112).

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

[Unreleased]: https://github.com/jmarcon/cuotai/compare/v0.82.3...HEAD
[0.82.3]: https://github.com/jmarcon/cuotai/compare/v0.82.2...v0.82.3
[0.82.2]: https://github.com/jmarcon/cuotai/compare/v0.82.1...v0.82.2
[0.82.1]: https://github.com/jmarcon/cuotai/compare/v0.82.0...v0.82.1
[0.82.0]: https://github.com/jmarcon/cuotai/compare/v0.81.0...v0.82.0
[0.81.0]: https://github.com/jmarcon/cuotai/compare/v0.80.0...v0.81.0
[0.80.0]: https://github.com/jmarcon/cuotai/compare/2bbb39a...v0.80.0
