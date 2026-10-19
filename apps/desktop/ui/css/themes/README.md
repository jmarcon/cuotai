# Cuotai themes

A theme = a `css/themes/<id>.css` file + an entry in `js/themes.js` + an `@import` in `css/themes/index.css`.

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
With `data-motion="reduced"`/`"off"` on `<html>` (Settings → Appearance → Animations) or with
`prefers-reduced-motion: reduce` (except `data-motion="always"`), `themes.css` forces the durations to `0ms`
with `!important`, overriding the theme.

## Quality rules

* Run `python3 tools/contrast.py ui/css/themes/<id>.css` (WCAG contrast) and `python3 tools/lint_theme.py ui/css/themes/<id>.css` (scope/rules) — both must pass. They are optional development tools; the app does not depend on Python.
* `--good`, `--warn` and `--bad` must be distinguishable from each other and from `--fg`.
* No external images, remote fonts or `url(http…)`: the app works offline. Fonts: use system font
  stacks in `--font-ui`/`--font-mono`.
* Effects (glow, scanlines) must be light and must not reduce legibility.

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
