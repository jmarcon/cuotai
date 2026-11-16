#!/usr/bin/env node
// Browser-free UI smoke for PRD 0026 RF4/RF5: the Stats view and the reset timeline render the core's answers in every
// built-in locale. Empty history reads "—" and a reason (never 0), a week with hits marks the full day, a day without
// samples is a hatched ghost column (never a zero bar), every chart has its text alternative and table, the lanes
// carry their accessible name, and the UI-owned catalog keys of the feature match across locales.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: () => {},
};
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
globalThis.setInterval = () => 0;
Object.defineProperty(globalThis, "navigator", {
  value: { languages: ["en-US"], language: "en-US" },
  configurable: true,
});
globalThis.location = { search: "" };
globalThis.localStorage = {
  getItem: (key) => storage.get(key) ?? null,
  setItem: (key, value) => storage.set(key, value),
};
globalThis.fetch = async (relative) => {
  try {
    const path = new URL(`apps/desktop/ui/${relative}`, root);
    const data = JSON.parse(await readFile(fileURLToPath(path), "utf8"));
    return { ok: true, json: async () => data };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { kpisHtml, barsHtml, statsBodyHtml, rhythmChartHtml } = await import("../apps/desktop/ui/js/stats.js");
const { laneLabel } = await import("../apps/desktop/ui/js/timeline.js");
await i18n.initI18n();

const iso = (daysBack) => {
  const d = new Date();
  d.setDate(d.getDate() - daysBack);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
// PRD 0026 re-validation (V-03): the core's day text (`text::clock::day`) travels with each day.
const dayNote = (back) => ({ id: "at.weekday", params: { weekday: String(((new Date(Date.now() - back * 864e5).getDay() + 6) % 7) + 1) }, fallback: "Mon" });
const days = (shares) => shares.map((share, k) => ({ day: iso(shares.length - 1 - k), share, is_today: k === shares.length - 1, label: dayNote(shares.length - 1 - k) }));
const today = { id: "cost.period.today", params: {}, fallback: "Today" };
const timeNote = { id: "at.time", params: { hour: "18", minute: "5" }, fallback: "6:05 PM" };
const notEnough = { id: "stats.not_enough", params: {}, fallback: "Histórico insuficiente" };
const kept = { id: "stats.history_kept", params: { days: 90 }, fallback: "Histórico mantido por 90 dias neste dispositivo" };
const view = { reports: [{ index: 0, provider: "claude", account: "Work" }] };

const summary = (range, shares, coverage, extra = {}) => ({
  range, hits: 0, average_peak: null, extra_spent: null, unknown_limits: 0, retention_days: 90, first_sample_at: null, coverage,
  daily: days(shares), accounts: [{ account_index: 0, name: "Work", hits: 0, average_peak: null, days_with_data: 0, days: days(shares) }], ...extra,
});
const answer = (s, events = []) => ({ summary: s, indexes: [0], history_kept: kept, not_enough: notEnough,
  removed_account: { id: "journal.removed_account", params: {}, fallback: "Conta removida" }, events, recording: true });

// 1. No history: KPI cards read "—" with the core's reason, the chart is replaced by one sentence, nothing reads 0.
const empty = answer(summary("days7", Array(7).fill(null), { kind: "not_enough" }));
// 2. A week with hits: one full day, a gap, the average and an extra amount in two units.
const week = answer(summary("days7", [0.2, 0.5, null, 1, 0.4, null, 0.8], { kind: "partial", days_with_data: 5 }, {
  hits: 1, average_peak: 0.58, extra_spent: [{ unit: "USD", amount: 6.25 }, { unit: "créditos", amount: 120 }],
}), [
  { occurred_at: new Date().toISOString(), kind: "quota_exhausted", account: "Work", day: today, time: timeNote, when: timeNote, summary: { id: "events.summary.quota_exhausted", params: { metric: "5h" }, fallback: "5h: cota esgotada" } },
  { occurred_at: new Date(Date.now() - 9 * 36e5 * 24).toISOString(), kind: "refresh_failed", account: null, day: { id: "at.date", params: { day: "9", month: "11" }, fallback: "11/09" }, time: timeNote, when: timeNote, summary: { id: "events.summary.refresh_failed", params: { category: { id: "events.category.offline", params: {}, fallback: "provedor inacessível" } }, fallback: "Falha" } },
]);

const generation = 7;
const staleError = await lib.stats("days7", null, generation + 99).then(() => null, (e) => e);
assert.equal(staleError, lib.STALE, "the browser demo refuses another generation as stale");
const demo = await lib.stats("days30", null, (await lib.getView()).generation);
assert.equal(demo.summary.daily.length, 30);
assert.ok(demo.summary.daily.some((d) => d.share == null), "the demo history has a day without data");
const lanes = await lib.resetTimeline("days7", (await lib.getView()).generation);
assert.ok(lanes.lanes.length > 0 && lanes.lanes.every((l) => l.returns_at), "demo lanes are plotted only with a known return");

// PRD 0026 §7: the 7 d / 30 d and 24 h / 7 d toggles are remembered (`desktop.json` `stats`); the defaults are not stored.
assert.deepEqual(await lib.statsPrefs(), {}, "nothing remembered yet");
await lib.setStatsPrefs({ range: "days30" });
await lib.setStatsPrefs({ timeline: "days7" });
assert.deepEqual(await lib.statsPrefs(), { range: "days30", timeline: "days7" }, "each toggle keeps the other");
await lib.setStatsPrefs({ range: "days7", timeline: "hours24" });
assert.deepEqual(await lib.statsPrefs(), {}, "the defaults remove the stored choice");
assert.equal(await lib.setStatsPrefs({ range: "days90" }).then(() => null, (e) => e), "invalid_stats_range");

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const label = (key) => i18n.t(key);

  const none = kpisHtml(empty);
  assert.equal((none.match(/class="kpi unknown"/g) ?? []).length, 3, `${code}: three unknown cards`);
  assert.ok(none.includes("<b class=\"kpi-figure\">—</b>"), `${code}: unknown figure is a dash`);
  assert.ok(none.includes(i18n.tl(notEnough)), `${code}: the core's reason is the caption`);
  assert.ok(!/kpi-figure">0/.test(none), `${code}: no card shows 0 for missing data`);
  const emptyBody = statsBodyHtml(empty, view, "t");
  assert.ok(emptyBody.includes(label("stats.chart.empty")), `${code}: DS-CHT-11 sentence`);
  assert.ok(!emptyBody.includes('class="bars'), `${code}: no empty axes`);

  const cards = kpisHtml(week);
  assert.ok(cards.includes('<b class="kpi-figure">1</b>') && cards.includes(lib.fmtPercent(58)), `${code}: hits and average`);
  // PRD 0034 re-validation V-17: the percent sign sits where the language puts it.
  const share = { de: "58\u00a0%", tr: "%58", "en-US": "58%" }[code];
  if (share) assert.ok(cards.includes(share) && lib.fmtShare(58) === share, `${code}: the percent follows the language`);
  assert.ok(/6[.,]25/.test(cards) && /\$|USD/.test(cards) && cards.includes("créditos"), `${code}: one figure per unit`);
  assert.equal((cards.match(/data-kpi="extra"[\s\S]*/)[0].match(/<b class="kpi-figure[^"]*">≈/g) ?? []).length, 2, `${code}: every extra-spent amount is marked as an estimate`);
  const bars = barsHtml(week.summary.daily);
  assert.equal((bars.match(/class="gap"/g) ?? []).length, 2, `${code}: two gap days`);
  assert.equal((bars.match(/bar ghost/g) ?? []).length, 2, `${code}: gaps are ghost columns, not zero bars`);
  assert.equal((bars.match(/class="full"/g) ?? []).length, 1, `${code}: only the day at the limit is full`);
  // V-04: today at the limit keeps its accent class and its label.
  const fullToday = barsHtml([{ day: iso(1), share: 0.5, is_today: false, label: dayNote(1) }, { day: iso(0), share: 1, is_today: true, label: dayNote(0) }]);
  assert.ok(/class="full today"/.test(fullToday) && fullToday.includes(`>${label("stats.chart.today")}<`), `${code}: today at the limit keeps its highlight`);
  assert.ok(bars.includes(`>${label("stats.chart.today")}<`), `${code}: today is labelled`);
  const body = statsBodyHtml(week, view, "t");
  assert.ok(body.includes('role="img"') && body.includes('<div class="sr-only"><table id="t-table"'), `${code}: chart text alternative and table`);
  assert.ok(body.includes(`<h4 class="st-day-head micro">${i18n.tle(today)}</h4>`), `${code}: journal day header`);
  assert.ok(statsBodyHtml({ ...week, skipped: 2 }, view, "t").includes("data-stats-skipped"), `${code}: unreadable entries are counted`);
  assert.ok(body.includes(i18n.tl(week.removed_account)), `${code}: removed account label`);
  assert.ok(body.includes(i18n.tl(kept)), `${code}: coverage footnote`);
  assert.ok(body.includes(i18n.t("stats.footer.partial", { have: 5, total: 7 })), `${code}: partial coverage`);

  const at = new Date(Date.now() + 95 * 60_000 + 30_000).toISOString();
  // PRD 0024 re-validation (V-10): the duration is the core's `returns_in` text the lane carries.
  const returnsIn = { id: "text.duration.hours_minutes", params: { h: "1", m: "35" }, fallback: "1h 35m" };
  // PRD 0026 re-validation (V-01, V-12): the time is the core's clock (a date beyond six days), and one catalog
  // template joins the parts, with no ". " seam.
  const returnsClock = { id: "at.date", params: { day: "28", month: "11", hour: "9", minute: "5" }, fallback: "11/28 9:05 AM" };
  const name = laneLabel({ name: "Work", returns_at: at, returns_in: returnsIn, returns_clock: returnsClock, exhausted: false, remaining_share: 0.4, stale: true });
  assert.ok(name.includes("Work") && /\b1\D{1,12}35\D/.test(name) && name.includes("40") && name.includes("28"), `${code}: lane name "${name}"`);
  assert.ok(!/\.\. |\. [a-zà-ÿ]/.test(name), `${code}: lane name punctuation "${name}"`);
  console.log(`${code}: ${name}`);
}

// UI-owned catalog keys of this feature exist in every locale with the same placeholders.
// Distinct names: a language with one plural form (ja, zh-CN) repeats a placeholder fewer times.
const placeholders = (v) => JSON.stringify([...new Set([...JSON.stringify(v).matchAll(/\{(\w+)\}/g)].map((m) => m[1]))].sort());
let reference;
for (const { code } of i18n.LOCALES) {
  const catalog = JSON.parse(await readFile(fileURLToPath(new URL(`apps/desktop/ui/locales/${code}.json`, root)), "utf8"));
  const own = Object.fromEntries(Object.entries(catalog).filter(([k]) => /^(stats|timeline)\./.test(k) || k === "tray.stats"));
  reference ??= own;
  assert.deepEqual(Object.keys(own).sort(), Object.keys(reference).sort(), `${code}: stats/timeline keys drifted`);
  for (const key of Object.keys(own)) assert.equal(placeholders(own[key]), placeholders(reference[key]), `${code}: ${key} placeholders drifted`);
}

// PRD 0006 RF5.2 (re-validation V-02/V-03/V-07): a metric without measured use, or with one reading, shows its
// sentence and no chart or axis (DS-CHT-11); a drawn chart dates its span like the rest of the app; the empty state is
// one catalog sentence per span, with no ASCII space joined into Japanese or Chinese.
{
  const { metricHtml } = await import("../apps/desktop/ui/js/history.js");
  const span = { from: "2026-10-22T18:10:00Z", to: "2026-10-23T18:10:00Z" };
  const chart = { floor: 0, ceiling: 100, heights: [0.2, 0.4], last: { taken_at: span.to, used: 40, x: 0.75, y: 0.4 }, resets: [] };
  const base = { label: "5h", unit: "%", windows: [], chart };
  const none = metricHtml({ ...base, trend: null, samples: 3 }, span);
  assert.ok(!none.includes("hist-chart") && !none.includes("hist-axis"), "no measured use: no chart or axis");
  assert.ok(none.includes(i18n.t("trend.noUsed")), "no measured use: the sentence");
  const one = metricHtml({ ...base, trend: { min: 40, max: 40, last: { used: 40 } }, samples: 1 }, span);
  assert.ok(!one.includes("hist-chart"), "one reading: no chart");
  const drawn = metricHtml({ ...base, trend: { min: 20, max: 40, last: { used: 40 } }, samples: 2 }, span);
  assert.ok(drawn.includes("hist-chart") && drawn.includes("hist-axis"), "two readings draw the chart");
  assert.ok(drawn.includes(lib.fmtDateTime(span.from)), "the axis dates read like the rest of the app");
  for (const code of ["ja", "zh-CN", "en-US"]) {
    const catalog = JSON.parse(await readFile(fileURLToPath(new URL(`apps/desktop/ui/locales/${code}.json`, root)), "utf8"));
    for (const range of ["day", "week", "month"]) {
      for (const kind of ["emptyWithHelp", "emptyOff"]) {
        const text = catalog[`trend.${kind}.${range}`];
        assert.ok(text, `${code}: trend.${kind}.${range} missing`);
        if (code !== "en-US") assert.ok(!/[。．] /.test(text), `${code}: trend.${kind}.${range} joins with an ASCII space`);
      }
    }
    assert.equal(catalog["trend.emptyHelp"], undefined, `${code}: the joined fragments are gone`);
  }
}

// PRD 0022 re-validation V-07 (DS-CHT-10): the rhythm chart's name gives the peak and the last bucket's value.
await i18n.setLocale("en-US");
{
  const buckets = Array.from({ length: 24 }, (_, hour) => ({ hour, consumption: hour === 18 ? 10.2 : hour === 23 ? null : 1, samples: 1 }));
  const html = rhythmChartHtml({ buckets }, "rh", "day");
  assert.match(html, /aria-label="[^"]*busiest at [^"]*; last at [^"]*\(No data\)"/, "the last bucket without samples reads No data");
  buckets[23].consumption = 2.5;
  assert.match(rhythmChartHtml({ buckets }, "rh", "day"), /aria-label="[^"]*; last at [^"]*\(2\.5[^)]*\)"/, "the last value is named");
  for (const code of ["pt-BR", "de", "ja"]) {
    await i18n.setLocale(code);
    assert.doesNotMatch(rhythmChartHtml({ buckets }, "rh", "week"), /\{last|\{lastAt/, `${code}: every placeholder is filled`);
  }
  await i18n.setLocale("en-US");
}

console.log("PASS: Stats view and reset timeline render honestly in pt-BR, en-US, es and it; catalog keys match");
