# Themes

The Cuotai desktop app ships 15 built-in themes. **Every theme offers both a dark and a light mode**, and the mode follows your system unless you pick one. Choose the theme in **Settings › General** and the mode (system, light, dark) with the switch there. The terminal dashboard has its own fixed Dracula palette.

The screenshots below are the browser demo of the interface with made-up accounts (`example.com` addresses), captured at the same window size in each theme and mode.

## Gallery

### Original

Cuotai's own theme: a gray that leans to blue, a calm lilac accent and pastel states (in light mode the states are deepened versions of the same hues so they still read as text). It is not derived from any editor or system palette, and it is the first entry in the theme picker. Its palette and contrast ratios are documented in [`apps/desktop/ui/css/themes/README.md`](../apps/desktop/ui/css/themes/README.md#the-cuotai-theme).

| Theme | Dark | Light |
|---|---|---|
| **Cuotai** (default for new installs; existing profiles keep their theme, Dracula if they never chose one; switch in Settings › General) | <img src="assets/screenshots/theme-cuotai-dark.png" alt="Cuotai dark" width="420" /> | <img src="assets/screenshots/theme-cuotai-light.png" alt="Cuotai light" width="420" /> |

### Editors

| Theme | Dark | Light |
|---|---|---|
| **Dracula** | <img src="assets/screenshots/theme-dracula-dark.png" alt="Dracula dark" width="420" /> | <img src="assets/screenshots/theme-dracula-light.png" alt="Dracula light" width="420" /> |
| **One Dark** | <img src="assets/screenshots/theme-onedark-dark.png" alt="One Dark dark" width="420" /> | <img src="assets/screenshots/theme-onedark-light.png" alt="One Dark light" width="420" /> |
| **GitHub** | <img src="assets/screenshots/theme-github-dark.png" alt="GitHub dark" width="420" /> | <img src="assets/screenshots/theme-github-light.png" alt="GitHub light" width="420" /> |
| **Catppuccin** | <img src="assets/screenshots/theme-catppuccin-dark.png" alt="Catppuccin dark" width="420" /> | <img src="assets/screenshots/theme-catppuccin-light.png" alt="Catppuccin light" width="420" /> |
| **Monokai** | <img src="assets/screenshots/theme-monokai-dark.png" alt="Monokai dark" width="420" /> | <img src="assets/screenshots/theme-monokai-light.png" alt="Monokai light" width="420" /> |
| **Tokyo Night** | <img src="assets/screenshots/theme-tokyo-dark.png" alt="Tokyo Night dark" width="420" /> | <img src="assets/screenshots/theme-tokyo-light.png" alt="Tokyo Night light" width="420" /> |

### Operating systems

| Theme | Dark | Light |
|---|---|---|
| **macOS** | <img src="assets/screenshots/theme-macos-dark.png" alt="macOS dark" width="420" /> | <img src="assets/screenshots/theme-macos-light.png" alt="macOS light" width="420" /> |
| **Windows 11** | <img src="assets/screenshots/theme-windows11-dark.png" alt="Windows 11 dark" width="420" /> | <img src="assets/screenshots/theme-windows11-light.png" alt="Windows 11 light" width="420" /> |
| **Windows 98** | <img src="assets/screenshots/theme-win98-dark.png" alt="Windows 98 dark" width="420" /> | <img src="assets/screenshots/theme-win98-light.png" alt="Windows 98 light" width="420" /> |
| **Windows XP** | <img src="assets/screenshots/theme-winxp-dark.png" alt="Windows XP dark" width="420" /> | <img src="assets/screenshots/theme-winxp-light.png" alt="Windows XP light" width="420" /> |
| **Ubuntu** | <img src="assets/screenshots/theme-ubuntu-dark.png" alt="Ubuntu dark" width="420" /> | <img src="assets/screenshots/theme-ubuntu-light.png" alt="Ubuntu light" width="420" /> |

### Retro

| Theme | Dark | Light |
|---|---|---|
| **CRT** (tube monitor) | <img src="assets/screenshots/theme-crt-dark.png" alt="CRT dark" width="420" /> | <img src="assets/screenshots/theme-crt-light.png" alt="CRT light" width="420" /> |
| **Old Terminal** (synthwave) | <img src="assets/screenshots/theme-synthwave-dark.png" alt="Old Terminal dark" width="420" /> | <img src="assets/screenshots/theme-synthwave-light.png" alt="Old Terminal light" width="420" /> |
| **TRON** | <img src="assets/screenshots/theme-tron-dark.png" alt="TRON dark" width="420" /> | <img src="assets/screenshots/theme-tron-light.png" alt="TRON light" width="420" /> |

## Add your own theme

You can add themes without recompiling Cuotai.

1. Open the folder: `<configuration folder>/cuotai/themes/`. It is `~/Library/Application Support/cuotai/themes` on macOS, `%APPDATA%\cuotai\themes` on Windows and `~/.config/cuotai/themes` on Linux, or the `themes` folder inside the folder named by the `CUOTAI_USER_DIR` environment variable. Create it if it does not exist. (**Settings › Advanced › Files** shows the folder in use, opens it and lets you choose another.)
2. Put a file there named `<id>.css`. The id (the file name without `.css`) uses only `a-z`, `0-9` and `-`, up to 32 characters, and cannot repeat a built-in theme's id.
3. Optionally start the file with a header that names the theme: `/*! cuotai-theme {"label":"My theme","group":"custom","modes":["dark","light"]} */`. Without it the name is the id and the group is "My themes". Every user theme has both modes.
4. Restart Cuotai and pick the theme in **Settings › General**. If a file is invalid, it is ignored and the main window shows a warning with the reason.

A theme defines colors (and optionally shape, fonts and effects) as CSS variables, once per mode. A minimal theme named `ocean`:

```css
/*! cuotai-theme {"label":"Ocean","group":"custom","modes":["dark","light"]} */
:root[data-theme="ocean"][data-mode="dark"] {
  color-scheme: dark;
  --bg: #0b1a24;        --surface: #11293a;   --surface-2: #19384d;
  --border: #2b5a78;    --fg: #e6f4ff;        --fg-muted: #9cc3dc;
  --accent: #ffb454;    --accent-fg: #1b1203;
  --good: #5be3a8;      --warn: #ffd166;      --bad: #ff6b6b;   --info: #6cc8ff;
  --track: #1d3f57;
}
:root[data-theme="ocean"][data-mode="light"] {
  color-scheme: light;
  --bg: #eef6fb;        --surface: #ffffff;   --surface-2: #e2eff7;
  --border: #b6d2e3;    --fg: #0d2433;        --fg-muted: #4a6b80;
  --accent: #b45309;    --accent-fg: #ffffff;
  --good: #167a4f;      --warn: #8a6100;      --bad: #b3261e;   --info: #0b6a99;
  --track: #d3e6f1;
}
```

### The theme contract

A user theme sets theme tokens only. Cuotai parses the file before the interface sees it and inserts a copy rebuilt from what it parsed, so comments and anything it did not understand never reach the window. The rules:

- **Exactly two rules:** one `:root[data-theme="<id>"][data-mode="dark"]` and one `:root[data-theme="<id>"][data-mode="light"]`, where `<id>` is the file name. No other selector, no comma lists, no nested rules. Single or double quotes are accepted around the attribute values.
- **Only theme tokens:** `color-scheme` and these custom properties:
  - colors: `--bg`, `--surface`, `--surface-2`, `--border`, `--fg`, `--fg-muted`, `--accent`, `--accent-fg`, `--good`, `--warn`, `--bad`, `--bad-fg`, `--info`, `--track`, `--focus`, `--chart-neutral`, `--scrim`, `--switch-knob`, `--btn-fg`, `--tile-fg`;
  - fonts: `--font-ui`, `--font-mono`;
  - shape: `--radius-lg`, `--radius-md`, `--radius-sm`, `--radius-pill`, `--logo-radius`, `--border-w`;
  - surfaces and effects: `--shadow`, `--shadow-card`, `--shadow-dialog`, `--card-bg`, `--head-bg`, `--topbar-bg`, `--btn-bg`, `--input-bg`, `--body-bg`, `--hatch`;
  - motion: `--motion-fast`, `--motion-base`, `--motion-slow`, `--motion-spin`, `--ease-out`, `--ease-spring`.
- **Values by kind:**
  - a color is one `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` value, a color keyword, `rgb()`, `rgba()`, `hsl()`, `hsla()`, `color-mix()` or `var(--token)`;
  - a shape token is one number with `px`, `em`, `rem` or `%` (or `var(--token)`);
  - a motion duration is one value in `ms` or `s`, or `0`;
  - an easing is a keyword, `cubic-bezier()`, `steps()`, `linear()` or `var(--token)`;
  - a font is a comma-separated list of quoted family names and keywords (system fonts only, nothing is downloaded);
  - `color-scheme` is `dark`, `light`, `normal` or `only`;
  - a surface or effect accepts lists of numbers, colors and keywords plus `linear-gradient()`, `repeating-linear-gradient()`, `radial-gradient()` and `repeating-radial-gradient()`.
- `var()` names one of the tokens above, with no fallback. Functions nest at most three deep.
- **Refused:** `!important`, every at-rule (`@import`, `@font-face`, `@media`…), `url(…)` and every other function (`image-set`, `cross-fade`, `element`, `paint`, `image`, `calc`…), backslash escapes and control characters.
- Plain comments are allowed and dropped; the `cuotai-theme` header may appear once.

When a file breaks a rule it is skipped as a whole and the main window and **Settings › Advanced › Files** name it with one of these reasons:

| Reason shown | What to fix |
|---|---|
| invalid file name | Rename the file to `[a-z0-9-]`, up to 32 characters. |
| invalid CSS syntax or an escape | Unbalanced braces, quotes or parentheses, a missing `:`, a backslash or a control character. |
| uses @import, fonts or external URLs | Remove `@import`, `@font-face` and every `url(…)`; use system fonts. |
| uses disallowed @-rules | Remove `@media`, `@supports` and every other at-rule. |
| uses a CSS function themes may not use | Replace the function with a color, a gradient or a number. |
| selector outside the theme scope | Keep only the two `:root[data-theme="<id>"][data-mode="…"]` rules; component selectors (`.card`, `body::after`) are for built-in themes. |
| sets a property that is not a theme token | Remove the property or rename it to a token above. |
| a value outside the theme grammar | Check the value kind for that token; remove `!important`. |
| needs exactly one dark and one light rule | Add the missing mode or merge the duplicate rule. |
| missing or invalid metadata | Make the header a JSON object with string `label` and `group`, and write it once. |

Themes written for earlier versions that relied on component selectors or properties outside this list stop loading with one of these reasons; move their colors into the tokens.

The full token list (colors, shape, surfaces, effects, motion tokens), the quality rules and the contrast and lint tools used for the built-in themes are in [`apps/desktop/ui/css/themes/README.md`](../apps/desktop/ui/css/themes/README.md). A complete example is [`apps/desktop/examples/user/themes/ocean.css`](../apps/desktop/examples/user/themes/ocean.css).

## Add your own language

Languages work the same way, as one JSON file in the `locales` folder next to `themes`; see [`apps/desktop/ui/locales/README.md`](../apps/desktop/ui/locales/README.md).
