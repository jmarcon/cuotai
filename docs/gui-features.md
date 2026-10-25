# Desktop app: feature reference

This is the complete list of what the Cuotai desktop app does today, with every setting that changes its behavior. For installation and first steps on your system, start with the guide for your operating system: [macOS](gui-macos.md), [Windows](gui-windows.md) or [Linux](gui-linux.md). Screenshots here show the browser demo of the interface with made-up accounts (`example.com` addresses), not real data.

Everything the app shows comes from `cuotai-core`, the same engine the terminal dashboard and the command line use. The app is read-only: it asks your providers for quota, usage and billing numbers and never sends a request that spends credits.

<div align="center">
  <img src="assets/screenshots/desktop-dashboard.png" alt="Cuotai desktop main window with quota cards" width="100%" />
  <p><em>The main window: one card per account, each highlighting its most restrictive limit.</em></p>
</div>

**Settings** opens in a window of its own, beside the dashboard, so a change shows in the main window and the tray at once. Open it with the gear in the main window or the tray popover, **Settings** in the tray menu, or `⌘,` (macOS) / `Ctrl+,` (Windows, Linux); opening it again brings the same window to the front. A sidebar lists its pages: **General**, **Dashboard**, **Alerts**, **Data & privacy**, an **Advanced** group (**Integrations**, **Local cost**, **Files**) and **About**. The window remembers its size, position and last page.

## Contents

- [Dashboard and background refresh](#dashboard-and-background-refresh)
- [Consumption pace](#consumption-pace)
- [Reset notices and usage alerts](#reset-notices-and-usage-alerts)
- [Providers' public status](#providers-public-status)
- [How each account's usage is read](#how-each-accounts-usage-is-read)
- [Account details and history](#account-details-and-history)
- [Local cost](#local-cost)
- [Tray popover and tray menu](#tray-popover-and-tray-menu)
- [9router and other aggregators](#9router-and-other-aggregators)
- [Themes and languages](#themes-and-languages)
- [Integration: local server and hooks](#integration-local-server-and-hooks)

## Dashboard and background refresh

The desktop app watches your accounts in the background with incremental per-account scheduling, drag-and-drop reordering (synced with the TUI) and immediate visual alerts when a quota needs attention or runs out. Each account highlights its most restrictive limit: the tray ring and a `Week · 100%` chip on the card follow the window that blocks you (a weekly limit at 100% wins over a free 5-hour window), the list says "Blocked until 14:30", and Settings offers an optional "Most restrictive first" order that leaves your manual order untouched. Every card, tray entry and menu line also says how old its reading is ("Fresh · 3 min ago", "Aging", "Stale"), and the tray icon tooltip names the oldest one. When a query fails after a success, the card keeps the last valid numbers, dimmed, with a "Stale · 42 min ago" badge and the cause of the failure, until the next successful query; an authentication failure drops them.

**Settings › Dashboard** can also group exhausted or blocked accounts in a collapsible "Recharging" section that shows each return time ("Returns Today 14:30", or "Return unknown"). It is off by default and only changes the display: the manual order holds inside each group and the order saved in `cuotai.toml` never changes.

<div align="center">
  <img src="assets/screenshots/desktop-settings-appearance.png" alt="Settings, Appearance section" width="420" />
  <p><em>Settings → Appearance: color mode, theme, animations, the "Recharging" group and the consumption pace.</em></p>
</div>

## Consumption pace

Cards, details and the tray also show the **consumption pace until the reset**: whether what you used so far lasts at a steady rate ("Week: 8 points in reserve · lasts until the reset") or runs out early ("runs out at 16:40, 2h 10m before the reset"), with an "Expected 47% used" mark drawn over each bar in the details. It is a linear estimate, not a promise; windows that just opened, rolling rate limits and metrics without a reset time show nothing. **Settings › Dashboard › Show consumption pace** hides it (on by default).

The TUI shows it too: a stage icon (`↓` in reserve, `→` on pace, `↑` above the pace, `✕` limit reached) before each eligible metric on the cards, a pace line per metric in the details, and a `Ritmo:` line under each eligible metric in `--once` (see [the TUI guide](tui.md)). `--json` is unchanged.

## Paid extra usage

Some subscriptions keep working after the included quota runs out and bill the excess (Claude extra usage, Cursor and Grok on-demand, Kiro overage). Cuotai lists that amount apart from the plan limits: it is never the account's binding limit, has no pace, raises no 75%/90% alert and is never drawn as a meter that implies "left". While money is being billed the card shows a **warning chip** with the core's words ("On extra · $6.25", or "Limit hit · on extra $6.25" when the account is also at its limit) and a textual row ("$6.25 extra of $50.00"); the details add an **Extra usage** section with the state (off, on with nothing spent, active, budget used up or amount unknown), the cap, the cycle end and, for Claude, the reminder that its amounts are in provider units, not dollars, until their scale is verified. An amount the provider does not report shows "—", never `$0`. The tray rows and the tray tooltip carry the same chip. A system notification (with the amount) is sent **once per billing cycle** when extra usage starts; the preference is stored as `alerts.overage` in `desktop.json` (on whenever usage alerts are on). Cuotai only reads: it never changes a provider's billing settings.


**Show pace as.** **Settings › Dashboard › Show pace as** chooses how the deviation reads everywhere (cards, rows, details, tray): **Points** (the default, "10 pts ahead") or **Percent** ("20% faster than pace", relative to the expected use; early in a window it falls back to points with a tooltip). The words come from the core, so the terminal says the same (`--pace-unit`). Reset times read "Resets 19:30 · in 1h 35m", also from the core.

**Forecast.** While the pace points to running out before the reset, from the recent slope of the recorded readings (never from an average, a short early span, a reading that is not fresh or a single reading), the details show one callout ("At this rate Session (5h) runs out ≈18:53, 37 min before the reset") and the card an outlook chip ("↑ Runs out ≈18:53"). It is a linear estimate from recent readings and does not need the warning to be on.

## Best account in a group

Accounts that can replace one another can share a `group` (the account dialog and the terminal wizard have an optional **Group** field, or edit `group = "coding"` in the TOML; `[[groups]]` names and orders groups). The best account now of each group of two or more is marked **Best now** on its card, in the tray popover and in the tray tooltip, and the details of an account that is exhausted or projected to run out add "Work runs out ≈18:53 · Personal has 64% left" with a single **Show Personal** action that scrolls to the other card. Without a group, only accounts of the same provider are compared. It is information only: Cuotai never switches accounts, rotates credentials or edits your configuration, and **Settings › Dashboard › Show the best account** turns it off.

## Reset notices and usage alerts

**Settings › Alerts** can send a **system notification when a quota resets** (off by default; turning it on asks the system for permission, and a refusal keeps it off with guidance on allowing Cuotai in the system settings). Choose the moment (at the time, or 5, 15 or 60 minutes before), the filter (only if used, only if exhausted, always) and whether "other" windows count; **Send test notification** checks that notices arrive. The app schedules the notices itself, so they come with the window hidden in the tray, but not with the app closed. Each reset is notified once, even across restarts (`<OS local data dir>/cuotai/notified.json` keeps only digests and dates, for 8 days); a reset missed by more than 10 minutes while the app was closed or asleep is not notified late, and several resets of an account at once become one notice. A notice never queries the provider: it follows the time the provider reported and the new balance shows on the next refresh. Its text names only the provider, the account and the metrics, in the language chosen in the app. Each card and its details get a bell to silence one account; the choice is kept per provider and account name in `desktop.json`, so renaming an account turns its bell back on. The tray menu, its tooltip and the tray popover show the next notice ("Next notice: Claude 5h · today, 14:30").

The same section turns on **usage alerts** (off by default, with the same permission request and test button): a system notification when a quota with a limit reaches a threshold of use ("Claude: 90% of the 5-hour window used", with the account, the quota and its reset time), runs out (**Notify exhaustion**) or has slack again in the next window after running out (**Notify recovery**). The thresholds are chips from 10 to 99% (75 and 90 by default); each one notifies once per window, oscillating around it stays silent, and it re-arms only after use falls 5 points below it or a new window starts. Balances, spend, rate limits and missing values never alert, and no alert queries a provider: the app judges each reading its refresh already took, also with the window hidden in the tray. An account's alerts of one refresh become one notification, at most one notification goes out per minute (the rest are grouped into the next), and nothing is resent after a restart (the alert state joins `notified.json`, digests and dates only). The account's bell silences its alerts too, and each quota can be muted from the account details; the card, the tray and the details highlight an account with an active alert. Preferences live in `desktop.json`, never in `cuotai.toml`.

**Warn before running out.** The same page has a **Warn before running out** switch (off by default; turning it on asks the system for permission like the other notices) with a lead of 15 min, 30 min or 1 h. When the recent pace projects that a quota will run out before its reset, within the lead and at least 5 minutes before the reset, you get one notice per window ("Claude · Work: may run out before the reset"). It is a linear estimate from recent readings: it needs at least 3 recorded readings spanning 30 minutes (or 20% of the window), a fresh reading, at least 25% used, and two readings in a row that agree, so a burst early in a window or a single reading never triggers it. It works on its own, with the usage alerts off; the account's bell and a muted quota silence it, and a queued warning is dropped if the projection passes the reset, the quota runs out or the window resets. It is stored as `alerts.projected_lead_minutes` in `desktop.json` (absent means off). Hooks can also react to `quota_exhaustion_projected` ([`events.md`](events.md)).

<div align="center">
  <img src="assets/screenshots/desktop-settings-notify.png" alt="Settings, Notifications and History sections" width="420" />
  <p><em>Settings → Notifications (and, below it, History).</em></p>
</div>

## Providers' public status

**Settings › Data & privacy** can show the **providers' public status** (off by default). Turned on, the app reads the public status pages of the providers you configured (the section lists their domains; each page at most once every 5 minutes, without credentials, cookies or account data, though those sites see your IP address and that you use the provider). A card then shows a colored badge with an icon and text when its provider reports degraded performance, a partial or major outage or maintenance, or when its status page could not be read, with **Open status page**; the details show the full status and when it was read, the tray popover lists the affected providers, and the tray icon gets a red dot while any of them reports a partial outage or worse. Providers without a verified public source never show as operational. A failed query during an incident says the failure may not be from your account.

The TUI and the CLI read the same status only when you ask (`--status`, or `CUOTAI_STATUS=1` in the environment); `--no-status` always turns it off, also in scripts that inherit the variable. A card then shows the badge on its top border (`▲ Interrupção parcial`, `? Página de status indisponível`), the details list the provider's full status with the incident, its start, when it was read and the page, and `--once` prints a `Status do provedor:` line under the affected account. `--json` keeps its shape.

<div align="center">
  <img src="assets/screenshots/desktop-settings-privacy.png" alt="Settings, Privacy section" width="420" />
  <p><em>Settings → Privacy: the provider status switch lists the domains it would read.</em></p>
</div>

## How each account's usage is read

When an account has no usage query in its mode, its card says why instead of showing a generic error, and when an internal interface stops matching, the card says it may have changed and names the public contract Cuotai follows. How each integration reads usage is documented in [`providers.md`](providers.md#usage-access-matrix), not in the app; the "Add account" list shows provider names only and leaves out providers that have no usage query at all.

## Account details and history

The magnifier on a card opens the account details: every quota with its bar, the pace mark, the reset time, buttons to hide or mute a single quota, the models catalog and the history.

<div align="center">
  <img src="assets/screenshots/desktop-details.png" alt="Account details dialog" width="520" />
  <p><em>Account details: every window with its pace mark and reset time.</em></p>
</div>

Every successful reading (TUI, desktop app, `--once`, `--json`, `--json-v1`, `--serve` and `--watch`) also enters a local measurement history on this computer only: `<OS local data dir>/cuotai/history/history.sqlite3`, private to your user, holding numbers and window metadata, never credentials, account names or provider text. On opening, each account shows its last recorded reading, marked stale, until the first query answers, and the consumption pace projects from the recent slope. The history keeps 90 days by default (`--history-days 7…365` on the command line, **Settings › Data & privacy** on the desktop), keeps one sample per hour after 7 days (each window's peak stays) and stays under 64 MiB, warning when it had to delete old samples. The desktop account details draw an area chart of each metric over the last 24 hours, 7 days or 30 days, with the last reading highlighted and a marker at each reset (and a table for screen readers); **Settings › Data & privacy** turns recording off (on by default; what was recorded stays until you erase it), shows the size on disk, exports to CSV or JSON and erases one account's history or all of it, after a confirmation. The TUI account details draw a sparkline of each metric over the last 24 hours, 7 days or 30 days (keys `1`, `2`, `3`), and `--history [ACCOUNT]` prints the recorded series (`--json` for scripts; `--history-days` picks the days shown, 7 by default).

## Local cost

**Settings › Advanced › Local cost** reads the local session logs of Claude Code and Codex CLI to count their tokens per day and per model, off by default and per source: turning a source on first shows a dialog with exactly the directories read, what for and that only numbers (time, model, token counts, an anonymous session digest) are extracted, and nothing is read until you accept it. A low-priority background scan then reads only what grew, pauses in energy-saving mode and never delays the quota refresh; the section shows what it found (logs followed, invalid lines ignored, or no access), the period shown and the index retention (120 days by default), and turning a source off or **Erase the index** removes everything derived. The same sources are managed with `cuotai --cost-*` (see [`advanced.md`](advanced.md#local-cost-from-the-command-line)); the estimated cost at API prices arrives with PRD 0015 RF3 and RF4.

<div align="center">
  <img src="assets/screenshots/desktop-settings-cost.png" alt="Settings, Local cost and Integration sections" width="420" />
  <p><em>Settings → Local cost (and, below it, Integration).</em></p>
</div>

## Tray popover and tray menu

Closing the main window only hides it: Cuotai keeps running and refreshing from the tray (macOS menu bar, Windows notification area, Linux status notifier). A left click on the tray icon opens a quick popover; a right click opens a native text menu with the accounts and actions. The popover offers a donut view (more accounts per screen) or a list view, a sort control, a refresh button, the language flag, and a footer that summarizes alerts and failures. It closes when it loses focus.

<div align="center">
  <img src="assets/screenshots/desktop-tray-donut.png" alt="Tray popover, donut view" width="300" />
  <img src="assets/screenshots/desktop-tray-list.png" alt="Tray popover, list view" width="300" />
  <p><em>The tray popover in the donut view and in the list view.</em></p>
</div>

The tray icon, its tooltip, the right-click menu and the popover also carry the update notice ("Update 0.72.0 available"), the next reset notice and, when status is on, the red outage dot. Details that depend on the operating system are in the platform guides.

## 9router and other aggregators

A 9router instance you run (see [`providers.md`](providers.md#9router-nine_router)) is one account whose upstream connections are shown as accounts of their own:

- **Adding it.** The account dialog has a **Test connection** box: it checks the address, the version, the dashboard password and the connection list, and answers with one sentence. Saving runs the test first and saves only when it connects. A self-signed HTTPS instance shows its certificate's SHA-256 fingerprint in a confirmation dialog; after you compare it with the server's, **Trust and test** pins it.
- **Main window.** Below your direct accounts, each instance gets a section: "9ROUTER · HOME" with "7 connections · 9router 0.5.109", refresh, the bell, details and **Open dashboard** (the instance's own usage page in your browser). Its connections follow as cards grouped by provider, each under the core's pool sentence ("Claude · 2 of 3 available", or when every account of a provider is exhausted, the earliest return). Connection cards have no switch of their own (the switch belongs to 9router); one that is off in 9router is dimmed with "Off in 9router". Refreshing refreshes the whole instance.
- **Tray popover.** In "All", an instance is one row with the pool figure ("5/7 available") and its tightest connection or exhausted pool; activating it expands it in place with the pool lines, the connection rows and **Open the 9router view**, the instance's full list. **Settings › Dashboard › 9router connections in All** shows it grouped (default), lists its active connections with your accounts marked "via 9router", or hides it.
- **Details.** A connection's details show its quotas, the provider in 9router and in Cuotai, "via 9router" with the reminder that 9router may zero-fill a missing value or serve a cached reading, and **Open dashboard**. The instance's details add its address (host only), version, connection count, the pools and **Alert on each connection**: off by default, an instance notifies once when every account of one provider runs out and once when one returns; on, each connection alerts like an account.

## Themes and languages

The app ships 15 themes, each with a dark and a light mode, and follows the system appearance by default. The interface is available in Portuguese, English, Spanish and Italian, and you can add your own themes and languages without recompiling. See [`themes.md`](themes.md) for every built-in theme and how to add yours.

<div align="center">
  <img src="assets/screenshots/desktop-dashboard-light.png" alt="Cuotai desktop main window in light mode" width="100%" />
  <p><em>The same window in light mode.</em></p>
</div>

## Integration: local server and hooks

**Settings › Advanced › Integrations** runs the same read-only **local server** as `cuotai --serve` from inside the app, off by default. Turned on, it listens on `127.0.0.1` only, on exactly the port you choose (6738 by default), and serves this app's own readings (`/health`, `/v1/snapshot`, `/v1/accounts/{key}`, `/v1/limits`), so reading it never queries a provider. The section shows the effective address (with **Copy address**) or why it is not serving; a taken port, for example by a `cuotai --serve` already running, is named as such there and in a banner on the main window. **Generate token** keeps a bearer token in the system vault (never in the TOML) and requires it; **Copy token** copies it without showing it, **Generate new token** stops the old one at once and **Revoke token** removes it. **Allow web browsers** asks for confirmation first, because any page open in your browser could then read your usage. See [the local server guide](local-server.md).

**Settings › Advanced › Integrations › Hooks** runs your own commands on account events from inside the app, off by default: an editor for the rules of the `hooks.toml` next to the accounts' file (name, events and the command as one field per argument, never a shell string), shared with `cuotai --watch --hooks`. Turning hooks on, or saving changed rules while they are on, shows a dialog with the exact commands, and nothing runs until you confirm it. The confirmed rules run on the events of the app's own readings (no extra query); **Test** sends a rule a sample event marked `"sample": true`, **Latest runs** lists the local run log, and a file changed outside the app asks again (with a banner on the main window). See [`events.md`](events.md#hooks-in-the-desktop-app).

Other desktop details (the accounts file, the add-account dialog, the Models section, configuration conflicts, how the shell is built) are in [`apps/desktop/README.md`](../apps/desktop/README.md).

## Statistics and limits coming back

The chart button in the main window's top bar opens the **Stats** view (`Esc` goes back). A scope menu picks all accounts or one, and a 7 d / 30 d switch picks the range. It shows three cards (limit hits, average peak of the tightest limit and extra spent per currency), the peak usage per day as bars (today in the accent colour and labelled, days at the limit in the "bad" colour, days without readings as hatched gaps), a row per account with a mini-bar per day, the 20 most recent events from the journal and a note of how many days of history are kept. Without enough history the cards read "—" with the reason instead of 0, and the chart is replaced by a sentence. Every chart has a text alternative and a table for screen readers.

"Limits come back" draws, per account, when its tightest limit returns (24 h or 7 d); later returns and unknown ones are listed as text, and choosing a lane opens that account. The tray popover shows it under the accounts, and its **Stats** button opens a compact version (the cards and the 7-day bars) with "Open full stats". Nothing here queries a provider: the figures come from the history stored on this computer (Settings → Data & privacy → History controls how long it is kept and whether it records).
