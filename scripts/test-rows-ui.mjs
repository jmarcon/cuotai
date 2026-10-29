#!/usr/bin/env node
// PRD 0022 RF3-RF6: account rows, figure block, monograms, the status line and its overflow order, the state ring
// and the honest-data marks, rendered by the production component module in Node (no browser).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { installUiEnv, report } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const c = await import("../apps/desktop/ui/js/components.js");

// RF3.1: figure on line 1, the unit word below it (one block, unit never inline in rows)
const ok = report({ left: 22 });
const html = c.rowHtml(ok, { title: "Claude Code", sub: "work@example.com" });
assert.match(html, /<span class="fig"><b class="">22%<\/b><small>left<\/small><\/span>/, "figure block with the unit below");
// RF3.2: the whole row header is one button with aria-expanded and a complete accessible name
assert.match(html, /<button class="rh" data-open="claude::work@example.com" aria-expanded="false" aria-label="Claude Code, work@example.com, 22% left/);
assert.doesNotMatch(html.replace(/<button class="rh"[^>]*>/, ""), /<button/, "no nested interactive element in the row header");
// RF3.2 (DS-CMP-ROW-4): a fresh age is never repeated per row; an aging one is
assert.doesNotMatch(html, /Fresh/);
assert.match(c.rowHtml(report({ left: 40, freshness: "aging" }), { title: "x" }), /Aging · /);

// RF3.3: several accounts of a provider → the account label is the title (the caller passes it), monogram on tile
const two = [report({ account: "work@example.com", index: 0 }), report({ account: "side@example.com", index: 1 }), report({ provider: "kimi", account: "ops", index: 2 })];
assert.deepEqual([...c.repeatedProviders(two)], ["claude"]);
// RF3.4 (DS-CMP-MONO-1..3): only providers with two or more accounts; first two letters, collisions resolved
const monos = c.monograms(two);
assert.equal(monos.get("claude::work@example.com"), "WO");
assert.equal(monos.get("claude::side@example.com"), "SI");
assert.equal(monos.has("kimi::ops"), false, "a single account of a provider has no monogram");
const clash = c.monograms([report({ account: "Work Acme", index: 0 }), report({ account: "Work Apex", index: 1 })]);
assert.equal(clash.get("claude::Work Acme"), "WA");
assert.notEqual(clash.get("claude::Work Apex"), "WA", "collision gets another letter");
assert.match(c.tileHtml("claude", 32, "WO"), /class="mono-badge"[^>]*aria-hidden="true">WO</);
assert.doesNotMatch(c.tileHtml("claude", 32, ""), /mono-badge/);
// DS-CMP-MONO-4: the user override (the view's `monogram`, at most two characters, upper-cased) wins, shows on a
// provider's only account too, and the derived letters of its siblings avoid it
const over = c.monograms([{ ...report({ account: "work@example.com", index: 0 }), monogram: "so" }, report({ account: "Some Other", index: 1 }), { ...report({ provider: "kimi", account: "ops", index: 2 }), monogram: " x9z " }]);
assert.equal(over.get("claude::work@example.com"), "SO");
assert.notEqual(over.get("claude::Some Other"), "SO", "a derived monogram never repeats the override");
assert.equal(over.get("kimi::ops"), "X9", "an override shows on a single account, clipped to two characters");
assert.equal(c.monogramOverride({ monogram: "  " }), "");

// RF4.1: one status line with the binding label and the absolute reset at the right
assert.match(html, /<span class="stl"[^>]*><span class="stl-l"><svg class="ring pct bad"[^>]*>.*<span class="lbl bad">Running low<\/span><\/span><span class="rst"/);
assert.doesNotMatch(html, /class="lbl[^"]*">Week</, "without a pace the status line names the level, never the window (fa gap 1)");
// RF4.2: the overflow order: compact label → drop detail → truncate; the reset is never measured or cut
const width = (p) => [...`${p.label}${p.detail ? ` · ${p.detail}` : ""}`].length * 7;
const longest = { label: "34 puntos por encima del ritmo", compact: "↑ 34 pts", detail: "se agota a las 18:53, 1d 7h antes del reinicio" };
assert.deepEqual(c.planFit(1000, longest, width), [], "fits: nothing yields");
assert.deepEqual(c.planFit(304, longest, width), ["compact", "drop"], "304 px: compact label, then the detail goes");
assert.deepEqual(c.planFit(304, { ...longest, compact: "" }, width), ["drop"]);
assert.deepEqual(c.planFit(40, { ...longest, compact: "" }, width), ["drop", "truncate"]);

// RF5.1: the ring is static, aria-hidden, one arc; icons replace it for non-quota states
const ring = c.ringHtml("pct", 22, 12);
assert.match(ring, /aria-hidden="true"/);
assert.equal((ring.match(/class="ra"/g) ?? []).length, 1, "one arc");
assert.doesNotMatch(ring, /animate|transition/);
assert.match(c.stateGlyph(c.accountState(report({ status: "authentication_error", left: null }))), /class="i sg accent"/);
assert.match(c.ringHtml("exhausted", 0, 12), /ring exhausted/);

// RF6.1: unknown is "—" and a hatched meter, never 0%; stale is dimmed; exhausted reads the word
const unknown = report({ left: null });
const unknownHtml = c.rowHtml(unknown, { title: "Kimi" });
assert.match(unknownHtml, /<b class="">—<\/b>/);
assert.match(unknownHtml, /class="mtr none"/);
assert.doesNotMatch(unknownHtml, />0%</, "no 0% for an unknown metric");
const stale = report({ status: "error", left: null, lastGood: { checked_at: new Date(Date.now() - 42 * 60e3).toISOString(), metrics: [report({ left: 74 }).metrics[0]] } });
const staleHtml = c.rowHtml(stale, { title: "Groq" });
assert.match(staleHtml, /class="fig stale"/);
assert.match(staleHtml, /class="mtr good stale"/);
assert.match(staleHtml, /Stale/);
assert.doesNotMatch(staleHtml, /class="pm/, "no pace mark on a stale meter");
const gone = c.rowHtml(report({ exhausted: true }), { title: "Cursor" });
assert.match(gone, /<b class="">0%<\/b><small>left<\/small>/, "an exhausted figure stays a number; the status line names the state (fa gap 5)");
assert.match(gone, /<span class="lbl bad">Limit hit<\/span>/, "the status line carries the exhausted state");

// every locale renders the unit word below the figure
for (const [locale, unit] of [["pt-BR", "disponível"], ["es", "disponible"], ["it", "disponibile"]]) {
  await i18n.setLocale(locale);
  assert.match(c.rowHtml(ok, { title: "x" }), new RegExp(`<small>${unit}</small>`));
}

// DS-CMP-ROW-8: the core's pair of windows (`windows`, `metrics::window_pair`): thick shortest window, thin next one.
await i18n.setLocale("en-US");
const lib = await import("../apps/desktop/ui/js/lib.js");
const resetIn = (h) => (h == null ? null : new Date(Date.now() + h * 3600e3).toISOString());
const q = (label, window, left, resetH = 5, extra = {}) => ({ label, used: 100 - left, limit: 100, remaining: left, unit: "%", kind: "quota", window, period: null,
  resets_at: resetIn(resetH), state: left === 0 ? "exhausted" : "available", ratio: left / 100, uncapped: false, no_cap: false, pace: null, ...extra });
// A pace in the core's shape (`PaceView`): `delta_points` = used − expected, in points.
const pace = (stage, expected, used, { runsOut = false, ratio = null } = {}) => {
  const d = String(Math.round(Math.abs((used - expected) * 100)));
  const chip = stage === "reserve" ? { glyph: "↓", text: { id: "pace.chip.reserve", params: { d }, fallback: `${d} pts in reserve` }, compact: `${d} pts`, hint: null }
    : stage === "ahead" ? { glyph: "↑", text: { id: "pace.chip.ahead", params: { d }, fallback: `${d} pts ahead` }, compact: `${d} pts`, hint: null }
      : { glyph: "→", text: { id: "pace.chip.on_pace", params: {}, fallback: "On pace" }, compact: null, hint: null };
  const pct = ratio == null ? null : String(Math.round(Math.abs(ratio) * 100));
  const pchip = pct == null ? chip : { glyph: chip.glyph, text: { id: stage === "ahead" ? "pace.chip.ahead_pct" : "pace.chip.reserve_pct", params: { p: pct }, fallback: `${pct}%` }, compact: `${pct}%`, hint: null };
  return { elapsed_fraction: expected, expected_used_fraction: expected, used_fraction: used, delta_points: (used - expected) * 100, delta_ratio: ratio, stage,
    tone: { reserve: "good", ahead: "warn", on_pace: "neutral" }[stage], source: "average", resets_at: resetIn(5), projected_exhaustion_at: runsOut ? resetIn(1) : null,
    lasts_until_reset: !runsOut, label: { id: `pace.${stage}`, params: { delta: d }, fallback: stage }, outlook: null, chips: { points: chip, percent: pchip } };
};
const dual = (metrics, windows, { binding = null, cls = "available", provider = "claude" } = {}) => {
  const b = binding ?? metrics.reduce((best, m, i) => (best < 0 || m.ratio < metrics[best].ratio ? i : best), -1);
  const m = metrics[b];
  return { ...report({ provider }), metrics, class: cls, windows,
    binding: { index: b, used_ratio: 1 - m.ratio, window: m.window, label: m.label, reason: m.state === "exhausted" ? "exhausted" : "tightest", blocked_until: m.state === "exhausted" ? m.resets_at : null } };
};
const W = (primary, secondary = null, by_model = false) => ({ primary, secondary, by_model });
const meters = (html) => (html.match(/role="meter"/g) ?? []).length;

// two windows, the session free and the week limiting: the big figure is the session's, the thin line warns
const claude = dual([q("session (5h)", "window_5h", 100, 4, { pace: pace("reserve", 0.12, 0) }), q("weekly (7d)", "weekly", 49, 70, { pace: pace("ahead", 0.01, 0.51, { runsOut: true }) })], W(0, 1));
const cHtml = c.rowHtml(claude, { title: "Claude Code" });
assert.match(cHtml, /<span class="fig dual"><span class="f1"><b class="">100%<\/b><span class="wl"[^>]*>5h<\/span>/, "the thick window's share and name");
assert.match(cHtml, /<span class="f2 warn"><span class="fv">49%<\/span><span class="wl"[^>]*>Week<\/span>/, "the limiting thin window in the warning role");
assert.match(cHtml, /<span class="pd good" aria-hidden="true" title="12 pts in reserve"><span class="pg">↗<\/span><span>\+12<\/span>/, "ahead of pace: ↗ +12 in the success role");
assert.match(cHtml, /<span class="pd bad" aria-hidden="true" title="50 pts ahead"><span class="pg">↘<\/span><span>−50<\/span>/, "running out before the reset: ↘ −50 in the danger role");
assert.equal(meters(cHtml), 2, "two meters");
assert.match(cHtml, /<span class="rm dual"><div class="mtr good" role="meter" aria-label="5h"[^>]*aria-valuetext="100% left, 12 pts in reserve, Resets [^"]+">.*?<span class="pm tri" style="left:88%" aria-hidden="true"><\/span><\/div>/);
assert.match(cHtml, /<div class="mtr warn thin" role="meter" aria-label="Week"[^>]*aria-valuetext="49% left, 50 pts ahead, Most restrictive limit right now, Resets [^"]+">.*?<span class="pm dot" style="left:99%" aria-hidden="true"><\/span><\/div>/);
assert.match(cHtml, /aria-label="Claude Code, work@example.com, 49% left, [^"]*5h: 100% left, 12 pts in reserve; Week: 49% left, 50 pts ahead, Most restrictive limit right now"/, "the deltas are in the accessible name");
assert.doesNotMatch(cHtml, /class="pm"/, "no corridor band on the pair");
// a good limiting thin window still reads as a warning; a non-limiting one stays muted
assert.match(c.rowHtml(dual([q("5h", "window_5h", 100), q("week", "weekly", 80)], W(0, 1)), { title: "x" }), /<span class="f2 warn">[\s\S]*class="mtr warn thin"/);
assert.match(c.rowHtml(dual([q("5h", "window_5h", 40), q("week", "weekly", 80)], W(0, 1)), { title: "x" }), /<span class="f2"><span class="fv">80%/);

// a single window keeps today's row: one bar with its corridor, "64% left" and no thin line
const single = c.rowHtml(dual([q("Weekly", "weekly", 64, 5, { pace: pace("reserve", 0.5, 0.36) })], W(0)), { title: "Kimi" });
assert.equal(meters(single), 1);
assert.doesNotMatch(single, /fig dual|class="f2/);
assert.match(single, /<span class="fig"><b class="">64%<\/b><small>left<\/small><\/span>/);
assert.match(single, /<span class="pm" style="left:/, "the corridor band, as today");
// a hidden thin metric (Settings › the account's hidden metrics) falls back to the single bar too
assert.equal(c.windowBars({ ...claude, windows: W(0, 7) }), null);
// day + month: thick day, thin month
assert.match(c.rowHtml(dual([q("Monthly", "monthly", 90, 300), q("Daily", "daily", 30, 9)], W(1, 0)), { title: "x" }), /<b class="">30%<\/b><span class="wl"[^>]*>Day<.*<span class="fv">90%<\/span><span class="wl"[^>]*>Month</);
// exhausted: 0% in the danger role, an exhausted empty bar, the thin bar kept; the status line keeps the return
const out = c.rowHtml(dual([q("weekly", "weekly", 0, 30), q("5h", "window_5h", 100, 2)], W(0, 1), { cls: "exhausted" }), { title: "x" });
assert.match(out, /<b class="bad">0%<\/b>/);
assert.match(out, /class="mtr bad" role="meter" aria-label="Week"[^>]*aria-valuenow="0"/);
assert.match(out, /class="mtr good thin"/);
assert.match(out, /<span class="lbl bad">Limit hit<\/span>.*class="rst"/);
// no reset date (so no core pace): the bars stay, without a marker or a delta
const noReset = c.rowHtml(dual([q("5h", "window_5h", 70, null), q("week", "weekly", 40, null)], W(0, 1)), { title: "x" });
assert.equal(meters(noReset), 2);
assert.doesNotMatch(noReset, /class="pm|class="pd/);
// per-model quotas: the tightest model thick, the second thin, named by the core's scope
const model = (name, left) => q(name, "daily", left, 9, { scope: { kind: "model", id: name.toLowerCase(), label: name } });
const gemini = c.rowHtml(dual([model("Gemini Flash", 80), model("Gemini Pro", 20)], W(1, 0, true), { binding: 1 }), { title: "Gemini" });
assert.match(gemini, /<b class="">20%<\/b><span class="wl model" title="Gemini Pro">Gemini Pro<\/span>.*<span class="fv">80%<\/span><span class="wl model" title="Gemini Flash">Gemini Flash</);
// a prepaid balance (no pair from the core) keeps today's row: the text figure and no meter
const balance = { ...report({ provider: "deepseek_demo", left: null }), metrics: [{ label: "Balance", used: null, limit: null, remaining: 12.4, unit: "USD", kind: "balance", window: null, period: null, resets_at: null, state: "available", ratio: null, uncapped: true, no_cap: false }], windows: null };
assert.doesNotMatch(c.rowHtml(balance, { title: "x" }), /fig dual|role="meter"/);
// a stale reading keeps the single dimmed bar
assert.doesNotMatch(c.rowHtml({ ...claude, status: "error", last_good: { checked_at: new Date().toISOString(), metrics: claude.metrics } }, { title: "x" }), /fig dual/);
// an aging reading is dimmed and carries no pace marks or deltas (PRD 0024 RF6.4)
const aging = c.rowHtml({ ...claude, freshness: "aging" }, { title: "x" });
assert.match(aging, /class="fig dual stale"/);
assert.doesNotMatch(aging, /class="pm|class="pd/);
// "< 1%" and never "100%" for an unfinished window (DS-CMP-MTR-4)
assert.deepEqual([c.fmtShare(0.4), c.fmtShare(99.6), c.fmtShare(100), c.fmtShare(0)], ["< 1%", "99%", "100%", "0%"]);
// the pace unit: percent shows the core's compact form (never a signed bare percent); early windows stay in points
await lib.setPaceUnit("percent");
const pctHtml = c.rowHtml(dual([q("5h", "window_5h", 60, 4, { pace: pace("reserve", 0.5, 0.4, { ratio: -0.2 }) }), q("week", "weekly", 90, 70, { pace: pace("ahead", 0.05, 0.1) })], W(0, 1)), { title: "x" });
assert.match(pctHtml, /<span class="pd good"[^>]*><span class="pg">↗<\/span><span>20%<\/span>/);
assert.match(pctHtml, /<span class="pd warn"[^>]*><span class="pg">↘<\/span><span>−5<\/span>/, "too early for a ratio: points; a warning without a projected run-out");
await lib.setPaceUnit("points");
// hiding pace (Settings › Show consumption pace) removes the marks and the deltas
await lib.setShowPace(false);
assert.doesNotMatch(c.rowHtml(claude, { title: "x" }), /class="pm|class="pd/);
await lib.setShowPace(true);
// on pace: the arrow alone, muted
assert.match(c.rowHtml(dual([q("5h", "window_5h", 50, 4, { pace: pace("on_pace", 0.5, 0.5) }), q("week", "weekly", 80)], W(0, 1)), { title: "x" }), /<span class="pd muted"[^>]*><span class="pg">→<\/span><\/span>/);
// the row-display "Pace corridor" switch also hides the pace marks and deltas of the pair
const css0 = await readFile(new URL("../apps/desktop/ui/css/components.css", import.meta.url), "utf8");
assert.match(css0, /:root\[data-rows-hide~="corridor"\] \.mtr \.pm,\n:root\[data-rows-hide~="corridor"\] \.fig \.pd,/);

// the status-line styles keep it to one line (DS-CMP-STL-5)
const css = await readFile(new URL("../apps/desktop/ui/css/components.css", import.meta.url), "utf8");
assert.match(css, /\.stl \{[^}]*height: 20px;[^}]*white-space: nowrap;/);

console.log("PASS: rows, figure block, monograms, status line fitting, state ring and honest marks");
process.exit(0); // the shared modules keep a BroadcastChannel open
