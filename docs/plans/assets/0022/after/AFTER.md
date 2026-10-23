# PRD 0022: the "after" mockups ("Para")

Static mockups of the desktop UI with the new design system (`docs/design-system.md`, rule IDs `DS-*`) applied, paired with the
"before" screenshots in `../before/` (inventory in `../before/INVENTORY.md`). Every PNG is 2x device pixels, default theme
Dracula (dark) unless the name says `light` or `cuotai` (the app's pastel theme, `01-tray-overview.cuotai[-light]` and `10-main-overview.cuotai[-light]`), UI language English unless the suffix is `.pt-BR`, `.es` or `.it`.

These are **design references, not an implementation**. They are generated from static HTML in `src/` that links the real theme
tokens (`apps/desktop/ui/css/themes.css`, `css/themes/*.css`), the real logo (`img/logo.svg`) and the real provider glyphs
(`img/providers/`). No hex color is defined outside those files; the only literals are the provider brand tile colors copied from
`js/lib.js` (DS-ID-3) and the `#000` inside the `--scrim` / `--shadow-card` defaults. Core-owned texts (pace labels, access
summaries, incident states) are English approximations of what the Rust core will return through `Localized` (DS-MSG-6).

Fixture data (example.com only; the demo fixture in `apps/desktop/ui/js/lib.js` now uses `alex@example.com`, these mockups keep their own addresses), "now" is Thu 22 Oct 2026, 18:21: Claude Code Max `work@example.com` (weekly 22% left, binding, 20
points ahead of pace), Claude Code Pro `side@example.com` (71% left, added so the multi-account case shows), OpenAI Codex Plus 63% left,
Cursor Pro limit hit on extra balance $6.25, Kimi 59% left (no 5 h window), Groq stale last-good reading (42 min), Anthropic API
key rejected.

## Regenerate the PNGs

Needs Node 22+, Python 3 and Google Chrome. From the repository (or worktree) root:

```sh
sh docs/plans/assets/0022/after/src/render.sh            # build the HTML and render every screen listed in src/screens.tsv
sh docs/plans/assets/0022/after/src/render.sh 01 10 50   # only screens whose name starts with 01, 10 or 50
node docs/plans/assets/0022/after/src/build.mjs          # rebuild only the HTML (src/*.html + src/screens.tsv)
CHROME="/path/to/chrome" MOCKUP_PORT=8861 sh docs/plans/assets/0022/after/src/render.sh   # overrides (default port 8861)
node docs/plans/assets/0022/after/src/check.mjs > docs/plans/assets/0022/after/i18n-check.json   # overflow check of all 71 pages (own server and Chrome, stopped on exit)
```

The overflow check is `node docs/plans/assets/0022/after/src/check.mjs > docs/plans/assets/0022/after/i18n-check.json` (spawns its own server and Chrome). `render.sh` serves the repository root on `127.0.0.1:$MOCKUP_PORT` (the pages load glyph masks over http; `file://` is blocked), runs
`chrome --headless=new --screenshot --window-size=W,H --force-device-scale-factor=2` once per screen and stops the server on exit.
Window sizes come from `src/screens.tsv` (name, width, height; the PNG is twice that). Sources: `src/lib.mjs` (icons, tiles, meter,
ring, timeline, tray shell), `src/fixtures.mjs`, `src/tray.mjs`, `src/main.mjs`, `src/dialogs.mjs`, `src/settings.mjs`,
`src/stats.mjs`, `src/common.css` (the "new" tokens of DS section 2 and the components of section 4).

## Owner decisions applied (revision of 2026-10-22)

| # | Decision | Where it shows |
|---|---|---|
| 1 | Tray default view is the list. Alternatives **Rings** and **Compact** exist and are chosen in Settings > Dashboard, not in the tray header. The single ring stays in the "All" tab and as the "Icon only" menu-bar style. | `01-tray-overview` (list), `01c-tray-rings-view` (per-account static rings, UI-font figures, DS tiles, no animation), `01d-tray-compact-view` (44 px rows, continuous 72 px bar, state icon, figure), `50-settings-dashboard` (view option cards) |
| 2 | Footer age = last completed refresh; a refresh button sits next to it. | Every tray screen: `1 min ago` plus an icon button in the footer (DS-CMP-FOOT-1, -2) |
| 3 | With more than one account of a provider the row title is the account label; the tile shows the provider. | The two Claude rows in `01`, `01c`, `01d`, `02`, `03`, `09`, the two Claude cards in `10`/`11`/`13`/`15` (provider name moves to the muted second line), `20` header, `70` list, `50-settings-menubar` list. Sub-limit meta lines wrap to two lines when long. |
| 4 | Neutral big figures stay. | all |
| 5 | Settings sidebar as proposed, more pages allowed, plus a Dashboard page with the moved controls. | `50-settings-dashboard`: tray view, sort, grouping of exhausted accounts, pace, refresh interval, and quick access to mode and language |
| 6 | Header simplifications accepted. | tray and main headers (no list/grid toggle, sort, flag, refresh-all in the tray header; theme and language switches only in Settings) |
| 7 | `--shadow-card` kept for main-window cards. | `10`, `11`, `13`, `15`, `14` cards |
| 8 | Warning callout: lighter tint plus a warning-colored border. | `common.css` `.call.warning` = 6% warn mix on `--surface` and a 1 px border of `--warn` mixed 55% with `--surface`. Visible on `03`, `05`, `06`, `20`. DS-CMP-CALL-1 ("no border") now has this exception for the warning tone only. |
| 9 | The pace marker snaps to a segment boundary. | Superseded on 2026-10-23: meters are continuous, so the mark is the exact pace corridor/tick (see the 2026-10-23 revision) |
| 10 | Smaller choices accepted (see the decisions list below). | - |

New files in this revision: `01c-tray-rings-view.png`, `01d-tray-compact-view.png`, `50-settings-dashboard.png`.
All other PNGs were re-rendered (footer, titles, callout, marker).

## Revision 2026-10-23: continuous bars, flags, i18n, identity ideas

Rendered from the design system as it stood on 2026-10-23 (`docs/design-system.md`, which the DS agent was still extending; where it was more specific than the coordinator note the DS wins).

### 1. Continuous bars everywhere

Every meter is now one solid, complete bar (DS-CMP-MTR-2, "Cuotai's own identity, not borrowed from other monitors"): tray rows, sub-limits, cards, details, the stale and error states, the menu-bar "Meters" preview and the compact tray view. Nothing segmented remains. Per-account rings (01c) are one open C arc (DS-CMP-RING-1: "never more than one arc"), and the "All" tab ring is 16 px. The exact expected share is shown by the **pace corridor** on 6 px bars and a 2 px tick on 4 px bars (compact view, menu bar); unknown values are an empty hatched track with "—"; stale bars are at 50% opacity with a dotted outline and no pace mark.

Stacked unit word (owner, 2026-10-23): in every account row and card the unit word ("left", "disponível", "disponible", "disponibile", and "extra" for money) is a 10 px muted caption (`--text-nano`, a new step below `--text-micro`; normal weight, line-height 1.1) **below** the big figure, forming a right-aligned two-line block in rows (about the height of the figure) with the chevron centered on it; in cards the figure and its caption are left-aligned in the window column. The freed width lets account labels show more (e.g. "me@exampl…" instead of "me…" in pt-BR on `01`). Not stacked, on purpose: the compact tray view (`01d`, one-line rows), sub-limit headers and the Stats lists, which stay single-line.

### 2. Language selector with flags

`50-settings-general.png` shows the language field with flag and endonym; `50-settings-general-language.png` (and `.pt-BR`, `.es`, `.it`) shows the open menu with the endonym of each real catalog (`_meta.name`): Português with the Brazil flag, English with the United States, Español with Spain, Italiano with Italy. `50-settings-general-language-14.png` shows the same menu with the 14 languages of PRD 0035 and their endonyms (中文（简体）, Français, Deutsch, 日本語, 한국어, हिन्दी, Bahasa Indonesia, Русский, Türkçe, Tiếng Việt); flags with a white field (Japan, South Korea, Indonesia) carry the 1 px `--border` hairline of DS-CMP-SEL-3, the others none. The artwork for cn, kr, in, id, tr and vn is drawn in `src/settings.mjs` (simplified), because `js/flags.js` does not have it yet; `flagCountry()` also needs ko, vi and tr in its language map. The flags are the app's own (`apps/desktop/ui/js/flags.js`, `flagSvg` and `flagCountry`, region wins: pt-BR becomes br). The main topbar no longer has a language menu, so there is no second place to put one; Settings > Dashboard "Quick access" shows the same field with a flag.

### 3. Identity ideas applied

| Idea | Where it appears |
|---|---|
| Pace corridor (+/- 5 points, DS-CMP-MTR-6; tick on 4 px bars, MTR-6a; none when stale/unknown/exhausted, MTR-6b) | All 6 px meters of `01`, `02`, `03`, `05`, `10`, `11`, `13`, `15`, `20`, `09`; the 4 px bars with a tick in `01d-tray-compact-view`; absent on Groq (stale), Cursor (exhausted), Kimi 5 h (unknown) |
| C-ring as account-state glyph (DS-CMP-RING-6): 12 px in the status line, 12 px at the tile corner of the filter tabs, always with text; icon for non-quota states (key for sign-in, alert for incidents, wrench for maintenance); empty tinted track for exhausted, dotted and dim for stale | Status lines of every row and card; tab corners in all tray and main screens; compact view marks; journal rows in `70`; `07b` (alert icon on the Kimi tab) |
| Honest data (DS-CMP-HON): unknown hatch, "≈" on estimates and projections, dotted outline plus dim for stale | Hatched track and "—" on Kimi 5 h (`10`, `15`); hatched ghost column "No data" on Monday in `70`; hatched Tuesday (unpriced) in `20`; "≈" on local cost (rows, cards, KPI cards, daily bars in `20`), on the projected run-out time (chips, callouts) and on the stats average "≈52%"; dotted outline and 50% opacity on the Groq meter and ring (`01`, `06`, `10`, `11`) |
| Shared time chips with the TUI shapes (DS-CMP-TIME): "↺" returns chip and "↑" runs-out chip, plus ↓ in reserve, → on pace, ✕ exhausted. Wording is the design system's table, in the four languages | Compact chips (glyph + time, DS-CMP-TIME-3) on collapsed rows; full chips ("Returns Sun 17:20" form) are `chip.*` in `src/strings.mjs`; cards (`10`) show the full "Runs out ≈Sat 10:20", "Lasts to reset", "8 pts in reserve" chips. No "empty ~HH:MM" or "vs pace" wording anywhere |
| Account monogram (DS-CMP-MONO-1..3) when a provider has several accounts: "WO" and "SI" (first two letters of the one-word label, per the rule; the user may override them, MONO-4) | The two Claude accounts on tray rows, cards (`10`, `11`, `13`, `15`), details header (`20`), compact view, Stats list (`70`), menu-bar account list (`50-settings-menubar`) |

| CuotAI logotype in headers (DS-ID-2; shipped as `wordmark()`) | Main topbar at 16 px in `10`, `11`, `13`, `14`, `15`, every locale and the Cuotai-theme variants; four-arc C from `img/logo-on-dark.svg` or `-on-light.svg` (copied into `src/img/`), "uot" in the text color, "AI" in `#ff79c6` (dark) or `#990058` (light), normal letter spacing, accessible name "Cuotai". The tray overview header does not carry it (see decision 20) |

Not applied: the monogram on the 14 px timeline lanes (too small to read; the lane label carries the account short name) and on provider filter tabs (one tab per provider, the count is shown instead); the menu-bar items stay monochrome glyphs (DS-CMP-MENU-1). The state ring is a single open C arc starting at the logo's lower right and running clockwise (the DS text says 12 o'clock; the logo's "C" opening is kept).

### 4. New-feature components shown

| Component | Screen |
|---|---|
| Extra-usage sub-row (DS-CMP-EXTRA-1): label, "On extra · $6.25" warning chip, continuous meter of the remaining extra budget in `--warn` without pace mark; same chip on the card foot | `05-tray-detail-cursor-extra`, `10`/`11`/`15` (Cursor card) |
| Extra spent KPI card (DS-CMP-EXTRA-3) | `70-stats` (and `.pt-BR`, `.es`, `.it`) |
| "Found on this device" chip (info role, check icon, DS-CMP-CAT-2) and "Other ways to connect" disclosure (DS-CMP-CAT-3) | `31-account-add-catalog` (Claude Code, OpenAI Codex rows, all locales), `32-account-add-claude` |
| Diagnostic block "What changed" (DS-CMP-DIAG-1) and "Report a problem" entry; the report dialog with the read-only preview and "Nothing is sent by Cuotai; GitHub opens with this text" (DS-CMP-RPT-1) | `07b-tray-error-schema` (new), `07c-report-problem` (new) |
| "Verified N months ago" chip on the access line (DS-CMP-VER-1) | `20-detail-claude`, `07b` |
| Menu-bar live preview with continuous meters (monochrome template look, OS foreground color) and the 18 pt ring in the "Icon only" card (DS-CMP-MENU-2, PREV-1) | `50-settings-menubar` (all locales) |

### 5. i18n length validation

`src/i18n.mjs` and `src/strings.mjs` render the key screens in pt-BR, es and it next to the unsuffixed English set. A string comes from (1) the real UI catalogs (`apps/desktop/ui/locales/<loc>.json`), (2) the core catalogs (`crates/cuotai-core/locales/<loc>.json`; pt-BR has none, so its core strings are the Rust fallbacks copied from `pace.rs`, `access.rs` and `incidents/status.rs`), or (3) a **draft** translation written for this document. Dates and times use `Intl` as the app does (`31 de out., 02:20`, `dom. 17:20`, `há 1 min.`), money uses `Intl` currency (`US$ 6,25`, `6,25 US$`, `6,25 USD`).

Per locale: 35 strings from the UI catalogs, 16 from the core (Rust fallbacks for pt-BR), 159 drafts (all drafts, listed by id in `src/i18n-usage.json` and defined in `src/strings.mjs`). The drafts are the wording of features that do not exist yet or were rewritten (pace chips follow the design system's table; stats, alerts, catalog hints, diagnostics, extra usage, forecast callouts, nav labels). They need a native-speaker review before shipping; none of the drafts is a real catalog string.

Locale files (suffix `.pt-BR`, `.es`, `.it`): `01-tray-overview`, `03-tray-account-claude-expanded`, `05-tray-detail-cursor-extra`, `10-main-overview`, `15-main-narrow`, `31-account-add-catalog`, `50-settings-general`, `50-settings-general-language`, `50-settings-menubar`, `50-settings-alerts`, `70-stats` (11 screens x 3 = 33 PNGs).

**Overflow detection** is `src/check.mjs` (`node src/check.mjs > i18n-check.json`): headless Chrome over the DevTools protocol loads every page at its window size and flags (a) text whose box is wider than its container with ellipsis or hidden overflow, (b) text or controls outside the window, (c) text clipped by an ancestor, (d) text spilling out of its chip, button, KPI or card, (e) overlapping text boxes (clipped to scroll containers). Only account labels and names may ellipsize (they carry a `title`). The first run (`i18n-check-first-run.json`) found 39 problems on the first locale build; later runs after each fix found the rest below. Final run (round 3): **0 problems** on 71 pages; 40 allowed ellipses, all account labels, and 10 status lines that yielded in the DS-CMP-STL-5 order (recorded, not problems).

| Screen | Locale | Issue found | Fix |
|---|---|---|---|
| 01, 03, 05, 10, 15 (status lines) | pt-BR, es, it | The detail after the label ("· no saldo extra", "· dura fino al reset", "· se agota sáb 10:20") was ellipsized (13 in es, 12 in pt-BR, 9 in it on the first run) | Status line may wrap: detail and chips wrap inside the line instead of truncating (`.stl` flex-wrap, `.det` normal white-space); structural change to DS-CMP-STL-5 (see below) |
| 01, 03 | es | Returns chip "↺ dom 17:20" pushed out of the 380 px window (Por encima del ritmo + runs-out chip + returns chip) | Same wrapping: the returns chip drops to a second line, right-aligned |
| 01 (tray footer) | es, pt-BR, it | "Configuración ⌘," + "Estadísticas" + "hace 1 min" + refresh button did not fit: the refresh button was cut off at the right edge | Keycap dropped from the footer button (it stays in `aria-keyshortcuts` and the tooltip; DS-CMP-FOOT-1 says "with its keycap" and needs the exception); padding tightened |
| 01 | es | Row figure "Inicio de sesión requerido" (sign-in state) overflowed the row | State words that are not numbers wrap: right-aligned, at most 52% of the row, `--text-lg` |
| 01, 03, 05, 10, 15, 50, 70 | pt-BR, es, it | Account labels cut ("work@exam…") | Allowed: account labels ellipsize with a `title` tooltip (DS-CMP-ROW-1); also in Stats and nav labels |
| 01, 03, 05 | pt-BR, es, it | After stacking the unit, the title line is narrower in no case (the unit no longer sits beside the figure); the figure block is the widest item ('US$ 6,25' on `01`) | No change needed; final run 0 problems |
| 10 | pt-BR, es, it | Topbar (1200 px) overflowed: sort button "Personalizada (arrastar)", search field and "Añadir cuenta" pushed the extensions and settings buttons off the window | Sort menu becomes an icon button with the label in its tooltip; the search field flexes (120-216 px) |
| 50-settings-* | pt-BR, es, it | Search field placeholder "Buscar configurações" / "Buscar en la configuración" / "Cerca nelle impostazioni" ellipsized | Placeholder shortened to "Buscar" / "Cerca" (the accessible name stays "Search settings") |
| 50-settings-general-language | all | Open menu overlays the field by design; reported as overlap | Overlay menus excluded from the check |
| 70 | pt-BR, es, it | Extra spent KPI "US$ 6,25" at 24 px spilled out of its card | KPI figures longer than 6 characters step down to `--text-xl` (proposed rule below) |
| 05 | pt-BR, es, it | "Uso extra" label wrapped to two lines and "restantes de US$ 20,00" ran outside the sub-row | Sub-limit header may wrap (`.s1` flex-wrap): label and chip on the first line, figure on the second; label is `nowrap` |
| 50-settings-alerts | pt-BR, es | The "Saved" toast overlapped the last row ("Notificação de teste", "Notificación de prueba") after the notification preview made the page longer | Page height 720 to 960 (the whole page shows; in the app it scrolls inside 640 px) |
| 10, 15 (round 3 logotype) | all | The visually hidden accessible name of the logotype (`.wm-name`, 1 px clip) was reported as truncated text | Hidden accessible names are excluded in `check.mjs` (like the overlay menus) |
| 15 | pt-BR, es, it | Card window blocks: long uppercase labels ("REQUISIÇÕES RÁPIDAS") wrap with the "Mais restritivo" chip | Allowed: window label wraps, the chip stays whole |

### 6. Components that cannot fit pt-BR (or es, it) at 380 px without a structural change

1. **Status line (DS-CMP-STL-1, -5).** Label + core detail + two chips + reset do not fit one 304 px line in pt-BR ("Limite atingido · no saldo extra" + "↺ 31 de out., 02:20"). Proposal: the line wraps to a second line (chips and detail drop, the returns chip stays right-aligned); the DS says "truncate the detail first", which the owner's rule ("nothing important truncates") overrides. Cost: collapsed rows grow from about 80 to about 108 px in the longest case.
2. **Footer (DS-CMP-FOOT-1).** Settings (with keycap) + Stats + age + refresh need about 400 px in es and pt-BR. Proposal: no keycap in the footer; if it still overflows (a user language pack with longer words) Stats becomes icon-only with a tooltip.
3. **Sub-limit meta line (DS-CMP-SUB-1)** already wraps; with the real core text ("20 pontos acima do ritmo") plus "Reinicia dom. 17:20 · em 2d 23h" it always takes two lines in pt-BR. Proposal: make two lines the specified layout at 380 px.
4. **KPI figures (DS-CMP-KPI-1)**: three cards per row at 380 px leave about 85 px of figure width; amounts such as "US$ 6,25" need the step-down rule above (or a container-query font size in the real app).
5. **Time chips (DS-CMP-TIME-1)**: the "≤ 18 characters" budget holds, but the compact "glyph + time" form is what lets rows fit: "↺ 31 de out., 02:20" is already 18 characters in pt-BR. Proposal: in compact form use a short date ("31/10 02:20") for dates beyond the week.
6. **Main topbar at 1200 px** with a four-language build needs the sort control as an icon (done) and, below about 1100 px, the same treatment for "Add account"; the 520 px two-row layout (`15`) already covers narrow windows.
7. **Real access summaries** ("Janelas de uso do plano Claude pela interface interna usada pelos clientes oficiais.") are 80-90 characters; they wrap to two lines in rows and cards. They cannot be one line at 380 px; proposal: show the access level on the row and keep the summary in the details (tooltip in the row).

### 7. New and changed files (2026-10-23)

PNGs added: `07b-tray-error-schema`, `07c-report-problem`, `50-settings-general-language`, and the 33 locale renders (`<name>.pt-BR.png`, `.es.png`, `.it.png`, plus `50-settings-general-language.<loc>.png`). Changed (re-rendered): every English PNG (continuous bars, state rings, chips, monogram, hatch, "≈").
Sources added: `src/i18n.mjs`, `src/strings.mjs`, `src/lib-paths.mjs`, `src/check.mjs`, `src/i18n-usage.json`; `src/build.mjs` now renders the locale sets; `../i18n-check.json` (final run) and `../i18n-check-first-run.json` (first run, before the fixes). Regenerate with `sh src/render.sh` and check with `node src/check.mjs > i18n-check.json`.

### 8. Owner feedback on `01-tray-overview.es` and the new display screens (2026-10-23, later)

| # | Feedback | Done |
|---|---|---|
| 1 | The pace corridor looked like a slider thumb | A translucent band (`--fg` at 28%), no border, 1 px radius, bar height plus 1 px above and below, `pointer-events: none` (final DS-CMP-MTR-6) |
| 2 | Status line wrapped and pushed the reset chip to a second line; rows must never reach 3 lines | One fixed 20 px line, following DS-CMP-STL-5 in its final order, run by a small script in each page (`FIT` in `src/lib.mjs`): (1) the label switches to its compact form ("↑ 20 pts", "↓ 8 pts"), (2) the detail and the runs-out chip are dropped, (3) the returns time is already compact text without a chip background ("↺ dom 17:20"), (4) the label truncates. The returns time never moves or truncates. The check records every step taken and fails a status line taller than one line. On the key screens: es 01 and 03 use the compact label and keep the runs-out chip; pt-BR 03 compact label; the detail "· no saldo extra" (pt-BR 01, 05), "· con saldo extra" (es 01, 05) and "· su saldo extra" (it 05) is dropped; English: none. Full text stays in the row tooltip |
| 3 | The tab rings overlapped the provider logos | The 10 px state ring (DS-CMP-TAB-2) now sits outside the tile, to its top right, with a 2 px gap; the "+" button left the tray header (DS-CMP-HEAD-2: "the header holds only the filter tabs") to make room for the six tabs plus rings in 380 px in all languages. Where add-account lives in the tray is open (main window, Settings, or a footer menu) |
| 4 | Custom display | `50-settings-dashboard` has a "Row display" group (Detailed / Standard / Minimal / Custom and a "Choose what a row shows" entry); `50-settings-display-custom` is that sub-page: preset control (Custom selected), a live preview row built from the current toggles, and 13 element switches (plan badge, account label, monogram, unit word, pace corridor, status label, state ring, runs-out chip, returns time, freshness age, incident mark, extra-usage row, sub-limit preview). `01e-tray-minimal-view` renders the Minimal preset for all seven accounts (figure, bar, one status). The presets follow DS-CMP-DENS (Minimal keeps the monogram, hides the account label of single-account rows, and shows ONE status by priority: runs out before the reset for the Claude work account, the pace stage elsewhere, exhausted with its return time for Cursor, stale for Groq). They are defined in `src/fixtures.mjs` (`PRESETS`, `display()`) |
| 5 | Percent mode | `50-settings-dashboard` has "Show pace as: Points / Percent" with the worked example (expected 50%, actual 60%: "+10 pts ahead" vs "20% faster"). `01f-tray-percent-mode` shows the tray in percent mode: chips "↑ 34% faster" and "↓ 18% slower", and the expanded sub-limits "34% faster than pace" and "39% slower than pace" (draft strings `chip.*_pct`, `pace.*_pct`) |

Round 3 update of item 5: percent chips now use the design system's final wording ("20% faster than pace", "20% slower than pace", up to 22 characters, DS-CMP-TIME-1/2), and in percent mode the status label of the row itself is the percent text (compact form "↑ 34%"), not the points label plus an extra chip. The worked example in `50-settings-dashboard` reads "10 pts ahead" vs "20% faster than pace", with the one-line explanation under the control.

Also: the default port of `render.sh` is now 8861 (8791 was taken by another agent's server and made the render produce 404 pages). The check was re-run after all of this: 0 problems on 66 pages.

### 9. Round 3 (2026-10-23, after the process restart)

Resumed from the stopped round: everything of sections 1 to 8 was re-rendered (71 screens, 71 PNGs) and re-checked. What was added:

| Item | Where |
|---|---|
| CuotAI logotype in the main headers (decision 20 for the tray) | `10`, `11`, `13`, `14`, `15` and all locale sets; assets `src/img/logo-on-dark.svg`, `src/img/logo-on-light.svg`, CSS at the end of `src/common.css` (copied from the shipped `themes.css`, paths relative to `src/`) |
| Cuotai pastel theme variants (dark and light), built by `build.mjs` from the Dracula HTML with `src/themes/cuotai.css` (copy of `apps/desktop/ui/css/themes/cuotai.css` on main; the design branch does not have it yet) | `01-tray-overview.cuotai`, `01-tray-overview.cuotai-light`, `10-main-overview.cuotai`, `10-main-overview.cuotai-light` |
| Settings > Dashboard: account list and management (DS-CMP-SEG-5: grip, tile with monogram, label, plan, edit, remove, enable switch, "Add account", drag helper line, order shared with the tray) and "Use a different layout in the main window" with its dependent preset selector visible and disabled (DS-CMP-DENS-4, DS-CMP-DEP-1) | `50-settings-dashboard` (860 x 1620) |
| Menu bar "Colored" switch, off by default (DS-CMP-MENU-4) | `50-settings-menubar` and locales |
| OS notification preview with the 16 px state ring, title and body from core-style keys (DS-CMP-NOTIF-1/2) | `50-settings-alerts` and locales |
| 14-language menu with endonyms and flag hairlines (DS-CMP-SEL-3) | `50-settings-general-language-14` |
| Percent chips with the final wording (DS-CMP-TIME-2) and a percent status label | `01f-tray-percent-mode`, `50-settings-dashboard` |
| Token name aligned with the design system: `--text-2xs` (10 px) instead of `--text-nano` | `src/common.css` |

The mockups use three literals outside the theme files, all multicolor brand artwork exempted by DS-COL-1: the provider tile colors, the flag artwork and the logotype's "AI" pink.

## Before and after

"New" in the last column marks a capability that does not exist today and is planned in an upcoming PRD (the screen shows the
design intent, not shipped behavior). "none" means the screen only restyles what exists.

| Before | After | What changed and why | Kept for identity | Motion removed or simplified | New capability |
|---|---|---|---|---|---|
| `01-tray-overview.png` (donut grid) | `01-tray-overview.png`, `01b-tray-overview-scrolled.png`; alternatives `01c-tray-rings-view.png`, `01d-tray-compact-view.png` | One list, hairline-separated rows (DS-CMP-ROW-1, -3) instead of a 3-column grid of boxed donut tiles: provider tile, plan badge, account, "N% left", continuous meter with the pace corridor (DS-CMP-MTR-2, -6), one status line with reset time (DS-CMP-STL-1..4). Header is the filter: ring "All" tab plus one icon tab per provider with count and attention dot (DS-CMP-TAB-1..3); list/sort/language/refresh buttons leave the header (DS-CMP-HEAD-1), refresh and its age move to the footer (DS-CMP-FOOT-1, -2). Fresh age is no longer repeated per row; only the stale Groq row shows one (DS-CMP-ROW-4). `01b` is the same body scrolled to "Limits come back". `01c` is the Rings view (the old per-account donut grid, restyled: static rings, 3 columns, tile + title + one status line) and `01d` the Compact view (one 44 px line per account); both are chosen in Settings > Dashboard. The footer has the last-refresh age and a refresh button. With two Claude accounts the row title is the account label. | Dracula tokens, logo, brand tiles with white glyph (DS-ID-3), the ring as the "C" (decision 1), "left" criterion (DS-ID-5), 4 locales, per-account rows for every account | Ring fill animation (`animateRings`), popover slide-in, count-up and level flash removed (DS-MOT-1, -5; inventory 9, 10, 12, 13) | Provider filter tabs; reset timeline "Limits come back" 24h/7d (DS-CHT-6); Stats entry in the footer; Rings and Compact views (the Rings view exists today as the donut grid). **new** |
| `02-tray-incidents.png` | `02-tray-incidents.png` | Incident is the single extra line of a collapsed row, in its state role with icon (DS-CMP-ROW-4, DS-MSG-11): Partial outage and Major outage in `--bad`, Degraded in `--warn`, Maintenance in `--info` (DS-COL-3). The tab dot takes the same role. Operational/unknown states appear only in details. | All four incident states of PRD 0011 and their wording | Same as 01 | none |
| `03-tray-account-claude.png`, `03-tray-list.png`, `04-tray-detail-claude*.png` | `03-tray-account-claude-expanded.png` | Accordion row instead of a drill-in page: sub-limit rows (DS-CMP-SUB-1) with Binding chip, pace stage and "Resets Sun 17:20 · in 2d 23h"; one forecast callout "At this rate the weekly limit runs out around Sat 10:20, 1d 7h before it resets" (DS-CMP-CALL-1..3) with an "Alert me" action; incident, access level and local-cost lines below; Details / Refresh actions. One expanded row at a time (DS-CMP-ROW-5). | Everything of the old detail card is still reachable: binding, pace outlook, access level, provider status with "Open status page", local cost, models (in Details) | Card slide/fade on open becomes an instant or 120 ms swap (DS-MOT-2, -3) | Forecast callout action "Alert me" (predictive alert). **new** |
| `05-tray-detail-cursor.png`, `05-...-end.png` | `05-tray-detail-cursor-extra.png` | Limit-hit state: "Exhausted" in `--bad` with an empty meter and "500 of 500 used" (DS-CMP-MTR-5); extra balance "$6.25 left of $20.00" in the warning role (decision 6), with a meter only because the provider reports the budget; warning callout "usage now draws on your extra balance". Row figure is "$6.25 extra" (DS-CMP-ROW-2). | Fast-requests exhausted semantics, balance shown as text value | Orange pulse/shake on exhaustion removed (DS-MOT-5; inventory 14) | Extra-usage alert action. **new** (budget meter needs the core to expose the cap) |
| `06-tray-detail-groq-stale.png`, `06-...-end.png` | `06-tray-groq-stale.png` | Stale pattern (DS-MSG-10, DS-CMP-STATE-4): figure and meter at 50% opacity, chip "Stale · 42 min ago" on the status line, callout with the cause and "Retrying in 4 min", line "Alerts, pace and ordering ignore this reading". Provider incident (Degraded) stays as a line. | Last-good reading kept and visibly old (PRD 0010) | Nothing animates on a stale reading (DS-MOT-6) | none |
| `07-tray-detail-error.png` | `07-tray-error.png` | Error is a critical callout with the core's cause and one recovery action, "Update key" (DS-CMP-STATE-3, DS-MSG-9); the row shows "Auth needed" in the accent role with a key icon (DS-COL-3) instead of a ring with an icon; no raw response, URL or token. | Authentication state, access level, retry | none | none |
| `09-tray-overview-light.png` | `09-tray-overview-light.png` | Same as 01 in the light mode of Dracula; contrast comes from the theme roles (DS-COL-5..7) | Light variant of the same theme | Same as 01 | none |
| `10-main-overview.png` | `10-main-overview.png`, `11-main-overview-full.png` | Topbar: the CuotAI logotype (16 px), filter tabs, search with `/` keycap, sort menu, refresh with age, "Add account", extensions, settings (DS-CMP-SHELL-3); theme switch and language flag move to Settings (General). Cards are the expanded row at window scale (DS-CMP-CARD-1): head with 40 px tile, plan badge, refresh, more menu and switch; two windows with neutral "N% left" figures, segmented meters, pace marker and reset; one status line; access line; Local cost folded into one disclosure row marked "estimate" (DS-CMP-KPI-3); foot reduced to counts plus a Details button (DS-CMP-CARD-3). Equal card heights (DS-CMP-CARD-2). Cursor shows a single 3 px critical stripe (DS-TOK-BRD-3). Reorder arrows are gone; the grip stays and has a keyboard path (DS-CMP-DRAG-2). `11` shows the whole scroll with the stale, error and exhausted cards. | Per-window 5 h / weekly summary, binding chip, local cost, drag handle, enable switch, 6+1 accounts, all states | Card hover scale and shadow lift, entrance stagger, FLIP after refresh, exit ghost, shimmer skeleton, bar width interpolation, count-up (DS-TOK-ELV-2, DS-MOT-5; inventory 3, 4, 6, 12, 15, 16, 17) | Sort control now a labelled menu; filter by provider tabs (existing filter kept). none |
| `13-main-update-available.png` | `13-main-update-available.png` | Update banner is an info banner (DS-CMP-MSG-2): download icon, "Update 0.72.0 available", date "Released 22 Oct 2026" instead of raw `2026-10-22T12:00:00Z`, What's new disclosure, "Remind me later" ghost and a single primary "Update now" (DS-CMP-BTN-4). | What's new text, both actions | Disclosure opens at `--motion-base` | none |
| `14-main-empty.png` | `14-main-empty.png` | Empty state (DS-CMP-STATE-1): icon, one sentence, one primary action, popular-provider shortcuts, config path in a muted line. Topbar drops the controls that need accounts. | Config path disclosure, Add account | none | Provider shortcut tiles go to the catalog. **new** |
| `15-main-narrow.png` | `15-main-narrow.png` | Fixes the clipped topbar: at 520 px the bar becomes two rows (brand and four icon buttons, then filter tabs and sort) so nothing overflows (DS-TOK-LAY-3, DS-CMP-TAB-4); search collapses to an icon; meters use 10 segments below 240 px (DS-CMP-MTR-2). | Single column cards, all actions reachable | none | none |
| `20-detail-claude-a..d.png` | `20-detail-claude.png` (full length, not clipped by the dialog scroll) | Details dialog built from the shared components (DS-CMP-DLG-1): incident callout; Limits group with sub-limit rows, hidden metric at 45% opacity with its show toggle (DS-CMP-SUB-2) and the forecast callout; Reading group (last reading, access level, next refresh); Local cost with three KPI cards, a daily bar chart (zero-based, dashed gridlines, "Today" in accent, unpriced Tuesday in `--track` and named, DS-CHT-3..5) and a Models and daily table disclosure; History with a 24h/7d/30d segmented control, area chart with a dashed reset marker, a real gap where the computer slept, lowest/highest/last (DS-CHT-4, -8, -11). Charts carry `role="img"` labels (DS-CHT-10). | Every section of the old dialog: plan badge, edit and delete, access line, provider status, metric hide toggle, local usage, history ranges and reset markers, models | History wipe-in (`hist-in`), level colour fades, dialog scale-in; dialog now fades (DS-MOT-3; inventory 11, 20) | none |
| `31-account-add-default.png` | `31-account-add-catalog.png`, `31b-account-add-catalog-search.png` | The 37-option native select becomes a searchable catalog (DS-CMP-SRCH-1): groups by how an account connects, one-line hint "auth method · access level", outlined Connect buttons (DS-CMP-BTN), "Billed per query" chip on AWS Bedrock (AGENTS.md cost caveat). `31b` is the filtered state. | Access level shown for every provider, extension path for providers not listed | none | Searchable catalog with auth hints. **new** |
| `32-account-add-claude-a/b.png` | `32-account-add-claude.png` | Form step after choosing a provider: name, authentication as a segmented control, one info callout, refresh interval presets, and an Advanced disclosure that holds credential variable, models and session; one primary at the end ("Sign in with browser"). | Same fields and options, OAuth vs environment variable | none | none |
| `40-extensions.png` | `40-extensions.png` | Grouped list (DS-CMP-LIST-1, -2): name, version badge, state chip, description, id and last run; row actions Test / Disable or "Review and enable" (consent kept); intro cut to one line. | Consent-before-run model, ids, last run, folder shortcut | none | none |
| `50-settings-a..h.png` (one 14-section dialog) | `50-settings-general.png`, `50-settings-dashboard.png`, `50-settings-menubar.png`, `50-settings-alerts.png`, `50-settings-advanced.png` | PRD 0020 structure: sidebar (about 220 px) with search, selected page marked by an accent bar and heavier label, content max 640 px, rows with a one-line description and the control on the right, dependent rows indented, no Apply buttons, a "Saved" toast (DS-CMP-MSG-3, DS-CMP-SW-1, DS-CMP-SEG-4). Segmented control and switch replace radios and check boxes. Advanced is a collapsible sidebar group (Integrations, Local cost, Extensions, Files), open on `50-settings-advanced` where the server shows a status chip and a "Connection details" disclosure (token is copy-only), hooks and the other pages are one row each. | All settings still reachable, the access-token and web-page-access safeguards, theme and language, hooks and server | Page change cross-fade removed (instant); disclosure at `--motion-fast` | Dashboard page (default tray view List/Rings/Compact, row display presets with the optional separate main-window layout, account list with add, edit, remove, order and enable switch, "Show pace as" Points/Percent with a worked example, sort, grouping, refresh interval, quick mode and language); Menu bar page (live preview, 4 style cards, ordered list with circular checks and drag handles); Alerts page with pace lead time 15/30/60 min, extra-usage alert and "remind below $"; Menu bar and Alerts are sidebar pages. **new** |
| none | `70-stats.png`, `70b-stats-scrolled.png` | Popover drill-in (DS-CMP-SHELL-2, DS-CMP-NAV-1): back, title, account filter and `esc` keycap; three KPI cards (DS-CMP-KPI-1, -2); "Peak usage per day" bars with 7d/30d, 50 and 100 gridlines and "Today" in accent; "By account" list with shared-scale sparklines (DS-CHT-7) and the last bar in `--bad` for a limit hit; Anthropic shows "—" and "No readings yet", never zero; recent events; retention line. | Local, 30-day history only; breadth per account | none | Whole screen. **new** (needs history aggregates in the core) |

## What was intentionally kept for identity

- Dracula as the default theme and every token coming from the theme files, so the 14 themes keep working (DS-ID-1); light is shown in `09`.
- The logo (colored arcs) in the main topbar, and its open "C" echoed by the ring.
- The tray **ring** (decision 1): a static four-arc "C", single value (the tightest limit), used only as the "All" tab icon and as the "Icon only" menu-bar style. It is not a per-account donut any more and has no fill animation.
- Brand-colored provider tiles with a white glyph (decision 3), including the dark tiles of Codex and Cursor.
- Big figures in the UI font with tabular digits; monospace only for times, plan badges, keycaps and axis/tick labels (decision 4, DS-TOK-TYP-1, -2).
- "Left" as the unit for remaining share (decision 6, DS-ID-5), uppercase micro labels with 0.04em tracking, weight 650 headings (DS-ID-4).
- Breadth of truth (DS-ID-6): accounts per provider, binding limit, pace, stale reading, incidents, access level, local cost, history, extensions, alerts. Everything that left a screen moved into a disclosure, a callout line or Details.
- Four locales: every layout keeps room for +40% text (names truncate, reset times never do); the key screens are also rendered in pt-BR, es and it (section 5).

## Motion per screen (all screens are static)

No screen in this set depends on animation. Removed against the inventory (`../before/INVENTORY.md`): shimmer skeleton, bar-width and
level-colour transitions, card hover scale/shadow, entrance stagger, FLIP after refresh or sort, exit ghost, shake, level flash, count-up,
ring arc interpolation, popover slide, history wipe-in, `spinOnce`, `pulseIfDue`, dialog scale/grow. Kept: the refresh spinner while a
refresh runs, a 120 ms fade for disclosures and menus, an opacity-only dialog fade at 180 ms, the 120 ms theme cross-fade (DS-MOT-1..9).

## Design decisions the design system did not settle

Status after the owner review of 2026-10-22: 1 accepted, 2 resolved (last refresh, with a refresh button), 3 resolved (account label as title), 4 accepted, 5 and 6 accepted, 7 accepted, 8 accepted, 9 accepted (Dashboard page added), 10 accepted, 11 accepted, 12 accepted (`--shadow-card` kept), 13 to 18 accepted. The DS agent applies the DS-side changes; the mockups only show them.

1. **Ring placement.** Decision 1 keeps the ring but DS-CMP-TAB-1 specifies a plain-text "All" tab and DS-CHT-2 grandfathers the ring. I put it inside the "All" tab (and in the main topbar's "All" tab) as the tightest-limit gauge, and as the "Icon only" menu-bar style. DS-CHT-2 needs to say where it is allowed.
2. **Footer age. Resolved:** the footer shows the age of the last completed refresh cycle ("1 min ago") with a refresh button next to it; a stale row carries its own age. DS-CMP-FOOT-2 is being aligned.
3. **Truncation in 380 px rows. Resolved:** when a provider has several accounts the row title is the account label (the tile shows the provider), so "work@example.com [Max] 22% left" fits; single-account rows keep provider name plus account.
4. **Status-line sizes.** DS-CMP-STL-1 does not give the size of the right-hand reset. I used 11 px mono (`--text-micro`) so "Ahead of pace · empty Sat 10:20" and "Sun 17:20" fit; sub-limit meta lines use the same size and wrap the reset to a second line when the core text is long (flex-wrap).
5. **Extra balance meter.** DS-CMP-MTR-5 forbids meters for uncapped values. `05` draws a meter for "$6.25 left of $20.00" because Cursor reports a budget; without a cap from the core it must be text only. Extra balance uses `--warn` (decision 6), which contradicts DS-COL-3 ("extra balance = info"); the DS table needs updating.
6. **Exhausted label.** An exhausted metric shows the word "Exhausted" in `--bad` in place of "0% left" next to an empty meter.
7. **Figures are not level-colored.** Before, the big percentages were green/yellow/red. After, figures are `--fg` and the level lives in the meter fill, status label and tab dot (less color, same information). The DS does not state this.
8. **Reset timeline edge cases (DS-CHT-6).** Limits that return beyond the selected range show "31 Oct, 02:20 ›" at the right edge without a dot; stale readings and unauthenticated accounts are listed under the lanes as "Return unknown". Lane labels carry the account short name when a provider has several accounts.
9. **Settings sidebar differs from PRD 0020 (accepted; Dashboard page added).** To fit the new pages the sidebar is General, Dashboard, Menu bar (new), Alerts (Notifications renamed, with the new rows), Data & privacy, a collapsible Advanced group (Integrations, Local cost, Extensions, Files) and About. PRD 0020 listed seven pages with Local cost inside Data & privacy and no Extensions page; it needs an amendment. The page was kept as navigation, with segmented tabs only inside pages (decision 8).
10. **Menu-bar and Alerts pages are taller than 640 px** (`50-settings-menubar` is 860x880 and `50-settings-alerts` 860x720) so the whole page shows; in the app they scroll inside the 640 px window. PRD 0020's "no more than one and a half screens" holds (880 / 640 = 1.4).
11. **Top bar actions.** "Add account" is outlined (the only primary on an empty window is the empty-state button); language and theme switch moved to Settings; sort stays a menu. The tray header lost the list/grid toggle, sort, language flag and refresh-all (sort and view are Settings → Dashboard; refresh is in the footer), which needs a place in PRD 0020's Dashboard page.
12. **Card shadow token (kept).** Decision 7 softens card shadows, but DS-TOK-ELV-1 says in-flow surfaces are flat. I added `--shadow-card: 0 1px 2px color-mix(in srgb, #000 16%, transparent)` for main-window cards only; the DS and `themes.css` need that token and a sentence in ELV-1.
13. **Update date format.** DS-MSG-8 covers date-times, not a release date. The banner uses a date-only locale format ("Released 22 Oct 2026", with the year). A rule should be added.
14. **Catalog groups.** Grouped by how an account connects ("Sign in with your browser" vs "API key") and not by popularity, because the auth method decides the next step; each row carries auth method and access level in one line.
15. **Today bar at 100%** (`70`): DS-CHT-5 allows `--warn`/`--bad` for bars at 100%, but "Today" uses `--accent` and a "100%" label. Sparkline last bars use `--bad` only for a limit hit, as DS-CHT-7 says.
16. **Fixture:** a second Claude account was added to show multi-account behavior; the "before" set has six accounts.
17. **Details dialog** is rendered at full length (560 x 1620) instead of as scroll slices; section order is incident, limits and forecast, reading, local cost, history, models.
19. **Alternative tray views.** Rings and Compact are not in the design system yet; they reuse the ring (DS-CHT-2 exception) and the continuous bar (DS-CMP-MTR-3, with a 2 px pace tick). The DS needs a short section for "tray view" with the three layouts and the rule that all three show the same states.
18. **New glyphs** the mockups needed beyond `js/lib.js` (DS-TOK-ICO-1 requires adding them there): chevDown, chevUp, tick, chart, chartBar, info, download, puzzle, sliders, shield, plug, file, menubar, history, zap, globe, ext, more, dollar, terminal, db, arrowUp, filter, rocket, mapPin (Lucide paths, ISC). Only some are used on screen.

20. **Logotype in the tray (open).** The tray overview header holds only the filter tabs (DS-CMP-HEAD-2) and they already take 330 to 350 of the 368 usable pixels in every language (six provider tabs, rings, counts, "All 7"), while the 15 px logotype needs about 50 px. It is therefore not drawn in the tray overview. Options for the owner: (a) keep it out of the tray (main topbar, About and the empty state carry it); (b) show it only in tray views that have a title slot (drill-ins), where it would compete with the view title; (c) a second 32 px brand row above the tabs, which costs list height and breaks "the header holds only the filter tabs". The mockups follow (a).
21. **Account list in Settings > Dashboard.** DS-CMP-SEG-5 removes the Accounts page, so the list lives next to the row layout settings. The monogram override is in the account form (DS-CMP-MONO-4), not in the list; the list only shows the result on the tile.
22. **Flags artwork.** `js/flags.js` has no cn, kr, in, id, tr or vn; the mockup draws simplified versions to show the 14-language menu. The real artwork must be added before PRD 0035.

## Design-system rules that proved impractical or need amendment

- **DS-CMP-TAB-1** (text "All" tab): see decision 1; also at 380 px with 22 px ring the strip holds "All" plus six provider tabs; a seventh needs the "More" menu (DS-CMP-TAB-4), which is not drawn.
- **DS-CMP-FOOT-2**: oldest reading vs last refresh (decision 2).
- **DS-CMP-ROW-1**: provider label + plan badge + account + figure + chevron exceeds 304 px. Resolved by the account-label title for multi-account providers; single-account rows still truncate long account names.
- **DS-CMP-SUB-1**: one meta line cannot hold a long core pace text plus "Resets Sun 17:20 · in 2d 23h" at 330 px; it wraps to two lines (accepted).
- **DS-CMP-CALL-1** (fill = 12% role on `--surface`, no border): the warning tint on Dracula (a pale yellow) rendered as a muddy olive. Resolved for the warning tone with a 6% fill and a role-colored border; other tones are unchanged.
- **DS-COL-3** extra balance role and **DS-TOK-ELV-1** flat cards (decisions 5 and 12).
- **DS-CMP-FOOT-3** (attention chips in the footer): dropped in the mockups because Settings, Stats and the age already fill 380 px; the tab dots carry attention instead.
- **DS-CMP-MTR-6**: resolved twice: first by snapping to a segment boundary, then (2026-10-23) by making every meter one continuous bar with the pace corridor (6 px bars) or a 2 px tick (4 px bars).
