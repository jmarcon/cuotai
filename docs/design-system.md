# Cuotai desktop design system

| Field | Value |
|---|---|
| Status | Normative (adopted by PRD 0022, whose RF1–RF13 are delivered; amendments in section 10) |
| Applies to | `apps/desktop/ui` (tray popover `tray.html`, main window `index.html`, dialogs, the settings window of PRD 0020) |
| Related | `apps/desktop/ui/css/themes/README.md` (theme contract), PRD 0004 (motion), 0007 (pace), 0009 (binding limit), 0010 (age and stale reading), 0011 (incidents), 0014 (widgets), 0020 (settings window) |

This document is the rulebook for every screen of the desktop app. It defines shapes, sizes, components, charts, wording and motion. It does **not** define colors: colors are semantic roles whose values each theme supplies (section 3).

Keywords: **MUST** / **MUST NOT** are requirements a PR reviewer enforces; **SHOULD** / **SHOULD NOT** are defaults that need a stated reason to break; **MAY** is optional.

Rule IDs are stable and never reused. A retired rule keeps its ID and is marked *retired*. Scheme:

| Prefix | Section |
|---|---|
| `DS-PRI-n`, `DS-ID-n` | Principles, identity |
| `DS-TOK-<SPC/RAD/BRD/ELV/TYP/ICO/HIT/LAY>-n` | Tokens |
| `DS-COL-n` | Color roles |
| `DS-CMP-<COMPONENT>-n` | Components (`SHELL`, `HEAD`, `TAB`, `ROW`, `CARD`, `BADGE`, `MTR`, `RING`, `STL`, `SUB`, `CALL`, `KPI`, `LIST`, `SRCH`, `BTN`, `SEG`, `SEL`, `CHK`, `SW`, `DRAG`, `KEY`, `NAV`, `FOOT`, `STATE`, `TIP`, `MSG`, `DLG`, `MONO`, `HON`, `TIME`, `EXTRA`, `NOTIF`, `DEP`, `DENS`, `MENU`, `PREV`, `STATS`, `JRNL`, `CAT`, `DIAG`, `RPT`, `VER`) |
| `DS-CHT-n` | Charts |
| `DS-MSG-n` | Content and messages |
| `DS-MOT-n` | Motion |
| `DS-A11Y-n` | Accessibility and keyboard |
| `DS-GOV-n` | Governance |

## 1. Principles and scope

- **DS-PRI-1 Data first.** Every screen answers "how much is left, until when, and can I trust the number" before anything else. Decoration that does not carry data is removed.
- **DS-PRI-2 Calm.** The app lives in the tray all day. Nothing moves, blinks or pulses unless the user acted or a value changed, and even then briefly (section 7).
- **DS-PRI-3 Clean.** Flat surfaces, 1 px hairlines, generous container radius, one accent per view, at most one primary action per view. Fewer boxes: separate list items with hairlines rather than nested cards.
- **DS-PRI-4 Honest.** Absent data is never drawn as zero; estimates say they are estimates; a stale reading looks stale; texts that interpret data come from the core (`Localized`), never from UI heuristics.
- **DS-PRI-5 Identity preserved.** The refresh simplifies layout and motion; it keeps everything listed in section 1.1.
- **DS-PRI-6 Accessible.** WCAG 2.2 AA: contrast, keyboard operation, visible focus, no information carried by color or motion alone.

**Scope.**

- **DS-PRI-7** These rules bind the tray popover, the main window, every dialog and the settings window. Each new screen MUST be built from the components of section 4.
- **DS-PRI-8 TUI.** The TUI (`crates/cuotai-tui`, `docs/tui.md`) is not bound by sizes or components, but MUST share the vocabulary: status names, pace stages, the binding/stale/incident concepts and the content rules of section 6. A concept named differently in the TUI and the desktop is a bug in one of them. The pace and status shapes (↓ → ↑ ✕ ↺, incident ! ▲ ✕ i ?) have automatic ASCII fallbacks (for example `v > ^ x @`) that the TUI uses when the terminal renders East Asian ambiguous-width glyphs as double width (or the locale/terminal is not known to render them single-width), so columns never shift; the meaning stays in the adjacent text.
- **DS-PRI-9 Widgets.** Native widgets (PRD 0014, `apps/desktop/widgets`) use the platform's fonts and controls, but MUST follow the color roles (section 3), the meter rules (DS-CMP-MTR-*), the content rules (section 6) and have no animation.

### 1.1 What "Cuotai identity" means

These traits define Cuotai and MUST survive any redesign:

- **DS-ID-1 The theme system.** 15 built-in themes, Cuotai's own plus three groups (editors, systems, retro), each with dark and light modes, plus user themes, all through the token contract. Retro themes (Win98, Windows XP, CRT, Synthwave, TRON) keep their character: bevels, square corners, scanlines, phosphor glow, title-bar gradients on surfaces. A component that only looks right in one theme is wrong.
- **DS-ID-2 The mark.** The logo (`img/logo.svg`) is a ring of four separate arcs. The logo keeps its own fixed colors; it is the only multi-color brand artwork.
- **DS-ID-3 Provider tiles.** Providers are shown as rounded tiles in the provider's brand color with a white glyph mask (`logo()` in `js/lib.js`, `.logo`, `--logo-radius`), with a letter fallback. Brand tile colors are provider identity, not theme colors, and are the one allowed exception to DS-COL-1.
- **DS-ID-4 Typography.** System UI stack (`--font-ui`), headings at weight 650, large bold tabular figures for "how much is left", and uppercase micro labels with `0.04em` tracking (`.win-name`, `.lc-head`, settings group labels).
- **DS-ID-5 The "available" criterion.** Meters show what is **left**, not what was used; levels are `good` ≥ 70 % left, `warn` 30–70 %, `bad` < 30 % (`level()` in `js/lib.js`).
- **DS-ID-6 Breadth of truth.** Cuotai shows what other monitors hide: several accounts per provider, the binding limit, pace, measurement age and stale readings, provider incidents, local cost estimates, history, usage alerts and extensions. Simplification moves these into disclosures; it never deletes them.
- **DS-ID-7 Multilingual.** 14 shipped languages, plus user language packs.
- **DS-ID-8 The ring.** The tray's ring gauge (`.donut` tiles in `tray.js`) is the "C" of Cuotai: an open ring that echoes the logo. It is an identity element, not a donut chart, and is specified as a single-value component in DS-CMP-RING. Static and 12–16 px, it is also Cuotai's **account-state glyph** (DS-CMP-RING-6): the leading mark of status lines, the attention mark on filter tabs, the menu-bar icon, notifications where the OS allows an image, and widgets. It appears as a gauge in three places: one small static ring in the "All" filter tab, the "Icon only" menu-bar style, and the per-account ring grid, which is an **alternative** tray view the user selects in Settings; the default tray view is the list of account rows with continuous meters (DS-CMP-ROW).

## 2. Tokens (no colors)

All tokens are CSS custom properties on `:root`, defined in `css/themes.css`. Existing names are kept; names marked **new** are proposed and MUST be added there before use.

### 2.1 Spacing

- **DS-TOK-SPC-1** Paddings, margins and gaps MUST use the spacing scale; one-off values (5, 6, 9, 10, 14, 22, 26 px found today) migrate to the nearest step.

| Token (new) | Value | Typical use |
|---|---|---|
| `--space-0` | 2px | segmented-control track padding, icon-to-badge nudge |
| `--space-1` | 4px | chip inner gap, list of chips |
| `--space-2` | 8px | gap inside a row, between stacked lines |
| `--space-3` | 12px | row padding, popover body padding |
| `--space-4` | 16px | card and header horizontal padding, grid gap |
| `--space-5` | 20px | dialog padding, main window gutter |
| `--space-6` | 24px | section separation |
| `--space-8` | 32px | empty-state padding |

- **DS-TOK-SPC-2** Spacing is layout: themes MUST NOT redefine it.

### 2.2 Radius

| Token | Default | Use |
|---|---|---|
| `--radius-lg` | 18px | containers: popover (`.pop`), main-window cards (`.card`), dialogs (`dialog.dlg`) |
| `--radius-md` | 12px | rows-in-a-box, metric boxes, tiles, callouts, KPI cards, list containers |
| `--radius-sm` | 8px | buttons, fields, selects, segmented controls, menu items |
| `--radius-xs` (new) | 4px | plan badges, keycaps, bar-chart bar tops |
| `--radius-pill` | 99px | status chips, switches, continuous bars, dots |
| `--logo-radius` | 22% | provider tiles and the logo |

- **DS-TOK-RAD-1** A nested element MUST NOT have a larger radius than its container; inner radius = outer radius − inner padding, floored at `--radius-xs`.
- **DS-TOK-RAD-2** Components MUST reference these tokens, never literal radii (`3px` in `.lc-bar`, `6px` fallbacks in `detail.css` migrate).

### 2.3 Borders and elevation

- **DS-TOK-BRD-1** Hairline = `var(--border-w) solid var(--border)`. Every surface boundary uses it.
- **DS-TOK-BRD-2** Emphasis borders are 2 px and reserved for: focus (DS-A11Y-1), the selected option card (DS-CMP-CHK-3) and the drop target while dragging.
- **DS-TOK-BRD-3** State stripes (`inset 3px 0 0` today for binding, usage alert and incident) are limited to **one** stripe per element. When several states apply, the element shows the highest-priority stripe (critical > warning > binding) and the others as chips.
- **DS-TOK-ELV-1** Three levels only:

| Level | Token | Use |
|---|---|---|
| 0 flat | border only; main-window account cards alone add `--shadow-card` (new, DS-TOK-ELV-4) | everything in flow: rows, cards, KPI cards, callouts |
| 1 floating | `--shadow` | menus, tooltips, the topbar while scrolled |
| 2 modal | `--shadow-dialog` + `--scrim` (new) | dialogs |

- **DS-TOK-ELV-2** In-flow surfaces SHOULD separate by border and surface step (`--bg` → `--surface` → `--surface-2`), not by shadow. Hover and focus MUST NOT change elevation (no `scale`, no added shadow).
- **DS-TOK-ELV-3** Backdrop blur is a theme effect only (macOS, Windows 11). Components MUST NOT add `backdrop-filter` (the topbar's `blur(12px)` and the dialog backdrop's `blur(3px)` migrate to opaque fills).
- **DS-TOK-ELV-4 Soft card shadows.** A new token `--shadow-card` is the only shadow on level 0 surfaces and is applied only to main-window account cards (`.card`); popover rows, KPI cards, callouts and list containers have none. Its default in `themes.css` is one soft layer, and a theme that sets it MUST keep it soft: a single layer, blur ≤ 8 px, offset ≤ 2 px, opacity ≤ 0.15 in light mode and ≤ 0.25 in dark mode. Shadows are softened, not removed; themes whose character is a hard outline or none (Win98 `none`, CRT, Synthwave, TRON ring shadows) keep it. **Migration:** the built-in themes with today's two-layer `0 1px 2px …, 0 8px 24px …` card shadow (Dracula, One Dark, Catppuccin, Monokai, and the `themes.css` fallback) move it to one soft layer in `--shadow-card`; `.card` switches from `--shadow` to `--shadow-card`; `--shadow` remains for level 1 (menus, tooltips) and `--shadow-dialog` (level 2) is unchanged.

### 2.4 Typography

- **DS-TOK-TYP-1** Two families only: `--font-ui` for text, `--font-mono` for codes, times, keycaps, plan badges, chart tick labels and model ids. No web fonts (the app is offline).
- **DS-TOK-TYP-2** Every number that can change MUST render with tabular figures (`font-variant-numeric: tabular-nums`), whatever the family.
- **DS-TOK-TYP-3** Scale (new tokens; current sizes in parentheses migrate):

| Token | Size / line | Role |
|---|---|---|
| `--text-2xs` (new) | 10px / 1.1 | the unit word under a figure (DS-CMP-ROW-1b) only; minimum size in the app |
| `--text-micro` | 11px / 1.3 | micro labels (uppercase), axis ticks, keycaps, plan badges |
| `--text-sm` | 12px / 1.35 | secondary text, status line, chips, captions |
| `--text-md` | 13px / 1.4 | dense body (settings descriptions, list subtitles) |
| `--text-base` | 14px / 1.4 | body, row titles, buttons |
| `--text-lg` | 16px / 1.3 | popover and window titles, sub-limit figure (15 and 16 today) |
| `--text-xl` | 18px / 1.25 | dialog titles, card names, row figure (17 and 18 today) |
| `--text-figure` | 24px / 1.1 | KPI figure, per-window figure (26 today) |
| `--text-hero` | 30px / 1 | single hero figure of a card |

- **DS-TOK-TYP-4** Weights: 400 body, 500 labels, 600 emphasis and status labels, 650 headings, 700 figures. No other weights.
- **DS-TOK-TYP-5** Micro labels are uppercase with `letter-spacing: 0.04em` at `--text-micro`/600 in muted text (identity, DS-ID-4). Uppercase is applied with CSS, never written in the catalog.
- **DS-TOK-TYP-6** The unit word of a figure ("left", "extra", "used") is muted, weight 400. In account rows and cards it sits **below** the figure at `--text-2xs` (DS-CMP-ROW-1b); inline (same baseline, after the figure, `--text-sm`) only in sub-limit rows, tables, chips and sentences.
- **DS-TOK-TYP-7 Scripts.** The UI ships Latin, Cyrillic, Han (Simplified Chinese), Japanese, Korean and Devanagari text (PRD 0035). Text is set as `font-family: var(--font-ui), var(--font-script)` (mono: `var(--font-mono), var(--font-script)`): the theme's stack stays first for Latin, and per-glyph fallback reaches a per-script system stack defined in `themes.css` under `:root:lang(…)` (new token `--font-script`, layout-owned, not a theme token). No web fonts.

| Locale | macOS | Windows | Linux |
|---|---|---|---|
| zh-CN | "PingFang SC" | "Microsoft YaHei UI", "Microsoft YaHei" | "Noto Sans CJK SC", "Source Han Sans SC", "WenQuanYi Micro Hei" |
| ja | "Hiragino Sans", "Hiragino Kaku Gothic ProN" | "Yu Gothic UI", "Meiryo UI" | "Noto Sans CJK JP", "Source Han Sans JP" |
| ko | "Apple SD Gothic Neo" | "Malgun Gothic" | "Noto Sans CJK KR", "Source Han Sans KR" |
| hi-IN | "Kohinoor Devanagari", "Devanagari Sangam MN" | "Nirmala UI" | "Noto Sans Devanagari", "Lohit Devanagari" |
| ru (Cyrillic) | system UI (SF covers it) | "Segoe UI" | "Noto Sans", "DejaVu Sans" |
| vi (Latin + stacked diacritics) | system UI | "Segoe UI" | "Noto Sans", "DejaVu Sans" |

- **DS-TOK-TYP-8 Language attribute.** `<html lang>` follows the active locale (today `index.html` and `tray.html` hard-code `pt-BR`; it must be set at boot), and any text in another language (endonyms in the language list, provider texts) carries its own `lang`. This selects the right Han glyph variants (SC/JP/KR), Turkish casing (`İ`/`ı` under `text-transform`) and hyphenation.
- **DS-TOK-TYP-9 Script adjustments.** Line-height: CJK body 1.5, Devanagari 1.6, Vietnamese 1.45 (others keep the scale's values); the figure line keeps 1.1. Minimum size for CJK and Devanagari is 11 px: `--text-2xs` resolves to 11 px under those locales. CJK and Devanagari: `font-synthesis: none` (no faux bold or italic), italics are never used for emphasis (use weight or `--fg-muted`), uppercase micro labels drop `text-transform` and `letter-spacing` (DS-TOK-TYP-5). CJK line breaking uses `line-break: strict` and no hyphenation. Vietnamese strings are stored NFC.
- **DS-TOK-TYP-10 Numbers in every locale.** Figures stay in the UI font with tabular digits (DS-TOK-TYP-2) and use the locale's default numbering system as `Intl` returns it; all 14 shipped locales default to Latin digits. Decimal and group separators always come from `Intl.NumberFormat` (DS-MSG-7).

### 2.5 Icons

- **DS-TOK-ICO-1** UI icons come from the single set in `js/lib.js` (`icon()`, 24-unit viewBox, 2 px round stroke, `currentColor`). New glyphs are added there; inline SVGs elsewhere are not allowed. *Note:* the refresh needs about 25 new glyphs (for example stats, timeline, forecast, drag grip variants, circular check, keycap-adjacent symbols, auth-method and "billed per query" marks); each is drawn to the same grid and stroke and added to this set before use, never inlined in a view.
- **DS-TOK-ICO-2** Sizes: 12 px inside `--text-micro`/`--text-sm` lines, 14 px in chips, segmented controls and links, 16 px default inline and list icons, 18 px in icon buttons, 22 px in a tile center. An icon MUST NOT be larger than its hit target minus 8 px.
- **DS-TOK-ICO-3** Provider tiles: 14 px inline (text lines, timeline lanes), 20 px in icon tabs, 28 px in list rows, 32 px in popover account rows, 40 px in main-window card heads.

### 2.6 Hit targets

- **DS-TOK-HIT-1** Every interactive element MUST have a target of at least 24 × 24 px (WCAG 2.5.8). Icon buttons SHOULD be 28 px in the popover and 32 px in windows (`.icon`). The 26 px `.move` and filter-clear buttons stay above the minimum.
- **DS-TOK-HIT-2** Whole rows that open or expand MUST be a single `<button>` or have one primary target covering the row; nested interactive elements inside it MUST NOT exist (use a trailing action column instead).

### 2.7 Layout

- **DS-TOK-LAY-1** Window sizes (from `tauri.conf.json` and PRD 0020):

| Surface | Size | Gutter |
|---|---|---|
| Tray popover | 380 × 600 px, fixed width; body scrolls, header and footer fixed | `--space-3` |
| Main window | default 1200 × 820, min width 520; card grid `minmax(min(100%, 380px), 1fr)`, gap `--space-4` | `--space-5` |
| Settings window | 860 × 640, min 640 × 460; sidebar ≈ 220 px, content max 640 px | `--space-5` |
| Dialog | `--dialog-sm` 440, `--dialog-md` 560, `--dialog-lg` 680, `--dialog-xl` 760 px (the Add-account catalog), each capped at `100vw − 32px` | `--space-5` |

- **DS-TOK-LAY-2** Dialog widths MUST use the three tokens (today: 520, 560, 600, 640, 680 px).
- **DS-TOK-LAY-3** Content MUST NOT scroll horizontally at the minimum size or at 200 % text zoom.

## 3. Color roles

- **DS-COL-1** Components MUST reference color roles only. No hex, `rgb()`, `rgba()` or named colors outside `css/themes.css` defaults and `css/themes/*.css`. Exceptions, the only multicolor artwork allowed: provider brand tiles (DS-ID-3), language flags (DS-CMP-SEL-3) and the logo. Known violations to migrate: `dialog.dlg::backdrop` `rgba(0,0,0,.5)`, `.btn.danger-btn` `#fff`, `.flag` shadow, `.logo` `#fff`.
- **DS-COL-2** Derived tints MUST be built from roles (`color-mix(in srgb, var(--warn) 12%, var(--surface))`), never from literals.

### 3.1 Roles

| Role token | Meaning |
|---|---|
| `--bg` | window and popover background |
| `--surface` | cards, rows, dialogs, controls, header and footer bars |
| `--surface-2` | hover, selected segment track, grouped blocks, callout neutral fill |
| `--border` | hairlines and control outlines |
| `--fg` | primary text, figures, pace corridor and tick (as a mix) |
| `--fg-muted` | secondary text, unit words, captions, unknown state |
| `--accent` / `--accent-fg` | interaction and selection (primary button, switch on, selected option, focus ring, binding emphasis, highlighted chart bar) and the text on it |
| `--good` / `--warn` / `--bad` | quota levels and states |
| `--info` | neutral information (maintenance, configuration needed, credits and uncapped usage) |
| `--track` | empty part of meters and unmeasured bars |
| `--btn-bg`, `--btn-fg`, `--input-bg`, `--card-bg`, `--head-bg`, `--topbar-bg`, `--body-bg`, `--switch-knob` | surface fills a theme may restyle (theme character) |
| `--scrim` (new) | dialog backdrop; default `color-mix(in srgb, #000 50%, transparent)` in `themes.css` |
| `--bad-fg` (new) | text on a `--bad` fill (destructive button) |
| `--focus` (new) | focus ring; default `var(--accent)` |
| `--chart-neutral` (new) | non-highlighted chart bars; default `color-mix(in srgb, var(--fg-muted) 55%, transparent)` |

New roles have defaults derived from existing ones, so no theme has to change to adopt them.

### 3.2 Status → role mapping

- **DS-COL-3** States map to roles as below. The UI MUST NOT pick a role from text; it uses the core's classification (`level()`, `status`, `freshness`, `Pace.tone`, incident state).

| State | Role | Required non-color cue |
|---|---|---|
| ok / available ≥ 70 % left, pace `good` | `--good` | figure + "left", check icon in chips |
| warning: 30–70 % left, aging reading, pace `warn`, degraded incident, rate limited, usage alert | `--warn` | label text, warn/clock/bell icon |
| critical: < 30 % left, exhausted, blocked, query error, partial or major outage | `--bad` | label text ("Exhausted", "Blocked until …"), x-circle/battery icon |
| stale reading (PRD 0010) | `--warn` chip; the reading itself at 50 % opacity | "Stale · 42 min ago" text |
| unknown / unmeasured / unsupported | `--fg-muted` | "—" or "No data", empty track |
| incident: maintenance | `--info` | core label + icon |
| incident: status page unreadable | `--fg-muted` | core label |
| extra balance (the plan's limit is used up and paid overage is being charged) | `--warn` | "$6.25 extra" text value, no meter, "On extra balance" label |
| credits, uncapped usage | `--info` | text value, no meter |
| authentication needed | `--accent` (today's `ALERTS` tone): it asks for a user action, not a quota state, so it deliberately uses the interaction role and never a level role | key icon + "Sign in again" label |
| binding limit | `--accent` emphasis (stripe or chip) | "Binding" chip text |
| pace `neutral` | `--fg-muted` | core pace text |

- **DS-COL-4** One state, one role, everywhere: popover, main window, tray menu text and widgets.

### 3.4 What a theme may define

A theme (built-in or user, `css/themes/README.md`) defines, per mode, **only**:

1. **Color roles** (required): `color-scheme`, `--bg`, `--surface`, `--surface-2`, `--border`, `--fg`, `--fg-muted`, `--accent`, `--accent-fg`, `--good`, `--warn`, `--bad`, `--info`, `--track`; optionally the derived roles `--scrim`, `--bad-fg`, `--focus`, `--chart-neutral` (defaults in `themes.css`).
2. **Theme character** (optional, closed list): `--font-ui`, `--font-mono` (system stacks only); `--radius-lg`, `--radius-md`, `--radius-sm`, `--radius-xs`, `--radius-pill`, `--logo-radius`; `--border-w`; `--shadow` (level 1), `--shadow-card` (soft, DS-TOK-ELV-4), `--shadow-dialog`; surface fills `--card-bg`, `--head-bg`, `--topbar-bg`, `--btn-bg`, `--btn-fg`, `--input-bg`, `--switch-knob`, `--body-bg`; `--hatch` (`none` turns the unknown hatch into a plain dim track, DS-CMP-HON-1); static effects scoped to `:root[data-theme="<id>"]` (bevels, scanlines, vignette, glow, platform blur) that keep the contrast of section 3.3.
3. **Motion** (optional): `--motion-*` and `--ease-out` only to lower durations or use `steps()` (DS-MOT-8).

Anything else (spacing, sizes, layout, component anatomy, chart types, icons, wording) is outside a theme (DS-GOV-3). Every theme passes `tools/contrast.py` and `tools/lint_theme.py` in both modes.

### 3.3 Contrast each theme MUST meet

`tools/contrast.py` and `tools/lint_theme.py` enforce these per mode:

- **DS-COL-5** `--fg` and `--fg-muted` on `--bg`, `--surface` and `--surface-2`: ≥ 4.5:1. (Today `--fg-muted` on `--bg` is checked at 4.0; raise to 4.5.)
- **DS-COL-6** `--accent-fg` on `--accent` and `--bad-fg` on `--bad`: ≥ 4.5:1.
- **DS-COL-7** Status roles and `--accent` used as graphics (meter fill, dots, stripes, focus ring) on `--surface` and `--bg`: ≥ 3:1 (WCAG 1.4.11). Status roles used as **text** (status label, incident line, pace line) MUST reach 4.5:1; where a theme cannot, the component renders the label in `--fg` and keeps the role on the dot or icon only.
- **DS-COL-8** `--good`, `--warn` and `--bad` MUST be distinguishable from each other and from `--fg` (existing rule), and `--track` MUST remain visible on `--surface` (≥ 1.1:1, existing).

## 4. Components

Each component lists anatomy, rules and the states it MUST style. Unless a component says otherwise, states are: **hover** = `--surface-2` fill (no elevation change), **focus-visible** = DS-A11Y-1 ring, **active** = no transform (theme MAY add a bevel, as Win98 does), **disabled** = opacity 0.45 and `cursor: default`, still announced.

### 4.1 App shell and popover (`SHELL`)

```
┌─ .pop (radius-lg, hairline, --bg) ──────────────┐
│ header: title or filter tabs        [actions]   │  fixed
├─────────────────────────────────────────────────┤
│ body: account rows (hairline separated)         │  scrolls
│ optional section: reset timeline                │
├─────────────────────────────────────────────────┤
│ footer: Settings ⌘,  History      ⟳ 1 min ago   │  fixed
└─────────────────────────────────────────────────┘
```

- **DS-CMP-SHELL-1** The popover is one `.pop` container: `--radius-lg`, hairline, `--bg`. Header and footer use `--surface` with a hairline separator; only the body scrolls.
- **DS-CMP-SHELL-2** The popover has exactly two levels: the overview and a drill-in view (account details, history/stats, add account, quick settings). A drill-in replaces the body and shows a back control (DS-CMP-NAV-1); it never stacks a dialog over the popover.
- **DS-CMP-SHELL-3** The main window keeps a sticky topbar (title, filter, global actions) over a responsive card grid; window-level messages sit between topbar and grid as banners (DS-CMP-MSG-2). The theme and language switchers leave the topbar for Settings › General; "Add account" is an outlined button (DS-CMP-BTN).
- **DS-CMP-SHELL-5 Tray views.** The default tray view is the list of account rows (DS-CMP-ROW). The ring grid (DS-CMP-RING) is an alternative view, and further compact views (for example a dense one-line list) MAY be added as alternatives; the view is chosen in Settings › Dashboard, not in the tray header. Every view shows the same accounts, order and states.
- **DS-CMP-SHELL-4** States: loading (DS-CMP-STATE-2), empty (DS-CMP-STATE-1), backend error (banner in window, callout in popover).

### 4.2 Header and filter tabs (`HEAD`, `TAB`)

```
[ All 6 ] [▣] [▣2] [▣•] [▣]                  [⟳]
  text     provider icon tabs (count, dot)   actions
```

- **DS-CMP-HEAD-1** Header height 48 px, padding `--space-3` `--space-4`. Title `--text-lg`/650. At most three trailing icon buttons; extra actions go into a menu.
- **DS-CMP-HEAD-2** The tray overview header holds only the filter tabs. The view toggle (list/grid), sort menu, language flag and refresh-all move to Settings › Dashboard (language to Settings › General); refreshing stays in the tray footer (DS-CMP-FOOT-2).
- **DS-CMP-TAB-1** Filter tabs: a leading text tab "All" with a small static ring (DS-CMP-RING, 16 px, the tightest binding share across listed accounts) and the account count in muted tabular digits, then one icon tab per provider that has at least one enabled account, in account order. Icon tabs use the 20 px provider tile, a 32 × 28 px target and `--radius-sm`.
- **DS-CMP-TAB-2** A provider with several accounts shows the count as a small muted number next to the icon; a provider needing attention shows a 10 px account-state ring (DS-CMP-RING-6) of its worst account as a corner badge **outside** the 20 px tile: top-right, with a 2 px gap from the tile's edge, inside the tab target (a tab with a badge widens to 36 px); the badge never overlaps or covers the provider logo, and it replaces the former 6 px dot; and the state in the accessible name ("Claude, 2 accounts, 1 exhausted").
- **DS-CMP-TAB-3** Selected tab: `--surface-2` fill with a hairline; unselected: transparent, icon at full opacity (provider tiles are never desaturated). Hover: `--surface-2` without hairline.
- **DS-CMP-TAB-4** When tabs exceed the width, the last visible tab becomes a "More" menu; tabs never wrap or scroll horizontally.
- **DS-CMP-TAB-5** Tabs are a `role="tablist"` with roving `tabindex` (DS-A11Y-4). The selection is remembered per window.

### 4.3 Account row, popover (`ROW`)

Collapsed:

```
┌──┐ Claude  [Max]  work@acme                22%  ›
│▣ │                                        left
└──┘ ██████░░░▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░
     ◔ ↑ 8 pts ahead · Runs out ≈18:53      ↺ 19:30
     ◷ Aging · 12 min ago                          (only when not fresh)
```

Expanded (accordion):

```
┌──┐ Claude  [Max]  work@acme                22%  ⌄
│▣ │                                        left
└──┘ ██████░░░▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░
     ◔ ↑ 8 pts ahead · Runs out ≈18:53      ↺ 19:30
     ── sub-limit rows (DS-CMP-SUB) ──
     ── forecast callout (DS-CMP-CALL) ──
     ── incident / session / local cost lines ──
     [Details]  [⟳ Refresh]  [🔔]
```

- **DS-CMP-ROW-1** Anatomy, in order: provider tile (32 px); title line = provider label (`--text-base`/600) + optional plan badge (DS-CMP-BADGE-1) + account name when it has one (muted, truncates first; the account name is optional, PRD 0029, and an unnamed account shows only the provider name, never a placeholder or the internal id); trailing figure block (DS-CMP-ROW-1b) and a chevron vertically centered on that block; meter (DS-CMP-MTR); status line (DS-CMP-STL). When more than one listed account belongs to the same provider (the only case where the core requires a name), the **account label becomes the row title** and the provider is identified by the tile alone (its name stays in the accessible name and tooltip), so rows fit 380 px.
- **DS-CMP-ROW-1a Neutral figures.** The trailing figure, like every large figure (cards, sub-limits, KPI), is drawn in `--fg`, never in a level role. The level is conveyed by the meter fill, the status label and the tab dot. An exhausted limit shows the word "Exhausted" in place of the figure, with an empty meter.
- **DS-CMP-ROW-1b Figure block.** The trailing figure is a two-line, right-aligned block: line 1 the figure (`--text-xl`/700, `--font-ui` with tabular digits, `--fg`; "22%", "$6.25", "—", or "Exhausted"), line 2 the unit word (`--text-2xs`, 10 px, weight 400, `--fg-muted`, line-height 1.1, so the whole block stays within about 1.3 × the figure's height: the catalog's `card.available`, "left" in en-US, "disponível", "disponible", "disponibile"; "extra" for overage money; "left" for balances). The unit line is a single line and never truncates or wraps; the block's width is the wider of the two lines, and the title line and status line give way to it. When the figure is a word ("Exhausted", a status label), the unit line is omitted. Contrast note: at 10 px the unit word is normal-size text, so `--fg-muted` on the row surface MUST meet 4.5:1 (DS-COL-5) in every theme; no other text in the app goes below `--text-micro`.
- **DS-CMP-ROW-2** The trailing figure is the core's **binding** metric (`bindingPct`), shown as "N% left". Accounts without a capped metric show the text value ("$12.40 left", "$6.25 extra") and no meter; failed accounts show the status label in its role ("Auth needed", "Error") and the cause on the status line.
- **DS-CMP-ROW-3** Rows are separated by hairlines inside the body, not boxed individually (replaces today's bordered `.row` boxes). An expanded row gets a `--surface` fill with `--radius-md`.
- **DS-CMP-ROW-4** Collapsed rows show at most **one** extra line under the status line, chosen by priority: provider incident > stale or aging age > active usage alert. Fresh age is not repeated per row; the footer shows the global age (DS-CMP-FOOT-2).
- **DS-CMP-ROW-5** The row header is one `<button aria-expanded>` (DS-TOK-HIT-2). The chevron points right when collapsed and down when expanded; the change is instant or ≤ `--motion-fast` (DS-MOT-3). Only one row is expanded at a time in the popover.
- **DS-CMP-ROW-6** A stale reading (PRD 0010) renders the meter and figure at 50 % opacity, adds the stale chip on the status line and the cause; it never animates (DS-MOT-6).
- **DS-CMP-ROW-7** A disabled account (switched off) is not listed in the popover; in the main window it keeps its card at 55 % opacity (`.card.off`).
- **DS-CMP-ROW-8 Two windows.** When the core reports a pair of windows for a current reading (`windows`, `metrics::window_pair`), the tray row draws two bars instead of the single summary bar: a **thick** 6 px bar for the account's **shortest** window (5 h, day, week or month) and, below it, a **thin** 4 px bar for the **next longer** window. An account whose plan limits are all scoped to models gets its tightest model as the thick bar and the second tightest of the same window as the thin bar, named by the model. The core decides the pair; the UI never classifies windows from labels. When the limiting (binding) metric is neither of the two, the core puts it on the thin bar, so the window that blocks the account is always on screen; when it is exhausted, it takes the thick bar. A single window, a failed, pending or stale reading, a hidden metric of the pair, an uncapped balance and an exhausted account on paid extra usage keep the single bar and figure of DS-CMP-ROW-1..2.
  ```
  ┌──┐ Claude  [Max]                    100% 5h ↗ +12  ›
  │▣ │                                 49% Week ↘ −50
  └──┘ ▼
       ████████████████████████████████████
       ███████████████●░░░░░░░░░░░░░░░░░░░░
       ◔ Ahead of pace · ↑ ≈18:53            ↺ 19:30
  ```
  - **DS-CMP-ROW-8a Figure block.** Line 1: the thick window's remaining share (`--text-xl`/700, `--fg`; `--bad` with "0%" when that window is exhausted), its window name ("5h", "Day", "Week", "Month": the catalog's `card.binding.*`) or model name at `--text-2xs` muted, and its pace delta. Line 2, right-aligned below: the thin window's share and name at `--text-2xs` muted, and its delta. The window name replaces the unit word; it never truncates, a model name may (DS-MSG-15) with its full text in the tooltip and the row's accessible name. Shares round honestly (DS-CMP-MTR-4: "< 1%", never "100%" before the window is full).
  - **DS-CMP-ROW-8b Limiting emphasis.** When the thin window is the binding one, its bar and its line use at least the `--warn` role (`--bad` when its level is critical or it is exhausted), so a free short window's big figure does not read as the account's state. This is the one exception to DS-CMP-MTR-7's "binding is not recolored": the emphasis is the role, with "Most restrictive limit right now" (`card.binding.title`) in the meter's value text and the row's name. The status line keeps the account's state (binding pace or level, exhaustion with its return time).
  - **DS-CMP-ROW-8c Pace delta.** Only from the core's pace of that window, only on a fresh reading, hidden with pace (Settings › Show consumption pace) and with the row-display "Pace corridor" switch. In reserve: ↗ and the points in hand ("+12"), `--good`. Above the pace: ↘ and the points over ("−50"), `--bad` when the core projects the run-out before the reset, else `--warn` (the core's tone). On pace: → alone, muted. In percent mode the text is the core chip's compact form ("20%", DS-CMP-TIME-1), never a signed bare percent; early in a window (no ratio) it stays in points. The delta is decorative (`aria-hidden`); its words (the core chip's full text and outlook) are the tooltip, the meter's value text and part of the row's accessible name. No pace (just opened, no reset time, stale or aging): no delta and no mark; no "unavailable" text is added.
  - **DS-CMP-ROW-8d Accessibility.** Each bar is a `role="meter"` named by its window (or model) with its own `aria-valuetext` (DS-CMP-MTR-9: share, pace text, limiting note, reset); the row's accessible name adds "5h: 100% left, 12 pts in reserve; Week: 49% left, 50 pts ahead, Most restrictive limit right now" in place of the sub-limit preview text.

### 4.4 Account card, main window (`CARD`)

- **DS-CMP-CARD-1** The card is the expanded row at window scale: head (40 px tile, name `--text-xl`/650, account name muted when present — the line is omitted for an unnamed account —, actions, enable switch), summary (per-window figures for 5 h and weekly, else the tightest quota, each as a figure block with the unit word below the figure, DS-CMP-ROW-1b, as today's `.sum-bar .big small`), status line, extra lines (pace, incident, web session, local cost) and a foot of chips (count, binding, rate limit, alert, session expiry, age, next refresh).
- **DS-CMP-CARD-2** Cards in one grid row MUST keep equal heights; a missing window keeps its slot with "—" (current `.wins` behavior).
- **DS-CMP-CARD-3** The card foot shows at most four chips; the rest move into the details dialog.
- **DS-CMP-CARD-4** "Waiting" cards (no report yet) show a static skeleton (DS-CMP-STATE-2) with `aria-busy="true"`.

### 4.5 Badges and chips (`BADGE`)

```
[Max]        plan badge: outlined, mono, neutral
(◷ 3 min ago) status chip: pill, tinted, icon + text
```

- **DS-CMP-BADGE-1 Plan/tier badge.** Outlined hairline, `--radius-xs`, height 18 px, padding 0 6px (`--space-1` + `--space-0`), `--font-mono` `--text-micro`, `--fg-muted` text, no fill. Content comes from the core (plan or identity badges); the UI never invents a tier.
- **DS-CMP-BADGE-2 Status chip** (`.mini`, `.age`, `.alert-chip`). Pill, height 20 px (28 px when interactive), tint `color-mix(in srgb, currentColor 16%, transparent)`, text and icon in the state role, `--text-sm`/600, icon 14 px. It MUST contain text or have an accessible name; an icon-only chip is allowed only with a count and a tooltip that repeats the meaning.
- **DS-CMP-BADGE-3** Badges are not buttons. A chip that acts (opens a list, links) MUST be a `<button>` with the button focus ring.
- **DS-CMP-BADGE-4** Tones on identity badges (`.badge.accent/info/good`) are allowed only for core-supplied identity metadata (region, SSO, active); plan badges stay neutral.

### 4.6 Meters (`MTR`)

```
22 % left, expected ≈ 38 % left, corridor 33–43 %:
████████░░▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░   one solid bar; translucent band ±5 pts, no border or knob
                                       fill ends left of the band = ahead of pace
4 px bar: the corridor becomes a 2 px tick at the expected share
unknown / absent:
╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱╱   hatched track (DS-CMP-HON-1) + "—" as the figure
```

- **DS-CMP-MTR-1** A meter shows the **remaining** share of a capped limit (DS-ID-5), from the core's `remainingPct`. Fill role = level of that value (`--good`/`--warn`/`--bad`); empty part = `--track`.
- **DS-CMP-MTR-2 Continuous, always.** Every quota meter is one solid, complete bar (`.bar`): a `--track` pill with a single fill pill from the left edge, both `--radius-pill`. Meters MUST NOT be segmented, partitioned, stepped or dashed; the continuous bar is part of Cuotai's own identity (DS-ID-5) and is not borrowed from other monitors.
- **DS-CMP-MTR-3 Sizes.** Height 6 px in popover rows, sub-limits and cards; 4 px in compact contexts (dense list view, tables, menu-bar preview); 8 px MAY be used for the single hero meter of a card. The fill keeps the full radius at both ends; a fill narrower than its height renders as a dot of that height (never invisible).
- **DS-CMP-MTR-4** Rounding MUST NOT lie: a value > 0 and < 1 % shows a visible sliver and "< 1%"; a value < 100 % never shows a full meter or "100%". Used values above 100 % are clamped in the drawing and kept in the text.
- **DS-CMP-MTR-5 Absent values are never zero.** Unmeasured, uncapped or unknown metrics render no fill: the **unknown hatch** track (DS-CMP-HON-1) with "—" (or "No data") as the figure, or no meter at all and the core's balance text (`textualMetric()`) for uncapped metrics. Only `exhausted` renders a full-opacity empty track, always with the word "Exhausted" (DS-CMP-ROW-1a).
- **DS-CMP-MTR-6 Pace corridor (bars ≥ 6 px).** When the core supplies `pace` (PRD 0007) and the user has not hidden it, the bar carries Cuotai's signature **pace corridor**: the band of remaining shares the core counts as "on pace". Geometry:
  - center `c = (1 − expected_used_fraction) × 100 %` of the bar width; band from `c − 5` to `c + 5` percentage points, the same ±5-point rule the core uses for its stages (PRD 0007 RF1.4);
  - clamped to 0–100 %: the part beyond an end is clipped; a clipped band keeps a minimum visible width of 4 px against the end it touches;
  - it MUST NOT look interactive: a translucent band only, with **no border, no edge lines, no handle and no rounded knob**; height equal to the bar or at most 2 px taller (1 px overhang above and below); square ends (radius 0, or 1 px at most); drawn **over** track and fill as `color-mix(in srgb, var(--fg) 28%, transparent)`; `pointer-events: none`, no hover or cursor change. The band is supplementary to the stage text (status line, chip, accessible text), so it is exempt from the 3:1 non-text rule, but it MUST remain visible on the fill and on the track in every theme;
  - reading: the fill ending inside the band = on pace; to the left (less left than expected) = ahead of pace; to the right = in reserve. The core's stage text stays authoritative near the edges (hysteresis).
  - accessibility: `aria-valuetext` adds the core stage and the expected share ("22% left, 8 points ahead of pace, about 30% expected"); the tooltip says it is a linear estimate.
- **DS-CMP-MTR-6a Pace tick (4 px bars).** Bars under 6 px use a 2 px `--fg` tick at `c` instead of the corridor, overhanging 3 px above and below. It sits on the meter row only and never covers text.
- **DS-CMP-MTR-6b No pace mark** (neither corridor nor tick) for stale readings, unknown or absent values, exhausted limits, overage meters and metrics without a core pace.
- **DS-CMP-MTR-6c Pace marks of the tray's two windows (DS-CMP-ROW-8).** The thick bar carries a small downward triangle (8 × 5 px, `--fg`) just above the bar at `c`; the thin bar carries a 6 px `--fg` dot centered on the bar at `c`, ringed by 1 px of `--surface`. Both replace the corridor and the tick on those two bars only, follow DS-CMP-MTR-6b, are `aria-hidden` and non-interactive, and hide with the row-display "Pace corridor" switch.
- **DS-CMP-MTR-7 Thresholds.** The level comes only from the fill role at the 70 % / 30 % thresholds (DS-ID-5); thresholds are not drawn as marks, bands or gradients on the bar. Usage-alert thresholds (PRD 0008) appear as text in the status line, not on the meter. The binding metric's meter is not recolored; binding is shown by the stripe or chip (DS-COL-3).
- **DS-CMP-MTR-8 Stale.** A preserved stale reading (PRD 0010) uses the stale treatment (DS-CMP-HON-3): bar at 50 % opacity with the dotted outline, no pace mark, no transitions (DS-CMP-STATE-4).
- **DS-CMP-MTR-9** Meters MUST expose `role="meter"` (or `role="img"` where `meter` is unsupported) with `aria-valuemin="0"`, `aria-valuemax="100"`, `aria-valuenow` and `aria-valuetext` such as "22% left, resets at 19:30, ahead of pace". Today's `.bar` has no role; this is a required migration.

### 4.6a Ring gauge (`RING`)

```
   ╭───╮      single value, open "C" ring (DS-ID-8)
  │ 22% │     center: figure, or the state icon
   ╰─  ╯      track --track, arc in the level role
  ▣ Claude    tile name + account below
```

- **DS-CMP-RING-1** The ring shows **one** value: the core's binding remaining share (`bindingPct`) of one account, in the same level role as the meter (DS-CMP-MTR-1). It is never used to compare parts of a whole, never holds more than one arc, and as a gauge is allowed only in: the "All" filter tab (16 px, DS-CMP-TAB-1), the "Icon only" menu-bar style, the alternative ring-grid tray view (DS-CMP-SHELL-5), and as a brand mark. It is never the default tray view.
- **DS-CMP-RING-2** Geometry is fixed per size: 64 px grid tile (`viewBox 0 0 64 64`, radius 26, stroke 7) with center text `--text-lg`/700 tabular in `--fg` (DS-CMP-ROW-1a), or a 22 px state icon for an alert state; 16 px in the "All" tab and the menu-bar icon, without center text. Round cap, starting at 12 o'clock and running clockwise; the arc uses the level role.
- **DS-CMP-RING-3** It renders statically: the arc is drawn at its value with no transition or animation from a previous value (DS-MOT-1; the `.donut .arc` transition and `animateRings()` are removed).
- **DS-CMP-RING-4** Absent values follow DS-CMP-MTR-5: an unmeasured account shows the empty track and "—" in the center, never 0 %; a stale reading is at 50 % opacity.
- **DS-CMP-RING-6 Account-state glyph.** At 12 px (status line, tab corner, journal rows) to 16 px (menu bar, notifications, widgets) the static ring is the mark of one account's state: arc = binding remaining share in its level role; empty track + hatch for unknown (DS-CMP-HON-1); dotted and dimmed when stale; for a non-quota state (query error, sign-in needed, incident) the ring is replaced by that state's icon in its role. It is **always paired with text** (status label, accessible name or notification text) and never animates. Platform surfaces that cannot show color (menu-bar template images, monochrome notification icons) draw it in the OS foreground color with the arc length as the only encoding, next to the figure.
- **DS-CMP-RING-5** The tile exposes the value as text: an accessible name such as "Claude, work, 22% left, resets 19:30" (today's tooltip lines), with the SVG `aria-hidden`.

### 4.7 Status line (`STL`)

```
● Ahead of pace · runs out ~18:53                 ⟳ 19:30
mark  label (600)   detail (muted)          reset (mono, muted)
```

- **DS-CMP-STL-1** One line: leading mark + label (`--text-sm`/600, state role or `--fg` per DS-COL-7) + optional detail (" · " separator, muted) + right-aligned reset (refresh glyph 12 px + `--font-mono` time, muted).
- **DS-CMP-STL-2** The leading mark is the 12 px account-state ring (DS-CMP-RING-6), replacing the generic dot; for a non-quota state it is that state's icon. Pace stages and returns are carried by the time chips (DS-CMP-TIME), freshness by the age chip.
- **DS-CMP-STL-3** Label priority: blocked/exhausted > query failure > pace stage (core `pace.label`) > binding window name. The detail is the core's outlook or cause.
- **DS-CMP-STL-4** The reset shows the absolute time (DS-MSG-8); the relative countdown goes into the tooltip and the expanded sub-limit row. No reset known: omit the right part, never "now".
- **DS-CMP-STL-5 Overflow order.** The status line is always **one line**; it never wraps, so a collapsed row never grows beyond its title line, bar and status line (plus the single extra line of DS-CMP-ROW-4). When the content does not fit, elements yield in this order until it fits:
  1. the status label switches to its compact form (glyph + short text: "↑ 8 pts", "↑ 20%");
  2. the detail (" · Runs out ≈18:53") is dropped;
  3. the reset chip becomes compact text without chip background: "↺ dom 17:20" (glyph, short weekday when not today, time);
  4. the status label truncates with an ellipsis.
  The reset time itself never truncates. Everything dropped stays in the tooltip and the accessible name.

### 4.8 Sub-limit row (`SUB`)

```
5-hour session [Binding]                         22% left
█████░░░│░░░░░░░░░░░░░░
↑ 8 points ahead of pace             Resets 19:30 · in 1h 35m
```

- **DS-CMP-SUB-1** One per metric in the expanded row and the details: label (`--text-base`/500) + optional "Binding" chip; figure `--text-lg`/700 + unit word; meter; a meta line with the pace stage (core text, tone icon) on the left and "Resets HH:MM · in 1h 35m" (mono, muted) on the right. At narrow widths (the 380 px popover) the meta line MAY wrap to two lines: pace on the first, reset on the second, right-aligned; it never truncates the reset.
- **DS-CMP-SUB-2** Metrics the user hid are not listed in the popover; in the details they show at 45 % opacity with the show toggle (`.metric.muted`).
- **DS-CMP-SUB-3** Rate-limit windows are listed after quota windows and never become the row figure (core rule, `planMetrics`).

### 4.9 Callout (`CALL`)

```
┌──────────────────────────────────────────────┐
│ ◷  At this rate the session runs out around  │  tinted fill, radius-md
│    18:53, 37 min before it resets.           │
└──────────────────────────────────────────────┘
```

- **DS-CMP-CALL-1** Tones: `neutral` (`--surface-2`), `info`, `warning`, `critical`. Toned callouts use a 1 px border in the role plus a light tint: fill = `color-mix(in srgb, var(<role>) 8%, var(--surface))`, border = `var(--border-w) solid color-mix(in srgb, var(<role>) 60%, transparent)`. The border carries the tone, so the fill stays light and never reads muddy (for example warning on Dracula). Neutral has the hairline and `--surface-2` fill; `--radius-md`; padding `--space-3`; leading icon 16 px in the role; text `--fg` `--text-md`.
- **DS-CMP-CALL-2** The key phrase MAY be in the role color **and** weight 600 (never color alone).
- **DS-CMP-CALL-3** At most one callout per row or dialog section. Forecasts come from the core's pace outlook; callouts with actions put one ghost button at the end.
- **DS-CMP-CALL-4** Existing ad-hoc boxes (`.cost-advisory`, `.login-panel`, `.extension-consent`, `.incident-detail`, `.local-cost`) migrate to this component.

### 4.10 KPI stat card (`KPI`)

```
┌──────────────┐
│ Limit hits   │ label: --text-sm muted
│ 1            │ figure: --text-figure / 700, tabular
│ in 7 days    │ caption: --text-sm muted
└──────────────┘
```

- **DS-CMP-KPI-1** `--surface`, hairline, `--radius-md`, padding `--space-3`. Up to three per row in the popover (min 100 px each), up to four in windows.
- **DS-CMP-KPI-2** Values come from the core (history aggregates, cost summaries). Insufficient data shows "—" and a caption saying why ("Not enough history"); never 0.
- **DS-CMP-KPI-3** Estimated figures carry the "≈" prefix (DS-CMP-HON-2) and "estimate" in the caption (local cost is always an estimate, PRD 0015). A KPI with unknown value shows "—" over a hatched baseline strip (DS-CMP-HON-1); a KPI built only from stale data gets the stale treatment (DS-CMP-HON-3).

### 4.11 Lists and grouped lists (`LIST`)

```
PROVIDERS                                Connects locally
┌──────────────────────────────────────────────────────┐
│ ▣ Claude                                  [Connect]  │
│   Browser sign-in                                    │
├──────────────────────────────────────────────────────┤
│ ▣ Codex                                   [Connect]  │
└──────────────────────────────────────────────────────┘
```

- **DS-CMP-LIST-1** A group = section header + one container (`--surface`, hairline, `--radius-md`) with rows separated by hairlines. Section header: micro label (DS-TOK-TYP-5) on the left, optional muted hint (`--text-sm`, sentence case) on the right.
- **DS-CMP-LIST-2** Row: min height 44 px (52 px with a subtitle), padding `--space-2` `--space-3`; leading 28 px tile or 16 px icon; title `--text-base`; subtitle `--text-md` muted (one line, truncates); one trailing control (button, switch, check, chevron or value).
- **DS-CMP-LIST-3** A subgroup inside a container ("Not in menu bar") uses an inset muted label row, not a new container.
- **DS-CMP-LIST-4** Lists longer than the viewport scroll inside the body; the section header of the visible group MAY stick.
- **DS-CMP-LIST-5** An empty list shows one muted line inside the container, not an empty box.
- **DS-CMP-LIST-6 Provider catalog.** The "Add account" catalog is grouped by **sign-in method** (owner decision; for example browser sign-in, local app or CLI credentials, API key, cloud profile, extension), one group per method from the core's descriptors (PRD 0027's category grouping must change to this; categories remain a search term), each row with the provider tile, name, a muted subtitle naming the method and an outlined "Connect" button. Providers billed per query (for example AWS Cost Explorer) carry a "Billed per query" status chip (DS-CMP-BADGE-2, warning role, icon + text) in the row and on the account afterwards.

### 4.12 Search field (`SRCH`)

- **DS-CMP-SRCH-1** Height 32 px, `--radius-sm`, `--input-bg`, hairline; leading search icon 14 px muted; placeholder names what is searched and how many ("Search 38 providers"); a clear button appears when non-empty.
- **DS-CMP-SRCH-2** Matching ignores case and accents (today's `norm()`). `Esc` clears a non-empty field, then blurs; `/` focuses the main window filter; `↓` moves into results.
- **DS-CMP-SRCH-3** No results: one muted line with the query ("No provider matches 'xyz'").

### 4.13 Buttons (`BTN`)

| Variant | Fill | Border | Text | Use |
|---|---|---|---|---|
| primary | `--accent` | `--accent` | `--accent-fg`, 600 | the one main action of a view or dialog |
| secondary (default `.btn`) | `--btn-bg` | hairline | `--btn-fg` | other actions |
| outlined | transparent | hairline | `--fg` | per-row actions in lists ("Connect") |
| ghost (`.link`) | transparent | none | `--fg` or role | footer actions, inline links, low emphasis |
| icon (`.icon`) | transparent | none | `--fg-muted` → `--fg` on hover | toolbar and row actions |
| destructive | `--bad` | `--bad` | `--bad-fg`, 600 | confirming a destructive action in a dialog |

- **DS-CMP-BTN-1** Sizes: small 26 px, default 30 px, large 34 px (dialog footers next to 34 px fields). Horizontal padding `--space-3`; radius `--radius-sm` for all sizes.
- **DS-CMP-BTN-2** Labels: sentence case, verb first, at most three words ("Sync models", "Open status page"). No trailing period, no ellipsis unless the action opens another step that asks for input ("Change…").
- **DS-CMP-BTN-3** Icons lead the label, 14 px (small) or 16 px, gap 6px (`--space-1` + `--space-0`). An icon-only button MUST have `aria-label` and a tooltip with the same text (today several have only `title`).
- **DS-CMP-BTN-4** At most one primary per view; in action rows it is the last (rightmost in LTR). A destructive button is never the default-focused one and always sits apart from the primary or behind a confirmation.
- **DS-CMP-BTN-5** States: hover `--surface-2` (secondary, outlined, ghost, icon) or 8 % darker/lighter mix (primary, destructive); disabled opacity 0.45; a disabled button that needs explaining stays focusable with `aria-disabled="true"` and a tooltip (`.btn.is-disabled`). Loading: the label stays, a 14 px spinner replaces the icon, `aria-busy="true"`, the button is disabled.

### 4.14 Segmented control and tabs (`SEG`)

```
┌───────────────┐
│ [24h]  7d     │   track --surface-2, padding 2px, radius-sm
└───────────────┘   selected: --surface fill, --fg text, 600
```

- **DS-CMP-SEG-1** One implementation for every 2–5 exclusive choice: track `--surface-2`, padding `--space-0`, `--radius-sm`; segments height 24 px (28 px in settings rows), inner radius `--radius-sm` − 2 px; selected = `--surface` fill + `--fg` 600; unselected = transparent + `--fg-muted`. No shadow on the selected segment. Today's two variants (`.segmented` with shadow, `.history-ranges` with accent fill) merge into this one.
- **DS-CMP-SEG-2** Labels are short and parallel ("24h / 7d / 30d", "Light / Dark / System"); range labels use the mono family for digits.
- **DS-CMP-SEG-3** Semantics: `role="radiogroup"` with `role="radio"`/`aria-checked`, or buttons with `aria-pressed` (today's history ranges). Arrow keys move the selection.
- **DS-CMP-SEG-4** Page tabs (settings pages, details sections) use the same look at full width with `role="tablist"`. The settings window keeps PRD 0020's sidebar as its page navigation (a navigation landmark, not tabs); segmented tabs are used only inside a page, never to switch settings pages.
- **DS-CMP-SEG-5 Settings sidebar.** More than four pages is expected: settings are organized, not centralized. Order: General, Dashboard, Menu bar, Alerts, Data & privacy, a collapsible **Advanced** group (Integrations, Local cost, Extensions, Files), then About separated at the bottom. There is **no separate Accounts page**: Settings › Dashboard hosts the account list and its management (add, edit, remove, order, enable/disable, monogram override) next to the row layout settings. Each entry is icon + one- or two-word label; the Advanced group is a disclosure whose open state is remembered, and its pages are reachable by search while it is collapsed. This supersedes the seven-page list of PRD 0020 RF2.1.

### 4.15 Select and menus (`SEL`)

- **DS-CMP-SEL-1** Choices from a list longer than five use a native `<select>` styled as a field (32 px, `--radius-sm`, hairline, `--input-bg`) with a trailing chevron.
- **DS-CMP-SEL-2** Action and option menus (`.lang-menu`, `.sortmenu`) are floating panels: `--surface`, hairline, `--radius-md`, elevation 1, item height 30 px, selected item marked by a check icon and weight 600 (not only the accent outline used today). `role="menu"` with `menuitemradio`; `Esc` closes and returns focus.
- **DS-CMP-SEL-3 Language selector.** Wherever a language is chosen (Settings › General and any quick menu), each option shows a flag (`flagSvg()` in `js/flags.js`, 3:2, 20 × 13 px in menus and lists; no outline, except flags with a white field (Japan, South Korea, Indonesia), which get a 1 px `--border` hairline so they stay visible on light surfaces) followed by the language's own name (endonym, the catalog's `_meta.name`), never a translated name. Flag resolution follows `flagCountry()`: an explicit `_meta.flag`, else the **region** of the locale code, else the language. An unknown country falls back to the globe. The 14 planned languages (PRD 0035):

| Locale | Endonym | Flag |
|---|---|---|
| pt-BR | Português | Brazil (never Portugal) |
| en-US | English | United States |
| es | Español | Spain, unless a region says otherwise |
| it | Italiano | Italy |
| zh-CN | 中文（简体） | China |
| fr | Français | France |
| de | Deutsch | Germany |
| ja | 日本語 | Japan |
| ko | 한국어 | South Korea |
| hi-IN | हिन्दी | India |
| id-ID | Bahasa Indonesia | Indonesia |
| ru | Русский | Russia |
| tr | Türkçe | Türkiye |
| vi | Tiếng Việt | Vietnam |

Locales without a region (fr, de, ja, ko, ru, tr, vi) resolve through `flagCountry()`'s language map; every row of the locale registry has drawn artwork (a test fails on the globe). The list follows the registry order; a language whose translation is still in progress is listed after the others with its flag, its endonym and a muted "Translation in progress" caption, `aria-disabled`, skipped by the arrow keys and never selected; the caption, not color alone, carries the state. Each endonym uses its own script's font stack (DS-TOK-TYP-7) and the menu scrolls inside a bounded height. The flag is decorative (`aria-hidden="true"`); the endonym is the accessible label, with `lang` set to that language. Flags never stand alone as the only cue.

### 4.16 Checks, switches and option cards (`CHK`, `SW`)

- **DS-CMP-SW-1** A **switch** turns a setting or an account on and off and applies at once (PRD 0020 RF4). Shared `.switch` only, `role="switch"`, `aria-checked`; track `--track` → `--accent`, knob `--switch-knob`. The label sits on the left of the row; the switch on the right.
- **DS-CMP-CHK-1** A **circular check** (20 px) marks membership of an item in a set (accounts in the menu bar, providers to show): unchecked = hairline circle, checked = `--accent` disc with `--accent-fg` check. It is a `role="checkbox"`.
- **DS-CMP-CHK-2** Square checkboxes are not used, except inside consent confirmations where the platform convention is expected.
- **DS-CMP-CHK-3** **Option cards** (choosing a style with a visual preview): grid of 2 columns, `--surface`, hairline, `--radius-md`, preview on top, label below; selected = 2 px `--accent` border and label in `--fg` 600. `role="radio"` within a `radiogroup`.

### 4.17 Drag handle (`DRAG`)

- **DS-CMP-DRAG-1** Six-dot `grip` icon 16 px, muted, in a 24 × 32 px target at the row start; cursor `grab`/`grabbing`; shown only where order is editable (custom order without a filter, today's rule).
- **DS-CMP-DRAG-2** Every drag has a keyboard path: focus the handle and press `↑`/`↓` (main window today), and move buttons where space allows. The dragged item is at 45 % opacity; the drop target shows a 2 px dashed `--accent` outline.
- **DS-CMP-DRAG-3** A list with handles ends with one helper line ("Drag to set the order") in muted `--text-sm`.

### 4.18 Keycap hint (`KEY`)

- **DS-CMP-KEY-1** `<kbd>`: `--font-mono` `--text-micro`, hairline, `--radius-xs`, height 18 px, padding 0 `--space-1`, `--fg-muted`. Platform-aware text: `⌘,` on macOS, `Ctrl+,` elsewhere; `esc` lowercase.
- **DS-CMP-KEY-2** Keycaps are hints only: the control also exposes `aria-keyshortcuts`, and the keycap itself is `aria-hidden`. At most one keycap per control and two per view.

### 4.19 Chevrons and back navigation (`NAV`)

- **DS-CMP-NAV-1** Drill-in views show a back icon button (chevron left, 28 px) at the header start, the view title, and an `esc` keycap at the header end. `Esc` and the back button do the same thing.
- **DS-CMP-NAV-2** Chevron right (16 px, muted) means "opens a view"; chevron down means "expands here". The same element never mixes both meanings.

### 4.20 Footer bar (`FOOT`)

```
⚙ Settings ⌘,   ⌁ History                ⟳ 1 min ago
```

- **DS-CMP-FOOT-1** Height 44 px, `--surface`, hairline top. Left: up to two ghost buttons with icon + label (Settings with its keycap, History/Stats). Right: the global refresh control.
- **DS-CMP-FOOT-2** The right side shows the age of the **last completed refresh** ("1 min ago", mono digits) followed by a refresh icon button; it shows the spinner while any refresh runs. Accounts whose reading is aging or stale carry their own age on their row (DS-CMP-ROW-4), so the footer age never stands for them.
- **DS-CMP-FOOT-3** The footer has no attention chips (today's `.alerts` is removed): attention is shown by the dots on the filter tabs (DS-CMP-TAB-2) and by each row's status line. Quit and "open main window" move into the Settings or app menu if the footer runs out of width.
- **DS-CMP-FOOT-4** One-line notices (next reset, update available: today's `.pop-notice`) sit directly above the footer, at most one at a time, highest priority first (update < reset notice < backend error).

### 4.21 Empty, loading, error and stale states (`STATE`)

- **DS-CMP-STATE-1 Empty.** Icon 22 px muted, one sentence, one primary action ("No accounts yet" + "Add account"). Padding `--space-8`. A filtered empty list says the filter matched nothing and offers to clear it.
- **DS-CMP-STATE-2 Loading.** Static skeleton blocks in `--track` with the final layout's shape (`--radius-pill` for text lines, `--logo-radius` for tiles), `aria-busy="true"`; no shimmer. If loading lasts more than 2 s, a muted "Loading…" line appears.
- **DS-CMP-STATE-3 Error.** Inline, next to what failed: critical callout or the cause line (`.cause`), with the core's message and one recovery action (retry, sign in again, open settings). Never a raw response, URL or token (AGENTS.md).
- **DS-CMP-STATE-4 Stale.** Old values at 50 % opacity, still (no transitions), the stale chip ("Stale · 42 min ago") and the cause. The binding, alerts and ordering never use the stale values (PRD 0010).
- **DS-CMP-STATE-5 Partial.** A list with some failed accounts renders every account in its own state; one failure never blanks the screen.

### 4.22 Tooltips (`TIP`)

- **DS-CMP-TIP-1** Tooltips (today native `title`) only add detail: absolute times, full truncated text, expected pace share, the list behind a count. Anything needed to operate or understand the state MUST also be visible or in the accessible name, since `title` is not keyboard- or touch-accessible.
- **DS-CMP-TIP-2** At most two lines of content; no interactive content in tooltips (use an info popover, PRD 0020 RF5.3).

### 4.23 Messages: banners, toasts and inline feedback (`MSG`)

- **DS-CMP-MSG-1** Prefer inline feedback next to the control that caused it (field error under the field, row error in the row).
- **DS-CMP-MSG-2 Banner** (`.banner`): window-level and persistent conditions (backend error, server port taken, hooks unconfirmed, other config file). Full width, `--radius-md`, callout fill of its tone, icon + text + at most one link action + dismiss when dismissable; `role="status"` (or `role="alert"` for failures that block the app).
- **DS-CMP-MSG-3 Toast**: only to confirm a completed action ("Saved", "Account removed"); bottom corner, `role="status"`, auto-dismiss after 4–6 s, pausing on hover/focus. Errors and anything needing a decision MUST NOT be toasts.
- **DS-CMP-MSG-4** At most one toast at a time; a new one replaces the old.

### 4.24 Dialogs (`DLG`)

- **DS-CMP-DLG-1** Native `<dialog class="dlg">` opened through `showDialog()` (focus return), widths from DS-TOK-LAY-2, `--surface`, hairline, `--radius-lg`, elevation 2 with `--scrim` (no blur). Header: title `--text-xl`/650 and a close icon button; body padding `--space-5`; action row right-aligned, primary last.
- **DS-CMP-DLG-2** Dialogs are for focused tasks and confirmations (account form, details, destructive confirmations, consents). Settings is a window (PRD 0020); popover drill-ins are views (DS-CMP-SHELL-2). Dialogs never stack: a second step replaces the content or opens a sub-view.
- **DS-CMP-DLG-3** Destructive confirmation: the title names the action and the object ("Remove Claude · work?"), the body says what is kept and what is lost, the destructive button repeats the verb, Cancel has initial focus.
- **DS-CMP-DLG-4** `Esc` closes unless an unsaved draft exists, in which case it asks first (PRD 0020 RF4.4 pattern). Long content scrolls in the body; header and actions stay visible.

### 4.24a Row content and density (`DENS`)

Rows carry a lot of information; the user chooses how much of it rows show.

- **DS-CMP-DENS-1 Presets.** Settings › Dashboard offers **Detailed**, **Standard** (default), **Minimal** and **Custom**:

| Element | Detailed | Standard | Minimal | Custom |
|---|---|---|---|---|
| Figure and meter | always | always | always | always (not hideable) |
| Plan badge | on | on | off | toggle |
| Account label / e-mail | on | on (label) | off | toggle |
| Monogram (multi-account) | on | on | on | toggle |
| Unit word under the figure | on | on | off | toggle |
| Pace corridor / tick | on | on | off | toggle |
| Status label (pace stage, state) | on | on | one status (DS-CMP-DENS-3) | toggle |
| Account-state ring glyph | on | on | off | toggle |
| Runs-out chip | on | on (when projected) | one status | toggle |
| Returns (reset) chip | on | on | one status | toggle |
| Freshness age | always | aging/stale only (DS-CMP-ROW-4) | stale only | toggle |
| Incident mark | on | on | one status | toggle |
| Extra-usage sub-row | on | chip in the status line only when active or capped | off | toggle |
| Sub-limit preview line ("Week 71% · 5h 22%") | on | off | off | toggle |

- **DS-CMP-DENS-2 Never lose information.** The figure and the meter are the identity of a row and are never hideable; when a provider has several accounts the account label stays as the row title (DS-CMP-ROW-1) whatever the toggle. A hidden element only **moves**: it stays in the expanded row, the details, the tooltip and the accessible name. Hidden elements free their space; no empty gaps or placeholders remain.
- **DS-CMP-DENS-3 Minimal's one status.** Minimal shows a single status item chosen by priority: incident > stale > exhausted or blocked (with its return time) > runs out before the reset > the pace stage.
- **DS-CMP-DENS-4 Scope.** The choice applies to the tray and the main window **together** by default; a switch "Use a different layout in the main window" reveals a second preset selector for the window. Stored in `desktop.json` (`rows.tray`, `rows.window`: preset plus, for Custom, the element map; absent = Standard).
- **DS-CMP-DENS-5 Preview.** The settings page shows a live preview row (DS-CMP-PREV-1) built from a real account (the first one with a pace, else a fixed sample labeled as an example) that updates with every toggle, without animation. Custom lists the elements as switches in the order of the table above.

### 4.25 Account monogram (`MONO`)

- **DS-CMP-MONO-1** When more than one configured account belongs to the same provider, each of those accounts' provider tiles carries a monogram badge at the bottom-right corner: two uppercase letters on a `--surface` pill with a hairline, height 40 % of the tile (min 12 px), `--font-ui` 700 at 9–11 px, `--fg`. Accounts of a provider with a single account show no monogram. Because the core requires a name only in that multi-account case (PRD 0029), a monogram always has a label to derive from.
- **DS-CMP-MONO-2** Letters: the first letters of the first two words of the account label ("Work Acme" → "WA"); a one-word label gives its first two letters ("personal" → "PE"); letters are folded to ASCII when possible (NFKD), otherwise kept.
- **DS-CMP-MONO-3** Collisions among accounts of the same provider: keep the first letter and take the next letter of the label not yet used, then a digit 2–9; the result is stable in configuration order.
- **DS-CMP-MONO-4** The user MAY override the monogram (1–2 characters) per account in the account form; it is a desktop display preference (`desktop.json`, keyed by account key), never written to the TOML. The monogram is decorative: the account label stays in the row title (DS-CMP-ROW-1) and the accessible name. It appears on rows, tab tiles (DS-CMP-TAB-1), timeline lanes and menu-bar items where the tile is shown.

### 4.26 Honest-data language (`HON`)

One visual vocabulary for "we do not know", "this is an estimate" and "this is old", used identically in meters, rings, charts, KPI cards and the menu bar.

- **DS-CMP-HON-1 Unknown hatch.** Unknown or absent values are drawn as a diagonal hatch: `repeating-linear-gradient(135deg, var(--track) 0 2px, transparent 2px 5px)` over the 40 %-opacity track. It is a hard-stop pattern built only from roles, the single allowed use of a gradient function (DS-CHT-2). It always comes with "—", "No data" or "Cost unknown" in text. Fallback, plain 40 % track: themes whose character forbids patterns (Win98, Windows XP, CRT, or any theme that sets `--hatch: none`, a new optional theme-character token) and widths under 12 px.
- **DS-CMP-HON-2 Estimate prefix "≈".** Every figure that is an estimate (local cost amounts, projected run-out times, averages derived from partial history) is prefixed with "≈", placed inside the translated string so each language positions it; the tooltip or caption says "estimate" once. Provider-reported values never carry "≈".
- **DS-CMP-HON-3 Stale treatment.** A preserved stale reading is dimmed to 50 % and outlined with a 1 px dotted `--fg-muted` outline (offset 2 px) around the meter, ring or chart mark, plus the stale chip with its age (DS-MSG-10). Aging readings are not dimmed; they only show their age chip.
- **DS-CMP-HON-4** The three treatments never combine on one mark except stale + estimate ("≈" text on a dimmed mark). Color alone never carries any of them.

### 4.27 Time chips (`TIME`)

Shared with the TUI and the menu bar. The shapes come from the TUI's pace vocabulary (`docs/tui.md`): ↓ in reserve, → on pace, ↑ ahead of pace, ✕ exhausted; ↺ returns. On the desktop they are glyphs of the icon set (DS-TOK-ICO-1); in the TUI the Unicode characters.

- **DS-CMP-TIME-1** A time chip is a status chip (DS-CMP-BADGE-2) whose text comes from a **core catalog key** (`Localized`), never composed in the UI, so the tray, menu bar, notifications, widgets and TUI read the same words. Budget: ≤ 18 characters including "≈" and the time, excluding the glyph, in every catalog (DS-MSG-14); the two percent keys (`pace.chip.*_pct`) may use up to **22** characters, because they must name the comparison. Their compact form (DS-CMP-STL-5 step 1) is glyph + percent ("↑ 20%") and appears only beside the full text in tooltip and accessible name.
- **DS-CMP-TIME-2** Wording (final; the `{at}` example is 18:53, `{d}` is a 1–2 digit point delta, `{p}` a 1–3 digit percent):

| Key (core) | Glyph | en-US | pt-BR | es | it |
|---|---|---|---|---|---|
| `pace.chip.reserve` | ↓ | {d} pts in reserve | {d} pts de reserva | {d} pts de reserva | {d} pt di riserva |
| `pace.chip.on_pace` | → | On pace | No ritmo | Al ritmo | In linea |
| `pace.chip.ahead` | ↑ | {d} pts ahead | {d} pts acima | {d} pts por encima | {d} pt sopra |
| `pace.chip.ahead_pct` | ↑ | {p}% faster than pace | {p}% acima do ritmo | {p}% sobre el ritmo | {p}% sopra il ritmo |
| `pace.chip.reserve_pct` | ↓ | {p}% slower than pace | {p}% abaixo do ritmo | {p}% bajo el ritmo | {p}% sotto il ritmo |
| `pace.chip.exhausted` | ✕ | Exhausted | Esgotado | Agotado | Esaurito |
| `pace.chip.runs_out` | ↑ | Runs out ≈{at} | Acaba às ≈{at} | Se agota ≈{at} | Finisce ≈{at} |
| `pace.chip.lasts` | ↓ | Lasts to reset | Dura até renovar | Llega al reinicio | Dura fino al reset |
| `resets.chip.returns` | ↺ | Returns {at} | Volta {at} | Vuelve {at} | Torna {at} |

- **DS-CMP-TIME-3** Compact form (menu-bar title, ring tiles, 380 px rows under pressure): glyph + time only ("↺ 19:30", "↑ ≈18:53"), with the full chip text in the accessible name and tooltip.
- **DS-CMP-TIME-4** "Runs out" is shown only when the core projects exhaustion before the reset; otherwise "Lasts to reset". "Returns" uses `blocked_until` or the binding reset; an unknown return reads "Return unknown" (`group.returnUnknown`), never a time.
- **DS-CMP-TIME-5** These strings replace PRD 0024 RF1.1/RF1.2's `pace.short.*` wording ("{delta} pts under pace", "+{delta} pts vs pace", "empty", "empty ~{at}", "lasts"), which echoes another product and MUST be changed in that PRD.
- **DS-CMP-TIME-6 Pace deviation unit.** Deviations are shown in **points** by default ("8 pts ahead"). A user setting switches every surface (chips, status line, sub-limit meta line, details, accessible text, notifications; the TUI follows its own option) to **percent**, using the `_pct` keys: the deviation relative to the expected use, computed by the core (decision M: the core provides both forms; the UI never converts). The setting is a row in Settings › Dashboard, "Show pace as", with a segmented control **Points | Percent** (DS-CMP-SEG), saved on change (`desktop.json`, absent = points). The pace corridor (DS-CMP-MTR-6) is unaffected.
  - **Meaning.** Points = the difference between the used share and the expected share; percent = how much faster or slower than expected, relative to the expected use. Example: expected 50 % used, actually 60 % → "10 pts ahead" or "20% faster than pace".
  - **The setting explains itself.** Under the segmented control, one line shows both forms with the live example from a real account when available (else the fixed example above), and a one-line explanation: "Points: difference from the expected share. Percent: how much faster or slower than expected."
  - **Unambiguous chips.** Percent chips always name the comparison ("20% faster than pace", "20% slower than pace"); a bare "+20%" is forbidden anywhere a quota figure could be read. Accessible text spells it out: "Using quota 20 percent faster than the expected pace".

### 4.28 Components for planned features

**Extra usage (PRD 0023, `EXTRA`)**

- **DS-CMP-EXTRA-1** An **extra-usage sub-row** follows the plan limits in an expanded row, a card and the details: label "Extra usage" + the core's overage state chip; when the provider gives a positive cap, a continuous meter of the **remaining extra budget** in `--warn` (`--bad` at the cap), without pace mark. Overage never forms the binding limit, a pace, a ring or a timeline lane.
- **DS-CMP-EXTRA-2** States and their chips (texts from the core `overage.*`): `Disabled` → nothing on rows, "Extra usage off" in details only; `Armed` → muted chip "Extra on · nothing spent"; `Active` → warning chip "On extra · $6.25" and, when the plan limit is exhausted, the row figure "$6.25 extra" (DS-COL-3); `CapReached` → critical chip "Extra budget used up"; `Unknown` → hatched chip "Extra amount unknown" (DS-CMP-HON-1), never $0.
- **DS-CMP-EXTRA-3** The **Extra spent** KPI card (DS-CMP-KPI) lists one figure per currency; provider units appear as a second line, never summed with money; unknown shows "—".

**Notifications (PRD 0024, `NOTIF`; also 0003/0008/0023 alerts)**

- **DS-CMP-NOTIF-1** OS notifications carry plain text from the core (`alerts.*`): title "{Account}: {what}" ≤ 50 characters ("Claude · Work: may run out before the reset"); body one sentence ≤ 110 characters with absolute local times, "≈" for projections and the margin ("At this rate Session (5h) runs out ≈18:53, 37 min before the reset"). No emoji, no color reliance, no URLs or tokens.
- **DS-CMP-NOTIF-2** The image, where the OS allows one, is the 16 px account-state ring (DS-CMP-RING-6) or the provider tile; otherwise the app icon. One notification per account per pass (existing grouping); the projection is listed first. Clicking opens the popover on that account.

**Dependent settings (PRD 0024 RF4, 0020 RF3.3, `DEP`)**

- **DS-CMP-DEP-1** A switch row may own dependent controls (a segmented control such as "15 min / 30 min / 1 h", a select or a number field) rendered directly below it, indented by `--space-6`, attached by the group hairline. While the switch is off they stay visible and **disabled** (not hidden), with their saved value; turning the switch on enables them without moving the layout.

**Menu bar (PRD 0025, `MENU`, `PREV`)**

- **DS-CMP-MENU-1** By default the menu-bar item is OS-painted: template images and plain title text only, and the default style is **Tightest %** (decision N). Brand colors and provider tiles are never used there, and theme roles only in the colored variant, DS-CMP-MENU-4 (PRD 0025 RF3.6), which is the single allowed exception to this monochrome rule; provider marks are monochrome glyphs. The title uses tabular figures, at most 24 characters, items separated by a thin space, dropping items from the end with "+2" before any figure is cut.
- **DS-CMP-MENU-2** Styles: **Meters** = up to four continuous bars (DS-CMP-MTR-2, never segmented) in a 16–18 pt template image, each the remaining share with the < 1 % sliver rule; **Logos + %** and **Tightest %** = text; **Icon only** = the 16–18 pt template ring (DS-CMP-RING-6) of the tightest binding share. Unknown = "—" (or an empty ring), stale = the trailing stale mark of PRD 0025 RF1.4, an outage = a template-compatible notch in the icon, never a color dot.
- **DS-CMP-MENU-3** The item updates at most once per second and never animates (DS-MOT-1).
- **DS-CMP-MENU-4 Colored variant.** Settings › Menu bar offers a "Colored" switch (off by default). When on, the level is drawn with the theme's status roles (`--good`/`--warn`/`--bad`; `--info` and `--fg-muted` where DS-COL-3 says so) taken from the theme's mode that matches the **menu bar's** appearance, not the window's, and rendered as a non-template image. Only the ring or meter fill and the status glyph are colored; the figure stays in the OS foreground color and is always present, so the variant adds color but never replaces text. Each role used MUST reach 3:1 against both the light and the dark menu-bar background; a role that fails falls back to the monochrome rendering for that item. Unknown, stale and outage keep the monochrome marks of DS-CMP-MENU-2.
- **DS-CMP-PREV-1** A **live preview card** shows what a setting will produce: `--surface-2` strip inside a `--surface` card with hairline and `--radius-md`, a muted "Live preview" label on the left and the rendered result on the right, built from current readings through the core (`tray_preview`). It never animates, is not a live region, and has a text equivalent in its accessible name.

**Stats (PRD 0026, `STATS`, `JRNL`)**

- **DS-CMP-STATS-1 Placement.** Stats is a **view of the main window** (entry in the topbar next to the account grid; back with `Esc`). The popover footer's "Stats" ghost button opens a compact **drill-in** (the three KPI cards and the 7-day bar chart) whose last line is "Open full stats", which focuses the main window on that view.
- **DS-CMP-STATS-2 Scope picker.** A select (DS-CMP-SEL-1) at the view header end: "All accounts", then each account with its tile and monogram; the range uses a 7 d / 30 d segmented control (DS-CMP-SEG).
- **DS-CMP-JRNL-1 Journal row.** Leading 12 px account-state ring or event icon; the core's event summary (`Event::summary`, `--text-base`) over the account name (muted); right-aligned mono time (DS-MSG-8). Rows group under day headers (micro labels: "Today", "Yesterday", then dates); newest first; at most 20 rows in the popover drill-in, paged in the window.
- **DS-CMP-STATS-3 Coverage footnote.** One muted `--text-sm` line at the end of the view, from the core (`stats.history_kept`): "History is kept on this computer for 30 days"; partial coverage adds "Data from 12 of 30 days". It is never a callout; "Not enough history" replaces the charts (DS-CHT-11).

**Provider catalog (PRD 0027, `CAT`)**

- **DS-CMP-CAT-1 Grid or list.** The catalog is a list (DS-CMP-LIST-6) below 560 px of available width (popover, dialog-sm/md) and a tile grid (96 px tiles: provider tile, name, one-line sign-in hint; 3–4 columns) at 560 px and above. Search results are always a list, ranked by the core.
- **DS-CMP-CAT-2** "Found on this device" is a status chip in the `--info` role with a check icon, on tiles and rows of detected local sources (presence only, PRD 0027 RF6); it never implies a working account.
- **DS-CMP-CAT-3** After choosing a provider the recommended connection mode opens directly; the others sit in an **"Other ways to connect"** disclosure (native `<details>`, collapsed, chevron down), each with its hint.
- **DS-CMP-CAT-4** No match: the empty state (DS-CMP-STATE-1) "No provider matches 'xyz'" with an outlined **Request a provider** button (opens the issue template through `open_external`).

**Contract health (PRD 0028, `DIAG`, `RPT`, `VER`)**

- **DS-CMP-DIAG-1** A failing account whose provider response changed shows a **diagnostic block** in its details: a neutral callout titled "What changed", the core's sentence ("The provider changed `five_hour.utilization` (number → string)"), up to eight field paths in `--font-mono`, and the note "Values are never recorded". It is selectable text, never a raw body.
- **DS-CMP-RPT-1** **Report a problem** (failing account details and About) opens a dialog (`--dialog-md`): title, a read-only preview of the exact issue text in a scrollable mono box (max 240 px), the line "Nothing is sent by Cuotai; GitHub opens with this text", and actions Copy (secondary) and Open on GitHub (primary, last).
- **DS-CMP-VER-1** "Verified N months ago" is a muted status chip (relative months, absolute date in the tooltip) on the access line of details; when the core flags the evidence as old it takes the warning role and the text "Not verified since {date}" (DS-MSG-8 date-only format). **Not rendered:** by the owner decision of 2026-10-25 no app surface shows integration status (level, summary or verification age); the access line and this chip stay out of the details, and the age lives in `docs/providers.md`.

## 5. Charts

- **DS-CHT-1 Allowed types**, and only these:

| Type | Use | Today |
|---|---|---|
| Continuous meter | remaining share of one limit | `.bar`, DS-CMP-MTR |
| Reset timeline ("limits come back") | when each account's binding limit returns, one lane per account | new |
| Daily bar chart | one value per day (peak use, local cost, tokens) | `cost.js` `.lc-bars` |
| Area/line history chart | one metric over 24 h / 7 d / 30 d with reset markers | `history.js` `.hist-chart` |
| Sparkline mini-bars | 7–30 compact bars per list row | new |
| Ring gauge | one binding remaining share: "All" tab, "Icon only" menu-bar style, alternative ring-grid tray view (identity, DS-ID-8) | `tray.js` `.donut` |

- **DS-CHT-2 Forbidden:** pie and donut charts, 3D, gradients in data marks (the unknown hatch of DS-CMP-HON-1 is a pattern, not a gradient, and is the only exception), dual axes, stacked areas, decorative backgrounds, animated entrances. Pie and donut charts are banned for any comparison of parts or series. The ring gauge (DS-CMP-RING) is not a donut chart: it is a single-value identity element, allowed only in the places DS-CMP-RING-1 lists (the "All" tab, the "Icon only" menu-bar style and the alternative ring-grid tray view, never the default view).
- **DS-CHT-3 Data honesty.** A bucket or day with no sample is a gap in lines and areas (`chartPaths()` already breaks runs); in bar charts it is a full-height **hatched ghost column** (DS-CMP-HON-1) labelled "No data" in its tooltip and table, never a zero bar. A day with real zero usage is a zero-height bar with a 1 px baseline tick. A day with usage but no price is a hatched bar at its token-proportional height (replacing `.lc-bar.unknown`'s plain `--track`). Estimated series (local cost, projections) prefix their figures with "≈" (DS-CMP-HON-2); stale points use the stale treatment (DS-CMP-HON-3).
- **DS-CHT-4 Axes.** Value axes start at zero. Percent axes run 0–100 with dashed `--border` gridlines at 50 and 100 only; other axes have at most three gridlines. Tick labels: `--font-mono` `--text-micro` `--fg-muted`. Time axes label the first and last bucket plus at most five evenly spaced ticks.
- **DS-CHT-5 Marks.** Bars: equal width, gap `--space-0` (dense) to `--space-1`, top corners `--radius-xs`, bottom square on the baseline. Neutral bars use `--chart-neutral`; the current period (today) uses `--accent` and its label reads "Today". Threshold colors (`--warn`/`--bad`) are allowed only when the axis is a share of a limit and only for bars at ≥ 100 %. Lines 1.5 px with `vector-effect: non-scaling-stroke`; areas a flat `color-mix` of the line role at ≤ 22 %.
- **DS-CHT-6 Reset timeline.** Lane per account (14 px tile + optional account name when a provider has several), sorted by return time; a vertical "Now" line in `--fg-muted` labelled "Now"; from Now to the reset a 2 px line in the binding metric's level role; a 8 px dot at the reset with its time label (mono). Range toggle 24h / 7d (DS-CMP-SEG). Accounts whose return is unknown are listed below as text ("Return unknown"), never plotted at an edge. Data: the core's binding `resets_at`/`blocked_until` only.
- **DS-CHT-7 Sparkline mini-bars.** Inline, 16–24 px tall, shared scale across the list so rows are comparable; last bar highlighted with `--accent` (or `--bad` when it hit the limit); numeric summary right-aligned next to it ("61% avg", "1 hit").
- **DS-CHT-8 Legends.** Avoid legends: label marks directly (time on the reset dot, "Today" on the bar). When a legend is unavoidable (history reset marker, `.hist-key`), it is one inline key with a shape sample, below the chart.
- **DS-CHT-9 Formatting.** Numbers and dates follow DS-MSG-7/8. Tooltips on marks give the exact value and date.
- **DS-CHT-10 Accessibility.** Every chart has a text alternative: `role="img"` with an `aria-label` summarising range, peak and last value, plus an equivalent table (visually hidden or in a disclosure), as `history.js` and `cost.js` do today. Highlighting never relies on color alone: the highlighted bar is also labelled, the reset is a dashed shape.
- **DS-CHT-11 Insufficient data.** Fewer than two points (history) or no day with data (bars) shows no chart: one muted sentence explaining why and when it fills in ("The chart fills in as readings are recorded"), not empty axes.

## 6. Content and messages

- **DS-MSG-1 Voice.** Calm, factual, brief. State the fact, then what to do. No exclamation marks, no blame ("you failed"), no jokes in states, no anthropomorphism ("Cuotai thinks…").
- **DS-MSG-2 Casing.** Sentence case for everything (titles, buttons, tabs, menu items); uppercase only via CSS for micro labels (DS-TOK-TYP-5). Product and provider names keep their own casing.
- **DS-MSG-3 Length.** Labels ≤ 4 words; descriptions ≤ 90 characters on one line (PRD 0020 RF5.1); status details ≤ 6 words in the collapsed row.
- **DS-MSG-4 Units.** Figure first, unit word after (below the figure in rows and cards, DS-CMP-ROW-1b), muted: "22% left", "$6.25 extra", "1.2M tokens". "Left" means remaining share of a limit: the English (en-US) value of the catalog's `card.available` is "left", other catalogs keep their own word ("disponível"); "used" only where the core reports use. Pace deltas are percentage **points** by default ("8 pts in reserve"); "%" appears only when the user chose percent (DS-CMP-TIME-6), and then it is the core's relative deviation, always with its comparison words ("20% faster than pace"), never points relabelled and never a bare "+20%".
- **DS-MSG-5 Absent and estimated values.** Unknown = "—" or "No data"; unpriced = "Cost unknown"; below a cent = "< $0.01"; estimated figures carry "≈" (DS-CMP-HON-2) and say "estimate" once per view (cost, pace tooltip "linear estimate"). Never "0" for absent, never "unlimited" unless the core says so.
- **DS-MSG-6 Core texts.** Pace stages and outlooks, binding summaries, incident states, freshness and causes come from the core's `Localized` (`tl()`); the UI MUST NOT compose them from parts or translate them itself. UI-only strings live in `ui/locales/*.json`.
- **DS-MSG-6a Terminal names.** No desktop text names an environment variable or a `params.*` value (PRD 0047 G1/G7). A core text that does is replaced, by its id, with the desktop's own wording (`js/desktop-wording.js`, keys `coreText.*`); when the fix is a web session the cause line offers "Connect session".
- **DS-MSG-7 Numbers.** `Intl.NumberFormat(currentLocale())` for every number; percentages as integers in rows and cards, up to two decimals only in tables; compact notation for token counts; currency with `style: "currency"` (USD today, PRD 0015).
- **DS-MSG-8 Time.** Absolute clock times use the locale's `toLocaleTimeString` with 2-digit hour and minute: within today the time only ("19:30"), within 6 days the short weekday and time ("Sat 11:54"), otherwise the short date and time ("22 Oct, 19:30"). Durations until a reset: "in 1h 35m", "in 3d 15h" (`fmtCountdown`). Ages: `Intl.RelativeTimeFormat` short ("3 min ago"). Rows show absolute reset times, details add the duration; the tooltip always has the full date. Dates without a time (release dates, price-table dates, "since" dates) use `toLocaleDateString(currentLocale(), { day: "numeric", month: "short", year: "numeric" })` inside a translated sentence ("Released 22 Oct 2026", "Lançada em 22 de out. de 2026"), never an ISO string on screen.
- **DS-MSG-9 Error pattern.** "What happened. What to do." using the core's classification: authentication ("Sign-in expired. Sign in again."), rate limit, network, schema change, configuration. A provider incident adds the core note that the failure may not be the account's (PRD 0011 RF3). Never response bodies, URLs, tokens or stack traces.
- **DS-MSG-10 Stale pattern.** Chip "Stale · 42 min ago" + cause line + the old values dimmed. Aging: "Aging · 12 min ago" without dimming.
- **DS-MSG-11 Incident pattern.** Core state label ("Partial outage") + optional incident title + "Open status page". Operational and plain unknown states appear only in details.
- **DS-MSG-12 i18n.** Every string exists in every shipped catalog (today pt-BR, en-US, es, it; PRD 0035 adds zh-CN, fr, de, ja, ko, hi-IN, id-ID, ru, tr, vi). Plurals use each catalog's plural categories (`Intl.PluralRules`: one/other, few/many for ru, none for zh/ja/ko). Names truncate with an ellipsis and keep the full text in the accessible name. No sentence is built by concatenating translated fragments: use one key with placeholders (`{time}`, `{count}`) and the catalog's plural forms. Word order, separators (" · ") and punctuation live inside the translated string.
- **DS-MSG-14 Design against the longest catalog.** pt-BR, es and it typically run 20–35 % longer than English, and short labels expand far more. With PRD 0035, **German is the expected worst case**, followed by Russian and French (long compounds and words that cannot break); CJK strings are short in characters but each glyph is about 1 em wide and taller, so budgets are measured in rendered pixels, never in character counts. Every fixed-width component is laid out against the **longest shipped string** of each key across all catalogs (UI: `apps/desktop/ui/locales/*.json`; core: `crates/cuotai-core/locales/*.json` plus the Portuguese fallbacks in the code), never against English. Examples from today's catalogs:

| Key | en-US | Longest | Growth |
|---|---|---|---|
| `freshness.stale` | Stale | Desactualizada (es) | ≈ 3× |
| `freshness.aging` | Aging | In invecchiamento (it) | ≈ 3× |
| `settings.title` | Settings | Configurações (pt-BR) | +60 % |
| `common.refresh` | Refresh | Actualizar (es) | +40 % |
| `incident.trayTitle` | Provider status | Estado de los proveedores (es) | +65 % |
| core `pace.ahead` | {delta} points above the pace | {delta} puntos por encima del ritmo (es) | +20 % |
| core `pace.runs_out_at` | runs out at {at}, {margin} before the reset | se agota a las {at}, {margin} antes del reinicio (es) | +15 % |
| core `incident.state.degraded` | Degraded performance | Prestazioni degradate (it) | +5 % |

- **DS-MSG-15 Truncation and wrap policy per component.** Numbers, units, percentages, currency amounts, reset times and durations MUST NEVER truncate or wrap apart from their unit; the text around them gives way first.

| Component | Policy |
|---|---|
| Tray row title (provider or account) | one line; truncate with ellipsis, full name in tooltip and accessible name |
| Plan badge | one line; truncate after 12 characters with tooltip |
| Trailing figure block (figure over unit word) | never truncates or wraps; block width = wider line; the title shrinks first. The unit word below the figure frees row width: "disponibile" (it, 11 characters) no longer competes with the title |
| Status line (DS-CMP-STL) | one line, never wraps; yields in the DS-CMP-STL-5 order (compact label → drop detail → compact reset → truncate label); the reset time never truncates; full text in the accessible name |
| Sub-limit meta line | wraps to two lines (DS-CMP-SUB-1), never truncates |
| Status chips, freshness chip | one line; the word truncates only in the 380 px popover, with tooltip; the age digits never |
| Buttons | one line, no truncation: labels are budgeted (DS-CMP-BTN-2) and the button grows; in action rows buttons wrap to a new line instead |
| Filter and page tabs, segmented controls | one line, no truncation; icon tabs carry text only in the accessible name; segments are budgeted to fit the longest catalog |
| Settings sidebar items | one line; truncate with tooltip below the icon-rail breakpoint (PRD 0020 RF8.4), labels budgeted to two words |
| KPI label and caption | label wraps to two lines; caption truncates with tooltip; the figure never |
| Callouts, banners, descriptions, errors | wrap; never truncate |
| Dialog titles | wrap to two lines |

- **DS-MSG-16 Longest-locale check.** UI review MUST render each changed screen at the minimum size in: the longest catalog for its strings (de once shipped; until then pt-BR or es, and it for status words), one CJK locale (ja or zh-CN, for glyph height and line breaking) and hi-IN (line height). Development builds offer a pseudo-locale `en-XA` that expands every string (+100 % up to 10 characters, +50 % up to 20, +40 % beyond), replaces letters with accented and stacked-diacritic forms, and wraps each string in ⟦ ⟧ so concatenation, hard-coded text and clipping are visible; it is never offered in release builds. A screen passes when nothing in the "never truncates" list is cut.
- **DS-MSG-13** Glossary (one term per concept, all surfaces including the TUI): *account* (not agent), *provider*, *limit*/*quota*, *binding limit*, *pace*, *reset*, *reading*, *fresh/aging/stale*, *incident*, *local cost (estimate)*, *history*, *usage alert*, *reset notice*, *extension*.

## 7. Motion

### 7.1 Policy

- **DS-MOT-1** Motion is allowed only to show **continuity of a direct user action** (a disclosure opening, a view replacing another, a dragged item settling) or a **state in progress** (a refresh running). Data changes are not animated: new values appear at once.
- **DS-MOT-2** Allowed properties: `opacity`, and `height`/`grid-template-rows` for disclosures. No `transform` on data, no scale, no shake, no bounce, no spring overshoot.
- **DS-MOT-3** Durations: `--motion-fast` (redefined to 120 ms) for hover, focus and color; `--motion-base` (redefined to 180 ms) for disclosures, view switches and dialog fade. Nothing exceeds 200 ms. `--motion-slow` and `--ease-spring` are deprecated aliases of `--motion-base` and `--ease-out`, removed once unused.
- **DS-MOT-4** Easing: `--ease-out` only. Exits are instant or `--motion-fast`.
- **DS-MOT-5 Forbidden:** looping or ambient animation (except DS-MOT-7), shimmer, pulses, count-up numbers, level flashes, shakes, staggered entrances, exit ghosts, animated gradients, chart entrances, decorative parallax.
- **DS-MOT-6** Stale readings, hidden windows (`document.hidden`) and reduced motion run no animation at all.
- **DS-MOT-7** The only loop is the refresh spinner (`.spin`, `--motion-spin`) on the control whose work is in progress, while it is in progress. With reduced motion it is replaced by the static icon plus the text "Refreshing…" in the accessible name.
- **DS-MOT-8 Reduced motion.** `prefers-reduced-motion: reduce` and **Settings → Animations: Reduced/Off** zero every duration (existing `themes.css` mechanism, `!important`). "Reduced" MAY keep opacity fades ≤ 120 ms; "Off" keeps nothing. Themes may lower durations (Win98 `steps()`, CRT 0 ms) and MUST NOT raise them.
- **DS-MOT-9** An idle window has `document.getAnimations().length === 0` (PRD 0004 acceptance rule, kept).

### 7.2 Inventory of current motion (input for the PRD)

| # | Where | Effect | Decision |
|---|---|---|---|
| 1 | `main.css` `.spin`, `@keyframes spin` (topbar, card, tray header refresh) | continuous rotation while busy | **Keep** (DS-MOT-7) |
| 2 | `detail.css` `.models-sync:disabled svg.i` | spin while syncing models | **Keep** (same rule) |
| 3 | `main.css` `.skeleton .sk::after`, `@keyframes shimmer` | shimmer on "Waiting" cards | **Remove**: static skeleton |
| 4 | `main.css` `.bar > span` `transition: width --motion-slow` | bar width interpolation 400 ms | **Remove**: value changes at once (DS-MOT-1) |
| 5 | `main.css` `.bar, .win-pct, .sum-bar .big` color transition | level color fade | **Simplify**: keep at `--motion-fast` or drop; no other effect depends on it |
| 6 | `main.css` `.card:hover/:focus-within` `scale(1.01)` + shadow | hover elevation | **Remove** (DS-TOK-ELV-2) |
| 7 | `main.css` `.switch` background transition `--motion-base` | track color | **Simplify** to `--motion-fast` |
| 8 | `main.css` `.switch span` transform `--ease-spring` | knob spring | **Simplify**: `--motion-fast`, `--ease-out`, no overshoot |
| 9 | `tray.css` `.donut .arc` stroke-dasharray transition + `tray.js` `animateRings()` | ring fills from the previous value | **Remove** the arc animation; the ring itself stays, rendered statically (DS-CMP-RING-3) |
| 10 | `tray.js` focus handler: `.pop` fade + 6 px slide | popover entrance | **Simplify**: opacity only, `--motion-fast`, or nothing |
| 11 | `detail.css` `.hist-enter`, `@keyframes hist-in` (`history.js` `paint()`) | chart wipe-in | **Remove** |
| 12 | `motion.js` `countUp()` | percentage counting | **Remove** |
| 13 | `motion.js` `after()` level change outline | 600 ms outline flash | **Remove** |
| 14 | `motion.js` `after()` exhaustion shake | translateX shake | **Remove** |
| 15 | `motion.js` `after()` entrance | fade + 8 px, 30 ms stagger | **Remove** |
| 16 | `motion.js` `after()` FLIP | cards slide to new positions | **Simplify**: only after a user-initiated move (drag, arrow keys), `--motion-base`; never after a refresh or sort |
| 17 | `motion.js` `after()` exit ghost | removed card shrinks and fades | **Remove** |
| 18 | `motion.js` `spinOnce()` | one turn of the refresh icon on click | **Remove** (item 1 covers feedback) |
| 19 | `motion.js` `pulseIfDue()` | next-refresh chip pulses at zero | **Remove** |
| 20 | `motion.js` `showDialog()` | fade + scale 0.96, grow from the card, backdrop fade, reverse exit | **Simplify**: opacity fade in `--motion-base`, instant exit, no scale or grow; keep the focus-return logic |
| 21 | `motion.js` `crossFade()` + `themes.css` `::view-transition-*` | theme/language cross-fade | **Simplify**: `--motion-fast` (avoids a flash), off with reduced motion |
| 22 | `dialog.css` `::backdrop` `blur(3px)`; `main.css` `.topbar` `blur(12px)` | blur effects (static) | **Remove** from components; theme-owned blur stays (macOS, Windows 11) |
| 23 | `css/themes/crt.css`, `synthwave.css` scanlines, vignette, glow; Win98 bevels | static theme character | **Keep** (DS-ID-1), MUST NOT reduce contrast below section 3.3 |
| 24 | TUI `motion.rs` (PRD 0004 RF4): gauge interpolation, label blink, braille spinner, 120 ms wipe | terminal effects | **Recommend** for the PRD: keep the spinner, remove the blink and wipe, interpolation off by default (DS-PRI-8 vocabulary only; the TUI is not bound by this section) |

## 8. Accessibility and keyboard

- **DS-A11Y-1 Focus.** `:focus-visible` shows a 2 px solid `--focus` outline, offset 2 px (−2 px inside scrolling lists and segmented tracks). Focus is never removed, hidden by overflow or animated away. Mouse focus MAY hide it (`:focus-visible` only).
- **DS-A11Y-2 Keyboard map.** `Esc` closes the topmost layer: menu, then popover drill-in (back), then dialog; in the popover overview it hides the popover. `⌘,`/`Ctrl+,` opens Settings from any window. `/` focuses the main window filter. `Tab` follows reading order; arrow keys move inside tabs, segmented controls, menus and lists; `Enter`/`Space` activate and toggle disclosures. New global shortcuts need an entry in this list.
- **DS-A11Y-3 Roles.** Filter tabs and page tabs: `tablist`/`tab` with `aria-selected` and `aria-controls`. Accordion rows: `<button aria-expanded aria-controls>`. Meters: DS-CMP-MTR-9. Switches: `role="switch"`. Segmented: DS-CMP-SEG-3. Charts: DS-CHT-10. Status updates (refresh result, saved, sync progress): `role="status"` / `aria-live="polite"`; blocking failures `role="alert"`. Skeletons: `aria-busy`.
- **DS-A11Y-4** Composite widgets (tabs, segmented, menus, lists of rows) use roving `tabindex`: one tab stop per widget.
- **DS-A11Y-5 Not color alone.** Every state shown by a role also has text or a shape (section 3.2). Every icon-only control has an accessible name.
- **DS-A11Y-6 Targets.** DS-TOK-HIT-1. The popover remains fully operable with the keyboard after it opens (focus lands on the first tab or the expanded row).
- **DS-A11Y-7 Zoom.** At 200 % text zoom no information is lost; figures and reset times stay on one line, names wrap or truncate.

## 9. Governance

- **DS-GOV-1** This document is the source of truth for desktop UI shapes, sizes, components, charts, wording and motion. A PR that needs something not covered here updates this document in the same PR (new rule with a new ID, or a changed rule with its rationale).
- **DS-GOV-2 Adding a component.** First try to compose existing ones. If a new one is needed: add its anatomy, rules and states here; build it from tokens only; give it one class namespace; render it in the four reference themes (Dracula, GitHub light, Win98, CRT) and the four languages.
- **DS-GOV-3 Themes** define only what section 3.4 lists: color roles, the closed list of theme-character tokens (`--font-*`, `--radius-*`, `--logo-radius`, `--border-w`, `--shadow`, `--shadow-card`, `--shadow-dialog`, surface fills, theme-scoped bevels and scanlines), and lower motion durations. Themes MUST NOT change spacing, sizes, layout, component anatomy, chart types or wording (`css/themes/README.md` already forbids layout changes).
- **DS-GOV-4 Enforcement.** `tools/lint_theme.py` and `tools/contrast.py` keep checking themes (extended with the new roles and the 4.5:1 rules of section 3.3). A lint over `ui/css/*.css` and `ui/js/*.js` SHOULD reject literal colors, literal durations and `@keyframes` outside the allowed spinner.
- **DS-GOV-5 PR checklist for UI changes:**
  - [ ] Built from section 4 components; no one-off shapes, sizes or radii.
  - [ ] Only color roles; no literal colors; contrast holds in light and dark of the reference themes.
  - [ ] No new motion beyond section 7; idle window has no running animations; reduced motion checked.
  - [ ] Absent values render as absent, never zero; estimates and stale readings are labelled.
  - [ ] Interpretive texts come from the core; new UI strings exist in pt-BR, en-US, es and it, without concatenation.
  - [ ] Longest-locale (or pseudo-locale) check done at the minimum size; numbers, units and reset times never cut (DS-MSG-14..16).
  - [ ] Keyboard path, focus ring, accessible names and roles present; targets ≥ 24 px.
  - [ ] Screenshots in Dracula, GitHub light, Win98 and CRT attached; rule IDs cited for any exception.

## 10. Decisions log

Decisions of the project owner that settled questions raised while drafting this document. They are binding.

| Date | Decision | Rules |
|---|---|---|
| 2026-10-22 | The tray ring stays: it is the "C" of Cuotai, an identity element and a single-value component, rendered statically with a text value. Pie and donut charts stay banned for comparisons. | DS-ID-8, DS-CMP-RING-*, DS-CHT-1, DS-CHT-2, section 7.2 item 9 |
| 2026-10-22 | Themes keep the closed "theme character" list beyond colors (fonts, radii, logo radius, border width, shadows, surface fills, scoped bevels and scanlines); no layout, size, anatomy, chart or wording changes. | DS-GOV-3, DS-ID-1 |
| 2026-10-22 | Provider tiles keep their brand colors. | DS-ID-3, DS-COL-1 |
| 2026-10-22 | Large figures keep the UI font with tabular digits; monospace only for times, plan badges, keycaps, codes and chart ticks. | DS-ID-4, DS-TOK-TYP-1, DS-TOK-TYP-2 |
| 2026-10-22 | A fresh reading's age moves to the footer; rows show the age only when aging or stale (footer age refined below: last completed refresh). | DS-CMP-ROW-4, DS-CMP-FOOT-2 |
| 2026-10-22 | English shows "left" for `card.available`; extra balance uses the warning role (money is being charged); authentication needed keeps the accent role as a call to action, not a quota level. | DS-MSG-4, DS-COL-3 |
| 2026-10-22 | Built-in themes' card shadows are softened, not removed. | DS-TOK-ELV-1, DS-TOK-ELV-4 |
| 2026-10-22 | Settings keeps PRD 0020's sidebar navigation; segmented tabs only inside pages. | DS-CMP-SEG-4 |
| 2026-10-22 | The default tray view is the list of account rows (meters made continuous on 2026-10-23); the per-account ring grid is an alternative view the user selects, and further compact views may be added. A single static ring stays in the "All" tab and as the "Icon only" menu-bar style. | DS-ID-8, DS-CMP-SHELL-5, DS-CMP-RING-1/2, DS-CMP-TAB-1, DS-CHT-2 |
| 2026-10-22 | The footer shows the age of the last completed refresh (replaces "oldest reading" above); aging and stale rows carry their own age. | DS-CMP-FOOT-2, DS-CMP-ROW-4 |
| 2026-10-22 | With several accounts of one provider, the account label is the row title and the tile identifies the provider; the sub-limit meta line may wrap to two lines. | DS-CMP-ROW-1, DS-CMP-SUB-1 |
| 2026-10-22 | Large figures are neutral (`--fg`); the level is shown by the meter fill, status label and tab dot; an exhausted limit reads "Exhausted" with an empty meter. | DS-CMP-ROW-1a, DS-CMP-RING-2 |
| 2026-10-22 | Settings is organized in more than four pages: General, Dashboard, Menu bar, Alerts, Data & privacy, collapsible Advanced (Integrations, Local cost, Extensions, Files), About. | DS-CMP-SEG-5 |
| 2026-10-22 | The tray header loses the view toggle, sort, language flag and refresh-all (to Settings › Dashboard / General); refresh stays in the tray footer next to the age. The main topbar moves theme and language to Settings; "Add account" is outlined. | DS-CMP-HEAD-2, DS-CMP-FOOT-2, DS-CMP-SHELL-3 |
| 2026-10-25 | The usage access level (Official, Undocumented, Derived, Unavailable) and its summary leave every UI surface (picker, cards, details, dialog notes); integration status lives only in `docs/providers.md` and `docs/oauth.md`. The picker lists providers by name only and leaves out those without any usage query (core `access::has_usage_query`). | DS-ID-6, DS-MSG-6, DS-MSG-13 |
| 2026-10-22 | New token `--shadow-card` (one soft layer) for main-window cards only. | DS-TOK-ELV-1, DS-TOK-ELV-4, section 3.4 |
| 2026-10-22 | Toned callouts use a role-colored border plus a light tint, so warning does not read muddy on Dracula. | DS-CMP-CALL-1 |
| 2026-10-22 | *Superseded 2026-10-23:* the pace marker snapped to segment boundaries on segmented meters. | DS-CMP-MTR-6 |
| 2026-10-22 | The provider catalog is grouped by authentication method; metered providers show a "Billed per query" chip; about 25 new glyphs join the icon set; date-only values follow a localized format; the tray footer drops attention chips in favor of tab dots. | DS-CMP-LIST-6, DS-TOK-ICO-1, DS-MSG-8, DS-CMP-FOOT-3 |
| 2026-10-22 | What a theme may define is listed explicitly (color roles + closed theme-character list + lowering motion), for new custom themes. | section 3.4, DS-GOV-3 |
| 2026-10-23 | Meters are continuous, solid and complete bars, never segmented: Cuotai's own identity, not a copy of other monitors. The pace tick sits at the exact expected share; unknown = empty track + "—"; stale = dimmed. The claim that the meter echoes the logo is withdrawn (the ring keeps that role). | DS-ID-2, DS-CMP-MTR-1..9, DS-CHT-1 |
| 2026-10-23 | Language choices show a flag plus the endonym; flags resolve by explicit `_meta.flag`, then region, then language (pt-BR = Brazil); flags are the only multicolor icons besides provider tiles and the logo, and are decorative for assistive technology. | DS-CMP-SEL-3, DS-COL-1 |
| 2026-10-23 | Fixed-width components are designed against the longest shipped catalog string, with a per-component truncation and wrap policy and a longest-locale check in review. | DS-MSG-12, DS-MSG-14..16, DS-GOV-5 |
| 2026-10-23 | Pace corridor: the ±5-point band on bars ≥ 6 px, the 2 px tick on 4 px bars, no mark for stale, unknown, exhausted or overage. | DS-CMP-MTR-6, MTR-6a, MTR-6b |
| 2026-10-23 | The static C-ring (12–16 px) is the account-state glyph, replacing the status dot and tab dot, always with text; used in menu bar, notifications and widgets. | DS-ID-8, DS-CMP-RING-6, DS-CMP-TAB-2, DS-CMP-STL-2 |
| 2026-10-23 | Honest-data language: unknown hatch (the only gradient-function exception, plain dim in retro themes), "≈" for estimates, dotted outline + dim for stale; applied to meters, rings, charts and KPI cards. | DS-CMP-HON-1..4, DS-CMP-MTR-5, MTR-8, DS-CHT-2, DS-CHT-3, DS-CMP-KPI-3, DS-MSG-5 |
| 2026-10-23 | Shared time chips with the TUI pace shapes and Cuotai's own wording in four languages, from core keys; PRD 0024 RF1 wording must change. | DS-CMP-TIME-1..5 |
| 2026-10-23 | Account monogram on the tile corner when a provider has more than one account; user override in `desktop.json`. | DS-CMP-MONO-1..4 |
| 2026-10-23 | Components for PRDs 0023–0028 added; catalog grouped by sign-in method (PRD 0027 must change); Stats is a main-window view with a popover drill-in. | DS-CMP-EXTRA, NOTIF, DEP, MENU, PREV, STATS, JRNL, CAT, DIAG, RPT, VER; DS-CMP-LIST-6 |
| 2026-10-23 | The return ladder and the retro-themes ↔ TUI parity are recorded as candidates, not normative. | Appendix A |
| 2026-10-23 | In account rows and cards the unit word goes below the big figure: a right-aligned two-line figure block, unit on one line never truncated, chevron centered on the block; frees width for pt-BR, es and it. | DS-TOK-TYP-6, DS-CMP-ROW-1, ROW-1b, CARD-1, DS-MSG-15 |
| 2026-10-23 | The unit word under the figure uses a new smallest step `--text-2xs` (10 px, line-height 1.1, muted, weight 400), keeping the block near the figure's height; it must still meet 4.5:1. | DS-TOK-TYP-3, DS-TOK-TYP-6, DS-CMP-ROW-1b |
| 2026-10-23 | Typography for the ten planned languages: per-script system stacks via `--font-script`, `lang` on text, line-height and minimum-size adjustments for CJK and Devanagari, no faux bold or italic for CJK, Latin tabular digits everywhere; German is the expected worst case and an `en-XA` pseudo-locale joins the review; flags and endonyms for all 14 languages. | DS-TOK-TYP-7..10, DS-CMP-SEL-3, DS-MSG-12, DS-MSG-14, DS-MSG-16 |
| 2026-10-23 | Pace deviation in points by default, with a user setting (Settings › Dashboard, "Show pace as": Points / Percent); percent chips added; the core provides both forms (decision M). | DS-CMP-TIME-2, DS-CMP-TIME-6, DS-MSG-4 |
| 2026-10-23 | Menu bar defaults to Tightest % as a monochrome template; a "Colored" variant uses the theme's status roles for the menu bar's appearance, with text always present (decision N). | DS-CMP-MENU-1, DS-CMP-MENU-4 |
| 2026-10-23 | The account name is optional (PRD 0029): rows and cards show the provider name alone for an unnamed account; a name, and therefore a monogram, exists only when a provider has more than one account. | DS-CMP-ROW-1, DS-CMP-CARD-1, DS-CMP-MONO-1 |
| 2026-10-23 | Row content and density: presets Detailed, Standard (default), Minimal and Custom with per-element toggles; figure and meter never hideable; hidden information only moves; one setting for tray and window with an optional separate window layout; live preview. | DS-CMP-DENS-1..5 |
| 2026-10-23 | Percent mode means the deviation relative to expected use, explained in the setting with a worked example; percent chips name the comparison ("20% faster than pace"), up to 22 characters; never a bare "+20%". | DS-CMP-TIME-1, TIME-2, TIME-6, DS-MSG-4 |
| 2026-10-23 | The pace corridor is a borderless translucent band (no knob or slider look, at most 2 px taller than the bar); the status line never wraps and yields in a fixed order; tab rings sit outside the tile with a 2 px gap. | DS-CMP-MTR-6, DS-CMP-STL-5, DS-MSG-15, DS-CMP-TAB-2 |
| 2026-10-25 | Amendments from delivering PRD 0022: the tray footer's Settings keycap lives in its tooltip and `aria-keyshortcuts` only (DS-CMP-FOOT-1); attention chips are dropped for tab badges (DS-CMP-FOOT-3); the tray views are List (default), Rings and Compact (DS-CMP-SHELL-5); the logotype appears in the main topbar and drill-in headers, never in the tray overview header (DS-ID-2); a changed figure gets a static accent marker until the next refresh or until opened, with a polite announcement (section 7); KPI figures longer than six characters step down to `--text-xl` (DS-CMP-KPI-1); sub-limit headers may wrap label and figure onto two lines (DS-CMP-SUB-1); the 64 px ring shows a state icon, never a word, in its center (DS-CMP-RING-2). | DS-CMP-FOOT-1, FOOT-3, SHELL-5, DS-ID-2, section 7, KPI-1, SUB-1, RING-2 |
| 2026-10-23 | Account list and management live in Settings › Dashboard (no Accounts page); the colored menu-bar variant is the single exception to the monochrome rule (PRD 0025 RF3.6); white-field flags (jp, kr, id) get a 1 px `--border` hairline; the TUI falls back to ASCII shapes when ambiguous-width glyphs render double width. | DS-CMP-SEG-5, DS-CMP-MENU-1, DS-CMP-SEL-3, DS-PRI-8 |
| 2026-10-27 | Core texts naming an environment variable or a `params.*` value read in the desktop's own words, with "Connect session" when a web session is the fix (PRD 0047 G7). | DS-MSG-6a |
| 2026-10-29 | Tray rows show the shortest window as a thick bar and the next longer one (or the second tightest model) as a thin bar, chosen by the core; the big figure is the thick window's with its name, the thin one's share sits below it; a limiting thin window takes at least the warning role; pace marks become a triangle above the thick bar and a dot on the thin one, with a ↗/↘ points delta beside each share. | DS-CMP-ROW-8, ROW-8a..8d, DS-CMP-MTR-6c, MTR-7 |

## Appendix A. Candidates (guidelines, not normative)

Ideas the owner kept for later. They do not bind any PR; adopting one means writing normative rules in section 4 or 5 and logging the decision in section 10.

- **A1 Return ladder.** A form of the reset timeline (DS-CHT-6) where each lane starts with the account's current continuous bar (remaining share, pace corridor) and continues as a time line to its return dot, lanes sorted by return time. It joins "how much is left" and "when it comes back" in one row. Open points: density at 380 px, a shared time scale for 24 h / 7 d, and keeping the meter and the time axis visually distinct.
- **A2 Retro themes ↔ TUI parity.** The CRT and Windows 98 themes deliberately mirror the TUI's look, and the TUI's shape vocabulary (pace ↓ → ↑ ✕, incident ! ▲ ✕ i ?) becomes the desktop's icon set for those concepts, so screenshots of the terminal and the desktop read as one product. Open points: documenting each glyph once for both front ends, and the risk that ↑ ("using faster") reads as "good".
