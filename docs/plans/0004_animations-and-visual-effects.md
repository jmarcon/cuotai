# PRD 0004 — Animations and visual effects

| Field | Value |
|---|---|
| ID | 0004 |
| Status | Implemented (0.17.0 to 0.20.0) |
| Owner | Desktop (primary) and TUI |
| Consumers | `apps/desktop/ui`, `crates/cuotai-tui` |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-19)

Each RF ships in its own version, with `cargo fmt`, `clippy -D warnings`, the workspace and desktop tests, and an interface smoke test before the commit.

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.17.0 (`6725548`) | `--motion-*`/`--ease-*` tokens in the theme contract (Win98 in `steps()`, CRT without interpolation), `prefers-reduced-motion` and the **Appearance → Animations** option in `desktop.json`, applied as `data-motion` before the JS runs; the dashboard reconciles cards by `data-card` (`patch()` in `util.js`), preserving focus, scroll and the bars. |
| RF2 | 0.18.0 | `js/motion.js`: percentage counting, a single highlight on a level change, a shake only on exhaustion, staggered entrance, FLIP on reordering, shrinking exit, skeleton with shimmer (paused while the window is hidden), spinner only on the account being refreshed, elevation on hover/focus, chip pulse when it reaches zero and a single refresh spin. An idle dashboard runs no animations. |
| RF3 | 0.19.0 | `showDialog()` in `motion.js` for every dialog: fade + scale 0.96 → 1, blurred backdrop, closing animated after the native `close()` (focus returns to the opener at once, including in WKWebView) and details growing from the card. The tray fades in with 6 px of slide and the ring animates from the previous value. Theme and language switches use `startViewTransition` (250 ms) when supported and motion is on. Smoke test in the Tauri app on macOS for dialogs, focus and theme; the tray entrance was proven only in the browser (the automation cannot open the tray window). |
| RF4 | 0.20.0 | `crates/cuotai-tui/src/motion.rs`: with an animation active the loop draws at ~30 fps (33 ms poll), and when idle it draws only when `dirty`. The gauge and the detail bars interpolate over 300 ms with 1/8-cell resolution, a level gradient with truecolor, the class label blinks twice on a change, a braille spinner in the title of the account being refreshed and a 120 ms wipe on details, About and Models. `--no-animations`, `CUOTAI_NO_ANIMATIONS`, off on `TERM=dumb` and by default over SSH (`CUOTAI_ANIMATIONS=1` re-enables). With animations off the output is identical to the previous one; when idle, the TUI does not redraw beyond the existing 1 s tick. |

## 1. Context

The desktop had almost no motion: a `@keyframes spin` (`themes.css`, `detail.css`), `transition: width 0.4s` on the bars (`themes.css`), the tray ring (`tray.css`: `stroke-dasharray`) and two 0.2 s transitions in `main.css`. No CSS handled `prefers-reduced-motion`. In addition, `main.js` redrew the dashboard with `root.innerHTML = …`, which recreated the elements on every update. In practice, the bar transition almost never ran: the new value appeared at once. The 14 themes follow the token contract in `css/themes/README.md`.

The TUI redraws only when `dirty`, with a 100 ms `event::poll` (`ui.rs`). The cards use Ratatui's `Gauge` (`dashboard.rs`), and the detail view uses `█`/`░` text bars. There was no temporal effect.

## 2. Problem

Quota updates, state changes (available → exhausted), dialogs opening and card reordering happen with no visual continuity: it is hard to see what changed after a refresh. On the other hand, too much animation:
- costs CPU and battery in an app that stays open in the tray;
- bothers people who are sensitive to motion;
- in a terminal, it can cause flicker and traffic over SSH/tmux.

## 3. Goals

- Make **what changed** (value, state, order) noticeable with short motion, 150 to 400 ms.
- Give the desktop a modern, cohesive feel (entrances, dialogs, hover, loading) without breaking any of the 14 themes.
- In the TUI, have discreet, optional effects that do not raise the cost while the terminal is idle.
- Respect `prefers-reduced-motion` and offer a dedicated switch in both front ends.

## 4. Non-goals

- Animation libraries (GSAP, Framer, Lottie) or UI frameworks. Only CSS, the Web Animations API and what Ratatui already offers.
- Continuous, decorative effects while the dashboard is idle (particles, animated backgrounds, glow pulsing for no reason).
- Changing data, core contracts or the `--once`/`--json` output.

## 5. Functional requirements

### RF1 — Motion foundation on the desktop (high priority; prerequisite)

- **RF1.1** Motion tokens in the theme contract (`themes.css` and `css/themes/README.md`):
  - durations `--motion-fast` (150 ms), `--motion-base` (250 ms) and `--motion-slow` (400 ms);
  - curves `--ease-out` and `--ease-spring`, the latter through `linear()` with a fallback.

  Retro themes may override them (Win98 uses `steps()`, the CRT may use 0 ms). User themes inherit the defaults.
- **RF1.2** Reduced motion:
  - `@media (prefers-reduced-motion: reduce)` zeroes durations and removes displacements, keeping only opacity changes of up to 150 ms;
  - the option **Settings → Appearance → Animations: System / Always / Reduced / Off** lives in `desktop.json` and is applied as `data-motion` on `<html>`, without waiting for the main JS (as is done today in `index.html` for the theme).
- **RF1.3** Incremental dashboard update:
  - replace the full `innerHTML` with per-card reconciliation, keyed by `data-card` (already present): create, remove and update only what changed;
  - preserve focus, scroll position and the bar elements, so the transitions really run;
  - no new framework: a small function that compares by key and replaces the inner nodes that changed.
- **Acceptance criterion:**
  - a refresh with a changed value animates the bar width (checked with `getAnimations()` or an intermediate capture);
  - focus on a card's button survives the refresh;
  - with `prefers-reduced-motion` emulated and with "Off", no transition lasts longer than 150 ms;
  - the 14 themes still pass the visual smoke test.

### RF2 — Desktop dashboard effects (high priority)

- **RF2.1** Data:
  - bars and percentages interpolate between the old and the new value (numeric counting with `requestAnimationFrame`, only during the transition);
  - a level change (`good` → `warn` → `bad`) makes the color transition and gives the card a single 600 ms highlight;
  - exhausting the quota applies a short, 1-cycle "shake" only when the state changes, never on every refresh.
- **RF2.2** Entrance and order:
  - cards enter with a fade + 8 px shift, staggered by 30 ms (capped at 300 ms in total);
  - reordering (drag, arrows or sorting) uses FLIP (measure → invert → animate `transform`), so the cards slide to the new position;
  - removing the account shrinks and fades it before it leaves the DOM.
- **RF2.3** Loading: a skeleton with a "shimmer" on the cards in "Waiting", and the current spinner only on the button of the account being refreshed. The shimmer stops when the data arrives and does not run while the window is hidden (`document.hidden`).
- **RF2.4** Micro-interactions:
  - hover and focus with a subtle elevation (shadow/1.01 scale);
  - the switch slides with a spring;
  - the next-query chip pulses once when it reaches zero;
  - the refresh icon spins once on click.

  The visible focus (`:focus-visible`) is never removed nor animated in a way that makes it disappear.
- **Acceptance criterion:** a short recording or a sequence of captures of each effect in at least Dracula, GitHub light, Win98 and CRT; an idle dashboard with 15 cards has no active animation (`document.getAnimations().length === 0`); no noticeable frame drop in the WebKit profile when refreshing 15 cards.

### RF3 — Dialogs, tray and screen transitions (medium priority)

- **RF3.1** Dialogs (`<dialog>`: account, details, about, settings):
  - open with a fade + 0.96 → 1 scale and a lightly blurred backdrop where supported;
  - close with the reverse animation before `close()`, without delaying the return of focus;
  - the details popover grows from the originating card.
- **RF3.2** The tray window (`tray.html`) appears with a fade + 6 px slide from the icon. The usage ring animates from the previous value, not from zero, on every opening.
- **RF3.3** Use the View Transitions API, where the system WebKit supports it, to switch theme and language (250 ms cross-fade); without support, switch at once.
- **Acceptance criterion:** a smoke test in the Tauri app (not only in the browser) on macOS: opening and closing each dialog from the keyboard (`Esc`, `Tab`) keeps focus and screen-reader reading; switching theme without a white flash.

### RF4 — TUI effects (medium priority)

- **RF4.1** A minimal animation engine in `ui.rs`:
  - with an active animation, the loop draws at about 30 fps (33 ms `poll`);
  - without animation, it returns to the current behavior (draw only when `dirty`);
  - the animation state (source value, target value and start) is kept per card, with no new threads.
- **RF4.2** Effects:
  - the `Gauge` and the detail bars interpolate from the old to the new value in about 300 ms after the refresh;
  - a state change blinks the class label (Available/Exhausted) twice;
  - a braille spinner (`⠋⠙⠹…`) in the title of the card being refreshed, in place of the static text;
  - an opening and closing transition for details, About and Models with a 120 ms vertical "wipe";
  - bars with partial blocks (`▏▎▍▌▋▊▉█`) for 1/8-cell resolution, and a color gradient per level on truecolor terminals (falling back to 16 colors when `COLORTERM` does not indicate truecolor).
- **RF4.3** Control: `--no-animations` and the `CUOTAI_NO_ANIMATIONS` variable. Animations are also turned off automatically on `TERM=dumb` and, by default, over SSH (`SSH_CONNECTION`), where each frame costs bandwidth. `--once`/`--json` do not change.
- **Acceptance criterion:**
  - a tmux smoke test: a capture during the animation shows an intermediate value, and then the final one;
  - with animations off, the output is identical to today's;
  - when idle, the TUI does not redraw (measured by counting calls to `draw`);
  - no visible flicker in Terminal.app, iTerm2 and tmux.

## 6. Non-functional requirements

- **Accessibility:**
  - no effect carries information on its own: color, text and `aria-live` remain the source;
  - nothing flashes more than 3 times per second (WCAG 2.3.1);
  - reduced motion is respected by default.

  Full validation requires manual testing with assistive technologies.
- **Performance:**
  - animate only `transform`, `opacity` and color; width only on the bars (up to 15 elements);
  - no continuous animation while the window is hidden or the dashboard is idle;
  - an idle TUI keeps today's cost.
- **Themes:** every effect uses tokens. The system themes (macOS, Windows 11, Ubuntu) follow the platform's native duration and curve, and the retro ones may turn interpolation off.

## 7. Order of execution and verification

1. **RF1:** tokens, reduced motion, the Appearance option and incremental reconciliation (a prerequisite for any transition to work).
2. **RF2:** dashboard effects, one per version, with captures in four themes.
3. **RF3:** dialogs, tray and View Transitions, with a smoke test in the Tauri app.
4. **RF4:** the TUI engine and effects, with a tmux smoke test and a measurement of idle redraws.

At each step: `cargo fmt`, `clippy -D warnings`, the workspace and desktop tests, `node --check`, and a visual smoke test on the desktop and the TUI before the version.

The requirement is only delivered when a refresh that changes values shows the transition in both front ends, reordering slides the cards, dialogs open and close with motion without losing focus, and with reduced or disabled motion everything still works instantly and with no idle animation.
