# Desktop UI inventory: the "before" state (PRD 0022 reference)

Screenshots of the current desktop UI (`apps/desktop/ui`, version 0.71.x) used as the "De" side of the redesign.
All images are 2x device pixels, default theme (Dracula) in dark mode unless the name says `light`, UI language
English. Windows were rendered at their Tauri sizes: main 1200x820, tray popover 380x600
(`apps/desktop/src-tauri/tauri.conf.json`).

All data is fixture data (example.com addresses, fake numbers). Texts such as "Consulta de uso de demonstracao"
and "pontos acima do ritmo" are Portuguese core fallbacks: the browser demo has no Rust core catalog, so
core-owned strings (pace labels, access summaries, incident states) appear in Portuguese even with the UI in English.
The app itself shows them in the chosen language.

## Fixture accounts

| Provider | Account | State shown |
| --- | --- | --- |
| Claude Code (Max) | work@example.com | 5h 66% left, week 22% left (binding), faster than pace by 20 points |
| OpenAI Codex (Plus) | me@example.com | 5h 88% left, week 63% left, in reserve |
| Cursor (Pro) | dev@example.com | Fast requests 500/500 exhausted, extra balance 6.25 USD |
| Kimi | ops@example.com | week 59% left, no 5h window (placeholder slot) |
| Groq | team@example.com | query failed, last good reading kept: "Stale 42 min ago" |
| Anthropic API | api@example.com | authentication error |

With `?status=on` the demo adds provider incidents: Claude partial outage, Cursor major outage, Groq degraded,
Kimi maintenance. Local cost (Claude Code, Codex CLI) is turned on in the fixture.

## Screens

Suffixes `-a`, `-b`, ... are consecutive scroll positions of one scrollable dialog (about 60 px overlap).

| File(s) | Window / state | What it shows and main components |
| --- | --- | --- |
| `01-tray-overview.png` | Tray popover, donut grid (default) | Header (logo, "Cuotai", list-view toggle, sort, language flag, refresh-all); 3-column grid of tiles (ring with binding-window percent or state icon, provider logo + name, account, age chip, "Stale" chip); footer with alert chips by category (errors, credentials, exhausted) and settings / open-main-window icons |
| `02-tray-incidents.png` | Tray, `?status=on` | Same grid with provider-incident badges on tiles |
| `03-tray-list.png` | Tray, list view | One row per account: logo, name, percent, account + binding window, age, pace line, bar, chevron |
| `03-tray-account-claude.png`, `04-tray-detail-claude.png`, `04-...-end.png` | Tray, one account open (Claude, incident on) | Back button, account header, age, access line, provider-status box, one card per metric (percent, used/limit, bar with expected-use tick, pace line, reset time, binding chip), Models disclosure; `-end` is scrolled to the bottom |
| `05-tray-detail-cursor*.png` | Tray detail, limit hit + balance | Exhausted metric card (orange accent), "Extra balance" balance card, Models |
| `06-tray-detail-groq-stale*.png` | Tray detail, stale reading | Stale reading dimmed with the failure cause |
| `07-tray-detail-error.png` | Tray detail, authentication error | Error state with no metrics |
| `09-tray-overview-light.png` | Tray, light mode | Light variant of 01 |
| `10-main-overview.png` | Main window, first viewport | Top bar (logo, title, filter input, system/light/dark switch, sort menu, language menu, settings, add, extensions "E", refresh-all); two-column grid of account cards |
| `11-main-overview-full.png` | Main window, full scroll height | All six cards: quota cards with 5h/week windows, percent, bar, used/limit, pace line, access line, local-cost box, footer chips (counts, binding, freshness, next refresh) and reorder arrows; stale card; error card |
| `12-main-incidents-full.png` | Main window, `?status=on` | Same plus incident lines with "Open status page" link |
| `13-main-update-available.png` | Main window, update banner (what's new) | Banner "Update 0.72.0 available", What's new disclosure (expanded), Update now / Remind me later, raw ISO release date |
| `14-main-empty.png` | Main window, no accounts | Centered "No accounts configured." with Add account button and config path |
| `15-main-narrow.png` | Main window at the 520 px minimum width | Single-column cards; the top bar does not wrap and its right-hand icons (language, settings, add, extensions, refresh-all) are clipped off-screen |
| `16-main-overview-light.png` | Main window, light | Light variant of 10 |
| `17-main-sort-menu.png`, `18-main-language-menu.png` | Top-bar popovers (cropped) | Sort options (custom, most restrictive, reset, percent) and language list with flags |
| `19-main-filter.png` | Filter typed ("cur") | Cards filtered by text |
| `20-detail-claude-a..d.png` | Account details dialog (560 px wide; body scrolls under a fixed header and footer) | Header (logo, name, email, plan badge, edit and delete icons), age, access line, provider-status box, metric cards with hide-metric eye, local-usage block (today / yesterday / period, bar chart by day, model table), History (24 h / 7 d / 30 d, area charts per metric with reset markers, lowest / highest / last), Models disclosure, Close button |
| `21-detail-cursor-a/b.png` | Details, exhausted + balance | Orange-accent exhausted card, balance card |
| `22-detail-stale-groq-a/b.png` | Details, stale reading | Dimmed reading + cause |
| `23-detail-error.png` | Details, auth error | Error without metrics |
| `24-detail-local-cost.png` | Local usage details dialog | Cost summary and models for one tool (opened from the card's search icon) |
| `30-account-edit-claude-a/b.png` | Edit account dialog | Same form as Add, filled |
| `31-account-add-default.png` | Add account dialog | Provider select (37 options, label + access level), Name, Authentication radios (environment variable / OAuth), credential env var, provider-specific fields, refresh interval, "Browse models" select, Cancel / Save |
| `32..35-account-add-*.png` | Add account with another provider selected | Provider-specific field sets (Claude, Codex, Kiro with Identity Center fields, API-key provider). The provider picker is a native `<select>`; its open list is OS-drawn and is not captured |
| `40-extensions.png`, `41-extensions-consent.png` | Extensions dialog (680 px) | List of local extensions (name, id, state, version, description, last run, Test / Disable / Enable) and the consent panel ("Review extension access", disclosure lines, checkbox, Approve and enable) |
| `50-settings-a..h.png` | Settings dialog (one 5,387 px scrolling dialog, 560 px wide, 14 sections) | a: Appearance (mode switch, theme select, animations, recharging group, pace), Language, Account order, Refresh start; b-h continue in this order: Notifications (reset notices, usage alerts and thresholds), History (recording, retention, export, erase), Privacy, Local cost (sources, directories, period, retention, price table), Integration (local server, widgets, hooks), Updates, Accounts file, Themes and languages folder, About with the Open-source libraries button (the exact split between b-h follows scroll position, not section boundaries) |
| `60-about-libraries-a..c.png` | Open-source libraries dialog (from Settings > About) | Thanks block, then libraries grouped by role with license, component and repository link |

## Not captured, and why

- Real OS chrome, native menus, the system tray icon and the tray menu (Tauri-native, outside the webview), and the open native `<select>` lists.
- In-progress OAuth/device-code login (needs the Rust core; the demo reports "Login indisponivel na demonstracao").
- Real per-provider form schemas: the demo uses one generic schema (Kiro has its own), so field sets in 31-35 are representative of the component, not of each provider's real fields. A screenshot of the real app would be needed for exact provider forms.
- The Settings > Integration, Hooks and Widgets sub-states beyond the default (server running, token generated, rules editor) and the confirmation dialogs (erase history, hooks confirmation). They exist in the demo behind `?serve=taken`, `?widgets`, `?hooks=changed` but were not photographed.
- The 30-/7-day history ranges, history warning banner (`?history-warning`), the update downloading / failed states (`?update=error`), the notification-denied state (`?notify-denied`), and non-default themes (Win98, CRT, etc.).
- Error banner, notice (account removed) and serve/hooks banners in the main window.

## Animations and motion

Tokens (`css/themes.css`): `--motion-fast` 150 ms, `--motion-base` 250 ms, `--motion-slow` 400 ms,
`--motion-spin` 900 ms; `--ease-out: cubic-bezier(0.22, 1, 0.36, 1)`; `--ease-spring` (a `linear()` spring when
supported, else `cubic-bezier(0.34, 1.56, 0.64, 1)`). All four tokens are forced to 0 with the app option "Reduced / Off"
(`data-motion`) and with `prefers-reduced-motion`; the CRT theme sets fast/base/slow to 0 (spinner keeps turning);
`motion.js` also returns 0 while the window is hidden.

CSS keyframes and transitions

| Selector | Animates | Duration | Trigger |
| --- | --- | --- | --- |
| `@keyframes spin` on `.spin`, `.models-sync:disabled svg.i` | Rotates the refresh icon | `--motion-spin` (900 ms) linear, infinite | A refresh or models sync in progress |
| `@keyframes shimmer` on `.skeleton .sk::after` | Highlight sweeping across skeleton bars | `--motion-spin` x 2 (1.8 s) linear, infinite (stopped while `:root[data-hidden]`) | Account waiting for its first report |
| `@keyframes hist-in` on `.hist-enter .hist-area, .hist-line` | Left-to-right clip reveal of the history chart | `--motion-slow` (400 ms), `--ease-out`, both | Once per answer when the History section draws |
| `.bar > span` | Bar width | `--motion-slow`, `--ease-out` | Quota value changes |
| `.bar, .win-pct, .sum-bar .big` | Level color | `--motion-fast` | Level changes (good / warn / bad) |
| `.card` | `transform: scale(1.01)` + shadow lift | `--motion-fast`, `--ease-out` | Hover or focus-within |
| `.switch` | Track background | `--motion-base`, `--ease-out` | Enable / disable toggle |
| `.switch span` | Knob `translateX` with overshoot | `--motion-base`, `--ease-spring` | Enable / disable toggle |
| `.donut .arc` (tray) | Ring `stroke-dasharray` | `--motion-slow`, `--ease-out` | Tray value changes |
| `::view-transition-old/new(root)` | Cross-fade of the whole page | `--motion-base` | Theme or language change (`crossFade`) |
| `.stale-reading` (`detail.css`) | Disables the bar transition | none | Preserved stale reading stays still |

Web Animations in JS

| Where | Animates | Duration | Trigger |
| --- | --- | --- | --- |
| `motion.after` entrance | Card opacity 0 to 1 + `translateY(8px)` to 0, 30 ms stagger (cap 300 ms total) | `--motion-base` | New card appears (`data-card` key not seen before) |
| `motion.after` FLIP | Card slides from old to new position (`id: "flip"`) | `--motion-base` | Reorder, sort, filter or drag/arrows |
| `motion.after` level flash | Outline in the new level color fades out | `--motion-slow` x 1.5 | A card's worst level changes |
| `motion.after` exhausted shake | `translateX` -4 / +4 / 0 | `--motion-slow`, ease-in-out | A card becomes exhausted (not on every refresh) |
| `motion.after` exit | Removed card becomes a fixed ghost: opacity 1 to 0, `scale(1)` to `0.92` | `--motion-base` | Card removed or filtered out |
| `motion.countUp` | Percent text counts from the shown value to the new one (cubic ease-out, rAF) | `--motion-slow` | Quota percent changes (not on stale readings) |
| `motion.spinOnce` | One 360 degree turn of the refresh icon | `--motion-spin` | Click on refresh / refresh-all |
| `motion.pulseIfDue` | Next-refresh chip scales 1 to 1.15 to 1 | `--motion-base` x 2 | Countdown reaches zero (once per deadline) |
| `motion.showDialog` | Dialog and `::backdrop` fade, scale 0.96 to 1 (or grow from the originating card) | `--motion-base` in; `--motion-fast` out (reverse, inert ghost as manual popover) | Any dialog open and close |
| `tray.js` focus handler | Popover fade + 6 px slide from the icon side (top on macOS, bottom elsewhere) | `--motion-base` | Tray popover shown (window `focus`) |
| `tray.js` `animateRings` | Ring arc animates from the last shown value to the new one | `--motion-slow` | Tray reopened or re-rendered with changed values |

## How the screenshots were generated

No earlier method exists for PRD 0020's images (`0020_settings-current.png` and `0020_settings-proposed.png` were added
in one docs commit with no script). The UI has a built-in browser demo: when `window.__TAURI__` is absent, `js/lib.js`
answers every Tauri command from in-memory fixtures. These screenshots reuse that, with a patched temporary copy so the
repository stays untouched:

1. `prepare.py <worktree>` copies `apps/desktop/ui` to `$TMPDIR/cuotai-ui-shot/ui` and, in the copy only:
   replaces the `demoReports`, `DEMO_BINDINGS` and `DEMO_PACES` blocks of `js/lib.js` with the six example.com
   accounts above; turns the two local-cost sources on (`demoCost.on`); adds an `?empty` switch that yields zero
   accounts and no cost sources; returns two example extensions from `list_extensions` and three disclosure lines from
   `extension_disclosure`; and fixes the tray page for the demo (`js/tray.js` calls `render` synchronously from
   `onUpdateState` before its `let` bindings exist, a temporal-dead-zone error that blanks the tray in a browser; the
   copy defers the call with `setTimeout`). The stock demo fixture also contains a real e-mail address, so it was not used as is.
2. `python3 -m http.server 8765 --bind 127.0.0.1 --directory $TMPDIR/cuotai-ui-shot/ui` (background; stopped afterwards).
3. Google Chrome headless with a throwaway profile:
   `"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --remote-debugging-port=9333 --user-data-dir=$TMPDIR/cuotai-ui-shot/profile --hide-scrollbars --lang=en-US about:blank`.
4. Node scripts (Node 22+, built-in `WebSocket`, no dependencies) drive Chrome through the DevTools protocol:
   `Emulation.setDeviceMetricsOverride` (window size, `deviceScaleFactor` 2), `Emulation.setEmulatedMedia`
   (`prefers-color-scheme`, `prefers-reduced-motion: reduce` so no frame is captured mid-animation),
   `Page.navigate`, `Runtime.evaluate` to click (`[data-settings]`, `[data-details^=claude]`, `[data-add]`,
   `[data-extensions]`, `[data-cost-details]`, `[data-open=...]` in the tray) and set the provider select, and
   `Page.captureScreenshot` with a `clip` of the open `<dialog>` rect. Long dialogs are shot in scroll steps of
   (client height - 60 px) on the real scroller (`.det-body` for details, the dialog itself for Settings).
5. URLs (all `http://127.0.0.1:8765/`): `index.html?update=fresh` (no update banner), `&status=on` (incidents),
   `&empty=1`, no `update` parameter for the banner, `tray.html?update=fresh[&status=on]`. Light mode uses
   `localStorage.cuotai.themeMode = "light"` plus the emulated media feature.

The scripts (`prepare.py`, `cdp.mjs`, `lib.mjs`, `partA..E.mjs`, `partT.mjs`) were left in `$TMPDIR/cuotai-ui-shot/`,
outside the repository. A small committed `apps/desktop/tools/shoot/` (prepare + driver + a list of screens) would make
the "after" set a one-command regeneration; it is proposed in the report, not added here.

Caveats: the browser demo is not the packaged app, so fonts (system font stack of Chrome on macOS), native
scrollbars (hidden), window chrome and OS-drawn controls differ slightly; layout, spacing, colors and component
structure come from the real CSS and JS.
