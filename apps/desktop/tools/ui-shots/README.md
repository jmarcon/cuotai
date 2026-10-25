# UI screenshots and the overflow check (PRD 0022)

Dev-only tooling for the desktop UI's **browser demo**: it captures the real refreshed screens
(PRD 0022 RF18.2, RF18.3) and checks them for clipped, truncated or overlapping text in every
language (RF14.2). Nothing here is part of the app or a release. Nothing talks to a provider or
reads a credential: the demo answers every command with its own fixtures (`demo` in
[`ui/js/lib.js`](../../ui/js/lib.js), `example.com` identities), and the server binds to
`127.0.0.1` only.

Needs Node 22+, Python 3 and Google Chrome (or Chromium; set `CHROME` to its path). Run from the
repository root.

| Command | What it does |
|---|---|
| `sh apps/desktop/tools/ui-shots/capture.sh [--out DIR] [name-prefix …]` | Renders every screen of [`screens.tsv`](screens.tsv) at 2x into `docs/plans/assets/0022/delivered/` (same names as the mockups in `assets/0022/after/`) |
| `node apps/desktop/tools/ui-shots/check.mjs [--json] [locale …]` | Runs the overflow check on the tray (380 × 600, List, Rings and Compact), the main window (520 and 1200 wide) and three Settings pages (640 × 460) in en-US, pt-BR, es, it and the `en-XA` pseudo-locale; exits 1 on any problem outside the allowed list |
| `node apps/desktop/tools/ui-shots/check.mjs --selftest` | Forces a long string into a tray figure and passes only if the check catches it |

`screens.tsv` columns: name, page (with its query), width, height, theme, mode, language, clicks
(`|`-separated selectors) and storage presets (`key=value;…`, for example `cuotai.trayView=rings`).

How it works: [`driver.mjs`](driver.mjs) serves the repository root with `python3 -m http.server`,
starts headless Chrome with a throwaway profile and drives it over the DevTools protocol (window
size, scale, color scheme, reduced motion). The demo's language and theme are preset in
`localStorage` on the same origin before the page loads, so no app file is edited. Served from the
repository root, the demo also reads the core's catalogs (`crates/cuotai-core/locales/`), so its
texts match the app.

The pseudo-locale (`?pseudo` in the demo's address, DS-MSG-16) is generated at load from en-US:
every string grows (+100 % up to 10 characters, +50 % up to 20, +40 % beyond), takes accented
letters and is wrapped in ⟦ ⟧, so hard-coded text, concatenation and clipping show. The desktop
app never registers it.

The checker reports, per page and language: (a) text wider than its box with an ellipsis or hidden
overflow, (b) text or controls outside the window, (c) text clipped by an ancestor, (d) text
spilling out of a chip, button, tab, option card or figure, (e) overlapping text, (f) a status line
taller than one line, and a horizontal scroll. What may truncate by design (DS-MSG-15: account
labels and names, the status label after its fitting, one-line notices and access lines, the
Settings sidebar labels in its icon-rail width) is listed as allowed in `ALLOWED`.
