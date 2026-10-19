# PRD 0007 — Consumption pace until the reset

| Field | Value |
|---|---|
| ID | 0007 |
| Status | Proposed |
| Owner | `cuotai-core`, desktop and TUI |
| Consumers | `apps/desktop` (cards, details, tray), `crates/cuotai-tui`, PRDs 0008, 0012 and 0014 |
| Depends on | none; PRD 0006 improves the projection but is not required |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## 1. Context

Each `Metric` carries `used`, `limit`, `remaining`, `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other`) and `resets_at` (`crates/cuotai-core/src/model.rs`). `metrics::classify_metric` produces a `MetricState` (`Available { ratio, uncapped }`, `Exhausted`, `Unmeasured`) that only says how much is left, not whether the current pace will last until the reset. The cards show "in 4h 10m" for the reset and the consumption bar, but nothing compares the two.

CodexBar publishes the pace concept ("6% in reserve · Expected 47% used · Lasts until reset"), OpenUsage and OpenQuota show pace indicators, and QuotaTempo makes pace the whole product (weekly balance W, target P and difference ±).

## 2. Problem

Knowing that 40% is left does not say whether that is slack or danger: it depends on how much of the window has already passed. People using Claude (5 h and week), Codex, Copilot or Kimi need to decide whether to start a long task now. Without pace, the user does the math in their head on every query.

## 3. Goals

- Compute in the core, per eligible metric, the expected consumption at a uniform pace, the difference from the real consumption and whether the current pace exhausts the window before the reset.
- Show it briefly on the cards, in the details, in the tray and on the CLI.
- Never invent a pace when data is missing: omitting is better than estimating badly.

## 4. Non-goals

- Predicting future consumption by learning, seasonality or working hours.
- Alerting (PRD 0008) or recommending a switch of provider.
- Balance metrics, accumulated spend or short rate limits, which have no uniform consumption window.
- Changing `Report`, `Snapshot` or the current `--json` output; pace is derived on read.

## 5. Functional requirements

### RF1 — Pure computation in the core (high priority; prerequisite)

- **RF1.1** Create `pace::assess(metric: &Metric, now: DateTime<Utc>, recent: Option<&[Point]>) -> Option<Pace>`, a pure function with no network and no internal clock. `Pace` is `Serialize` and carries: `elapsed_fraction`, `expected_used_fraction`, `used_fraction`, `delta_points` (real minus expected, in percentage points), `stage`, `projected_exhaustion_at: Option<DateTime<Utc>>` and `lasts_until_reset: bool`.
- **RF1.2** Eligibility: `kind = Quota`, `window` in `window_5h`, `daily`, `weekly` or `monthly`, a finite and positive `limit`, a finite `used`, a future `resets_at`. `RateLimit`, `Balance`, `Credits`, `Spend`, `Usage` and `other` are left out. A missing piece of data returns `None`, never a default value.
- **RF1.3** Window length: `window_5h` = 5 h, `daily` = 24 h, `weekly` = 7 days. `monthly` = the calendar-month interval ending at `resets_at` (28 to 31 days). The start is `resets_at − length`. If the provider returns a known start in `period`, it takes precedence when coherent.
- **RF1.4** Expected consumption = `elapsed / length`. Stages, with a 5-percentage-point hysteresis:
  - `Reserve` (`delta_points ≤ −5`): below the pace;
  - `OnPace` (between −5 and +5);
  - `Ahead` (`delta_points ≥ +5`): above the pace;
  - `Exhausted` (`used ≥ limit`).
- **RF1.5** Projection: `projected_exhaustion_at = start + elapsed × limit / used` when `used > 0`. If `used` is zero, or the projection falls after `resets_at`, `lasts_until_reset = true` and there is no exhaustion date. With `recent` (PRD 0006), use the slope of the last 2 hours (or 10% of the window, whichever is greater) when there are at least 3 samples; otherwise, the average since the start of the window. The source used is recorded in `Pace`.
- **RF1.6** Freshly started window: if less than 5% of the length or less than 10 minutes has passed, there is no assessment (`None`), to avoid absurd projections from a single prompt.
- **Acceptance criterion:** table tests with a fixed clock cover 5 h, daily, weekly and monthly windows (February and a 31-day month), `used = 0`, `used ≥ limit`, a zero or missing `limit`, a past `resets_at`, non-finite values, a freshly started window, a projection before and after the reset, hysteresis at the thresholds, and recent slope versus average.

### RF2 — Localized labels and display contract (high priority)

- **RF2.1** The core emits `Localized` with stable ids (`pace.reserve`, `pace.on_pace`, `pace.ahead`, `pace.exhausted`, `pace.lasts_until_reset`, `pace.runs_out_at`) and parameters (`delta`, `at`, `margin`). Example texts: "On pace · lasts until the reset", "8 points in reserve", "Runs out at 16:40, 2 h before the reset". Complete `en-US` and `es` catalogs; Portuguese in the `fallback`.
- **RF2.2** The stage has a semantic tone (`good`, `neutral`, `warn`, `bad`) in `Pace`, so the interface does not pick a color from text. Tone and text never depend on color alone.
- **Acceptance criterion:** a catalog test and a test that `Pace` serializes stably.

### RF3 — Desktop (medium priority)

- **RF3.1** A Tauri command that returns the pace of each eligible metric of an account (or of all), computed by the core with the backend's clock.
- **RF3.2** Card: a short line under the bar of the most restrictive eligible metric (see PRD 0009). Details: the pace of each metric, with the expected-consumption mark drawn over the bar and the projected exhaustion time.
- **RF3.3** Tray: the tooltip and the list view include "lasts until the reset" or "runs out at HH:MM".
- **RF3.4** In **Settings → Appearance**, an option to show or hide pace (on by default). Preference in `desktop.json`.
- **Acceptance criterion:** a smoke test in three languages and in the light and dark themes, with one metric at each stage; the expected mark does not overlap the text at narrow widths.

### RF4 — TUI and CLI (medium priority)

- **RF4.1** TUI account details: a pace column or line per metric; on the dashboard, a discreet stage icon. Respects small widths.
- **RF4.2** `--once` prints the pace line under each eligible metric. `--json` stays the same; pace enters the versioned export contract of PRD 0012.
- **Acceptance criterion:** `TestBackend` rendering tests and a text snapshot of `--once`.

## 6. Non-functional requirements

- **Honesty:** pace is a linear estimate. The text and the documentation say so; they do not promise exact exhaustion. Rolling rate-limit windows and metrics with an unknown start are left out.
- **Determinism:** `assess` depends only on its arguments.
- **Cost:** no new provider query.

## 7. Order of execution and verification

1. **RF1 and RF2:** computation and texts in the core, with table tests.
2. **RF3:** desktop with a smoke test.
3. **RF4:** TUI and CLI.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when a weekly metric consumed above the pace shows the difference and the exhaustion time in the three languages, a freshly opened window shows nothing, metrics with no window or no `resets_at` show no invented pace, and no extra query is made.
