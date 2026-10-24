# Cuotai themes

A theme = a `css/themes/<id>.css` file + an entry in `js/themes.js` + an `@import` in `css/themes/index.css`.
Themes supply color roles and theme character only; shapes, sizes, components and motion rules live in [`docs/design-system.md`](../../../../../docs/design-system.md).

## Selectors

`<html data-theme="<id>" data-mode="dark|light">`. Each file defines **only** blocks with the selectors

```css
:root[data-theme="<id>"][data-mode="dark"]  { … }
:root[data-theme="<id>"][data-mode="light"] { … }
```

**Every theme must offer both modes (`dark` and `light`)**, with the same visual identity adapted to each (for example, a light "terminal" theme becomes paper/amber on cream; a light/dark operating-system theme follows the system's official variant). List both in `modes` in the registry. Every extra selector
(effects, fine adjustments) must also start with `:root[data-theme="<id>"]` (and, if applicable, `[data-mode=…]`),
so it never leaks into another theme. Do not use `!important`, do not change `html`/`body` outside the tokens
and do not redefine layout rules (width, grid, padding, font sizes): appearance only.

## Tokens (all required per mode, except those marked optional)

Colors — **always `#rrggbb`** (the contrast checker reads these values):

| Token | Use |
|---|---|
| `--bg` | window / popover background |
| `--surface` | cards, dialogs, controls |
| `--surface-2` | hover, secondary tracks, groups |
| `--border` | dividers and outlines |
| `--fg` / `--fg-muted` | primary / secondary text |
| `--accent` / `--accent-fg` | highlight (switch, primary button) and the text on it |
| `--good` / `--warn` / `--bad` / `--info` | states (≥ 70 %, 30–70 %, < 30 %, informational) |
| `--track` | empty track of the bars |
| `color-scheme` | `dark` or `light` |

Shape and typography (optional; defaults live in `themes.css`):

`--font-ui`, `--font-mono`, `--radius-lg` (cards, dialogs, popover), `--radius-md` (rows, metrics, boxes),
`--radius-sm` (buttons, fields), `--radius-pill` (badges, switch, bars), `--logo-radius`, `--border-w`.

Surfaces and effects (optional):

`--shadow`, `--shadow-dialog`, `--card-bg` (accepts a gradient), `--head-bg` (card header, accepts a gradient),
`--topbar-bg`, `--btn-bg`, `--btn-fg`, `--input-bg`, `--switch-knob`, `--body-bg` (accepts a gradient/image).

The Cuotai logo keeps fixed colors and is never themed. The headers show the logotype "CuotAI" (`wordmark()` in
`js/lib.js`, `.wordmark`/`.wm-c`/`.wm-ai` in `themes.css`): the four-arc mark is the C (`img/logo-on-dark.svg` in dark
mode, `img/logo-on-light.svg` in light mode, switched by `data-mode`, since an SVG inside an image cannot see the app's
mode), "uot" takes the theme's text color and "AI" the logo's pink (`#ff79c6` dark, `#990058` light). Arcs keep at least
3:1 and letters at least 4.5:1 on the built-in surfaces. A built-in theme whose top bar stays saturated blue in both
modes points `:root[data-theme="<id>"] .topbar .wm-c` at a brighter arc variant and turns `.wm-ai` white (Windows 98 uses
`img/logo-on-dark.svg`, Windows XP `img/logo-on-color.svg`). The product name in text stays "Cuotai".

Motion (optional; defaults live in `themes.css`, user themes inherit them):

| Token | Default | Use |
|---|---|---|
| `--motion-fast` | `150ms` | hover, focus, color changes |
| `--motion-base` | `250ms` | switch, entrances, dialogs |
| `--motion-slow` | `400ms` | quota bars and the tray ring |
| `--motion-spin` | `900ms` | one turn of the refresh-in-progress icon |
| `--ease-out` | `cubic-bezier(0.22, 1, 0.36, 1)` | default exit curve |
| `--ease-spring` | `linear(…)` (fallback `cubic-bezier(0.34, 1.56, 0.64, 1)`) | short spring (switch) |

Retro themes may override them (Win98 uses `steps()`; the CRT uses `0ms`). Always use the tokens, never fixed durations.
The dashboard effects (`js/motion.js`) derive from these tokens: entrance, FLIP, exit and the chip pulse use `--motion-base`;
the percentage counting, the "shake" on exhaustion and the level highlight (1.5 × `--motion-slow`) use `--motion-slow`; the button
spin and the "Waiting" shimmer (2 × `--motion-spin`) use `--motion-spin`. A token at `0ms` = the effect is off.
With `data-motion="reduced"`/`"off"` on `<html>` (Settings › General › Animations) or with
`prefers-reduced-motion: reduce` (except `data-motion="always"`), `themes.css` forces the durations to `0ms`
with `!important`, overriding the theme.

## Quality rules

* Run `python3 tools/contrast.py ui/css/themes/<id>.css` (WCAG contrast) and `python3 tools/lint_theme.py ui/css/themes/<id>.css` (scope/rules) — both must pass. They are optional development tools; the app does not depend on Python.
* `python3 tools/contrast.py --strict …` also applies the design-system rules (DS-COL-5 to 7): `--fg-muted` ≥ 4.5:1 on `--bg` and `--surface-2`, every status role ≥ 4.5:1 as text on `--surface` and `--bg`, and `--accent` ≥ 3:1 on `--bg`. New themes should pass it; older themes are not required to yet.
  Whatever the mode, the optional derived roles `--bad-fg` (≥ 4.5:1 on `--bad`), `--focus` (≥ 3:1 on `--surface` and `--bg`) and `--chart-neutral` (≥ 3:1 on `--surface`) are checked when a theme sets them as `#rrggbb`.
* `--good`, `--warn` and `--bad` must be distinguishable from each other and from `--fg`.
* No external images, remote fonts or `url(http…)`: the app works offline. Fonts: use system font
  stacks in `--font-ui`/`--font-mono`.
* Effects (glow, scanlines) must be light and must not reduce legibility.

## The Cuotai theme

`cuotai.css` is Cuotai's own theme (group "Original"), not derived from any editor or system palette.
Both modes use a gray that leans to blue, a calm lilac accent and pastel states; in light mode the states
are deepened versions of the same hues so they still read as text. The logo keeps its own colors. It passes
`contrast.py --strict` and `lint_theme.py`. Ratios are WCAG contrast against the named background.

**Dark**

| Role | Hex | Contrast |
|---|---|---|
| `--bg` | `#1b1f28` | — |
| `--surface` | `#232835` | 1.12 on `--bg` |
| `--surface-2` | `#2d3342` | — |
| `--border` | `#3c4354` | 1.49 on `--surface` |
| `--track` | `#353c4c` | 1.33 on `--surface` |
| `--fg` | `#e4e9f3` | 13.55 `--bg` · 12.09 `--surface` · 10.37 `--surface-2` |
| `--fg-muted` | `#a9b2c6` | 7.75 `--bg` · 6.92 `--surface` · 5.93 `--surface-2` |
| `--accent` / `--focus` (lilac) | `#c6bdf5` | 9.42 `--bg` · 8.41 `--surface` · 7.21 `--surface-2` |
| `--accent-fg` | `#1b1f28` | 9.42 on `--accent` |
| `--good` (aqua mint) | `#76d3c6` | 9.35 `--bg` · 8.35 `--surface` · 7.15 `--surface-2` |
| `--warn` (butter) | `#ecd28a` | 11.12 `--bg` · 9.92 `--surface` · 8.51 `--surface-2` |
| `--bad` (dusty rose) | `#e68f98` | 6.87 `--bg` · 6.13 `--surface` · 5.26 `--surface-2` |
| `--bad-fg` | `#1b1f28` | 6.87 on `--bad` |
| `--info` (periwinkle) | `#8fb2ee` | 7.67 `--bg` · 6.85 `--surface` · 5.87 `--surface-2` |
| `--chart-neutral` | `#7d879d` | 4.08 on `--surface` |
| `--scrim` | `#0b0e14` at 55 % | — |

**Light**

| Role | Hex | Contrast |
|---|---|---|
| `--bg` | `#e8ecf3` | — |
| `--surface` | `#f6f8fb` | 1.11 on `--bg` |
| `--surface-2` | `#e1e6ef` | — |
| `--border` | `#c6cedb` | 1.49 on `--surface` |
| `--track` | `#d3dae5` | 1.32 on `--surface` |
| `--fg` | `#252b37` | 11.98 `--bg` · 13.34 `--surface` · 11.33 `--surface-2` |
| `--fg-muted` | `#525c6f` | 5.68 `--bg` · 6.33 `--surface` · 5.38 `--surface-2` |
| `--accent` / `--focus` (lilac) | `#5f55b0` | 5.19 `--bg` · 5.78 `--surface` · 4.91 `--surface-2` |
| `--accent-fg` | `#ffffff` | 6.15 on `--accent` |
| `--good` (deep teal) | `#1a6b70` | 5.24 `--bg` · 5.84 `--surface` · 4.96 `--surface-2` |
| `--warn` (amber) | `#83601a` | 4.85 `--bg` · 5.40 `--surface` · 4.59 `--surface-2` |
| `--bad` (rose wine) | `#94384b` | 6.06 `--bg` · 6.74 `--surface` · 5.73 `--surface-2` |
| `--bad-fg` | `#ffffff` | 7.18 on `--bad` |
| `--info` (periwinkle) | `#35629f` | 5.21 `--bg` · 5.81 `--surface` · 4.93 `--surface-2` |
| `--chart-neutral` | `#808ba0` | 3.23 on `--surface` |
| `--scrim` | `#1b2130` at 35 % | — |

Theme character: radii 16 / 12 / 8 px, one soft shadow layer (`0 1px 6px`, opacity 0.25 dark / 0.12 light)
for `--shadow` and `--shadow-card`, the default unknown hatch in `--hatch`, and the default motion.

**Color-vision check.** `--good` is a blue-leaning green and `--bad` a warm rose of a different lightness, so the
pair separates by both hue and lightness. Simulated with the Machado, Oliveira and Fernandes (2009) matrices at
full severity and compared with CIEDE2000, `--good` and `--bad` stay ≥ 11 apart under protanopia and ≥ 16 under
deuteranopia in both modes (≥ 50 under tritanopia); `--warn` stays ≥ 13 from both. Color is never the only cue:
states also carry their label and icon.

## User themes (no recompiling)

Put `<id>.css` in `<configuration folder>/cuotai/themes/` (macOS `~/Library/Application Support/cuotai`,
Windows `%APPDATA%\cuotai`, Linux `~/.config/cuotai`, or `CUOTAI_USER_DIR`) and restart the app. Rules:

* `id` = the file name (`[a-z0-9-]`, up to 32 characters) and it cannot repeat that of a built-in theme.
* An optional header on the first line: `/*! cuotai-theme {"label":"Name","group":"custom","modes":["dark","light"]} */`.
  Without it, the name is the id, the group is "My themes" and the modes are deduced from the selectors.
* The same contract as the built-in ones, with stricter validation on load: no `@import`, `@font-face`, external URLs
  or any other at-rule, and every selector must start with `:root[data-theme="<id>"]`. Themes outside this are ignored and the
  reason appears in a warning in the main window.
* Example: `apps/desktop/examples/user/themes/ocean.css`.
