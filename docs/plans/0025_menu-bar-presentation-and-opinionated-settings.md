# PRD 0025 — Menu-bar presentation and an opinionated settings surface

| Field | Value |
|---|---|
| ID | 0025 |
| Status | Awaiting owner verification |
| Owner | `cuotai-core` (tray summary, recommended defaults), desktop (`apps/desktop`: tray, settings window, first run) |
| Consumers | `apps/desktop/src-tauri` (`lib.rs` tray), `apps/desktop/ui` (settings pages, tray popover, main-window rows), PRD 0031 (`--status-line` reuses `tray::summary`) |
| Depends on | PRD 0020 (RF1, RF2: the settings window and its sidebar registry) for RF4, RF5 and RF9; PRD 0029 (stable account ids) for RF2; PRD 0018 (RF2.6: the update question) for RF6.5 |
| Benefits from | PRD 0009 (binding), PRD 0010 (freshness), PRD 0008 (active alerts), PRD 0011 (incident), PRD 0023 (RF3: overage state), PRD 0024 (RF1: time chips; RF2: predictive alerts for the defaults), PRD 0022 (UI refresh: menu-bar page and preview visuals) |
| Extends | PRD 0020 RF2.1 (Menu bar page and the Advanced group, now DS-CMP-SEG-5), PRD 0018 RF2.6 (hosts the "ask before the first check" question) |
| Date | 2026-10-22; amended 2026-10-23 (owner decisions B, F and N; custom display mode; RF8 moved to PRD 0031; sidebar per DS-CMP-SEG-5) |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. "Menu bar" means the macOS status-bar item and, on Windows and Linux, the system-tray icon.

## Implementation status (2026-10-25)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | Delivered: `tray::summary` (four styles, Tightest % default, unknown "—", stale mark, exhausted return time, 24-character budget with "+N"), `TraySelection` with `pick`/`pruned`, `AccountView::from_report` for headless consumers (PRD 0031 RF2), `Localized` labels in the core catalogs. |
| RF2 | vNEXT | Delivered: `tray.style`/`tray.accounts` (stable ids) in `desktop.json`, pruned when accounts disappear; Auto caps 4 meters / 6 figures; the popover lists the menu-bar accounts first and the rest under "Not in menu bar". |
| RF3 | vNEXT | Delivered: macOS template image with `set_title`, cut-out notch badge, monochrome meters image and the colored variant; Linux `set_title`; Windows draws the tightest figure (two digits, "!" for an alert, "-" unknown) into the icon with the summary in the tooltip, Meters falling back to the icon; changes only when the summary changes and at most once per second; per-platform and per-desktop behavior in `docs/platforms.md`; shell tests per platform. The light/dark, Windows 11, GNOME and KDE smokes are owner checks. |
| RF4 | vNEXT | Delivered: Settings › Menu bar with option cards and live previews (`tray_preview`), "Use color", account checks with drag and Alt+↑/↓, Automatic; `tray.popover_view` in `desktop.json` with a one-time localStorage migration. |
| RF5 | vNEXT | Delivered: the Menu bar page in the sidebar (five everyday pages plus About), `settings.advanced_open`, search over every row through catalog keywords (PRD 0020 RF6); the account list with add, edit, remove and keyboard order on Settings › Dashboard (RF5.2; the dialogs stay in the main window, which Settings asks through an `account-action` event); "Other ways to connect" for a new account (RF5.4, delivered with PRD 0027 RF5.1). |
| RF6 | vNEXT | Delivered: first-run step with Turn on (permission, then the recommended usage alerts with the extra-usage notices and the 60-minute projected warning), Not now, a refused permission leaving the alerts off with the PRD 0003 guidance, the Advanced features named, the update question with its own buttons, the package manager's update command instead of the question on a managed install, `onboarding.version` and "Show again"; a request-counting test proves the first-run record never answers the update question. Owner smoke: permission granted and denied on the packaged app. |
| RF7 | vNEXT | Delivered: the summary's accessible label as the tooltip and, on macOS, the status item's accessibility label; texts in the four languages. VoiceOver pass is an owner smoke. |
| RF8 | — | Moved to PRD 0031 on 2026-10-23. |
| RF9 | vNEXT | Delivered: Settings › Dashboard › Row display with the Detailed / Standard / Minimal / Custom presets, the thirteen element switches in the DS-CMP-DENS-1 order, a live preview row from a real account (else an example), "Use a different layout in the main window" with a choice of which layout to edit, `rows.tray`/`rows.window`/`rows.separate` in `desktop.json` synced live; the figure and the meter are never hideable, hidden elements leave no gap and stay in the accessible name; Minimal shows one status by the DS-CMP-DENS-3 priority. Tests: `scripts/test-density-ui.mjs`, shell `rows_prefs_round_trip_and_read_leniently`. Note: the presets are a segmented control on the sub-page, as in the approved mockup `50-settings-display-custom`, rather than option cards on the page. |

## 1. Context

The tray item is a static image: `tray_icon(outage)` in `apps/desktop/src-tauri/src/lib.rs` (about line 1061) loads one PNG and paints a red dot for an incident (PRD 0011 RF4.2); the tray is built with `icon_as_template(false)` and never calls `set_title`. All numbers live in the popover (ring grid or list, chosen by a `localStorage` toggle in `ui/js/tray.js`) and in the tooltip. A person has to click to see whether anything is low.

Settings are being redesigned by PRD 0020 (Proposed): a dedicated window with a sidebar of seven pages (General, Dashboard, Notifications, Data & privacy, Integrations, Files, About), apply-at-once and search. PRD 0020 §4 excludes new settings. Its pages give equal weight to everyday choices and to the local server, hooks, local cost and file paths. New users also start with every notice off (owner decisions recorded for PRDs 0003 and 0008), so the features that matter most stay invisible until found.

**Market expectation.** A paid macOS monitor (WhatsMyUsage) advertises "opinionated settings: four tabs, sensible defaults, no data-source pickers, native feel", and a menu bar with style presets (Meters, Logos + %, Tightest %, Icon only), a chosen and ordered set of accounts, and a live preview. The owner decided on 2026-10-22 to keep PRD 0020's sidebar navigation; this PRD adds what is missing inside it.

## 2. Problem

- The menu bar, the one surface visible all day, carries no data.
- People with many accounts cannot choose which few matter in the menu bar, separately from the dashboard order.
- The settings surface exposes rare, technical features at the same level as everyday ones.
- Good defaults (usage alerts, predictive warnings) require finding and enabling several switches and a permission prompt.

## 3. Goals

- Four menu-bar styles from one core summary, legible on macOS light and dark menu bars, with honest fallbacks on Windows and Linux.
- A menu-bar account selection and order, separate from the dashboard, with a live preview.
- A default settings surface of about four everyday pages, with the rest grouped under **Advanced**, inside PRD 0020's sidebar and search.
- A one-step first run that turns on the recommended notices with the OS permission, asks the update question, and leaves every other privacy-relevant feature off.
- A display mode (Detailed, Standard, Minimal, Custom) that lets each person choose how much a row shows, without ever hiding the figure.

## 4. Non-goals

- Replacing PRD 0020's sidebar, window, row anatomy or apply-at-once rules.
- Platform forks of the settings layout (PRD 0020 §8 decision stands).
- Animated menu-bar items, sounds or a dock badge.
- Turning on network or file-reading features in the first run: provider status pages (PRD 0011), local cost (PRD 0015), the local server (PRD 0012), hooks (PRD 0013) and widgets (PRD 0014). The update check (PRD 0018) is **asked** in the same step as its own explicit question (RF6.5, decision F), never turned on by **Turn on**.
- This PRD **supersedes** the PRD 0020 §4 non-goal "New settings, new preferences or any change to `desktop.json`" for exactly the preferences listed in §7 (menu-bar style, colored variant, selection and order, Advanced group state, first-run record, display mode).

## 5. Functional requirements

### RF1 — Core tray summary (high priority; prerequisite)

- **RF1.1** Pure `tray::summary(accounts: &[AccountView], style: TrayStyle, selection: &TraySelection, now) -> TraySummary`, where `AccountView` is what the cycle already exposes per account (report or last good reading, freshness, binding, active alerts, incident, overage). `TrayStyle` is `Meters`, `LogosPercent`, `TightestPercent` or `IconOnly`.
- **RF1.2** `TraySummary` carries: `items` (per selected account, in selection order: account key, provider slug, binding remaining share or `None`, level `good`/`warn`/`bad` by the DS-ID-5 boundaries, `stale`, `exhausted`, `alert`, `on_extra` from PRD 0023), `title: Option<String>` (the text for `set_title`, empty for `Meters` and `IconOnly`), `badge` (outage, alert or none) and `accessible_label`/`tooltip` as `Localized`.
- **RF1.3** Style rules: `LogosPercent` is one "provider initial or glyph + %" per item ("C 22% · G 64%"; the glyph is drawn by the shell, the title holds the figures); `TightestPercent` (the **default**, decision N) is the single lowest remaining share across the selection with its account ("22% · Work") or the compact outlook chip from PRD 0024 RF1.5 when it is shorter ("↑ ≈18:53"); `Meters` renders up to four compact meters (RF3.2); `IconOnly` is the mark with the badge.
- **RF1.4** Honesty: an unmeasured or unknown account shows "—", never 0 % (DS-CMP-MTR-5); a stale reading is marked (a trailing "·" or the stale glyph) and excluded from "tightest" unless every reading is stale; an exhausted account shows its return time ("0% → 19:30").
- **RF1.5** Width budget: the title never exceeds 24 characters; items drop from the end (with "+2") before figures are truncated.
- **Acceptance criterion:** table tests per style with fresh, aging, stale, unmeasured, exhausted, on-extra and failing accounts, an empty selection, the width budget in the four languages, and the accessible labels.

### RF2 — Menu-bar selection and order (high priority)

- **RF2.1** `TraySelection` is `Auto` (every account in dashboard order, capped at 4 for `Meters` and 6 for `LogosPercent`) or `Custom(Vec<AccountKey>)` in the user's order, independent of the dashboard order (PRD 0009's "most restrictive first" never reorders it).
- **RF2.2** Stored as `tray.style` and `tray.accounts` in `desktop.json`; entries are the stable account ids of PRD 0029 (which must land first, so a rename never drops an account from the menu bar); removed accounts are pruned on save (the existing account-prune path), new accounts are appended only in `Auto`.
- **RF2.3** The popover (PRD 0022) and the menu-bar item read the same selection; the popover shows the rest under "Not in menu bar" (DS-CMP-LIST-3).
- **Acceptance criterion:** desktop tests for pruning, renaming (the account stays selected), `Auto` capping and order independence from the dashboard.

### RF3 — Native rendering per platform (high priority)

- **RF3.1** **macOS:** the item uses a template image (`icon_as_template(true)`) so it follows the light/dark menu bar; text styles use `TrayIcon::set_title` with tabular figures. The outage indicator becomes a template-compatible badge (a cut-out dot) instead of today's red paint, and an active alert adds "!" to the title.
- **RF3.2** **Meters style:** the shell renders a monochrome template image (height 16–18 pt, up to four bars, each the remaining share with a visible sliver below 1 %, DS-CMP-MTR-4) from `TraySummary.items`; no color in the template, levels shown by length only and an exhausted bar as an outline.
- **RF3.3** **Windows:** the Tauri tray title is not supported there (per the `TrayIcon::set_title` documentation, to be re-checked at implementation), so text styles fall back to an icon with the tightest figure drawn into it (up to two digits, "!" for an alert) and the full summary in the tooltip; `Meters` falls back to the icon plus tooltip.
- **RF3.4** **Linux:** `set_title` is shown by AppIndicator implementations that support labels; where it is not, the same fallback as Windows applies. The behavior per desktop environment is recorded in `docs/platforms.md`.
- **RF3.6** **Colored variant (decision N):** Settings → Menu bar offers "Use color in the menu bar" (off by default). When on, macOS uses a non-template image (and Windows/Linux the same icon) in which the meters, the ring or a small level mark take the DS-ID-5 level colors (`good`/`warn`/`bad`), checked for contrast against both light and dark menu bars; the text title stays system text. Levels are still carried by length, figures and the stale/alert marks, so the colored variant never carries meaning by color alone. `docs/design-system.md` DS-CMP-MENU-1 must gain this exception (design-system owner).
- **RF3.5** Updates: the title and image change only when `TraySummary` changes, at most once per second, never animated (DS-PRI-2).
- **Acceptance criterion:** shell tests that a summary maps to the right title/image calls per platform, in the monochrome default and the colored variant; manual smoke on macOS light and dark menu bars, Windows 11 light and dark taskbars and one GNOME and one KDE session (owner-blocked where no machine is available).

### RF4 — Settings → Menu bar page with live preview (high priority)

- **RF4.1** A new **Menu bar** page in PRD 0020's registry (RF2.4 there): four option cards for the styles (DS-CMP-CHK-3), the "Use color in the menu bar" switch (RF3.6), each with a live preview built from the current readings through a Tauri command `tray_preview(style, selection)` that calls RF1 (continuous meters in the preview, DS-CMP-MTR-3).
- **RF4.2** "Accounts in the menu bar": a list with circular checks (DS-CMP-CHK-1), drag handles for order (DS-CMP-DRAG, keyboard reorder with `Alt+↑/↓`), and the "Not in menu bar" subgroup; "Automatic" restores `Auto`.
- **RF4.3** The popover's grid/list toggle moves from `localStorage` to `tray.popover_view` in `desktop.json` so the settings window and the popover agree (PRD 0020 RF7.2).
- **Acceptance criterion:** changing style, selection or order updates the real menu bar and the preview at once; keyboard-only reordering works; copy fits PRD 0020 RF5.1's budget.

### RF5 — Everyday pages first, Advanced group (medium priority; aligned with DS-CMP-SEG-5 on 2026-10-23)

- **RF5.1** The sidebar follows `docs/design-system.md` DS-CMP-SEG-5, which supersedes PRD 0020 RF2.1: **General**, **Dashboard**, **Menu bar** (RF4), **Alerts** (usage alerts, predictive warnings, reset notices, extra usage), **Data & privacy** (history, provider status pages), a collapsible **Advanced** group (**Integrations** — server and hooks —, **Local cost**, **Extensions**, **Files**), then **About** separated at the bottom.
- **RF5.2** The account list with add, edit, remove and order (today spread between the main window and Settings → Account order) lives on the **Dashboard** page, since DS-CMP-SEG-5 has no separate Accounts page; the display mode of RF9 sits on the same page.
- **RF5.3** The Advanced group is collapsed by default and its open state is remembered (`settings.advanced_open`); search (PRD 0020 RF6) covers every row, and a result inside Advanced expands the group and opens the page.
- **RF5.4** "No data-source pickers": the account dialog opens on the recommended way to connect each provider (PRD 0027 RF5) and moves alternatives behind "Other ways to connect" (DS-CMP-CAT-3).
- **Acceptance criterion:** PRD 0020's parity checklist still maps 100 % of controls; with Advanced collapsed, the visible sidebar has five everyday pages plus About; searching "port" opens Integrations inside Advanced.

### RF6 — First-run recommended defaults (medium priority)

- **RF6.1** After the first account is saved (or on first launch with accounts already configured and no first-run record), the main window shows one step: "Get notified before you run out" with what will be turned on — usage alerts at 75 % and 90 % with exhaustion and recovery (PRD 0008), "Warn before running out" 60 minutes ahead (PRD 0024) and "extra usage started" (PRD 0023) — and two buttons, **Turn on** and **Not now**.
- **RF6.2** **Turn on** applies `AlertPrefs::recommended()` from the core and requests the OS notification permission once (PRD 0003's flow); a refusal leaves the preferences off and shows PRD 0003's guidance. Reset notices stay off (owner decision for PRD 0003).
- **RF6.3** The step lists, without enabling them, the privacy-relevant features available under Advanced (status pages, local cost, local server, hooks) in one line.
- **RF6.5** **Update question (decision F, PRD 0018 RF2.6):** the same step asks, as a separate question with its own two buttons, "Check for updates automatically?", stating what a check sends (the version, the operating system and the architecture) and that nothing installs without a click. No update request is made before the answer; **Turn on** does not answer it; closing the step leaves it unanswered and it is asked again on the next launch. When the install is managed by a package manager (PRD 0028 RF8.1), the question is replaced by the manager's own update command.
- **RF6.6** **Consent (decision B):** **Turn on** is the user's consent for the predictive warning of PRD 0024 RF2; nothing else (and never `--notify`) enables it implicitly.
- **RF6.4** The step is shown once per profile: `onboarding.version` in `desktop.json`; "Show the recommended setup again" in Settings → General resets it.
- **Acceptance criterion:** desktop tests that **Turn on** writes exactly the recommended preferences and nothing else, that **Not now** writes only the record, that no network or file-reading feature changes state, and that zero update requests happen before the update question is answered (a request-counting server); smoke with the permission granted and denied (owner-blocked on the packaged app like PRD 0003 RF3).

### RF7 — Accessibility and i18n (medium priority)

- **RF7.1** The menu-bar item exposes `TraySummary.accessible_label` ("Claude Work 22% left, resets 19:30; Gemini 64% left") as its tooltip and, on macOS, its accessibility description.
- **RF7.2** All texts in pt-BR, en-US, es and it; the 24-character title budget holds in every language.
- **Acceptance criterion:** a VoiceOver pass on macOS reads the summary (owner smoke); catalog tests.

### RF8 — Status line for terminals and bars (moved)

- **Moved on 2026-10-23 to PRD 0031 RF2** (integrations and automation contract), which owns `cuotai --status-line` together with the other terminal and bar integrations; it still renders from this PRD's `tray::summary` (RF1).

### RF9 — Custom display mode (medium priority; added 2026-10-23, owner decision; `docs/design-system.md` DS-CMP-DENS-1..5)

- **RF9.1** Settings → Dashboard gains "Rows" with four presets: **Detailed**, **Standard** (default), **Minimal** and **Custom**, as option cards (DS-CMP-CHK-3), with the element values of DS-CMP-DENS-1 for each preset. *Amended on 2026-10-25: the presets are a segmented control on the "Choose what a row shows" sub-page, as in the approved mockup `50-settings-display-custom`, not option cards on the page.*
- **RF9.2** **Custom** lists per-element switches in the DS-CMP-DENS-1 order: plan badge, account label, monogram, unit word, pace corridor, status label, ring glyph, runs-out chip, returns chip, freshness age, incident mark, extra-usage sub-row and sub-limit preview. Choosing a preset sets the switches to its values; changing a switch selects **Custom**.
- **RF9.3** **Never hideable:** the figure and the bar (meter), in every preset and in Custom; when a provider has several accounts the account label stays as the row title whatever the switch (DS-CMP-DENS-2).
- **RF9.4** **Hidden information only moves** (DS-CMP-DENS-2): it stays in the expanded row, the details, the tooltip and the row's accessible name; hidden elements leave no gap. **Minimal** shows one status chosen by priority: incident > stale > exhausted or blocked (with its return time) > runs out before the reset > pace stage (DS-CMP-DENS-3).
- **RF9.5** **Scope and storage (DS-CMP-DENS-4):** one choice applies to the tray and the main window together; a switch "Use a different layout in the main window" reveals a second preset selector for the window. Stored in `desktop.json` as `rows.tray` and `rows.window` (preset plus, for Custom, the element map; absent = Standard), synced live between windows (PRD 0020 RF7).
- **RF9.6** **Live preview (DS-CMP-DENS-5):** a preview row (DS-CMP-PREV-1) built from a real account (the first one with a pace, else a fixed sample labelled as an example), updated with every switch, without animation.
- **Acceptance criterion:** desktop tests that each preset maps to the DS-CMP-DENS-1 values and that a switch change selects Custom; that no combination hides the figure or the bar; that the accessible name of a row carries every element in every mode; Minimal's single-status priority table; `rows.window` used only when the separate-layout switch is on; a smoke in the four languages with the preview, and the setting surviving a restart.

## 6. Non-functional requirements

- **Native feel:** template images and system title text on macOS; no custom colors in the menu bar; the popover follows the theme.
- **Performance:** summary computation is pure and cheap; menu-bar updates at most once per second; no new provider query.
- **Privacy:** the menu bar shows account names only when the user selected them; e-mails never appear; the first run turns on nothing that reads files or makes requests.
- **Accessibility:** no state by color alone; figures tabular; keyboard reorder.
- **i18n:** four languages, +40 % length tolerated in settings (DS-MSG-12).

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `desktop.json` | `tray.style`, `tray.colored`, `tray.accounts` (PRD 0029 ids), `tray.popover_view`, `settings.advanced_open`, `onboarding.version`, `rows.tray`, `rows.window` | Additive, lenient read; `localStorage` tray view migrated once |
| Core API | `tray::summary`, `AlertPrefs::recommended()` | New |
| `--json`, `export::v1`, TOML | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | `tray::summary`, styles, width budget, texts, recommended preferences |
| Desktop shell | title/image/tooltip calls per platform, meter image rendering, preview command |
| Desktop UI | Menu bar page, account list on the Dashboard page, Advanced group, display mode, first-run step with the update question (visuals per PRD 0022 and `docs/design-system.md`) |

## 9. Dependencies and sequencing

RF1–RF3 can ship before PRD 0020 (the style can be set from today's settings dialog), but RF2 needs PRD 0029's stable ids. RF4, RF5 and RF9 need PRD 0020 RF1 and RF2. RF6.5 needs PRD 0018 RF2.6. RF6 needs PRD 0024 RF2 for the predictive part and PRD 0023 RF4 for the overage part; without them it turns on only PRD 0008's alerts. Visual design of the page, the preview and the first-run step follows PRD 0022.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Menu-bar space is scarce (notch on MacBooks) | 24-character budget; `IconOnly` style; items drop with "+N" |
| Windows/Linux tray limits | explicit fallbacks, documented per platform |
| Hiding Advanced makes features harder to find | search covers it; the first run names them; state remembered |
| Defaults perceived as intrusive | one explicit step with **Not now**; nothing silent |

## 11. Order of execution and verification

1. **RF1 and RF2:** core summary and the selection model.
2. **RF3:** native rendering.
3. **RF4 and RF5:** settings pages (after PRD 0020 RF1–RF2).
4. **RF6:** first run.
5. **RF9** display mode; **RF7** with each step (RF8 now lives in PRD 0031).

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, `node --check`, and a smoke test of the changed interface. The requirement is only delivered when each style shows correct, honest figures on the macOS menu bar in light and dark mode, the selection and order hold across restarts, and the first run turns on exactly the recommended notices.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Fold Dashboard into General to reach exactly four everyday pages | Keep five (DS-CMP-SEG-5); Dashboard holds the accounts, order, grouping, pace display and the display mode. |
| ~~Colored meters in the macOS menu bar~~ | **Decided on 2026-10-23 (decision N):** monochrome template by default, with a colored variant in Settings (RF3.6). |
| ~~Default style~~ | **Decided on 2026-10-23 (decision N):** `TightestPercent`. |
| ~~Exact elements of Detailed, Standard and Minimal~~ | **Set by `docs/design-system.md` DS-CMP-DENS-1.** |
| Should the first run also offer reset notices | No, per the PRD 0003 owner decision (off by default). |
