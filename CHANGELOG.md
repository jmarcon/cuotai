# Changelog

All notable changes to Cuotai are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project follows [Semantic Versioning](https://semver.org/) (still `0.x`: a minor bump may carry a breaking change, marked **Breaking** with its migration).

Each entry says what changed for the user, followed by the plan requirement it meets (`PRD 0006 RF2`, see [`docs/plans/0000_control.md`](docs/plans/0000_control.md)) and the short hash of the commit that delivered it.

How to read the history:

- `0.80.0` is the first published version. Each release section is written once, by `cargo xtask release prepare`, from the `Changelog:` trailers of the commits since the previous release (PRD 0039). Its version is the one that release publishes, and its footer link compares release tags.
- Versions `0.1.0` to `0.79.0` were **internal builds**. Each was raised by the commit that delivered it, under the per-commit rules used before PRD 0039, and none was tagged or published. They are summarized in one **pre-release history** at the bottom of this file, grouped by area, instead of one section per version. `cuotai --changelog 0.5.0`, or any other internal version, prints that history.
- The per-version detail of the internal builds lives in the Git history: the commits that raised each version (`git log -G '^version = ' -- Cargo.toml`), the full log up to [`2bbb39a`](https://github.com/jmarcon/cuotai/commits/2bbb39a), the plans in [`docs/plans/`](docs/plans/), and the "Delivered by version" table in §3 of [`docs/plans/0000_control.md`](docs/plans/0000_control.md). A version that a plan cites from that period means "the tree state at the commit that raised it".
- Commits with no effect for the user (`refactor`, `test`, `style`, `docs`, `chore`) do not appear, except security fixes in dependencies.

## [Unreleased]

## [0.87.6] - 2026-11-20

### Fixed

- JetBrains AI shows one row per tool even when JetBrains lists a tool twice, and a credit total too large to read is left out instead of shown as infinite (PRD 0055 RF3.1, RF3.2, RF3.3; 320024c).
- A JetBrains AI account whose client ID variable is not set now says that the variable must hold the service account's client ID (PRD 0055 RF4, RF7; 9c75dbe).
- The hidden Google sign-in window is now erased 30 minutes after its last capture even when the computer slept in between (PRD 0056 RF2.3, RF2.4; b743799).
- In the account dialog, "Forget the Google sign-in" now shows as a link button on its own line and answers next to it, and a shared sign-in window that times out or is stopped is said to be hidden rather than closed (PRD 0056 RF1.3, RF2.4, RF6; d5ee005).

### Security

- When a capture ends, the hidden Google sign-in window now erases each site's stored data as well as its cookies, keeping only Google's cookies (PRD 0056 RF3, RF6; PRD 0048 RF2.3; 3c24457).

## [0.87.5] - 2026-11-20

### Fixed

- About › What's new on the desktop and `cuotai --changelog` show each entry without its plan references and commit hashes, as the update notes already did (PRD 0053 RF7.4; PRD 0019 RF7.7; f2d89e0).
- When Repair under Open at login fails, the login entry stays as it was and the switch shows what the system reports, instead of reading on with no entry left (PRD 0054 RF4.1, RF4.2, RF4.3; 16797a9).
- The notice shown when the tray icon cannot be created keeps a space after its icon, and the Open at login description says the app opens in its window when there is no tray (PRD 0054 RF5.1, RF4.2; 6dc2faf).
- The `cuotai --doctor` report says in your language where it found the `aws` or `bl` program, instead of an internal word such as `well_known` (PRD 0054 RF6.3; ec2d38a).
- A damaged hook claims file is now kept aside and no hook runs from it, so the desktop app and `cuotai --watch --hooks` cannot both run the same hook after that file was damaged (PRD 0054 RF2.1, RF2.3; 1c2ba17).

## [0.87.4] - 2026-11-20

### Changed

- The `--once`, `--json` and `--json-v1` commands now exit with 2 when an account is skipped because this version does not know its provider or one of its keys (PRD 0051 RF1.2; 73f008b).

### Fixed

- A setting this version does not know inside [updates] or [[groups]] is now ignored with a notice instead of stopping every account (PRD 0051 RF1.3, RF1.6; 1c1bb50).
- The terminal dashboard shows how many accounts it skipped and how to check the file, even in a narrow window (PRD 0051 RF2.5; f61b66e).
- Closing the desktop's offer to reuse a moved file's history now only hides it until the next start; Keep apart declines it (PRD 0051 RF5.4, RF2.5; aad57aa).
- An unknown setting in the accounts file now shows as information in the desktop app instead of as an error (PRD 0051 RF5.4, RF2.5; aad57aa).
- The configuration check now says when a file is not UTF-8 text, and German texts about newer versions use the formal address (PRD 0051 RF2.1; 8305098).
- Erasing all data also removes Cuotai's lock and temporary files, so no empty Cuotai folder is left behind (PRD 0052 RF1.3, RF3.2, RF3.3, RF4.1, RF5.2; d7add92).
- Erasing all data in the desktop app now also clears the app's own window storage, turns off Open at login and removes leftover web-session capture profiles, so nothing comes back at the next login or start (PRD 0052 RF3.3, RF4.1, RF5.2; PRD 0054 RF4; 917411a).
- The privacy inventory now lists the app's window storage, the Open at login entry and the web-session capture profiles, and `cuotai --erase-all-data` names them as erased only in the desktop app (PRD 0052 RF3.3, RF4.1, RF5.2; PRD 0054 RF4; 917411a).
- The erase confirmation word is accepted however the keyboard composes its accented letters, and the German erase prompt uses the formal form (PRD 0052 RF3.2, RF3.4, RF5.3; 0fd4f67).
- Check setup says that its copied report names your environment variables, never their values (PRD 0052 RF3.2, RF3.4, RF5.3; 0fd4f67).

### Security

- Cuotai never deletes or narrows files in a data folder it did not create: a newly chosen data folder (in Settings or with CUOTAI_DATA_DIR) now holds Cuotai's files in a Cuotai folder inside it (PRD 0051 RF5.2, RF5.3; 882d9c8).
- If you set CUOTAI_DATA_DIR to an existing folder that does not hold Cuotai's data yet, Cuotai now keeps its files in a Cuotai folder inside it, so point backups and scripts at that subfolder; a folder Cuotai already uses, or a path that does not exist yet, is still used as it is (PRD 0051 RF5.2, RF5.3; 882d9c8).
- Copies of a damaged desktop settings file are now readable only by you (PRD 0051 RF4.3; 5b71caa).
- Erasing all data from the terminal now also erases the data folder chosen in the desktop app and any data left in the default folder, and removes the desktop settings last so a failed erase can be retried (PRD 0052 RF5.2, RF5.3; PRD 0051 RF5.2; 648f890).
- The setup check (`cuotai --doctor`) and the app log no longer pass control or text-direction characters from a configured path or an error to the terminal or the log (PRD 0052 RF1.3, RF3.2, RF3.3, RF4.1, RF5.2; d7add92).
- Erasing all data now refuses while the Cuotai app, `cuotai --serve`, `cuotai --watch` or the terminal dashboard is running, and names what to quit first (PRD 0052 RF5.2, RF5.3; d06d7e0).

## [0.87.3] - 2026-11-20

### Fixed

- A Meta Model API account that uses only the dev.meta.ai session now shows an error, and waits as the console asks after a rate limit, when the console returns no usage, instead of showing up to date with nothing read (PRD 0049 RF9, RF8; f0662cf).
- The Meta Model API account no longer offers a Project ID, because the dev.meta.ai console reports usage per team; an existing Project ID is ignored and the card says so (PRD 0049 RF8; 9a931fe).
- Meta Model API spend is read only from the console's amount field, so an unexpected answer leaves the spend unknown instead of showing it 100 times too small (PRD 0049 RF9; 2db19f0).
- Without a local-server token, the web browsers switch now asks you to generate one instead of opening its warning, and Erase log explains why it is unavailable when nothing was recorded (PRD 0050 RF4.1, RF4.4, RF6.3; ff4ed6b).

### Security

- The terminal's `--erase-all-data` now honours `--dev-credentials-sqlite`, clearing only that development store, and a release build refuses the flag instead of erasing the system credential vault (PRD 0052 RF5.3, RF5.4; PRD 0050 RF7.3; PRD 0012 RF5; 38c46f4).
- Settings that erase data, change a consent or privacy choice, or start a sign-in now act only when asked from the window that shows them, and every new desktop command must declare the windows that may call it (PRD 0050 RF7.1; PRD 0020 RF2.5; PRD 0001 C1; b085485).
- The desktop windows can no longer call the menu, tray, image and other Tauri functions they never use (PRD 0050 RF1.3; PRD 0003 S-06; 219618f).

## [0.87.2] - 2026-11-18

### Fixed

- A key that is missing from the system vault is now called a missing key, not a missing web session, and the desktop opens the account on the key to type it again (PRD 0047 G1, G4, G5, G7; fb33339).
- Saving an account with an empty key in the desktop shows Cuotai's own message in your language instead of the system's (PRD 0047 G1, G4, G5, G7; fb33339).
- The terminal wizard suggests readable variable names for account names with accents, and editing an account whose secret is in the system vault no longer writes a variable it would ignore (PRD 0047 T2, E1, E4; 5f79e15).
- In the account dialog's web-session box, "Sign in and capture session" and its explanation are no longer shown in bold, the session field is announced by its title alone, and the messages after a failed capture point to the box above (PRD 0048 RF1, RF7.2, RF8; PRD 0056 RF6; fc635a2).

### Security

- A key pasted by mistake where a variable name belongs is no longer repeated in notes, `--once`, `--json`, the setup check or the restore plan; that account says its variable name is not valid, and every other account keeps working (PRD 0047 G6, T3, T4, E5; f48e4ea).
- Moving a key into Cuotai or forgetting a web session now acts only on the account the dialog is editing, even if the accounts changed meanwhile, and only from the main window; a restore closes open account dialogs (PRD 0047 G5; PRD 0016 RF4.2; PRD 0001 C1; 7fec5e7).
- A page in the sign-in window can no longer open arbitrary addresses in your browser: only that capture's own sign-in pages open there, at most one a second, and only the main window can start or cancel a capture (PRD 0048 RF3, RF5; PRD 0056 RF5; PRD 0001 C1; a8184d1).
- The Alibaba Coding Plan sign-in window of each region loads only that region's console, not the other region's (PRD 0048 RF3.4; 9684901).

## [0.87.1] - 2026-11-18

### Fixed

- The "Others" row of a breakdown shows the real total of the rows it folds, without one key's budget or reset date, and an account that breaks down by two kinds (projects and keys, or keys and teams) lists and folds each kind on its own (PRD 0045 RF1.2, RF3, RF4, RF5.1, RF6.1; 4cff2e2).
- Breakdown rows without a name, the card's "Top" line amounts and the note about a breakdown that failed now follow your language and number format, and the note names the kind that failed (PRD 0045 RF6.1, RF6.2; e19c1a0).
- The account form's Breakdown choices are round checks in a framed group (no longer stretched bars in the Win98 and XP themes), and breakdown rows show their amount without a "Usage" prefix (PRD 0045 RF6.1, RF6.3; 5220bd9).
- Test connection of a LiteLLM proxy reads only the budgets route again, even with a breakdown ticked, and the TUI marks a key or team near or at its budget (PRD 0045 RF3, RF6.2, RF6.3; b5399b4).
- The desktop restore preview lists the keys, sign-ins and settings still to do before you restore, and refuses a backup file that changed after the preview (PRD 0046 RF3.1, RF3.3; PRD 0050 RF9; b46577d).
- A restore writes through a linked accounts or hooks file instead of replacing the link, a backup without a trustworthy date asks before replacing a file changed here, an unknown old format is reported as damaged rather than newer, and `--restore --json` prints only JSON (PRD 0046 RF1.1, RF3.1, RF3.2; e66d387).
- The backup and restore dialogs have their intended width again, with unchanged files muted and long paths wrapping inside the dialog (PRD 0046 RF2.1, RF3.3; d753cb0).
- German backup, restore and setup-check texts now address you consistently as "Sie" (PRD 0046 RF3.3, RF4.2; PRD 0035 RF11.5; 8c24ae0).

### Security

- A LiteLLM team whose id is "others" can no longer pass for the breakdown's sum row and drop out of history, alerts and budgets (PRD 0045 RF1.2, RF3, RF4, RF5.1, RF6.1; 4cff2e2).
- Breakdown row names hide a key even when it is glued to other text (key=sk-…, AWS access keys, tokens), and drop invisible zero-width and direction-mark characters (PRD 0045 RF1.3; 938490e).
- A restore no longer turns on the local server, browser access, update checks, device detection, widgets or their e-mails, status pages, notifications, alerts or hooks: each keeps this computer's choice, and the plan lists every one the backup had on (PRD 0046 RF1.2, RF3.1, RF3.3; PRD 0018 RF2.6; PRD 0027 RF6.5; PRD 0050 RF4.1; PRD 0032 RF7.1; 059f8fb).
- A restore lists each account that would send a key to another address (a proxy address or a program path) and goes ahead only after a specific confirmation: `--accept-hosts` in the command line, a separate checkbox in the desktop (PRD 0046 RF3.1, RF3.3; PRD 0050 RF9; b46577d).
- A crafted backup can no longer hide or reorder lines of the restore plan with terminal escapes or hidden direction characters, and a restore refuses theme and language files without their extension, names Windows cannot hold and names that differ only in case (PRD 0046 RF1.1, RF3.1, RF3.2; e66d387).
- A backup also stops when a file holds a Slack or Discord webhook address, a Bearer token, an xAI or Google token, an AWS secret key or a password inside an address, and it now says Cuotai found no key rather than that the file holds no secret (PRD 0046 RF1.3; 7913096).

## [0.87.0] - 2026-11-18

### Added

- Each macOS widget can now be edited to choose its accounts or quota, the order, what is left or what was used, a compact layout, hiding idle accounts, the reset time, the age line and, for History, the period (PRD 0014 RF3.2, RF3.3, RF3.4; 5c3e9a3).

### Changed

- The macOS widgets show what is left of each quota in the same colours as the app, with the quota's name, when it renews and one age line, and without the Cuotai name on the widget (PRD 0014 RF3.2, RF3.3, RF3.4; 5c3e9a3).
- A History widget asks for its account again once after this update (PRD 0014 RF3.2, RF3.3, RF3.4; 5c3e9a3).

### Fixed

- The 24-hour strip under the menu-bar popover's accounts draws the bars of the hours with use; only the hours without readings showed before (PRD 0043 RF4.2; b0f0b88).
- The `cuotai --digest --week current` command counts only this week's limit hits and extra spending; it included the days before Monday (PRD 0043 RF3.1, RF3.2; 5e199e3).
- The Rhythm charts, `cuotai --stats --rhythm` and the digest's busiest hours no longer put the use of a gap in the readings into the hour after it (PRD 0043 RF1.1, RF1.2; 97f375a).
- Limit-change notes no longer say "limit" twice for limit metrics, and the limit-change date, the digest's busiest hours, the digest hour picker and the text Rhythm chart follow your language's date, hour and column format (PRD 0043 RF2.2, RF3.1, RF5; 4cef036).
- Send a test digest is unavailable, like Send a test notification, while every notification is off (PRD 0043 RF4.3; 202e746).
- The `cuotai --channel-add` command accepts `--yes`, so a channel can be added with the address piped on standard input, as the guide shows; it was always refused (PRD 0044 RF5.2, RF5.3; fa61d80).
- A notification channel limited to an aggregator account also receives the alerts of that aggregator's connections; it received none (PRD 0044 RF1.3; 81fa9c6).
- The notification channels' delivery log, Send test errors and channel summaries are written in your language, the default filters read "All kinds", and "webhook" is no longer capitalised mid-sentence (PRD 0044 RF3.3, RF4.1, RF5.1, RF5.2; f9c87ef).
- The notification channels page lines its texts up with its rows, shows round checkboxes in the channel editor (and the hooks editor), says why Add channel is unavailable at five channels, and gives Slack and Discord their own icons (PRD 0044 RF5.1; 462db1c).
- Adding a notification channel shows the address's port and an example of a notification the channel will actually receive (PRD 0044 RF4.1, RF5.1; 7b8c181).
- A failing notification channel no longer delays the other channels while it waits to retry (PRD 0044 RF3.1; ffac551).
- The macOS widgets no longer draw the age over other lines or at the widget's edge, and long lists no longer cut off their last line (PRD 0014 RF3.2, RF3.3, RF3.4; 5c3e9a3).
- Widgets on the desktop now read the same published data, so an Account widget no longer shows an older reading than the Summary beside it (PRD 0014 RF3.2, RF3.3, RF3.4; 5c3e9a3).

### Security

- A notification channel limited to some accounts receives the weekly digest of those accounts only; it received every account's figures and names before (PRD 0043 RF3.2; PRD 0044 RF6.1; 319a811).
- A restored history label with hidden text-direction characters is refused, so it cannot reorder the digest, limit-change notes or channel messages (PRD 0043 RF2.2, RF3.1; 780a84c).
- The terminal no longer shows the channel address you paste into `cuotai --channel-add`, so it stays out of the scrollback (PRD 0044 RF5.2; 9186195).
- Notification channels scrub e-mail addresses, web addresses and key-shaped text from metric labels and from every message text, not only from the account name field (PRD 0044 RF2.2, RF6.1; 3ad8710).
- The desktop accepts new, changed, switched or tested notification channels only from the Settings window, where the consent is shown (PRD 0044 RF4.1, RF4.3, RF7; 18e9f3d).

## [0.86.10] - 2026-11-18

### Fixed

- A budget keeps counting after the provider resets its counter inside the budget period, and says when part of the period's use was not recorded (PRD 0041 RF2.2, RF2.3, RF4.1; 47ee346).
- Two budgets on the same metric get their own names and alerts, and each balance floor warns on its own, also when it is set above the current balance (PRD 0041 RF1.2, RF2.1, RF2.4, RF3.1; 03d4649).
- Budget notes, the budget period and the most restrictive limit line read in your language, with dates in your format (PRD 0041 RF2.1, RF2.4, RF2.5; 010148e).
- A budget in another currency than its metric is refused with both currencies named, instead of reading as a missing metric (PRD 0041 RF1.2; e9f1737).
- The budget and floor editors read amounts the way your language writes them, refuse one such as 1,000 that could be read two ways, and show each problem under its field (PRD 0041 RF1.2, RF5.1, RF5.2; e9d0ad2).
- The `--cost-plan` and `--cost-currency` options refuse an amount such as 1,000 instead of saving 1 (PRD 0041 RF1.2, RF5.1, RF5.2; e9d0ad2).
- A budget keeps its own name in the desktop details and carries the ◇ mark on the terminal card (PRD 0041 RF2; 5c78484).
- An invalid budget or floor is left out with a note on its account instead of stopping every account from loading (PRD 0041 RF1.2, RF3.2; 711ca26).
- The daily allowance says "less than" instead of 0 when little is left, shows nothing once a budget is used up, and gives a money budget's recent pace in money (PRD 0041 RF4.1, RF4.2; 2d22282).
- The live block line and its projection now include the cost of models without a known price as "+ unknown cost", and project only tokens when most of them have no price (PRD 0042 RF3.3, RF6.2; 6cf0081).
- The live block's amounts and the burn-rate message read in your language and say how many recent requests were seen (PRD 0042 RF3.3, RF6.2; 6cf0081).
- Hiding a project now leaves it out of every local cost total, in the terminal, the desktop and `export::v1`, and each view says how many projects are hidden (PRD 0042 RF4.1, RF6.1, RF7; 3de1254).
- Folder names and project dates now follow the index's retention (PRD 0042 RF4.3, RF1.1, RF7; 9587bca).
- Log lines dated in the future are no longer stored in the local cost index (PRD 0042 RF1.1; ee64ea1).
- The output of `cuotai --cost-sources` now shows the size of the local cost index (PRD 0042 RF1.1; ee64ea1).
- The session list of `cuotai --cost --by session` names each session's project by its label, keeps a labelled session's time, and explains the code in brackets (PRD 0042 RF2.2, RF3.2, RF5.1, RF5.2, RF5.3; 1cbcde7).
- Blocks and sessions that cross midnight show their end date, and the active block is listed first everywhere (PRD 0042 RF2.2, RF3.2, RF5.1, RF5.2, RF5.3; 1cbcde7).
- The live view of `cuotai --cost-live` redraws without filling the scrollback and exits 2 for a source that is off, and `--top` is refused where it does not apply (PRD 0042 RF2.2, RF3.2, RF5.1, RF5.2, RF5.3; 1cbcde7).
- The desktop's sessions, blocks and projects page fits every language, shows each session's name and code, lists older sessions on request, and announces only a changed live line to screen readers (PRD 0042 RF6.1, RF6.2; 0c81f8f).

### Security

- Budget names, units and filters, including budgets on breakdown rows, can no longer carry invisible direction characters into the outputs (PRD 0041 RF1.2, RF3.2; 711ca26).
- Turning the folder-name hint off now also stops a scan already running from keeping names, and removes them from the pre-upgrade copy of the index (PRD 0042 RF4.3, RF1.1, RF7; 9587bca).
- The `cuotai --cost --by project --json` output no longer prints folder names (PRD 0042 RF4.3, RF1.1, RF7; 9587bca).
- Turning a local cost source off removes the folder names it kept and turns its hint off (PRD 0042 RF4.3, RF1.1, RF7; 9587bca).
- Session and project labels and folder names can no longer carry invisible or direction characters (PRD 0042 RF4.2; 78a4dcd).
- The desktop's local cost breakdown, labels, hidden projects and source options can only be read or changed from the Settings window (PRD 0042 RF6.1; ab147fb).
- Session, request and log-file fingerprints in the local cost index are keyed to this computer, as project fingerprints already were (PRD 0042 RF1.1; 0cd98e0).

## [0.86.9] - 2026-11-17

### Added

- The new `--feed claude-statusline --forget` and a Settings button erase the stored Claude Code readings (PRD 0040 RF4.3, RF1.2; 396d6cf).
- The export and `/v1/limits` add `reading_age_seconds`, the age of the reading itself (PRD 0040 RF3.2; 8b006ac).

### Fixed

- A new Claude Code window or Codex session that has not reported its limits yet no longer blanks the account, and a hybrid account falls back to the query (PRD 0040 RF3.4, RF4.1, RF5.1; a02e657).
- The status line of a new Claude Code window shows the account's last known windows with their age (PRD 0040 RF3.4, RF4.1, RF5.1; a02e657).
- The Codex CLI consent now says it also covers the rate-limit windows, so it is asked again (PRD 0040 RF4.3, RF1.2; 396d6cf).
- Adding a Claude or ChatGPT account asks for the source first, skips the credential for a feed mode, and asks before it opens the browser to sign in (PRD 0040 RF3.5; de67df2).
- The `--feed claude-statusline` and `--statusline-claude` commands give up on a silent input after a quarter of a second and print nothing, so a prompt is never held (PRD 0040 RF1.3, RF2.3; a9df27d).
- The `--doctor` report and the account details count feed files that could not be read (PRD 0040 RF1.3, RF2.3; a9df27d).
- Turning off Show live agent session now stops the sessions from being computed and from being shared by the local servers (PRD 0040 RF7.2, RF1.1, RF8.1; fb024e1).
- Claude Code readings live in one fixed folder, so a desktop with a moved data folder sees them (PRD 0040 RF7.2, RF1.1, RF8.1; fb024e1).
- The Claude Code snippet in Settings says it needs the terminal version and uses its full path when Cuotai finds it (PRD 0040 RF7.2, RF1.1, RF8.1; fb024e1).
- A Claude Code or Codex CLI reading shows its real age and reads stale on the cards, in details, in the menu bar, in the widget, in `--once` and in `--status-line` (PRD 0040 RF3.2; 8b006ac).
- The `--status-line` and `--statusline-claude` commands and the menu bar show an old or failed reading's age in parentheses instead of a dot that looked like the separator (PRD 0040 RF5.1, RF5.2, RF5.3; 75f29f4).
- The `--statusline-claude --account` option takes an id, a key or a unique name like `--status-line`, and an unknown value is reported (PRD 0040 RF5.1, RF5.2, RF5.3; 75f29f4).
- A Claude account fed by Claude Code, or one that fell back to the query, says on its card which source gave the figure (PRD 0040 RF3.4, RF6.1; 5667290).

### Security

- Turning the Codex CLI local source off erases the Codex rate-limit readings too, and nothing shows or exports them without that consent (PRD 0040 RF4.3, RF1.2; 396d6cf).

## [0.86.8] - 2026-11-17

### Fixed

- A provider outage, maintenance or redirect now says so and suggests trying again later, instead of asking you to check access and permissions (PRD 0037 RF7; 1134672).
- A usage figure a provider reports as minus zero now shows as zero (PRD 0037 RF3; b757230).
- A metric without a limit now says why: "no cap" when the provider states it, the amount alone for money spent, or "limit not reported" (PRD 0037 RF5; PRD 0038 RF4; 7d34b64).
- A usage counter that is full, such as every voice slot taken, now reads "all in use" instead of "exhausted" (PRD 0037 RF5; PRD 0038 RF4; 7d34b64).
- The single-refresh output says when a spending cycle ends instead of calling it a renewal (PRD 0037 RF5; PRD 0038 RF4; 7d34b64).
- The account wizard's Review step names a chosen option by its label instead of its raw value (PRD 0037 RF5; PRD 0038 RF4; 7d34b64).
- An incident on one ElevenLabs region's status page now shows only on that region's accounts, and its link opens that region's page (PRD 0037 RF8; PRD 0011 RF2; 421dc8c).
- A quota whose reset time had already passed when the provider answered now reads "renewal due" instead of "blocked until" a past time, and is no longer suggested as the best account (PRD 0037 RF4; PRD 0033 RF2; f145747).
- A plan the provider reports with a zero allowance and nothing left now reads as exhausted instead of available (PRD 0037 RF3; PRD 0038 RF4; 0d5460c).
- Metric labels, units and periods that some providers reported in Portuguese (ElevenLabs voices, Meta usage, Ollama, Alibaba plans, Kimchi budgets, Vertex AI and others) now read in your language (PRD 0037 RF6; PRD 0034 RF2; 32c7960).
- Counted units such as credits, voices and requests take the singular or plural form their number needs in every language (PRD 0037 RF6; PRD 0034 RF2; 462c48a).
- ElevenLabs accounts say that extra usage is reported in money, list everything Cuotai reads, use the subscription currency when the extra-usage currency is missing, and explain an unknown currency (PRD 0037 RF2, RF5; PRD 0036 RF8; d5ebda4).
- Searching the settings for LiteLLM finds the aggregator connections row (PRD 0037 RF2, RF5; PRD 0036 RF8; d5ebda4).
- A prepaid balance now appears in the history charts and summaries, and a credits budget on a balance counts what was spent instead of saying there is no baseline (PRD 0038 RF9.3; PRD 0041 RF2.3; PRD 0006 RF3, RF6; cc170f4).
- A prepaid balance shows its amount, with its sign, as the main figure instead of a dash and "left", an exhausted balance says how much is left, and currency amounts on the desktop read as money (PRD 0038 RF5.1, RF5.5; 054ab7c).
- When a provider asks Cuotai to wait, the account says when it is queried again instead of promising the next refresh (PRD 0038 RF7.1; bcd08bb).
- The desktop details show each metric's period after its name, and a daily limit that resets at midnight UTC says so when your time zone is not UTC (PRD 0038 RF4.1; 694d0ed).
- OpenRouter's BYOK spend and budgets with an unnamed scope are labelled in your language (PRD 0038 RF2.3, RF3.2; PRD 0034 RF2; 9fe148f).
- OpenRouter's spend by provider or model is shown in credits, the unit of the balance, instead of dollars (PRD 0038 RF6.2, RF5.3; b041809).
- OpenRouter accounts explain a refused key, insufficient credits, a redirect and a bad low-balance threshold, and keep long model names apart (PRD 0038 RF1.4, RF6.2, RF7.1, RF9.2; 545fc09).
- When you add an OpenRouter account, Cuotai asks for the product first and then for the matching key, with the right page to create it (PRD 0038 RF1.3, RF7.2; fe3be32).
- Cuotai refuses to save OpenRouter's inference-key variable for the account balance, and an expired management key points to the management-keys page (PRD 0038 RF1.3, RF7.2; fe3be32).
- OpenRouter's texts and docs now say which key each view needs, what a management key can do if it leaks, and that every browser sign-in creates a new key you should delete on openrouter.ai when you no longer use it (PRD 0038 RF14.1, RF8.2; 2d4158a).

### Security

- Key, project and model names from a provider can no longer carry hidden direction or format characters into breakdown rows, model scopes or diagnostics (PRD 0038 RF6.2, RF6.4; PRD 0045 RF1.3; 2bcdafe).

## [0.86.7] - 2026-11-17

### Fixed

- Search and filters treat letters such as ø, ð and ħ the same way in the desktop and the terminal (PRD 0035 RF1.3, RF10.5; f70c4d1).
- In the terminal details view, a long account name is shortened with an ellipsis so the provider always stays visible (PRD 0035 RF6; afe9b88).
- The note about a session that belongs to several Meta teams uses the correct plural in every language (PRD 0035 RF4; aea4b92).
- Before the window loads, the tray menu already uses your chosen language (PRD 0035 RF1; ed2889a).
- The language, sort and theme menus work with the keyboard and open inside the window (PRD 0035 RF3.3, RF7; 44a2eeb).
- Every flag with a light band keeps its outline on light themes (PRD 0035 RF7; a96388f).
- German, Chinese, Japanese and Korean texts use one consistent form of address (PRD 0035 RF11; 4249949).
- Russian, Korean, Chinese, German, Vietnamese and Hindi texts use the same word for the same concept everywhere (PRD 0035 RF11; 0def01d).
- French punctuation no longer breaks across lines, and Japanese texts use full-width colons (PRD 0035 RF11; 328534d).
- Readings from 9router and LiteLLM are now bounded and checked: oversized answers are cut with a note, repeated connections are listed once, impossible values are hidden instead of shown, and long labels end with an ellipsis (PRD 0036 RF3.2, RF3.3, RF4.4; e388758).
- Quota labels from 9router are shown in your language, counts of connections and providers use the right singular or plural form, a reached limit without details no longer shows "0 left", and LiteLLM configuration errors name LiteLLM (PRD 0036 RF3.3, RF3.5, RF10.1; 4094d17).
- An exhausted pool's return time now reads in your local time and clock format instead of an ISO UTC timestamp (PRD 0036 RF6.3; 0196882).
- The terminal dashboard lists an aggregator's pools one per line on its card and in its details, and the one-shot output groups connections by upstream under their pool (PRD 0036 RF9.1; ef09e1c).
- Aggregator views name their own aggregator (a LiteLLM proxy no longer reads as 9router), show percentages in your language's format, link a duplicate connection to its direct account and list each instance's sign-in, encryption and last connection read (PRD 0036 RF5.3, RF8.3, RF8.5; 1b206c1).
- The menu bar, the tray summary, the status line and the local server's limits summary now count each active 9router or LiteLLM connection, so a setup with only aggregators no longer shows a dash (PRD 0036 RF5.3, RF7.2; 1af3474).
- When a pinned 9router certificate changes, Test connection shows the old and new fingerprints in a dialog of its own, and replacing the pin takes a deliberate second step on the desktop and in the terminal wizard (PRD 0036 RF2.3, RF8.7; 2ddef6b).
- Adding a 9router or LiteLLM account works with the defaults: a 9router password is no longer prefilled, the 120 s refresh floor is shown and checked where the interval is edited, and each Test connection result is shown at the field it is about (PRD 0036 RF2.1, RF2.2, RF4.1, RF8.7; 9de2175).

### Security

- A language pack the app cannot read safely is refused and named, instead of reaching the interface unfiltered (PRD 0035 RF12.3; PRD 0050 RF3.3; 6d3f61f).
- A language pack can no longer change the words of confirmations, consent, billed-query warnings, sign-in or security dialogs; those keep the shipped translation or English (PRD 0035 RF12.3; PRD 0050 RF3.3; 8500540).
- A user language pack is marked as such in the language selector and can no longer take the name of a built-in language or replace the English fallback (PRD 0035 RF12.3; PRD 0050 RF3.3; bc8a23f).
- Before it sends the 9router dashboard password or session, Cuotai now checks that the address answers like 9router and, on this computer, that the program listening on its port is yours, and the private-network HTTP option says the password or key travels unencrypted (LiteLLM now honours that option) (PRD 0036 RF3.1, RF2.3, RF3.8, RF10.1; 26079ab).
- A 9router or LiteLLM instance or a notification channel on a local or private address is now reached directly and never through a proxy set in the environment, so a password or key can no longer reach that proxy unencrypted (PRD 0036 RF2.3, RF10.1; 3f7b476).

## [0.86.6] - 2026-11-17

### Fixed

- AWS Bedrock never shows the cost of a previous profile or of another configuration's account after a profile edit (PRD 0033 RF5; 161ddba).
- The desktop app, the terminal and --watch share one record of billed AWS Bedrock queries even when the data folder is moved, and erasing all data removes it (PRD 0033 RF5; b9fdce4).
- Scripted commands such as --once, --json and --status-line never make a billed AWS Bedrock query when the shared record of billed queries cannot be used, and report why instead (PRD 0033 RF5; 03b6a56).
- The `--watch --interval` option no longer makes an AWS Bedrock account query more often than its own interval, hourly by default (PRD 0033 RF5; 199042d).
- The extra query after a quota reset runs at most twice an hour per account, at least five minutes apart (PRD 0033 RF1; d65d283).
- A card or tray row with a renewed window now says when it renewed instead of showing a dash (PRD 0033 RF2; 60e296b).
- When the binding limit has no pace, a card shows the pace of its other window, named (PRD 0033 RF4; 92f9276).
- Refreshing an AWS Bedrock card by hand asks first, makes a billed query at most every 15 minutes, and otherwise says when the next one is possible (PRD 0033 RF5; 4492493).
- The AWS Bedrock interval field says that a blank interval queries once an hour and that the real minimum is 15 minutes, with the cost warning next to it (PRD 0033 RF5; 3e94f38).
- The menu bar, the connection rows and the reset timeline no longer show a quota that has renewed since the last reading as exhausted (PRD 0033 RF2; 24ac863).
- A Claude Code or Codex CLI reading written long ago no longer reads up to date, and every card and the menu bar say "data as of" when the provider's data is older than the query (PRD 0033 RF3; 010a2d1).
- The terminal dashboard no longer shows "renews now" under a quota that has already renewed (PRD 0033 RF2; 9af182e).
- Two programs reading the same AWS Bedrock account at the same moment no longer pay for two queries in one interval (PRD 0033 RF5; d0c992b).
- A name or value that contains text in braces now shows exactly as written in desktop and terminal messages (PRD 0034 RF2, RF7; 34f34b2).
- The macOS widgets show their texts in all 14 interface languages and follow the language chosen in the app (PRD 0034 RF7; 90f84f0).
- A language pack whose file name is not a valid language tag is now refused instead of breaking number and date formatting (PRD 0034 RF4, RF6; ae74935).
- The Azure resource-ID error in German, Spanish, Indonesian, Italian, Russian, Turkish and Vietnamese no longer puts the field name at the end of the example path (PRD 0034 RF4; d2f99e2).
- Lists of names and "label: value" texts in the desktop app use each language's own punctuation and list words (PRD 0034 RF7; 235b94e).
- The --fallback-locale help lists every language it accepts (PRD 0034 RF3, RF5; 130c804).
- Counted texts such as hidden metrics, window lengths and deleted extension secrets use the correct plural form in every language (PRD 0034 RF4, RF2; 18dc3f4).
- Numbers stay on the same line as their units, and the account filter's hint fits its field in every language (PRD 0034 RF4, RF7; a2890ec).
- Numbers, percentages and the refresh time in the terminal dashboard, --once output and desktop app follow your language's separators and percent style (PRD 0034 RF2, RF7; 724dc3e).
- Reset times in the terminal dashboard are never cut in the middle; a narrow card shows a shorter form (PRD 0034 RF2; 007fd39).
- Metric names and units in the macOS widget, the desktop History panel, desktop amounts and the --watch usage line follow your language instead of showing the provider's internal labels (PRD 0034 RF2, RF7; 2308ac5).
- Rate-limit labels, extra-usage amounts in credits and percentages follow your language in more places (PRD 0034 RF1, RF7; dd72744).

### Security

- Plan names and other provider texts shown in account notes can no longer carry invisible or text-direction characters that reorder or hide what you read (PRD 0034 RF5; a109679).

## [0.86.5] - 2026-11-16

### Fixed

- In Settings › Integrations, keyboard focus stays on the control you changed, and "Share usage statistics" stays reachable and says why it cannot be turned on yet (PRD 0031 RF11; d01e030).
- The local server's `ETag` never repeats after a restart or a settings change, and `127.0.0.1` and `::1` share the six `/v1/stats` requests a minute (PRD 0031 RF6, RF11; 4b9d145).
- The local server's `/v1/limits` marks an account whose reading failed with `failure`, and `cuotai --status-line --from-server` then exits 2 like the line without a server (PRD 0031 RF2, RF4; 0f7eb40).
- The status line's `--account` accepts an account's id, key or unique name, queries only the chosen accounts, and stops with a list of ids when a value matches none (PRD 0031 RF2; d6234d5).
- The status-bar recipes show the share left, such as `45% left · Personal`, with the menu bar's colours, and `/v1/limits` adds `remaining_ratio`, `level` and `label` to `summary.tightest` (PRD 0031 RF1, RF4; d6b14b6).
- The status line counts every account it leaves out, the help and the server's messages read correctly in every language, `--reset-lead` refuses values above 1440 or without `--watch`, and `--serve --strict-exit` says why it exits 2 (PRD 0031 RF2, RF10, RF12; 50dda44).
- The quiet-hours editor counts days in the right plural, shows times in your language's clock and starts the week on your language's first day (PRD 0032 RF2.3; 667d8b5).
- The quiet-hours editor refuses duplicate, overlapping and empty intervals on the field itself, and says when the schedule never ends (PRD 0032 RF2.3; 667d8b5).
- Notifications that list several held alerts or the weekly summary show one item per line again, ending with "and N more" when some are left out (PRD 0032 RF3.3; PRD 0043 RF4.3; 8ba23d6).
- The notification that sums up alerts held during quiet hours keeps its whole title in every language (PRD 0032 RF3.3; 16b73d6).
- Erasing all data also removes the notification lock file and any copy of it a crash left behind, so the data folder is removed too (PRD 0032 RF4.2; 7e8e08b).
- The summary sent when quiet hours end no longer repeats a quota whose exhaustion was already notified (PRD 0032 RF3.3; 247c6e5).
- The cuotai --notify banner that closes quiet hours lists the held alerts, and its clock follows the language's 12- or 24-hour format (PRD 0032 RF6; 93f57a8).
- Quiet-hours times with a sign or non-ASCII digits are refused, and a schedule with no gap ends at most a week ahead (PRD 0032 RF2.1; c73eb5e).
- The macOS widgets draw their content again on macOS 27.2 instead of a grey placeholder, an empty card or a squeezed chart (PRD 0014 RF3.2, RF3.3; 2ea1a27).

### Security

- The status line and the Claude Code status line read with `--from-server` send the server token only after the server proves it holds it, so a program squatting on the port no longer receives it; update the server first, since an older one is refused (PRD 0031 RF2; 51488c0).
- The status lines read from a local server drop control and text-direction characters from account names, so a program on the port cannot change the terminal's title, write the clipboard or reorder the line (PRD 0031 RF2; 7c9e682).
- A broken or planted notification lock file no longer slows down every refresh, and its link target is never touched (PRD 0032 RF4.2; 382183e).

## [0.86.4] - 2026-11-16

### Fixed

- The macOS widgets show your usage again on macOS 27.2 instead of staying as grey placeholders (PRD 0014 RF3.2, RF3.5; 0ac8beb).
- A hand-edited configuration error now names the account table and says whether a name is missing or repeated, and an id line that copies another account's computed id is refused at load instead of merging the two accounts (PRD 0029 RF1.2, RF1.3, RF3.2, RF3.4; 6ffda6a).
- Saving the configuration no longer writes default refresh, concurrency and timeout settings you never set (PRD 0029 RF1.2, RF1.3, RF3.2, RF3.4; 6ffda6a).
- An unnamed extension account shows its extension's name instead of its id (PRD 0029 RF1.2, RF1.3, RF3.2, RF3.4; 6ffda6a).
- The terminal wizard says a name is optional only when it is, and unnamed accounts are no longer shown as an empty name or with their provider twice in the removal prompt, the cards, --once and --vault-audit (PRD 0029 RF3.4, RF3.5; 09d986c).
- The Add account form no longer shows the field for naming your existing account, or an empty Breakdown box, when they do not apply, and keeps what you typed in that field when you switch providers (PRD 0029 RF3.3, RF3.4, RF3.5; f8a4ad8).
- The account name field says it is optional only when it is (PRD 0029 RF3.3, RF3.4, RF3.5; f8a4ad8).
- Unnamed accounts are shown by their provider once everywhere, instead of an empty name or the provider twice (PRD 0029 RF3.3, RF3.4, RF3.5; f8a4ad8).
- Invalid saved plan prices or second currency are ignored with a note instead of showing infinite or negative amounts (PRD 0030 RF1, RF6; PRD 0046 RF3; 67e7f8e).
- A model repeated in pricing.json is now an error, and --doctor checks pricing.json (PRD 0030 RF1, RF6; PRD 0046 RF3; 67e7f8e).
- When several accounts share one local cost source, the plan line is combined, shown only on accounts with a plan, and gives no ratio unless every account has a plan in one currency and on one cycle day (PRD 0030 RF1, RF3, RF4, RF6; 8aee5f2).
- Local cost amounts, plan prices and converted amounts follow your language's number and currency format (PRD 0030 RF1, RF3, RF4, RF6; 8aee5f2).
- A small estimate reads as a share of the plan price instead of 0.0×, and a converted amount keeps its unknown part (PRD 0030 RF1, RF3, RF4, RF6; 8aee5f2).
- Local cost texts with counts use real plurals, and the exchange rate says when it was set (PRD 0030 RF1, RF3, RF4, RF6; 8aee5f2).
- The terminal shows a plan line only on the account that has the plan, puts the rate line under the period, and lists missing prices and price age per source (PRD 0030 RF1, RF2, RF4, RF6; 4c5daaf).
- The --cost-plan option refuses a name that matches no configured account instead of saving a plan nobody sees (PRD 0030 RF1, RF2, RF4, RF6; 4c5daaf).
- The desktop shows a plan line only on the account that has the plan, keeps the unknown part in converted amounts, and puts each price table's age on its own line (PRD 0030 RF1, RF3, RF4, RF6; f4ef857).
- The Plans form in Settings has visible labels and shows prices in your language's money format (PRD 0030 RF1, RF3, RF4, RF6; f4ef857).

### Security

- The status-bar recipes and the local-server examples no longer show the server token in the process list or send it through a proxy (PRD 0031 RF1; be150db).
- A restore no longer turns local cost sources or folder names on; it leaves them off and says to turn each source on on this computer (PRD 0030 RF1, RF6; PRD 0046 RF3; 67e7f8e).

## [0.86.3] - 2026-11-16

### Security

- Removing unused stored credentials now deletes only entries Cuotai can prove unused, keeps extension secrets, notification channels and other configurations' sign-ins, and deletes exactly the list you confirmed (PRD 0029 RF6; 9f441cb).

## [0.86.2] - 2026-11-16

### Fixed

- Gemini AI is now named Gemini CLI, after the sign-in that shows its numbers (PRD 0027 RF1.2, RF6.5, RF7.2; b2310be).
- The device check says exactly what each found tool's sign-in reads, including Cursor's local import and the AWS profile step (PRD 0027 RF1.2, RF6.5, RF7.2; b2310be).
- Provider search finds names typed in full-width characters, in Japanese, Chinese or Korean without spaces, and with Turkish capitals (PRD 0027 RF2.1; 9af2709).
- In the terminal, the device-check question wraps instead of being cut off and names only the checks the terminal makes (PRD 0027 RF4.1, RF6.5, RF7.2; c83b314).
- The terminal shows what a found tool's sign-in reads only while that sign-in is the chosen mode (PRD 0027 RF4.1, RF6.5, RF7.2; c83b314).
- The --list-providers aggregator mark follows the interface language (PRD 0027 RF4.1, RF6.5, RF7.2; c83b314).
- The Add-account search results show each sign-in method and hint again, and a found tool's note is visible in its row (PRD 0027 RF3.1, RF3.2, RF7.2, RF7.3; 6f7e0d6).
- The note about what a found tool's sign-in reads shows only while that sign-in is the chosen mode, with "Don't suggest this again" next to it (PRD 0027 RF3.1, RF3.2, RF7.2, RF7.3; 6f7e0d6).
- The Add-account search field has a single clear button, counts results in the right plural, and Enter no longer picks a provider when none is highlighted (PRD 0027 RF3.1, RF3.2, RF7.2, RF7.3; 6f7e0d6).
- When a provider changes its answer, LiteLLM, OpenRouter, Azure, Alibaba billing, model lists and extensions now name the field that changed, and LiteLLM no longer shows such an answer as up to date (PRD 0028 RF1.1, RF1.2, RF1.4; ef0df2f).
- Problem reports and drift details no longer show object keys that look like account or user ids (PRD 0028 RF1.1, RF1.2, RF1.4; ef0df2f).
- Problem reports now show how long ago the account last read well in the terminal too, and say "unknown" instead of "none" when the command line cannot tell (PRD 0028 RF1.3, RF2.2, RF2.3; cd3a402).
- Problem reports name the real connection mode, list only the reported account's setup problems, and the About report opens the bug form (PRD 0028 RF1.3, RF2.2, RF2.3; cd3a402).
- In the terminal, a provider that changed its answer shows a "What changed" block in the details and in --once, and the problem report opens in the browser with o (PRD 0028 RF1.3, RF2.4; 80db6ea).
- The text around the terminal's problem report now follows the interface language (PRD 0028 RF1.3, RF2.4; 80db6ea).
- The desktop details show a "What changed" block with the changed fields when a provider changes its answer, and a report too long for a link can still open the issue form (PRD 0028 RF1.3, RF2.3; 4a3ca9c).
- The desktop opens a problem report on GitHub only with the text Cuotai built, never with text from the page (PRD 0028 RF1.3, RF2.3; 4a3ca9c).

### Security

- The local canary no longer writes into, prunes or changes the permissions of a folder of other files, and an edited record can no longer put hidden characters into its report (PRD 0028 RF4.1; d4b8bef).

## [0.86.1] - 2026-11-16

### Fixed

- The menu bar writes return and reset times on another day as the weekday or date, in your clock format, and shows the provider instead of an account name unless you chose the accounts (PRD 0025 RF1, RF7.1; 8fe3c96).
- The menu bar's Icon only style shows a ring of your tightest limit, Meters is visible on Windows and Linux, the outage and alert badge shows in every style, the colored variant stays readable on light and dark bars, and account names with "&" show correctly in the right-click menu (PRD 0025 RF3, RF7.1; 6e53633).
- An account you switch off in the dashboard no longer shows in the menu bar, and your menu-bar account selection survives switching accounts files (PRD 0025 RF2.2, RF2.3, RF6.6; 41212c7).
- The row display no longer leaves empty lines, applies to the main window's cards, and Settings › Menu bar previews exactly what the menu bar draws; the popover's More menu works with the keyboard (PRD 0025 RF2.3, RF4, RF6, RF9; eb57a16).
- When the history file reaches its size cap, the oldest event-journal entries are trimmed together with the oldest readings, and one unreadable journal entry no longer hides the others (PRD 0026 RF3.3, RF1; 38e0fcd).
- The --stats --rhythm chart shows hours in your language's 12- or 24-hour clock and dates in its order (PRD 0026 RF2, RF4; 94aca14).
- The stats screen, --stats, --timeline and --events-log show dates in your language's order and group events by day, show "—" for limit hits without enough history, and --events-log --json adds projected_at, spent, currency and skipped (PRD 0026 RF6; 21d3388).
- Returns more than six days away show their date in the reset timeline, Stats shows times in your language's clock, today's bar keeps its highlight at the limit, recent events are grouped by day, and the Stats view keeps keyboard focus (PRD 0026 RF4, RF5; 825ff2a).
- The menu-bar Stats panel no longer breaks words or wraps its range toggle, its day labels stay inside the chart, keyboard focus moves into and back out of it, and the Windows 98/XP themes no longer draw chart bars as meters (PRD 0026 RF4; 6b8996f).

## [0.86.0] - 2026-11-15

### Added

- The menu-bar popover has a Cuotai button at the top left that opens the full window (PRD 0022 RF7.3; c932ca1).

### Fixed

- What's new now shows only the improvements of the version you are running, with a link to the full changelog on GitHub (PRD 0019 RF7.7; dafdf30).
- Times in the dashboard, notices and --once follow your language's clock and show a weekday or a short date instead of an ISO date (PRD 0024 RF1.2, RF1.3, RF1.5, RF6.1, RF7.3; 09dce54).
- The pace line no longer says "On pace" next to a projection that runs out before the reset, and marks a pace taken from recent readings (PRD 0024 RF1.2, RF1.3, RF1.5, RF6.1, RF7.3; 09dce54).
- The terminal shows the reset countdown in your language and suggests a better account only when the recent pace really runs out (PRD 0024 RF1.2, RF1.3, RF1.5, RF6.1, RF7.3; 09dce54).
- The desktop details hide "On pace" next to a projection that runs out before the reset, follow the "Show pace as" choice and mark a pace taken from recent readings (PRD 0024 RF1.3, RF1.5, RF6.1, RF7.3; 24988ea).
- A long account name in the best-account advice no longer squeezes the sentence (PRD 0024 RF1.3, RF1.5, RF6.1, RF7.3; 24988ea).
- A notice that an account is running out or exhausted also names the best account of its group, unless notices are private (PRD 0024 RF7.3; 5817d14).
- The terminal app refuses --notify-projected together with --once, --json or --history instead of ignoring it (PRD 0024 RF1.3, RF3.4; 4a182b0).
- The German, Russian and Chinese pace texts and the "Show pace as" example read more clearly (PRD 0024 RF1.3, RF3.4; 4a182b0).

## [0.85.12] - 2026-11-15

### Fixed

- Extra usage at its cap now reads "Extra budget used up" in the critical colour in the terminal dashboard and `--once`, instead of "on extra" (PRD 0023 RF1, RF2, RF3, RF8; d592ca8).
- Extra usage and budget amounts in the terminal, notifications, the weekly digest and event summaries use the number format of your language (PRD 0023 RF1, RF2, RF3, RF8; d592ca8).
- Claude and Kiro extra-usage metric names are translated in every language (PRD 0023 RF1, RF2, RF3, RF8; d592ca8).
- Kiro extra usage reported as 0 shows as on with nothing spent instead of unknown, and its allowance left no longer goes below zero (PRD 0023 RF1, RF2, RF3, RF8; d592ca8).
- Extra usage at its cap now reads "Extra budget used up" in the critical colour on desktop cards, the status line and the menu bar, instead of "on extra balance" (PRD 0023 RF7; df3b53a).
- Extra-usage amounts on desktop chips, tooltips and details use the number format of your language, like the figures beside them (PRD 0023 RF7; df3b53a).
- A Claude extra-usage amount in provider units is no longer cut off on the card, and armed or unknown extra usage now shows a chip (PRD 0023 RF7; df3b53a).

## [0.85.11] - 2026-11-15

### Fixed

- Status chips, links, the authentication label and stale figures are readable in every theme, and Win98 light shows the reset timeline on a card (7ee845a).
- An exhausted account reads "Exhausted" in the tray as in the window, hide buttons in the details name their limit, and Stats keeps the plain track of retro themes (PRD 0022 RF7; 8ccf6b7).

### Security

- Names and texts from hooks, language packs and providers are always shown as plain text, and the app's windows can no longer be navigated away from the app (0dcdc4c).

## [0.85.10] - 2026-11-15

### Security

- The update notice and the desktop updater show a Cargo command that installs the new release from its GitHub tag, because Cuotai is not on crates.io yet (PRD 0021 RF5.4, RF8.1; 2f5299b).

## [0.85.9] - 2026-11-15

### Fixed

- Settings descriptions wrap instead of being cut off, every field follows the theme, and multi-choice checks show a check mark (PRD 0020 RF3, RF5, RF9; e92aca8).
- Settings › About shows the update privacy note as one line with the full text a click away, and the update channel in your language (PRD 0020 RF2, RF3; PRD 0018 RF3.1; 67bfbeb).
- Settings shows refresh presets, theme names and counts correctly in every language, and local cost names with symbols display as typed (PRD 0020 RF5; 2146339).
- Settings search lists the closest match first, and keyboard focus follows the result you choose (PRD 0020 RF6, RF4.3; 4f3c6a6).
- Erase all Cuotai data now names the saved sign-ins and the local server token in plain words, with the vault service name underneath (PRD 0020 RF3; c323d4b).

### Security

- Renewing or revoking the local server token and deleting widget data now ask for confirmation in a system dialog (PRD 0020 RF3.4; dbda9c7).
- Destructive actions are refused when they come from a window that does not offer them, such as the menu bar popover (PRD 0020 RF3.4; dbda9c7).

## [0.85.8] - 2026-11-15

### Fixed

- The `cuotai --changelog` and `cuotai --help` commands end quietly when piped into a command that stops reading early (PRD 0019 RF7.7; 4b6c2d2).
- The output of `cuotai --version` names the program `cuotai` (PRD 0019 RF7.7; 4b6c2d2).
- The update banner shows the release notes as headings and lists, without internal references (PRD 0019 RF1.1, RF7.7; 335c7ab).
- Settings › About shows the project licence as MIT OR Apache-2.0 (PRD 0019 RF1.1, RF7.7; 335c7ab).

## [0.85.7] - 2026-11-14

### Changed

- The privacy inventory now names the update check's User-Agent and no longer calls the release manifest signed (PRD 0018 RF3.3; 9102530).

### Fixed

- The menu-bar popover shows a single scroll bar again, inside its account list (de4edcb).
- An update check or download that gets no answer now gives up after a short wait instead of stopping later checks (PRD 0018 RF2.1, RF2.3, RF3.2; fb14ff8).
- A failed background update check no longer puts a red alert on the dashboard, and a failed download can be retried (PRD 0018 RF2.1, RF2.3, RF3.2; fb14ff8).
- Settings › Updates lets you turn update checks on or off before you answered the first-run question, and updater messages are translated in every language (PRD 0018 RF2.4, RF2.6, RF3.1, RF3.3; dd5c271).

## [0.85.6] - 2026-11-11

### Fixed

- On Windows, an extension whose program has a same-named .exe beside it is refused, so the file the disclosure shows is the one that runs (PRD 0017 RF3.1; dfd26c7).
- The terminal's extension consent screen now warns when the extension asks for another account's credential variable, as the desktop does (PRD 0050 RF8.2; 552d370).
- Approving a changed extension again keeps its account's name, group, budgets, floors and refresh interval, and says the account was kept (PRD 0017 RF4.2; 93b5c04).
- Extension and LiteLLM accounts now show their provider name in your language (PRD 0017 RF5.1; 7928fc0).
- An extension that needs approval now points to the Extensions screen, not only to a terminal command (PRD 0017 RF5.1; 7928fc0).
- The terminal's extension list marks an enabled extension whose last run failed, without selecting it first (PRD 0017 RF5.1; 93e96ec).
- The privacy inventory now says exactly which environment values an extension receives (PRD 0017 RF5.2; fa4f2b5).

### Security

- An extension folder linked from elsewhere is now refused when any folder above it, not only its parent, can be written by other users (PRD 0017 FIX-05; PRD 0050 RF8.1; e818ada).

## [0.85.5] - 2026-11-11

### Fixed

- An account that reads its usage through a web session no longer needs an API key, and pasting a new session no longer adds one (PRD 0016 RF2.1, RF4.2; b6ed021).
- After "Forget this session", an account that reads its usage through a web session waits for a new session instead of reading the provider's default API key variable (PRD 0016 RF2.1, RF4.2; b6ed021).
- The session paste preview no longer says the session is saved before it is tested and saved (PRD 0016 RF2.1, RF4.1; e2eaeef).
- Session messages in the desktop app no longer mention the terminal wizard or the TOML file (PRD 0016 RF2.1, RF4.1; e2eaeef).
- The terminal dashboard shows the whole cause of a failed web session on its card, and "Paste a new session" opens the session that failed (PRD 0016 RF4.1, SES-01, SES-02; 5d609f9).
- A pasted web session too large to send is refused with a clear message instead of being stored (PRD 0016 RF2.2, RF1.3; b16d779).
- A credential too large for Windows Credential Manager now says so instead of reporting the vault as unavailable (PRD 0016 RF2.2, RF1.3; b16d779).
- Settings files that use Cuotai's internal credential names are refused (PRD 0016 RF2.2, RF1.3; b16d779).

## [0.85.4] - 2026-11-11

### Fixed

- Local cost now stays off when it is turned off at the same moment as another change, counts every line again after the index is erased during a background read, counts a final log line without a newline once the file is quiet, and reports an empty log folder as no logs (PRD 0015 RF1.1, RF1.4, RF2.1; 8e32462).
- The local cost row and tray line show "≈" only before an amount, never before "no usage" or "cost unknown", and the Settings › Local cost status line sits inside its group (PRD 0015 RF4.1; 99ad4c7).
- The local cost span of a single day reads "Last day" instead of "Last 1 days", with proper plural forms in every language (PRD 0015 RF4.2; d002349).

### Security

- Erasing the local cost index or turning a source off now also deletes the copy of the index kept before an upgrade, and that copy is removed after 7 days (PRD 0015 RF2.4; 537bb00).
- A log line with impossible token counts is now skipped as invalid instead of breaking the local cost figures, and an unreadable cost index is shown as an error in --cost, --json-v1 and the desktop app instead of looking like every source is off (PRD 0015 RF1.3, RF4.3; d7e435c).
- Turning on a local cost source in the desktop app now asks for consent in a system dialog, outside the app's window content (PRD 0015 RF1.1; PRD 0050 RF2; 9df429a).
- Project fingerprints in the local cost index and its exports are now keyed to this computer, so a guessed folder path can no longer be confirmed from them; the short project codes change once (PRD 0015 RF1.3; PRD 0042 RF1.1; b39aa2d).

## [0.85.3] - 2026-11-11

Version 0.85.2 was never published because a release check failed before publishing. This release ships its changes, repeated here.

### Fixed

- The macOS Account widget no longer repeats the main quota in place of another one, the Account and History widgets show the right empty or unavailable state, and an extreme reading can no longer crash or blank the widgets (PRD 0014 RF1.1, RF3.2, RF3.5).
- The macOS widgets keep showing your data after you move the data folder, and turning widgets off, deleting widget data or erasing all data now removes every copy of the widget file (PRD 0014 RF2.1, RF2.2).
- The macOS widgets use correct singular and plural forms, translate the account picker and show the provider's name instead of its internal id (PRD 0014 RF3.4).

## [0.85.2] - 2026-11-11

### Fixed

- The macOS Account widget no longer repeats the main quota in place of another one, the Account and History widgets show the right empty or unavailable state, and an extreme reading can no longer crash or blank the widgets (PRD 0014 RF1.1, RF3.2, RF3.5; 8178564).
- The macOS widgets keep showing your data after you move the data folder, and turning widgets off, deleting widget data or erasing all data now removes every copy of the widget file (PRD 0014 RF2.1, RF2.2; a645b42).
- The macOS widgets use correct singular and plural forms, translate the account picker and show the provider's name instead of its internal id (PRD 0014 RF3.4; 606de49).

## [0.85.1] - 2026-11-10

### Fixed

- Hook rule errors and the test confirmation now appear in the hooks Rules page instead of staying hidden (PRD 0013 RF4; 0bcd7e9).
- Hooks no longer receive a made-up provider for notifications that are not about an account (PRD 0044 RF6; 6ea0b3b).
- The --watch help now says SIGHUP reloads the configuration, the --hooks help lists CUOTAI_ACCOUNT_ID, and the interval error also covers values that are too large (PRD 0013 RF2; db8ad85).

### Security

- Hooks refuse a program, interpreter or hooks.toml that another user can change, and scripts run from a private verified copy so a file swapped after approval never runs (PRD 0013 RF3; PRD 0050 RF2.4; e0831a7).
- The hooks confirmation can no longer be disguised by control characters in file names or rules, and account names with control characters are refused (PRD 0013 RF3; 07547b9).

## [0.85.0] - 2026-11-10

### Fixed

- Danger buttons such as Revoke token and Erase log are readable again in the Windows 98, Windows XP and GitHub dark themes (PRD 0022 RF15.5; 5b2fe10).
- The port-taken message no longer repeats its advice, and the local-server help texts match what the server does (PRD 0012 RF4.1, RF5.2; 14fbbf2).

### Security

- The local server now holds its port on both loopback addresses and --from-server reads localhost as 127.0.0.1, so another user of the computer can no longer pose as the server and capture its token (PRD 0012 RF3.1; PRD 0031 RF2.1; 6f962b4).
- A port whose IPv6 loopback side is already taken now counts as taken, and the local server reports it instead of starting (PRD 0012 RF3.1; PRD 0031 RF2.1; 6f962b4).
- **Breaking**: The desktop local server now always requires its access token; a server that ran without one gets a token on its next start, and Settings › Advanced › Integrations says where to copy it. Migration: scripts that read the desktop local server without a token now get 401; copy the token with Settings › Advanced › Integrations › Copy token and send it as Authorization: Bearer <token> (PRD 0012 RF5, RF3.2; PRD 0052 RF4; e37526e).
- Requests the local server refuses no longer use up the rate limit of clients that send the token, and slow half-open connections can no longer block them (PRD 0050 RF5; PRD 0012 RF2.4; 1a4073c).

## [0.84.11] - 2026-11-10

### Fixed

- Provider incident titles and extension texts no longer keep invisible text-direction characters that could reorder the words around them (PRD 0011 RF2.3; 3547c7b).
- The privacy inventory now states the User-Agent status requests send, that cuotai --canary --status also reads status pages, and where the queried domains are listed (PRD 0011 RF2.1; 8ee045f).
- The local canary (cuotai --canary --status) reads each provider status page once per run instead of twice (PRD 0011 RF2.2; fc95a2c).
- The desktop no longer calls a status page that was never read a "previous reading", and Settings › Data & privacy lists the queried status domains right under the status switch without repeating the heading (PRD 0011 RF4.1, RF4.3; 6403180).
- On narrow TUI cards the provider status badge leaves out the reading age whole instead of cutting it mid-word (PRD 0011 RF5.2; cf8cba3).

## [0.84.10] - 2026-11-10

### Fixed

- After you change an account's key, login or provider, Cuotai no longer shows the old reading as its last valid one, and a reading rejected by the provider no longer returns after a restart (PRD 0010 RF2.1, RF2.3, RF2.4; 0aaa421).
- The terminal dashboard no longer cuts words in half on narrow cards and counts accounts with the right plural in every language (PRD 0010 RF5.1, RF5.2; dbccb76).
- An account that is already paying for extra usage no longer appears under "Recharging", and a stale card shows its age once and no longer shows "Waiting" as an error (PRD 0010 RF3.3, RF4.1; 69991ee).
- A stale reading whose time lies ahead of your clock is now marked as clock adjusted (PRD 0010 RF1.2; 811f3db).

## [0.84.9] - 2026-11-10

### Fixed

- An impossible reset time from a provider no longer crashes the terminal app, --once or the desktop app; the reset reads as unknown (PRD 0009 RF1; ff86368).
- Limit summaries in the terminal and in --once now name every limit in your language, never round a limit that is not reached up to 100% and show return dates in your locale's format (PRD 0009 RF3, RF4; c976e65).
- The desktop app names every binding limit in your language and never shows a limit that is not reached as 100% or a limit with a sliver left as 0% (PRD 0009 RF2; 687ec71).

## [0.84.8] - 2026-11-10

### Fixed

- Usage alerts on the desktop now name limits in your language, show a bell chip on the account card, and keep the full alert text in the tray tooltip and the screen-reader name (PRD 0008 RF3.4; PRD 0034 RF2; eea2b44).
- Usage alert notifications now fit in every language, name unnamed accounts, and show reset dates in your language; the terminal banner shows reset times in the same time zone as its clock, and `--watch` summaries and the desktop journal name limits in your language (PRD 0008 RF2.3; PRD 0013 RF2.3; 6b2615f).
- Usage alerts no longer repeat when a provider's reset time drifts, an alert held back by the one-minute spacing is no longer lost when Cuotai quits, and notice history written by a newer Cuotai is no longer discarded by an older one (PRD 0008 RF1.4, RF2.1; PRD 0051 RF3; 1e3af4e).

### Security

- With private notification texts on, extra-usage and shared-pool alerts no longer show the account name or the amount spent, and alerts for limits without a known window no longer show the limit's name (PRD 0008 RF2.3; PRD 0023 RF4.5; PRD 0036 RF6.4; PRD 0032 RF7; 6b2342f).

## [0.84.7] - 2026-11-09

### Fixed

- The consumption pace of a Claude Code or Codex CLI reading is now computed at the time of that reading, and it is hidden once the reading is no longer fresh (PRD 0007 RF1; PRD 0024 FIX-11; PRD 0040 RF3; c3d166d).
- Inside the on-pace band the pace chip now reads On pace instead of a small point count, and a terminal card drops the chip instead of cutting it when it does not fit (PRD 0007 RF4.1; PRD 0024 RF1.1; fd3fda1).
- The --once output now prints the pace under each aggregator connection and shows reset times in local time (PRD 0007 RF4.2; PRD 0036 RF9.1; a7f1bb6).
- The tray and the account details now use the same pace arrows as the cards and the terminal, and pace tooltips no longer repeat the stage (PRD 0007 RF3.2, RF3.3; 7b36a10).
- Claude's 5-hour and weekly limits are now named in your language instead of Portuguese, in the app and in notifications (PRD 0034 RF2; PRD 0007 RF2.1; e08dab3).
- A Claude Code or Codex CLI reading dated in the future is now ignored instead of being taken as the newest (PRD 0040 RF1.3; 700c06e).

## [0.84.6] - 2026-11-09

### Fixed

- History exports and backups are written through a private temporary file and refuse to overwrite Cuotai's own history, configuration or data files (PRD 0006 RF4; PRD 0052 RF5; PRD 0046 RF2; e26a182).
- The --history-export option accepts only --history-days from 7 to 365 instead of crashing on a huge value (PRD 0006 RF4; PRD 0052 RF5; PRD 0046 RF2; e26a182).
- The copy of the history kept before an upgrade is removed after 7 days, counts in "On disk" and yields to the size cap (PRD 0006 RF4; PRD 0051 RF3; c46d1dc).
- Erasing everything also works when the history file is damaged, and an erasure that another open Cuotai window keeps from wiping the disk says so (PRD 0006 RF4; PRD 0051 RF3; c46d1dc).
- History charts and sparklines of metrics without a limit start at zero, so a small change no longer looks like a full rise (PRD 0006 RF5, RF6; 87fbef0).
- A metric with no measured use shows a sentence instead of empty axes, and history dates read like "22 Oct, 18:10" (PRD 0006 RF5, RF6; 87fbef0).
- The terminal history heading is spaced and its spans are translated in every language, --history shows its end time in local time, and a metric without measured values says so (PRD 0006 RF6; 18a8385).

### Security

- The desktop app now chooses export, backup and restore files only through its own system dialog (PRD 0050 RF9; PRD 0006 RF4; 5a523e7).

## [0.84.5] - 2026-11-09

### Fixed

- The "interface may have changed" message no longer embeds a web address; the account details link the reference instead (desktop) or list it (terminal) (PRD 0005 RF4.2; 5e31b62).
- Japanese and Chinese text on terminal cards wraps between characters instead of cutting product names (PRD 0005 RF4.2; 5e31b62).
- Searching the Add account list for a provider Cuotai cannot query now says why it is not offered instead of suggesting a new-provider request (PRD 0005 RF3.4; PRD 0027 RF3.5; 3df1372).
- The "Save anyway" prompt for an account without a usage query is shown as a warning, not an error (PRD 0005 RF3.4; PRD 0027 RF3.5; 3df1372).

### Security

- The bundled SQLite is updated to a release that fixes a memory disclosure with crafted write-ahead-log files (RUSTSEC-2026-0459) (a81201d).

## [0.84.4] - 2026-11-09

### Fixed

- The terminal dashboard is readable on light terminals: it detects the background through COLORFGBG (or CUOTAI_TUI_PALETTE=light|dark), and gauge percentages stay legible on dark terminals (PRD 0004 RF4; PRD 0022 RF16; 8ea7854).
- Refresh buttons for 9router instances and connections announce "Refreshing…" while they work, including with reduced motion (PRD 0022 RF12.4; PRD 0004 RF2; 000727d).
- A user theme with a negative or oversized animation duration is refused instead of breaking dialogs (PRD 0050 RF3.1, RF3.2; 3dbd821).

## [0.84.3] - 2026-11-09

### Fixed

- A malformed rate-limit or token lifetime from a provider now fails only that account instead of stopping the whole refresh (PRD 0033 RF1.2; PRD 0055 RF2; cb32741).
- The tray's next-notice line now accounts for quiet hours and always shows the time in full (PRD 0003 RF3.4; PRD 0032 RF3.1; 370f9a4).
- Reset notices now show metric names in your language, name unnamed accounts correctly and fit the system's notification length (PRD 0003 RF2.2; PRD 0034 RF2; PRD 0032 RF7; f5841b7).
- Settings › Alerts no longer clips the Notify label in narrow windows (PRD 0003 RF2.2, RF3.1; PRD 0050 RF1.1; 6b7faf2).
- Reset and usage notices are no longer repeated when the notice record cannot be read or was written by a newer version (PRD 0003 RF2.3; PRD 0032 RF4.2; PRD 0051 RF3.2; ab7b483).
- The terminal's reset banner keeps the account name visible in narrow terminals (PRD 0003 RF4; 30062b9).

## [0.84.2] - 2026-11-09

### Fixed

- The reason a provider cannot list models no longer says twice that nothing is queried (PRD 0002 RF4.2; 5925381).
- The Sync models button no longer spills out of the tray in longer languages, and a failed model sync now stands out from a fresh catalog with the usual date format (PRD 0002 RF6.2; 6648538).
- Signing in from the account dialog now starts the CLI at the program path typed in the form (PRD 0054 RF6.1; 5c93aa3).
- Dashboard cards in the terminal show the account's note in the interface language instead of English (PRD 0035 RF5.1; 58012bc).
- The terminal Models view no longer offers to sync an account that cannot list models, and keeps the catalog on screen when saving a sync fails (PRD 0002 RF5.2, RF3.1; 73fd512).

### Security

- OpenAI, Anthropic and Cursor Teams accounts no longer send their admin key to the model list, and Mistral, xAI and Perplexity Computer accounts list models only when that key is kept in its own parameter; the Models section explains why (PRD 0002 RF4.2; PRD 0028 RF10.1; 8a966a6).

## [0.84.1] - 2026-11-08

### Changed

- The desktop app names providers the same way as the terminal and the menu bar, and NVIDIA NIM is spelled as the vendor spells it (PRD 0001 RF5; 19bee1e).

### Fixed

- Saving, removing or signing in to an account no longer changes a different account when the configuration file was edited elsewhere while the editor was open (PRD 0001 RF1, RF3; c18590a).
- Configuration and sign-in errors are shown in the chosen language in the terminal and the desktop app (PRD 0001 RF8; 24a9330).
- The account editors flag a refresh interval below the minimum of a provider billed per query on the interval field itself (PRD 0001 RF4, FIX-08; ec04735).
- When switching how an account signs in replaces an option, the explanation now appears next to that option (PRD 0001 RF11; 81dd29a).
- Account and channel names with symbols such as & are no longer shown with escape codes in some desktop messages (PRD 0001 RF8; 5db909d).

## [0.84.0] - 2026-11-08

### Added

- The desktop Stats view and the "Limits come back" timeline remember the range you last picked (PRD 0026 RF4, RF5; af18f4b).
- The terminal account wizard has a Budgets step to add, edit and remove budgets and balance floors (PRD 0041 RF5; bfe68bc).
- Set a budget... prefills the amount from your plan price in the same currency and shows what the last 30 days cost (PRD 0041 RF6; PRD 0043 RF4; e7df5f2).
- The macOS menu bar can show the last 24 hours of the tightest account as hourly bars, and the tray popover shows that chart on every system (PRD 0041 RF6; PRD 0043 RF4; e7df5f2).
- Clicking the weekly digest notification opens the Stats view (PRD 0041 RF6; PRD 0043 RF4; e7df5f2).
- The budget editor offers only the metrics the account's provider reports (PRD 0041 RF5; 9ef33a3).
- Testing a 9router connection warns when the instance is older than the oldest version Cuotai was tested with, once that version is recorded (PRD 0036 RF2.2, RF3.8; 5b81bfd).
- AWS Bedrock and the Alibaba plans accept an optional CLI path, Cuotai looks for aws and bl in the usual install folders when the app is not started from a terminal, and cuotai --doctor shows where each program was found (PRD 0054 RF6, RF3; 4486740).
- Accounts can be edited, added, moved and deleted in a file that also holds accounts a newer Cuotai wrote; those accounts are kept exactly as written (PRD 0051 RF1, RF2; 714a73a).
- A newer Cuotai's notice ledger, hook rules and widget snapshot are kept intact by this version, a damaged notice ledger is set aside without overwriting an earlier one, and cuotai --doctor reports it (PRD 0051 RF3, RF4; PRD 0003 COMP-02; c9b8960).
- When the accounts file was moved or renamed, Cuotai offers to carry over its history once the accounts match exactly, and the desktop app can keep its data in a folder chosen in Settings (PRD 0051 RF5; 4f6c8f7).
- You can send Cuotai's notifications to a webhook, an ntfy topic, Slack or Discord (Settings › Alerts › Notification channels or cuotai --channel-add), with the address kept in the system vault (PRD 0044 RF1, RF2, RF3, RF4, RF5, RF6, RF7; cd8fad8).
- Hooks can react to the new notification_sent event when a notification channel accepts a notification (PRD 0044 RF1, RF2, RF3, RF4, RF5, RF6, RF7; cd8fad8).
- You can back up and restore your settings in the desktop app (Settings › Advanced › Files) or restore a backup from the first screen, with next steps for the keys and sign-ins to redo (PRD 0046 RF1, RF2, RF3, RF4, RF5, RF6; PRD 0047 G1; b92dae3).
- Backups can include the last 90 days of history with --with-history, and a restore merges it into this computer's history (PRD 0046 RF1, RF2, RF3, RF4, RF5, RF6; PRD 0047 G1; b92dae3).
- The `cuotai --doctor` check also covers the system vault, extension approvals, local cost sources and the clock, and `--doctor --probe` checks that each provider host answers without sending a credential (PRD 0046 RF1, RF2, RF3, RF4, RF5, RF6; PRD 0047 G1; b92dae3).
- A card that needs setup links Check setup, and problem reports include the setup check's findings (PRD 0046 RF1, RF2, RF3, RF4, RF5, RF6; PRD 0047 G1; b92dae3).
- Backups carry your notification channels without their addresses, and a restore lists each address to type again (PRD 0046 RF1, RF2, RF3, RF4, RF5, RF6; PRD 0047 G1; b92dae3).
- The desktop's setup check speaks of keys instead of environment variables (PRD 0046 RF1, RF2, RF3, RF4, RF5, RF6; PRD 0047 G1; b92dae3).
- The first ten breakdown rows of an account are kept in the history, can alert when you turn on "Alert on breakdown rows", can carry a budget of their own, and their events name the row by scope and label (PRD 0045 RF5; PRD 0030 RF2.3; be15ec8).
- When Add account finds Claude Code, Codex CLI or Gemini CLI, it also offers to open Local cost to estimate that tool's cost from its logs (PRD 0045 RF5; PRD 0030 RF2.3; be15ec8).
- In the terminal dashboard, the account details export the recorded history with X and erase it with D after you type the confirmation word (PRD 0052 RF5.1; 7786a51).
- The desktop local server can keep a private, redacted access log (off by default), with an Erase log button beside it (PRD 0050 RF6.1, RF6.3; 74fa868).
- The self-check (cuotai --doctor and Check setup) also reports the app log, the network proxy, the system root certificates, a clock warning and Cuotai's own files, and the desktop can copy the exact report it previews (PRD 0052 RF3.1, RF3.2, RF3.3, RF3.4, RF7.2; 1c5b89a).
- Settings can erase all Cuotai data on this computer, including the sign-ins saved in the system vault, after listing everything and asking for a typed word; the app stops its background work first and quits when done (PRD 0052 RF5.2, RF5.3, RF7.2; 41eddd3).
- The desktop app warns with a banner when this computer's clock disagrees with the providers' servers by more than five minutes (PRD 0052 RF5.2, RF5.3, RF7.2; 41eddd3).

### Fixed

- The pace forecast says how long before the reset a limit runs out in your language, for example 18時間 in Japanese (PRD 0035 RF10.1, RF10.3, RF11.5, RF11.6, RF13.2, RF13.3, RF14.1, RF14.2; 3a8e0c6).
- The hooks settings line names the number of active rules correctly in Russian, Portuguese and Hindi (PRD 0035 RF10.1, RF10.3, RF11.5, RF11.6, RF13.2, RF13.3, RF14.1, RF14.2; 3a8e0c6).
- Breakdown row alerts are no longer forgotten between watch cycles or when the configuration is saved, so row crossings and recoveries are reported (e29f896).
- A hook now runs once for each breakdown row that crosses a level, not once for all rows crossing together (9d2f16c).
- Send test in Settings › Alerts › Notification channels posts once and Remove asks once, and channel labels with "&" or "<" show as typed (830cfa6).
- A notification channel edited by hand in desktop.json with a malformed id, name or host is ignored instead of being shown or used (8328364).
- The notification channel delivery log no longer grows without limit once it gets very large (b3eefe2).
- A notification channel edit that cannot be saved no longer changes the address the channel posts to (f61d97b).
- A notification channel to an ntfy server on this computer or your private network over plain http no longer goes through a configured proxy (cd80798).
- A backup or restore with --config or CUOTAI_CONFIG no longer reads or changes this computer's desktop settings, themes, languages or data folder unless CUOTAI_USER_DIR or CUOTAI_DATA_DIR names one (4c90e46).
- History restored on a new computer now shows under its accounts when they have no explicit id (3f9fb19).
- A restore no longer rewrites desktop settings written by a newer Cuotai, and a problem with this computer's history is no longer reported as a damaged backup (5484d5e).

### Security

- Erasing all data (cuotai --erase-all-data) also removes the backup and set-aside copies Cuotai kept of its local files (PRD 0051 RF7; PRD 0052 RF5; dfdb15c).
- Hooks now confirm the exact program file each rule runs (its path and SHA-256, plus a script's interpreter), and a hook whose program changes is stopped until you confirm it again (PRD 0050 RF2.4; 5929db0).
- User themes are checked against a strict token-only grammar before they load, and a refused theme names what to fix (PRD 0050 RF3.1, RF3.2; 45398f1).
- On Windows, Cuotai now gives its private files an access list for you alone and refuses to run an extension or use a server token file that other users of the computer could change (PRD 0050 RF7.5, RF8.1; PRD 0052 RF3.3; c044904).
- Cuotai no longer looks for the AWS or Bailian command line tools in relative PATH entries, which depended on the folder it was started from (f4ad538).
- A backup no longer carries the data folder path or the main window's position, and a restore never takes folder paths from a backup (d818dfe).
- Erasing the history now also removes the backup copy of the history file that an update or a history move left (2bb18d0).

## [0.83.1] - 2026-11-05

### Fixed

- On a system with no trusted root certificates, Cuotai no longer stops at start-up with an OAuth network error; each account that needs the network reports a network error instead (PRD 0052 RF6; 5dd4b69).

## [0.83.0] - 2026-11-05

### Added

- You can add a JetBrains AI organization account: Cuotai shows the AI credits used this month by AI Assistant, Junie and other agents, read with a service account (PRD 0055 RF1, RF2, RF3, RF4, RF5, RF6, RF7; issue #51; 16e9d10).
- Alibaba Model Studio accounts can sign in with the bl CLI to show the free-tier quota of the most urgent models and, with a workspace ID, the calls and tokens of the last 7 days (d88dab4).
- When a console's sign-in offers "Sign in with Google", the desktop capture window now keeps your Google sign-in hidden in memory for up to 30 minutes, so the next provider only asks you to pick the account (PRD 0056 RF1, RF2, RF3, RF4, RF5, RF6, RF7, RF8, RF9; f7c6562).
- An account whose provider or key comes from a newer Cuotai is skipped with a notice instead of rejecting the whole configuration file, and configuration errors now name the line, column and key (PRD 0051 RF1, RF2; PRD 0001 COMP-01; 425de85).
- The new cuotai --check-config [--json] checks the configuration file strictly and offline, without printing any value (PRD 0051 RF1, RF2; PRD 0001 COMP-01; 425de85).
- Desktop settings saved at the same time by two Cuotai windows or by the updater no longer overwrite each other, and settings written by a newer Cuotai are kept (PRD 0051 RF4; PRD 0018 RF2; da1f3c3).
- The new CUOTAI_DATA_DIR environment variable keeps every local store (history, caches, reset marks, local cost, hooks, extensions, server token) in one private folder of your choice (PRD 0051 RF5; 66d229e).
- Before upgrading the local history or cost index, Cuotai keeps a private .bak copy, and from this release on a newer Cuotai's additive changes no longer make an older one reject these files (PRD 0051 RF3; PRD 0006 COMP-03; 3567193).
- The new cuotai --doctor [--json] checks your installation offline (configuration, credential variables, databases, hooks, server token and port) and says how to fix each problem, without printing any value (PRD 0046 RF4; 412fcc5).
- The new cuotai --backup FILE writes your settings (accounts file, hooks, preferences, prices, themes and languages) to one archive that never holds a secret, and cuotai --restore FILE shows a plan, restores it and lists the variables and sign-ins still needed (PRD 0046 RF1, RF2, RF3, RF5; 5f7a3b0).
- Settings › Advanced › Files has a Check setup row that checks this computer's setup offline and says how to fix each problem (PRD 0046 RF4; 862609d).
- A hook rule now runs once per change even when the desktop app and cuotai --watch --hooks watch the same accounts (PRD 0054 RF2; 3a1ccf1).
- The main window now opens with its last size and position, and comes back on screen when its monitor is unplugged or smaller (PRD 0054 RF7; 56cc6eb).
- When the system shows no tray icon, such as GNOME without an AppIndicator extension, Cuotai keeps running in its window and says so instead of failing to start (PRD 0054 RF5, RF4; 341d5a8).
- A failure to start now opens a dialog instead of closing without a word (PRD 0054 RF5, RF4; 341d5a8).
- If you move Cuotai after turning on Open at login, Settings now says so on macOS and Linux and offers Repair to point the login entry at the app again (PRD 0054 RF4; c2f357e).
- Cuotai now keeps a private, redacted app log (cuotai.log in the local data folder, at most 512 KiB), whose level CUOTAI_LOG sets (PRD 0052 RF1, RF2; 517d690).
- When the desktop app cannot start, it shows a message naming the app log and exits instead of crashing silently (PRD 0052 RF1, RF2; 517d690).
- The command cuotai --list-providers --json prints the provider catalog as JSON (connection modes, access level, metered, model-list and status support) (PRD 0052 RF3.5; c163185).
- A new privacy page (docs/privacy.md) lists every network flow Cuotai can start and every file and vault entry it keeps, generated from the code (PRD 0052 RF3.3, RF4; fe10400).
- The command line can export the local history (--history-export) and erase it (--history-erase), and --erase-all-data removes every file and vault entry Cuotai keeps after a typed confirmation (PRD 0052 RF5.1, RF5.2, RF5.3, RF5.4; 36ee124).
- Cuotai trusts the operating system's certificate store (including roots your administrator installed) instead of its own bundled list, and follows HTTPS_PROXY and NO_PROXY for every connection (PRD 0052 RF6, RF7; 1c5b59b).
- The terminal warns when this computer's clock is more than five minutes off the providers' servers (PRD 0052 RF6, RF7; 1c5b59b).
- You can set your own budgets on spend, usage or credits in the accounts TOML; each one shows as a budget quota with pace, alerts, allowance and history (PRD 0041 RF1, RF2, RF3, RF4; PRD 0043 RF1, RF2, RF3, RF5; 82bba49).
- You can set a floor on a balance or credits and be warned once when it drops below it (PRD 0041 RF1, RF2, RF3, RF4; PRD 0043 RF1, RF2, RF3, RF5; 82bba49).
- Quotas with a reset show how much you can use per day (or per hour near the reset) and still reach it, in the details, in --once and in export::v1 (PRD 0041 RF1, RF2, RF3, RF4; PRD 0043 RF1, RF2, RF3, RF5; 82bba49).
- Cuotai notes when a provider changes a limit, journals it, sends the limit_changed event and can notify you when you turn that on (PRD 0041 RF1, RF2, RF3, RF4; PRD 0043 RF1, RF2, RF3, RF5; 82bba49).
- The terminal shows how your use spreads over the hours and days with --stats --rhythm and the r key of the statistics screen (PRD 0041 RF1, RF2, RF3, RF4; PRD 0043 RF1, RF2, RF3, RF5; 82bba49).
- The new --digest flag prints a weekly summary of your usage from the history recorded on this computer (PRD 0041 RF1, RF2, RF3, RF4; PRD 0043 RF1, RF2, RF3, RF5; 82bba49).
- The desktop account dialog edits budgets and balance floors, and the details offer Set a budget… and Set a floor… on spend and balances (PRD 0041 RF3, RF4, RF5; PRD 0043 RF2, RF3, RF4; 7677feb).
- Desktop cards and details mark budget quotas, show floor warnings, the daily allowance and limit changes of the last 48 hours (PRD 0041 RF3, RF4, RF5; PRD 0043 RF2, RF3, RF4; 7677feb).
- The desktop Stats view shows your hourly and daily rhythm, and Settings › Alerts can notify limit changes and send a weekly digest (PRD 0041 RF3, RF4, RF5; PRD 0043 RF2, RF3, RF4; 7677feb).
- Organizational accounts can opt into a breakdown with `breakdown = [...]`: OpenRouter keys, LiteLLM virtual keys and teams, OpenAI projects and API keys, and Anthropic workspaces, never per user (PRD 0045 RF1, RF2, RF3, RF4, RF6, RF7, RF8; 023fbb5).
- The TUI details, `--once` and the wizard show and set an account's breakdown rows (PRD 0045 RF1, RF2, RF3, RF4, RF6, RF7, RF8; 023fbb5).
- The `--json-v1` export and the local server's account route carry each account's breakdown rows (PRD 0045 RF1, RF2, RF3, RF4, RF6, RF7, RF8; 023fbb5).
- The desktop details show a Breakdown section sortable by spend or percentage, the Detailed card layout adds a "Top" line, and the account dialog offers each breakdown with the extra requests it adds (PRD 0045 RF6; 1c41d32).
- Claude accounts can be fed by Claude Code's own status line and ChatGPT / Codex accounts by Codex CLI's local logs, with no network request and no token reuse (PRD 0040 RF1, RF3, RF4, RF5, RF6, RF7, RF8; 7ffb8ef).
- The new `cuotai --statusline-claude` command shows Claude's own windows, the tightest other account and the context share in Claude Code's status line, and the terminal shows each fed account's live agent session (PRD 0040 RF2, RF5, RF7, RF3; 4b5ae68).
- The desktop shows each fed account's live agent session (context share and the agent's cost estimate), offers to add a Claude account fed by Claude Code, and gives the status-line snippet to copy (PRD 0040 RF7, RF8, RF3, RF2; 036e7a7).
- The local cost estimate now breaks down by session, 5-hour block and project, with a live burn rate and projection for the active block, and asks once to confirm the updated disclosure (PRD 0042 RF1, RF2, RF3, RF4, RF7; 6ffd65c).
- New `cuotai --cost --by session|block|project`, `--cost-live` and `--cost-label` commands show and label the local cost by session, 5-hour block and project (PRD 0042 RF5, RF6; 3a6e97b).
- The desktop app shows the local cost by session, 5-hour block and project in Settings, and the active block's cost and burn rate on the account cards (PRD 0042 RF6; b9d37e4).

### Fixed

- An AWS Bedrock account whose profile name is not valid now reports a configuration problem instead of starting the AWS command line with it (PRD 0054 RF6; 40f0cc0).
- The macOS widgets no longer spill past their edges, and they show more accounts with a clearer layout and readable ages (6d8677a).
- On Windows, saving settings, the configuration or other state no longer fails while another Cuotai window or the TUI has the file open (eb45547).
- The TUI and the desktop app recording history at the same time now wait for each other's writes instead of losing a reading to a busy database on a slow disk (eb45547).

### Security

- The desktop app loads no inline script, so its webviews run under a stricter Content Security Policy (PRD 0050 RF1, RF7; b798462).
- **Breaking**: Browser access to the local server needs an access token and a list of trusted origins, and only those origins can read the answers. Migration: `ServeConfig` gains `allowed_origins`, and browser mode needs a token and 1 to 16 trusted origins. A `desktop.json` with browsers allowed but no origins (or no token) loads with browsers off; add the origins of your pages in Settings › Advanced › Integrations and turn it on again (PRD 0050 RF4; 681d721).
- Saving, approving, turning on and testing hooks, and approving or testing an extension, now ask for confirmation in a system dialog instead of inside the app window (PRD 0050 RF2; 6e997a5).
- The local server limits slow and unauthenticated clients before reading any usage, so they cannot keep other clients waiting (PRD 0050 RF5; a21c868).
- The extension disclosure warns when a requested variable is another account's credential variable, naming those accounts (PRD 0050 RF8.2, RF8.3; bfcde2f).
- User language packs can no longer change the wording of security warnings, consent and confirmations, and symbolic links in the themes and languages folders are ignored (PRD 0050 RF3.3, RF3.4; 2c0514c).
- The local server's access log records only errors by default, chosen with --access-log off|errors|all, and never the peer address or the account key (PRD 0050 RF6.1, RF6.2; 04fd00c).

## [0.82.4] - 2026-11-04

### Fixed

- The desktop app no longer crashes on macOS when it restarts to finish an update (d2ad69b).
- A refreshed login is no longer lost when the system vault fails to save it or a login commit is rolled back (2720442).
- A login no longer ends logged out when the provider rotates its refresh token in an answer that Cuotai then rejects (1c5aec4).
- The terminal app, the local server and the desktop app no longer log each other out when they renew the same login at the same time (8b349aa).
- A paid Antigravity account keeps its per-model quotas when the plan lookup fails, with a note that the plan is unknown, instead of being shown as free (11e2a19).
- An expired or inactive Alibaba Coding Plan is reported as such instead of as an incompatible response (c6a3280).
- Alibaba Token Plan accounts read through the `bl` CLI now show the monthly quota and any other window the CLI reports (50266ea).
- Alibaba Token Plan errors from the `bl` CLI now say whether the CLI is missing, the profile is signed out, or the command failed (8bb378a).
- AWS Bedrock accounts are no longer queried early, and billed, when the system clock jumps forward or the computer wakes from sleep (a4d1973).

### Security

- The async-executor dependency moves off a yanked release (43f4eda).

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
- You can read the full changelog in the desktop app from Settings › About with What's new (PRD 0019 RF7.7; b2d8cbd).
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
- Settings › Alerts has a "Warn before running out" switch with a 15 min, 30 min or 1 h lead, and cards and details say when a quota is projected to run out before its reset (PRD 0024 RF4; ddae534).
- You can show the consumption pace in percentage points or in percent (Settings › Dashboard, or --pace-unit in the terminal), and reset times read the same on every surface (PRD 0024 RF1.3, RF1.4, RF6; a659393).
- Accounts can be put in a group in the TOML (group = "coding") so Cuotai can tell which of them has the most room right now; the export and /v1/limits list it as best (PRD 0024 RF7.1, RF7.2, RF7.4; 9f45749).
- The desktop app marks the best account of each group of comparable accounts ("Best now") and tells you what it has left when another one runs out; Settings › Dashboard can turn it off (PRD 0024 RF7.3, RF7.5; 8da15ed).
- The account wizard and dialog have an optional Group field, and the terminal dashboard marks the best account of each group (PRD 0024 RF7.1, RF7.5; b8a1042).
- A quota without a reset date now sends a recovery notice once usage falls to 80% or below (PRD 0032 RF3.2, RF8.1, RF9; 9329847).
- Notifications can follow quiet hours with a summary when they end, hide account names by default, and are never sent twice when the app and the terminal dashboard are both open (PRD 0032 RF1, RF2, RF3, RF4, RF7.1, RF8.1; 0b57bfe).
- The terminal dashboard's --notify now shares your desktop notification settings, quiet hours and sent-notice history, so the same crossing is not announced twice (PRD 0032 RF1.2, RF4.1, RF4.3, RF6.1, RF7.2; 8857393).
- A hook rule can set quiet = "skip" to stay silent during your quiet hours (PRD 0032 RF5.1; c34aeb4).
- Settings › Alerts has quiet hours per weekday with an end-of-night summary, a switch to hide account names in notifications, and cards mark quotas that crossed a limit while Cuotai was closed (PRD 0032 RF2.3, RF5.2, RF7.1, RF8.2; f050efc).
- You can monitor OpenRouter: an API key shows its limit, spend and free-model requests, and a management key shows the credit balance (PRD 0038 RF1, RF2, RF3, RF4, RF5, RF6, RF7, RF8, RF9, RF11, RF12; 72ee36f).
- You can sign in to OpenRouter from the browser instead of pasting an API key (PRD 0038 RF1, RF14; 4b40079).
- When you allow device checks, adding an account also offers OpenRouter if OPENROUTER_API_KEY is set; only the variable's name is stored (PRD 0038 RF5; PRD 0027 RF6; 52149a7).
- A provider's Retry-After on a rate limit now delays only that account's next refresh (PRD 0038 RF7.1; 2bffef9).
- The dashboard can now flag an ongoing ElevenLabs API incident in your account's region when status checks are enabled (PRD 0037 RF8; aeafa41).
- You can print one status-bar line with cuotai --status-line, and the Homebrew formula installs shell completions (PRD 0031 RF2, RF1, RF3; bd419a0).
- The tightest account in /v1/limits now carries its stable id (PRD 0031 RF4; adf71af).
- You can serve usage statistics at /v1/stats with cuotai --serve --serve-stats, behind the access token (PRD 0031 RF11; 7788f0c).
- The status line can read a running local server: cuotai --status-line --from-server reads a running cuotai --serve instead of querying the providers again (PRD 0031 RF2; 289dc63).
- The desktop app can share usage statistics on its local server (/v1/stats) from Settings › Advanced › Integrations (PRD 0031 RF11; e586170).
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
- Settings › General shows labelled mode choices, a theme swatch and a single language field (PRD 0022 RF9; 5e3e4fb).
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
- Settings › General has a Start at login switch that starts Cuotai in the tray when you sign in (PRD 0022 RF9, RF10; 778e00d).
- The plan prices and quiet hours settings use the same card rows as the rest of Settings, and a plan converted to your second currency is marked as an estimate (PRD 0022 RF9, RF10; 63a88b2).
- The Add-account catalog lists 9router and the LiteLLM proxy under their own "Self-hosted gateway" group (PRD 0036 RF10; f200a36).
- Rows and cards show the plan the provider reports, such as Max or Plus (PRD 0022 RF4; PRD 0023 RF10.3; PRD 0036 RF4.4, RF8.5; PRD 0039 RF10; dffb7af).
- Cards and details name a model that is at its limit and when it returns (PRD 0022 RF4; PRD 0023 RF10.3; PRD 0036 RF4.4, RF8.5; PRD 0039 RF10; dffb7af).
- The run-out warning in an expanded account has an "Alert me" button (PRD 0022 RF4; PRD 0023 RF10.3; PRD 0036 RF4.4, RF8.5; PRD 0039 RF10; dffb7af).
- Each 9router or LiteLLM connection can be refreshed, hidden or shown beside its direct account (PRD 0022 RF4; PRD 0023 RF10.3; PRD 0036 RF4.4, RF8.5; PRD 0039 RF10; dffb7af).
- On macOS, you can hide Cuotai from the Dock in Settings › General so it lives only in the menu bar (PRD 0022 RF9; 1a9b9fd).
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
- Settings › Advanced now has an Extensions page, searchable like every other setting (PRD 0020 RF2.5; bb8c0d2).
- The release notes now open once after you update Cuotai, with a switch in Settings › About to turn that off, and you can set a one or two character monogram for an account (PRD 0019 RF7.7; PRD 0022 MONO-4, RF15.1; 0a931fa).
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
- Settings › General says when the interface language is a machine translation and links the guide for reviewing it (PRD 0035 RF11.5; 4b06f7b).
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
- **Automation:** the versioned `export::v1` contract, the read-only local server (`cuotai --serve` and the desktop's Settings › Integration), `cuotai --watch` with event hooks, a widget snapshot for macOS widgets and desktop panels, and script extensions that run only after your explicit consent (PRD 0012 RF1; PRD 0013 RF1; PRD 0014 RF1; PRD 0017 RF1).
- **Costs and web sessions:** an estimated cost at API prices from the local logs of Claude Code and Codex CLI, off until you turn a source on, and pasted web sessions kept in the system vault for the providers that need one, with their risk spelled out (PRD 0015 RF1; PRD 0016 RF1).
- **Languages:** the interface in Portuguese, English, Spanish and Italian, with the TUI starting in the system's language when it is supported.
- **Distribution:** the `MIT OR Apache-2.0` license, the What's new screen and `cuotai --changelog`, a release pipeline with signed artifacts, checksums, an SBOM and provenance, Homebrew, winget, Scoop and AUR manifests, a daily update check in the desktop app, and `cuotai --install-source` (PRD 0019 RF1, RF7.7; PRD 0021 RF2; PRD 0018 RF1.1).

### Fixed

- Many provider, sign-in and display fixes found while the builds were used internally, for example the Ollama Cloud usage report, the Bailian sign-in profile, OAuth re-authentication of an unchanged account, and the retry backoff of providers billed per request (PRD 0001 RF6).

### Security

- Credentials travel only over HTTPS, OAuth client secrets are not kept in memory while a browser login waits, `cuotai --serve` requires a bearer token by default, hooks and extensions run in private directories with checked permissions, removing an account also deletes its web session, and dependency advisories were fixed as they appeared (PRD 0012 INT-04; PRD 0013 FIX-01, FIX-02; PRD 0017 FIX-04; PRD 0016 FIX-06).

[Unreleased]: https://github.com/jmarcon/cuotai/compare/v0.87.6...HEAD
[0.87.6]: https://github.com/jmarcon/cuotai/compare/v0.87.5...v0.87.6
[0.87.5]: https://github.com/jmarcon/cuotai/compare/v0.87.4...v0.87.5
[0.87.4]: https://github.com/jmarcon/cuotai/compare/v0.87.3...v0.87.4
[0.87.3]: https://github.com/jmarcon/cuotai/compare/v0.87.2...v0.87.3
[0.87.2]: https://github.com/jmarcon/cuotai/compare/v0.87.1...v0.87.2
[0.87.1]: https://github.com/jmarcon/cuotai/compare/v0.87.0...v0.87.1
[0.87.0]: https://github.com/jmarcon/cuotai/compare/v0.86.10...v0.87.0
[0.86.10]: https://github.com/jmarcon/cuotai/compare/v0.86.9...v0.86.10
[0.86.9]: https://github.com/jmarcon/cuotai/compare/v0.86.8...v0.86.9
[0.86.8]: https://github.com/jmarcon/cuotai/compare/v0.86.7...v0.86.8
[0.86.7]: https://github.com/jmarcon/cuotai/compare/v0.86.6...v0.86.7
[0.86.6]: https://github.com/jmarcon/cuotai/compare/v0.86.5...v0.86.6
[0.86.5]: https://github.com/jmarcon/cuotai/compare/v0.86.4...v0.86.5
[0.86.4]: https://github.com/jmarcon/cuotai/compare/v0.86.3...v0.86.4
[0.86.3]: https://github.com/jmarcon/cuotai/compare/v0.86.2...v0.86.3
[0.86.2]: https://github.com/jmarcon/cuotai/compare/v0.86.1...v0.86.2
[0.86.1]: https://github.com/jmarcon/cuotai/compare/v0.86.0...v0.86.1
[0.86.0]: https://github.com/jmarcon/cuotai/compare/v0.85.12...v0.86.0
[0.85.12]: https://github.com/jmarcon/cuotai/compare/v0.85.11...v0.85.12
[0.85.11]: https://github.com/jmarcon/cuotai/compare/v0.85.10...v0.85.11
[0.85.10]: https://github.com/jmarcon/cuotai/compare/v0.85.9...v0.85.10
[0.85.9]: https://github.com/jmarcon/cuotai/compare/v0.85.8...v0.85.9
[0.85.8]: https://github.com/jmarcon/cuotai/compare/v0.85.7...v0.85.8
[0.85.7]: https://github.com/jmarcon/cuotai/compare/v0.85.6...v0.85.7
[0.85.6]: https://github.com/jmarcon/cuotai/compare/v0.85.5...v0.85.6
[0.85.5]: https://github.com/jmarcon/cuotai/compare/v0.85.4...v0.85.5
[0.85.4]: https://github.com/jmarcon/cuotai/compare/v0.85.3...v0.85.4
[0.85.3]: https://github.com/jmarcon/cuotai/compare/v0.85.2...v0.85.3
[0.85.2]: https://github.com/jmarcon/cuotai/compare/v0.85.1...v0.85.2
[0.85.1]: https://github.com/jmarcon/cuotai/compare/v0.85.0...v0.85.1
[0.85.0]: https://github.com/jmarcon/cuotai/compare/v0.84.11...v0.85.0
[0.84.11]: https://github.com/jmarcon/cuotai/compare/v0.84.10...v0.84.11
[0.84.10]: https://github.com/jmarcon/cuotai/compare/v0.84.9...v0.84.10
[0.84.9]: https://github.com/jmarcon/cuotai/compare/v0.84.8...v0.84.9
[0.84.8]: https://github.com/jmarcon/cuotai/compare/v0.84.7...v0.84.8
[0.84.7]: https://github.com/jmarcon/cuotai/compare/v0.84.6...v0.84.7
[0.84.6]: https://github.com/jmarcon/cuotai/compare/v0.84.5...v0.84.6
[0.84.5]: https://github.com/jmarcon/cuotai/compare/v0.84.4...v0.84.5
[0.84.4]: https://github.com/jmarcon/cuotai/compare/v0.84.3...v0.84.4
[0.84.3]: https://github.com/jmarcon/cuotai/compare/v0.84.2...v0.84.3
[0.84.2]: https://github.com/jmarcon/cuotai/compare/v0.84.1...v0.84.2
[0.84.1]: https://github.com/jmarcon/cuotai/compare/v0.84.0...v0.84.1
[0.84.0]: https://github.com/jmarcon/cuotai/compare/v0.83.1...v0.84.0
[0.83.1]: https://github.com/jmarcon/cuotai/compare/v0.83.0...v0.83.1
[0.83.0]: https://github.com/jmarcon/cuotai/compare/v0.82.4...v0.83.0
[0.82.4]: https://github.com/jmarcon/cuotai/compare/v0.82.3...v0.82.4
[0.82.3]: https://github.com/jmarcon/cuotai/compare/v0.82.2...v0.82.3
[0.82.2]: https://github.com/jmarcon/cuotai/compare/v0.82.1...v0.82.2
[0.82.1]: https://github.com/jmarcon/cuotai/compare/v0.82.0...v0.82.1
[0.82.0]: https://github.com/jmarcon/cuotai/compare/v0.81.0...v0.82.0
[0.81.0]: https://github.com/jmarcon/cuotai/compare/v0.80.0...v0.81.0
[0.80.0]: https://github.com/jmarcon/cuotai/compare/2bbb39a...v0.80.0
