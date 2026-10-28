# PRD 0024 — Pace-based predictive alerts and action suggestions

| Field | Value |
|---|---|
| ID | 0024 |
| Status | Awaiting owner verification (RF1–RF8 delivered in 0.81.0; review fix FIX-11 delivered in 0.80.0) |
| Owner | `cuotai-core` (pace texts, alert evaluation, events, best-account ranking), desktop and TUI |
| Consumers | `apps/desktop` (cards, details, tray, Settings → Alerts), `crates/cuotai-tui` (cards, `--once`, `--notify`), `--watch`/hooks consumers, `export::v1` and `/v1/limits` consumers, PRDs 0025, 0026 and 0031 |
| Depends on | PRD 0007 (RF1, RF2: `pace::assess`, `projected_exhaustion_at`), PRD 0008 (RF1, RF2: `alerts::evaluate`, `AlertState`, delivery), PRD 0006 (RF2, RF3: recent samples for `source = recent`); PRD 0029 (RF1: account `id`) for RF7, whose `[[groups]]` and suggestions name accounts |
| Benefits from | PRD 0010 (freshness), PRD 0013 (RF1: event engine), PRD 0022 (UI refresh: chips, callout and row visuals), PRD 0023 (RF1: overage excluded from pace; MET-01 `Metric::scope`), PRD 0025 (RF6: first-run consent), PRD 0031 (`/v1/limits` summary), PRD 0032 (quiet hours) |
| Extends | PRD 0007 RF2 (compact chip texts in points or percent), PRD 0008 RF1 (new alert kind), PRD 0012 RF1 (additive `group`, `best` and `pace.delta_ratio`), PRD 0013 RF1.2 (new event) |
| Extended by | PRD 0051 (`group` and `[[groups]]` survivable by older readers before they understand them) |
| Date | 2026-10-22; amended 2026-10-23 (owner decisions B, C, D, M; review items DEC-01, DEC-02, FIX-11, FIX-12; design system DS-CMP-TIME-1..5) |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. "Lead time" is how long before the projected exhaustion the warning is sent. "Margin" is the time between the projected exhaustion and the reset. A **comparable group** is a set of accounts the user says can replace one another (RF7).

## Implementation status (2026-10-24)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.81.0 | Delivered. Core: `Pace::chip(PaceUnit)` and `Pace::outlook_chip`, `Pace::delta_ratio` (also in `export::v1`), `resets::returns_chip`, `resets::pair_label`, `text::countdown`/`text::duration` (abbreviated units, so no plural forms) in the four catalogs, with the 18/22-character budget test. Surfaces: `--pace-unit points\|percent` and `CUOTAI_PACE_UNIT` (the flag wins; an unknown variable reads as points) for the TUI and `--once`, and `pace.deviation_unit` in `desktop.json` with the Settings › Dashboard "Show pace as" segmented control (the view ships both chips and the interface only picks one). The desktop `fmtCountdown` is gone: reset times read the core's `resets.pair` ("Resets 19:30 · in 1h 35m"). The unknown return uses the core key `resets.chip.return_unknown`. |
| RF2 | 0.81.0 | Delivered in the core: `AlertKind::ProjectedExhaustion`, `alerts::evaluate_with` with `PaceInput`, `alerts::forecast` (conditions 1–3, reused for the RF4.3 callout), `alerts::cancels` (RF2.4), `pace::alert_grade` (FIX-12 span rule), `Lead`, `AlertPrefs::projected` (off by default) and `AlertPrefs::recommended()`. A queued projection cancelled before delivery keeps its mark, so it does not fire again in that window instance. |
| RF3 | 0.81.0 | Delivered. Marks persisted in `notified.json`, `alerts.projected.*` notice texts, TUI `--notify-projected [15\|30\|60]`. Desktop (RF3.2): `Notices::assess`/`record` take the account's `PaceInput` (its effective interval and the recorded points ending at the reading), a queued projection that a later reading voids is dropped with `alerts::cancels`, and the warning has its own switch (`alerts.projected_lead_minutes` in `desktop.json`, absent = off) that works with the usage alerts off. Quiet hours and digests follow PRD 0032: a projection held by quiet hours is a deferred mark in `notified.json` (with what it projected), re-judged when delivered, so it survives a restart and reaches the digest. |
| RF4 | 0.81.0 | Delivered. Settings › Alerts has the "Warn before running out" switch (off by default, the permission flow of the other notices, the honesty line "Linear estimate from recent readings") with the 15 min / 30 min / 1 h lead as a dependent control, built from `row()` components that now accept search keywords. While the core's forecast holds, the details show one callout and the card an outlook chip, both from core texts (`alerts::forecast_text` reuses the notification body). The account bell and the per-metric mute silence the notice (RF4.2). The first-run "Turn on" of PRD 0025 RF6 enables it with the recommended 60-minute lead (`AlertPrefs::recommended`), beside the user's own switch. |
| RF5 | 0.81.0 | Delivered: `quota_exhaustion_projected` (`events::diff_with`, `Watcher::observe_with`), `cuotai --watch --projected-lead 15\|30\|60`, hooks accept it, and the desktop hooks runner observes with the same pace input, so hook rules fire on it in the app. |
| RF6 | 0.81.0 | Delivered: desktop rows, details and tray text use the core chips and the reset pair; TUI cards show the core's chip beside the stage icon from 60 columns of card text, `--once` keeps its `Ritmo:` line (in the chosen unit); `export::v1` carries `delta_ratio`; a reading that is not `Fresh` shows no pace or outlook chip (FIX-11). |
| RF7 | 0.81.0 | Delivered. Core: `Account::group` (slug, at most 32 characters, validated by `Config::validate`), optional `[[groups]]` (`id`, `label`, `order`) kept with their comments by `ConfigFile`, `AccountDraft::group_text` (the TUI wizard has a Group step and the desktop dialog a Group field; an edit that does not mention it keeps it), `advice::best` (score = least effective remaining share over account-wide plan limits, 30-minute renewal credit, Fresh before Aging, stale/failed/unmeasured excluded, model-scoped and overage ignored, at least two candidates), `AccountExport.group` and `best` in `export::v1` and `/v1/limits`. Surfaces: "Best now" chip on desktop cards, in the tray popover and in the tray tooltip, the "runs out · has N% left" advice with a single "Show {name}" action, `advice.enabled` in `desktop.json` (Settings › Dashboard "Show the best account"), and the TUI chip on the card border and a `GRUPO` line in the details. RF7.1's optional member-id form is delivered (0.81.0): a `[[groups]]` table may list its members by PRD 0029 account id (`members = [...]`); the account's own `group` wins, an id listed by two groups or by a group other than its own is rejected, and ids of no configured account are ignored (`Config::group_of`). |
| RF8 | 0.81.0 | Delivered: `cuotai.example.toml` (`group`, `[[groups]]`), `README.md`, `docs/tui.md`, `docs/gui-features.md`, `docs/advanced.md`, `docs/export-v1.md` and its schema/example, and `docs/events.md` (RF5, already there). |
| FIX-11 | 0.80.0 | Delivered: `pace::assess_reading` is the one rule every surface uses (TUI cards and details, `--once`, `export::v1` and so `widget.json` and the local server, desktop cards and details): the pace is evaluated at the reading's `checked_at`, never at the current clock, and is absent unless the reading is `Fresh` for the account's effective refresh interval, or once its window reset after the reading. In `export::v1` a non-fresh reading now reports `null` pace entries (same shape). `docs/tui.md` states the recent-slope source and the freshness rule (review item DOC-03). The RF2.2 alert criterion reuses the same function when RF2 lands. |

## 1. Context

PRD 0007 already answers "when will I run out" in plain words: `pace::assess` (`crates/cuotai-core/src/pace.rs`) returns `projected_exhaustion_at`, `lasts_until_reset`, `source` (`average` or `recent`, the least-squares slope of at least 3 samples from PRD 0006) and the `pace.runs_out_at` outlook ("Runs out at 16:40, 2 h before the reset"). What is missing:

- **Warnings from the pace.** `alerts::evaluate` (PRD 0008) fires only on fixed thresholds (`AlertKind::{Threshold, Exhausted, Recovered, SessionExpiring}`). PRD 0008 §4 lists "a pace-based notice (PRD 0007), which may become a future option" as a non-goal. A person at 70 % with a steep slope gets no warning until 75 % or 90 %, which may be minutes before the stop or hours early.
- **Short forms.** The core texts are full sentences. Rows, the tray and the menu-bar title (PRD 0025) need compact forms; `docs/design-system.md` DS-CMP-TIME-1..5 fixes them as **time chips** with Cuotai's own wording from core keys. Today the desktop builds the countdown itself (`fmtCountdown`) and the TUI has its own, so the two can disagree.
- **Stale inputs.** The desktop backend (`backend.rs`, about line 618) combines an old reading's `used` with `Utc::now()`, so a preserved stale reading gets a fresh-looking pace (review item FIX-11); `recent_slope` (`pace.rs`, about line 188) accepts 3 samples with no minimum span (FIX-12).
- **What to do.** PRD 0007 §4 lists "recommending a switch of provider" as a non-goal. When a person has two accounts that can replace each other (two Claude accounts, or a Claude and a ChatGPT account used for the same coding work), Cuotai knows which one has headroom and says nothing.

**Market expectation.** A paid macOS monitor (WhatsMyUsage) advertises "tells you when you'll run out, in plain words" and "warns you from your pace 15, 30 or 60 minutes ahead". This is the forecast PRD 0007 computes, delivered as a notification, with Cuotai's own wording.

## 2. Problem

- Threshold alerts are blind to speed: 90 % reached slowly at the end of a week is harmless; 70 % reached in an hour of a 5-hour window is urgent.
- A naive pace alert is noisy: early in a window one burst of prompts extrapolates to an absurd exhaustion; projections jitter from reading to reading; a projection that moves past the reset must not leave a stale warning behind; a stale reading must not produce a forecast.
- Each front end formats short pace and reset texts its own way.
- The user has to work out alone which of their accounts has the most room right now.

## 3. Goals

- Warn once per window, at a chosen lead time (15, 30 or 60 minutes; 60 when turned on), when the recent pace projects exhaustion before the reset — **off by default**, turned on only by the user.
- Never warn from an average-only estimate, a short sample span early in a window, a non-Fresh reading or a jittering projection.
- Give every front end the same compact pace and reset chips from the core, in percentage points by default or in percent when the user chooses.
- Show the **best account now** inside each group of comparable accounts, prominently and advisory only.

## 4. Non-goals

- Learning usage patterns, working hours or seasonality (PRD 0007 §4 still applies).
- Switching accounts, rotating credentials or changing agent configuration automatically; the best-account mark is information only (PRD 0013 §4 still applies).
- Recommendations across accounts the user did not group: without an explicit `group`, only accounts of the same provider are compared (narrowed on 2026-10-23, decision C).
- Pace for rolling rate limits, balances or overage (PRD 0023), and model-scoped metrics in the account ranking (PRD 0023 MET-01).
- Quiet hours and digests for these notices (PRD 0032).
- This PRD **supersedes** the PRD 0008 §4 non-goal "a pace-based notice (PRD 0007)" (RF2–RF4) and the PRD 0007 §4 non-goal "recommending a switch of provider" for accounts of the same provider or of one user-defined group (RF7).

## 5. Functional requirements

### RF1 — Compact pace and reset chips in the core (high priority)

The wording is the design system's (DS-CMP-TIME-2), replacing the earlier `pace.short.*` strings (DS-CMP-TIME-5).

- **RF1.1** `Pace::chip(unit: PaceUnit) -> Chip` returns the glyph (↓ → ↑ ✕) and a `Localized` text from these core keys:

  | Key | Glyph | en-US | pt-BR | es | it |
  |---|---|---|---|---|---|
  | `pace.chip.reserve` | ↓ | {d} pts in reserve | {d} pts de reserva | {d} pts de reserva | {d} pt di riserva |
  | `pace.chip.on_pace` | → | On pace | No ritmo | Al ritmo | In linea |
  | `pace.chip.ahead` | ↑ | {d} pts ahead | {d} pts acima | {d} pts por encima | {d} pt sopra |
  | `pace.chip.exhausted` | ✕ | Exhausted | Esgotado | Agotado | Esaurito |

  `{d}` is the absolute `delta_points`, rounded (no "0 pts"; a rounded 0 reads "On pace").
- **RF1.2** `Pace::outlook_chip(at) -> Option<Chip>`: `pace.chip.runs_out` (↑ "Runs out ≈{at}") only when the core projects exhaustion before the reset, otherwise `pace.chip.lasts` (↓ "Lasts to reset"); `None` for an exhausted metric (DS-CMP-TIME-4). `resets::returns_chip(at) -> Chip` gives `resets.chip.returns` (↺ "Returns {at}") from `blocked_until` or the binding reset; an unknown return reads `group.returnUnknown` ("Return unknown"), never a time.
- **RF1.3** **Points or percent (decision M, refined 2026-10-23):** `PaceUnit::{Points, Percent}`, `Points` by default; the choice is the user's, never the app's.
  - **Two values from the core.** `Pace` carries `delta_points` (used share minus expected share, in percentage points, as today) and a new `delta_ratio: Option<f64>` = `(used − expected) / expected`, the deviation **relative to the expected use**. Example: expected 50 %, used 60 % → `delta_points = +10` ("10 pts ahead") and `delta_ratio = +0.20` ("20% faster than pace").
  - **Unambiguous wording (DS-CMP-TIME-2, TIME-6).** Percent mode uses `pace.chip.ahead_pct` (↑ "{p}% faster than pace") and `pace.chip.reserve_pct` (↓ "{p}% slower than pace"), never a bare "+20%" anywhere a quota figure could be read; on pace and exhausted keep their RF1.1 texts. These two keys may use up to 22 characters (DS-CMP-TIME-1). Their compact form is glyph + percent ("↑ 20%") and appears only together with the full text in the tooltip and the accessible name ("Using quota 20 percent faster than the expected pace").
  - **Undefined range.** `delta_ratio` is `None` when the expected share is below 10 % (`expected_used_fraction < 0.10`, which also covers zero) or not finite; the chip then falls back to the points form, with the tooltip "Too early in the window for a percentage". `{p}` is capped at 999 ("999% faster than pace" for anything above); a used share of zero with an expected share of at least 10 % reads "100% slower than pace".
  - **Explained setting (DS-CMP-TIME-6).** Settings › Dashboard, "Show pace as", a segmented control **Points | Percent**, saved on change. Under it, one line shows both forms for a real account when available (else the worked example above) and the explanation "Points: difference from the expected share. Percent: how much faster or slower than expected." The pace corridor is unaffected.
  - **Where.** `pace.deviation_unit` in `desktop.json` (absent = points), `--pace-unit points|percent` or `CUOTAI_PACE_UNIT` for the TUI and CLI; it applies to chips, the status line (PRD 0031), the details and the TUI alike. `export::v1` gains `delta_ratio` beside `delta_points` in the pace object (additive).
- **RF1.4** `text::countdown(duration) -> Localized` ("in 1h 35m", "in 3d 15h", "in 4 min", "now") with plural forms in the catalogs, and `resets::pair_label(at, in) -> Localized` ("Resets {at} · {in}") for the details. The caller formats the clock time `at` with the locale (DS-MSG-8); the core owns the duration so the TUI and the desktop agree. The desktop `fmtCountdown` is replaced by the core text in the view.
- **RF1.5** Compact form (DS-CMP-TIME-3): glyph + time only ("↺ 19:30", "↑ ≈18:53") for the menu-bar title, ring tiles and narrow rows, with the full chip text as the accessible name and tooltip. The displayed pace keeps PRD 0007's rule (3 samples for the recent slope, decision D) and shows a muted "recent" marker when `source = recent`.
- **RF1.6** Catalogs in pt-BR, en-US, es and it (and those of PRDs 0034/0035 when they land); budget ≤ 18 characters including "≈" and the time, excluding the glyph, in every catalog and for both units (DS-CMP-TIME-1, DS-MSG-14).
- **Acceptance criterion:** table tests for every stage in both units, rounding of `delta` (no "0 pts", no "0% faster"), the worked example (expected 50 %, used 60 % → "+10 pts" and "20% faster than pace"), the percent edge cases (expected share 0, 9.9 % and 10 %; used 0; a ratio above 999 %; non-finite inputs) with the points fallback below 10 %, durations at the unit boundaries (59 s, 59 min, 23 h 59 min, 6 d), the unknown return, and a catalog length test in every language (18 characters, 22 for `pace.chip.ahead_pct` and `pace.chip.reserve_pct`).

### RF2 — Projected-exhaustion evaluation (high priority; prerequisite of RF3–RF5)

- **RF2.1** New `AlertKind::ProjectedExhaustion`, evaluated inside `alerts::evaluate` for metrics that pass PRD 0023's `metrics::is_plan_limit` (or `kind == Quota` until PRD 0023 RF1 lands) and that `pace::assess` assesses. The event carries `projected_at`, `resets_at`, `lead_minutes`, `margin` (reset minus projection) and the pace `source`.
- **RF2.2** Firing conditions — the **union** of the review's and this plan's criteria (decision B, the stricter set), all required, on the reading `next`:
  1. `pace.source == Recent` with at least 3 samples **and** either a sample span of at least 30 minutes or at least 20 % of the window elapsed (review item FIX-12, for the alert only; the displayed pace keeps PRD 0007's rule, decision D); an average-only projection never fires;
  2. the reading is `Fresh` (PRD 0010) and the pace is evaluated at the reading's `checked_at`, never at the current time (FIX-11);
  3. `lasts_until_reset == false`, the metric is not exhausted and `used_fraction ≥ 0.25`;
  4. `now < projected_at ≤ now + lead` and `margin ≥ 5 min` (a warning only when the stop comes meaningfully before the reset);
  5. **jitter guard:** the previous reading of the same window instance also met conditions 1–3 and projected exhaustion before the reset (two consecutive confirmations);
  6. **margin hysteresis:** once armed, the state holds while `0 < margin < 5 min`; it is cancelled only when the projection reaches or passes the reset (`margin ≤ 0`), so a projection oscillating near the 5-minute margin neither repeats nor flickers.
- **RF2.3** Once per account, metric and window instance (the `resets_at` identity within `resets::DRIFT_TOLERANCE`, 2 minutes), whatever the lead; a new window instance re-arms it.
- **RF2.4** Cancellation: a queued, undelivered `ProjectedExhaustion` is dropped when a later reading cancels it (RF2.2.6), when the metric is exhausted (the `Exhausted` alert supersedes it), when the reading stops being `Fresh` or when the window resets. A delivered one is never "un-sent", but the in-app callout (RF4.3) disappears.
- **RF2.5** First-reading rule of PRD 0008 RF1.7: nothing fires on the first reading in the process unless the previous recorded reading (PRD 0006) qualifies for the jitter guard and is at most 10 minutes older.
- **RF2.6** When the same reading also crosses a threshold, both events go into the same per-account notification (existing grouping); the projection is listed first.
- **RF2.7** `AlertPrefs::projected: Option<Lead>` with `Lead::{Min15, Min30, Min60}`; `None` turns the kind off. **Off by default** everywhere: `AlertPrefs::default()` and the core-owned preferences of PRD 0032 have `None`. `AlertPrefs::recommended()` has 60 minutes and is applied only by the explicit **Turn on** of PRD 0025 RF6, which counts as the user's consent; `--notify` never enables it implicitly.
- **Acceptance criterion:** table tests with a fixed clock and synthetic sample series: a steep slope fires once at the lead; average-only never fires; 3 samples within 10 minutes early in a window do not fire (span rule) while the same samples after 20 % of the window do; an `Aging` or `Stale` reading never fires and cancels a queued one; a single-reading spike does not fire (jitter guard); a projection oscillating around the 5-minute margin fires once and never repeats; a projection that moves past the reset cancels the queued alert; drift of `resets_at` within 2 minutes stays one instance; a new window re-arms; the first-reading rule; 15/30/60 leads; exhausted before delivery drops it.

### RF3 — Persistence and delivery (high priority)

- **RF3.1** `AlertState` records the `ProjectedExhaustion` marks (digests and dates, as PRD 0008 RF2.1) in `notified.json`; additive to the persisted format.
- **RF3.2** Desktop backend: the same `Notices::assess`/`record` path as PRD 0008 RF2.2, spacing and grouping unchanged; quiet hours and digests follow PRD 0032 when it lands.
- **RF3.3** `alerts::notice` text (`alerts.projected.*`, DS-CMP-NOTIF-1): title "Claude · Work: may run out before the reset", body "At this rate Session (5h) runs out ≈18:53, 37 min before the reset" (`at` local time, `margin` from RF1.4). Only provider, account, metric label and times.
- **RF3.4** TUI: `--notify` alone never evaluates this kind. `--notify-projected [15|30|60]` (60 when the value is omitted; it requires `--notify`) turns it on for the session, with the session-only state, delivered in the header banner with the PRD 0003 RF4 `BEL`/OSC 9 rules.
- **Acceptance criterion:** scheduler tests with a fake clock and deliverer: one notice per window, restart without resend, cancellation before delivery, grouping with a threshold alert; a binary test that `--notify` without `--notify-projected` never emits a projection; a tmux smoke with a fake `bl` provider whose usage rises steeply.

### RF4 — Desktop preferences and in-app forecast (high priority)

- **RF4.1** Settings → Alerts (DS-CMP-SEG-5; PRD 0020 rows): "Warn before running out" switch (DS-CMP-SW), **off by default**, with a dependent segmented control "15 min / 30 min / 1 h" (DS-CMP-DEP-1), stored as `alerts.projected_lead_minutes` (absent = off) in `desktop.json` (or the PRD 0032 preferences file when it lands). It reuses PRD 0003's permission flow and test button, and shows the honesty line "Linear estimate from recent readings".
- **RF4.2** The PRD 0003/0008 account bell and the per-metric mute in the details also mute this kind.
- **RF4.3** While the RF2.2 conditions hold for a metric (whether or not the alert is turned on), the details show the forecast as one callout (DS-CMP-CALL-3: "At this rate the session runs out ≈18:53, 37 min before it resets") and the card shows the outlook chip ("↑ Runs out ≈18:53"); both come from the core texts.
- **Acceptance criterion:** smoke in the four languages with a synthetic steep series: switch on, choose 15 min, receive the notice with the window hidden, see the callout, mute the metric; preferences survive a restart; a fresh profile has the switch off.

### RF5 — Event `quota_exhaustion_projected` (medium priority)

- **RF5.1** `events::diff` emits `quota_exhaustion_projected` on the RF2 edge with the default lead of 60 minutes (or the `--projected-lead 15|30|60` of `cuotai --watch`); additive to events schema 1; `--events` and `hooks.toml` accept it, and `min_threshold` does not apply to it. The event is emitted only when selected (`--events` or a hook rule), which is the script's own opt-in. PRD 0031 lists it with the other new events (review item INT-07).
- **RF5.2** Payload `current` carries `projected_at`, `resets_at`, `lead_minutes`, `used_ratio` and `source`; no notes.
- **Acceptance criterion:** reading-sequence tests, the no-leak test extended, a `--watch --jsonl` binary test.

### RF6 — Chips on every surface (medium priority)

- **RF6.1** Desktop rows and tray tooltips use the RF1 chips and the RF1.4 reset pair ("Resets 19:30 · in 1h 35m": rows show the absolute time, details add the duration, DS-MSG-8).
- **RF6.2** TUI cards use the chip text beside the stage icon when the width allows; `--once` keeps the full `Ritmo:` line and adds nothing.
- **RF6.3** `export::v1` gains only `delta_ratio` in the pace object (RF1.3; the object already carries `delta_points` and `projected_exhaustion_at`); the chips are presentation.
- **RF6.4** No surface shows a pace or an outlook chip for a reading that is not `Fresh`; an `Aging` or `Stale` reading keeps its age chip only, and the pace is computed at the reading's `checked_at` (FIX-11).
- **Acceptance criterion:** desktop and TUI rendering tests in the four languages and both units at the minimum widths; a stale preserved reading shows no pace; the old `fmtCountdown` is gone.

### RF7 — Best account now in comparable groups (medium priority; rewritten 2026-10-23, decision C, review item DEC-02)

- **RF7.1** **Groups.** An account may carry `group = "coding"` in the TOML; accounts with the same `group` value are comparable. An optional `[[groups]]` table (`id`, `label`, `order`) gives a group a display name and position; membership stays on the account. An account without `group` is compared with the other ungrouped accounts of the **same provider**. `account_edit` validates the value (slug, ≤ 32 characters) and the TUI wizard and desktop dialog offer it as an optional field; `ConfigFile` preserves comments. When PRD 0029 lands, `[[groups]]` may also list member account ids.
- **RF7.2** **Ranking.** Pure `advice::best(accounts: &[AccountView], now) -> Vec<GroupBest>` over the cycle's current readings. Candidates are enabled accounts with no failure; `Fresh` readings rank first, `Aging` ones after every `Fresh` one, `Stale` or unmeasured ones are excluded. The score of an account is the minimum, over its plan limits (PRD 0023 `is_plan_limit`) **without a model scope** (PRD 0023 MET-01), of the effective remaining share `r + (1 − r) × max(0, 1 − t / 30 min)`, where `r` is the remaining share and `t` the time until that metric's reset. A limit that renews within 30 minutes is credited linearly toward full, so a 5-hour window at 80 % used that renews in 5 minutes does not lose to a weekly window at 70 % used. Ties go to configuration order. A group needs at least two candidates for a best account to exist.
- **RF7.3** **Presentation, prominent and advisory.** The best account carries a "Best now" chip (`advice.best`) on its card, in the tray popover and in the menu-bar tooltip; when another account of the group is exhausted or projected to run out, its details add "Work runs out ≈18:53 · Personal has 64 % left" (`advice.switch`) and, as a second line of an RF3 notice when one exists, never a notification of its own. The only action is "Show Personal" (focus the card). Nothing changes credentials, agent configuration or the TOML.
- **RF7.4** **Export.** `AccountExport` gains `group: string | null`; `/v1/limits` gains `best: [{ group, account }]` (PRD 0031 owns the rest of the `/v1/limits` summary); both additive under the v1 policy.
- **RF7.5** Preference "Show the best account" (on by default when a group has two or more accounts), `advice.enabled` in `desktop.json`; the TUI shows the chip on the card and the sentence in the details.
- **Acceptance criterion:** table tests: two Claude accounts, the one with more headroom is best; one exhausted → the other; a 5-hour limit at 80 % renewing in 5 minutes against a weekly limit at 70 %; a stale or failing candidate excluded, an `Aging` one demoted; a model-scoped exhausted metric ignored; an explicit cross-provider group compared, an ungrouped cross-provider pair never compared; a single-member group has no best; texts in the four languages; a check that no code path writes configuration from the ranking; export schema and reference-output tests.

### RF8 — Documentation (low priority)

- **RF8.1** `docs/events.md` (RF5), `docs/export-v1.md` (RF7.4), `cuotai.example.toml` (`group`, `[[groups]]`), `README.md` (predictive warnings, their honesty limits, groups and the best account), and the "Linear estimate from recent readings" sentence beside the setting (PRD 0007 §6 honesty rule).
- **Acceptance criterion:** documentation reviewed with the delivery; the changelog check passes.

## 6. Non-functional requirements

- **Honesty:** the warning is a linear estimate from recent readings; texts say "at this rate" and use "≈"; no warning from averages, short sample spans, non-Fresh readings or single readings.
- **Freshness (FIX-11):** pace and outlook are computed at the reading's `checked_at`; a reading that is not `Fresh` never gets a pace chip, a forecast callout, a projected alert or a best-account mark from a stale value.
- **Sample span (FIX-12):** the alert requires the RF2.2.1 span; the displayed pace keeps PRD 0007's rule with a "recent" marker (decision D).
- **Consent:** the projected alert is off by default; only the user's own switch, the first-run **Turn on** or `--notify-projected` enable it.
- **Noise:** at most one projection notice per metric and window; the PRD 0008 spacing (one notification per minute) and per-account grouping apply.
- **Determinism:** `evaluate` and `best` depend only on their arguments and the injected clock.
- **Cost:** no new provider query; the lead time never shortens refresh intervals. A 15-minute lead on an account refreshed every 10 minutes is documented as best effort.
- **Privacy:** notification texts are lock-screen safe (provider, account names, metric label, times); PRD 0032 may hide account names.
- **i18n and accessibility:** pt-BR, en-US, es and it; the callout conveys tone by icon and weight, not color alone (DS-CMP-CALL-2); chips have the full text as accessible name.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `desktop.json` | `alerts.projected_lead_minutes`, `advice.enabled`, `pace.deviation_unit` | Additive, lenient read |
| `notified.json` | `ProjectedExhaustion` marks | Additive, pruned after 8 days |
| TOML | optional `group` per account, optional `[[groups]]` | Additive |
| `export::v1` | `AccountExport.group`; `/v1/limits` `best`; `pace.delta_ratio` | Additive (v1 policy) |
| Events schema 1 | `quota_exhaustion_projected` | Additive |
| CLI | `cuotai --watch --projected-lead 15\|30\|60`, `--notify-projected [15\|30\|60]`, `--pace-unit points\|percent` | New flags |
| `--json` | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | chips in both units, countdown, `ProjectedExhaustion` evaluation and state, `advice::best`, group validation, event kind, notice texts, export fields |
| Desktop | settings rows, notification delivery, callout and chips, best-account chip, group field in the dialog |
| TUI/CLI | chips on cards, `--notify-projected` banner, best-account chip and sentence, group field in the wizard, `--watch` flag |

## 9. Dependencies and sequencing

RF1 has no dependency and unblocks PRD 0025's menu-bar title and PRD 0031's status line. RF2 needs PRD 0007 and 0008 (both delivered) and benefits from PRD 0023 RF1 so overage never triggers it. RF3–RF5 follow RF2. RF7 needs PRD 0023's MET-01 `scope` for the exclusion of model-scoped metrics (until then every plan limit counts) and needs PRD 0029 RF1 so `[[groups]]` reference accounts by id (PRD 0029 §9). Visual details of chips, callout and rows come from PRD 0022 and `docs/design-system.md`. Wave 5d of `0000_control.md`.

## 10. Risks

| Risk | Mitigation |
|---|---|
| False alarms from bursty use | `source = recent` with a 30-minute span or 20 % of the window, two-reading confirmation, `used_fraction ≥ 0.25`, Fresh only, once per window |
| Late warnings with long refresh intervals | documented; the lead never forces extra queries |
| The best-account mark read as encouragement to evade provider limits | facts only; accounts the user configured and grouped; advisory; can be turned off |
| Percent and points confused | points by default (DS-MSG-4); the unit is named in the setting and the tooltip |
| Chips clipped in long languages | 18-character budget tested in every language and both units |

## 11. Order of execution and verification

1. **RF1:** chips (both units) and countdown.
2. **RF2 and RF3:** evaluator, persistence and delivery.
3. **RF4:** desktop preferences and callout.
4. **RF5:** event.
5. **RF6:** chips everywhere, with the freshness rule.
6. **RF7:** groups and the best account; **RF8** documentation with each step.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface. The requirement is only delivered when a steep synthetic series produces exactly one notice at the chosen lead, an average-only estimate, a short early span, a stale reading and a single spike produce none, and no extra query is made.

## 12. Open questions

| Question | Recommendation |
|---|---|
| ~~"−10% vs pace" (market wording) or "−10 pts vs pace"~~ | **Decided on 2026-10-23 (decision M):** points by default, percent as a user setting; the wording is DS-CMP-TIME-2. |
| ~~Predictive warnings on by default~~ | **Decided on 2026-10-23 (decision B):** off; the first-run **Turn on** counts as consent; `--notify` never enables it. |
| ~~Percent form: relative deviation or the same delta with a "%" sign~~ | **Decided on 2026-10-23:** relative to the expected use, worded "{p}% faster/slower than pace", undefined below a 10 % expected share (RF1.3). The wording is DS-CMP-TIME-2/TIME-6. |
| Should groups also cover provider families automatically (Claude subscription and Anthropic API) | No; only explicit `group` values join different providers. |
| ~~Should `--notify` get a `--notify-lead` flag~~ | **Decided:** `--notify-projected [15\|30\|60]` (RF3.4), because `--notify` alone must never enable it. |
