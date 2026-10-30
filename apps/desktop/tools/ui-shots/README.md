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
| `node apps/desktop/tools/ui-shots/check.mjs [--json] [locale …]` | Runs the overflow check on the tray (380 × 600, List, Rings and Compact), the main window (520 and 1200 wide), the account details dialog (which must scroll in its body only) and three Settings pages (640 × 460) in en-US, pt-BR, es, it and the `en-XA` pseudo-locale; exits 1 on any problem outside the allowed list |
| `node apps/desktop/tools/ui-shots/locale-smoke.mjs [locale …]` | PRD 0034 RF7: renders the tray, the main window, the Settings pages and the dialogs in en-US, es and it with the core catalogs; exits 1 on any Portuguese word, or on a catalog key shown instead of its text |
| `node apps/desktop/tools/ui-shots/check.mjs --zoom [locale …]` | The 200 % zoom pass (PRD 0020 RNF): the same overflow check on the main window at 640 × 410 and four Settings pages at 640 × 360, the CSS viewports a 1280-wide screen leaves at 200 % |
| `node apps/desktop/tools/ui-shots/a11y.mjs [--json] [--all]` | axe-core (pinned 4.10.2 from cdnjs, SHA-256 verified before it runs) over the tray, main window and four Settings pages, light and dark in en-US and dark in every other shipped language; fails on serious/critical violations (`--all`: every impact; `color-contrast` stays advisory, `contrast_gate.py` owns contrast); exit 3 when axe could not be fetched |
| `node apps/desktop/tools/ui-shots/perf.mjs [--strict] [--runs N]` | Median of several runs: Settings open time (budget 300 ms, PRD 0020 RNF) and interaction work time in the tray (click, render and layout; budget 16 ms; the next-frame time is informational); advisory unless `--strict` |
| `node apps/desktop/tools/ui-shots/keyboard.mjs` | PRD 0032: drives Settings › Alerts › Quiet hours with the keyboard only (Tab, Shift+Tab, Enter, Space, typed digits): open the sub-page, switch on, add, type a time, remove, repeat on every day, reach the three-interval limit; exits 1 on the first step that needs a mouse |
| `node apps/desktop/tools/ui-shots/check.mjs --selftest` | Forces a long string into a tray figure and passes only if the check catches it |

`screens.tsv` columns: name, page (with its query), width, height, theme, mode, language, clicks
(`|`-separated selectors) and storage presets (`key=value;…`, for example `cuotai.trayView=rings`) and an optional `selector:px` to scroll an element before the shot (the scrolled mockups).

Demo switches (URL query, no app file edited, RF18.4): `?fixture=six` swaps the demo accounts for the mockups' set
(two Claude accounts, Codex, Cursor on extra usage, Kimi, a stale Groq and an Anthropic API account needing sign-in) so
the `*.fixture` captures compare with `assets/0022/after/` (01, 03, 10, 11); `&bedrock` adds an AWS Bedrock account
(the "Billed per query" chip); `?mono=XY` gives the first account a monogram override (DS-CMP-MONO-4); `?aggwarn` gives
the 9router instance both standing warnings (PRD 0036 RF2.3/RF2.4).

[`components.html`](components.html) is the PRD 0022 RF10.5 component fixture page: meters, state rings, the extra-usage
chip and sub-row, the forecast callout, rows with monograms, the filter tabs with an aggregator tab and the chips, drawn
by the production modules from the demo's fixtures (a surface without data is left out). `screens.tsv` captures it as
`90-components*` in the four reference themes (Dracula, GitHub light, Win98, CRT).

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
labels and names, the status label after its fitting, account names in the Stats rows, one-line notices and access lines, the
Settings sidebar labels in its icon-rail width) is listed as allowed in `ALLOWED`.

CI (`ui-checks` job, macOS): the overflow check, the 200 % zoom pass, the selftest and the axe pass block (an axe-core
download failure, exit 3, is only a warning), and so do the timing probes (`--strict`): they time the main-thread work
of an interaction (click, render, forced layout), a few milliseconds against the 16 ms budget, and print the time to
the next presented frame (which includes the vsync wait) only for information. The tray, the main window and the Settings pages report no serious or critical violation.
