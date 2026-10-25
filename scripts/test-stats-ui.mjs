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
const { kpisHtml, barsHtml, statsBodyHtml } = await import("../apps/desktop/ui/js/stats.js");
const { laneLabel } = await import("../apps/desktop/ui/js/timeline.js");
await i18n.initI18n();

const iso = (daysBack) => {
  const d = new Date();
  d.setDate(d.getDate() - daysBack);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
const days = (shares) => shares.map((share, k) => ({ day: iso(shares.length - 1 - k), share, is_today: k === shares.length - 1 }));
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
  { occurred_at: new Date().toISOString(), kind: "quota_exhausted", account: "Work", summary: { id: "events.summary.quota_exhausted", params: { metric: "5h" }, fallback: "5h: cota esgotada" } },
  { occurred_at: new Date(Date.now() - 9 * 36e5 * 24).toISOString(), kind: "refresh_failed", account: null, summary: { id: "events.summary.refresh_failed", params: { category: { id: "events.category.offline", params: {}, fallback: "provedor inacessível" } }, fallback: "Falha" } },
]);

const generation = 7;
const staleError = await lib.stats("days7", null, generation + 99).then(() => null, (e) => e);
assert.equal(staleError, lib.STALE, "the browser demo refuses another generation as stale");
const demo = await lib.stats("days30", null, (await lib.getView()).generation);
assert.equal(demo.summary.daily.length, 30);
assert.ok(demo.summary.daily.some((d) => d.share == null), "the demo history has a day without data");
const lanes = await lib.resetTimeline("days7", (await lib.getView()).generation);
assert.ok(lanes.lanes.length > 0 && lanes.lanes.every((l) => l.returns_at), "demo lanes are plotted only with a known return");

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
  assert.ok(cards.includes('<b class="kpi-figure">1</b>') && cards.includes("58%"), `${code}: hits and average`);
  assert.ok(cards.includes("USD") && cards.includes("créditos"), `${code}: one figure per unit`);
  const bars = barsHtml(week.summary.daily);
  assert.equal((bars.match(/class="gap"/g) ?? []).length, 2, `${code}: two gap days`);
  assert.equal((bars.match(/bar ghost/g) ?? []).length, 2, `${code}: gaps are ghost columns, not zero bars`);
  assert.equal((bars.match(/class="full"/g) ?? []).length, 1, `${code}: only the day at the limit is full`);
  assert.ok(bars.includes(`>${label("stats.chart.today")}<`), `${code}: today is labelled`);
  const body = statsBodyHtml(week, view, "t");
  assert.ok(body.includes('role="img"') && body.includes('class="sr-only" id="t-table"'), `${code}: chart text alternative and table`);
  assert.ok(body.includes(label("stats.events.today")), `${code}: journal day header`);
  assert.ok(body.includes(i18n.tl(week.removed_account)), `${code}: removed account label`);
  assert.ok(body.includes(i18n.tl(kept)), `${code}: coverage footnote`);
  assert.ok(body.includes(i18n.t("stats.footer.partial", { have: 5, total: 7 })), `${code}: partial coverage`);

  const at = new Date(Date.now() + 95 * 60_000 + 30_000).toISOString();
  const name = laneLabel({ name: "Work", returns_at: at, exhausted: false, remaining_share: 0.4, stale: false }, "hours24");
  assert.ok(name.includes("Work") && name.includes("1h 35m") && name.includes("40"), `${code}: lane name "${name}"`);
  console.log(`${code}: ${name}`);
}

// UI-owned catalog keys of this feature exist in every locale with the same placeholders.
const placeholders = (v) => JSON.stringify([...JSON.stringify(v).matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort());
let reference;
for (const { code } of i18n.LOCALES) {
  const catalog = JSON.parse(await readFile(fileURLToPath(new URL(`apps/desktop/ui/locales/${code}.json`, root)), "utf8"));
  const own = Object.fromEntries(Object.entries(catalog).filter(([k]) => /^(stats|timeline)\./.test(k) || k === "tray.stats"));
  reference ??= own;
  assert.deepEqual(Object.keys(own).sort(), Object.keys(reference).sort(), `${code}: stats/timeline keys drifted`);
  for (const key of Object.keys(own)) assert.equal(placeholders(own[key]), placeholders(reference[key]), `${code}: ${key} placeholders drifted`);
}

console.log("PASS: Stats view and reset timeline render honestly in pt-BR, en-US, es and it; catalog keys match");
