#!/usr/bin/env node
// Browser-free UI smoke for PRD 0033: the collapsed "Renewed at ..." metric of a
// preserved reading in the viewer's local time, the "data as of" age text and
// the pace line label rule, in every built-in language.
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
const { metricCardHtml } = await import("../apps/desktop/ui/js/detail.js");
await i18n.initI18n();

const resetsAt = new Date(Date.now() - 3 * 3600 * 1000).toISOString();
const collapsed = {
  label: "Week", state: "unmeasured", used: null, limit: null, remaining: null, unit: "%",
  window: "weekly", resets_at: resetsAt, expired: true, pace: null,
};
const live = {
  label: "5h", state: "available", ratio: 0.5, used: 50, limit: 100, unit: "%",
  window: "window_5h", resets_at: null, pace: { stage: "reserve", tone: "good", label: { id: "x", fallback: "8 points in reserve" }, expected_used_fraction: 0.4 },
};
const base = { provider: "claude", account: "work", status: "ready", checked_at: new Date().toISOString(), notes: [], freshness: "fresh" };
const wording = (code, clock) => ({
  "pt-BR": `Renovada às ${clock}; aguardando leitura`,
  "en-US": `Renewed at ${clock}; waiting for a reading`,
  es: `Renovada a las ${clock}; esperando una lectura`,
  it: `Rinnovata alle ${clock}; in attesa di una lettura`,
  de: `Erneuert um ${clock}; wartet auf einen Messwert`,
  ru: `Обновлено в ${clock}; ожидается показание`,
  ko: `${clock}에 갱신됨, 조회값을 기다리는 중`,
})[code];
const asOfWording = { "pt-BR": "dados de", "en-US": "data as of", es: "datos de", it: "dati delle", de: "Daten vom", ru: "данные от", ko: "기준 데이터" };

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const text = wording(code, lib.fmtClock(resetsAt));
  const report = { ...base, binding: null, metrics: [collapsed] };
  const html = metricCardHtml(report, collapsed);
  assert.ok(html.includes("data-renewed-waiting"), `${code}: collapsed marker missing`);
  assert.ok(html.includes(text), `${code}: renewed text drifted: ${html}`);
  assert.ok(!html.includes('class="resets"'), `${code}: a collapsed metric keeps no reset line`);
  assert.equal(lib.summaryOf({ ...report, last_good: null }).kind, "renewed", `${code}: card summary`);

  const lagging = { ...base, metrics: [live], data_as_of: new Date(Date.now() - 6 * 3600 * 1000).toISOString() };
  assert.ok(lib.ageText(lagging).includes(asOfWording[code]), `${code}: data-as-of text missing`);
  assert.ok(!lib.ageText({ ...base, metrics: [live] }).includes(asOfWording[code]), `${code}: no as-of without data_as_of`);
  console.log(`${code}: ${text}`);
}

// RF4.1: the pace line names its metric unless the binding chip says it already.
await i18n.setLocale("en-US");
const week = { ...live, label: "Week", window: "weekly", pace: null };
const report = { ...base, metrics: [week, live], binding: { index: 0 } };
report.metrics = [week, live];
assert.equal(lib.paceLineLabel(report, live), "5h", "a pace on another metric than the chip's keeps its label");
assert.equal(lib.paceLineLabel({ ...report, binding: { index: 1 } }, live), "", "same metric as the chip: label omitted");
assert.equal(lib.paceLabelOf({ window: "other", label: "Custom" }), "Custom");

console.log("PASS: renewed text, data-as-of and pace labels render in pt-BR, en-US, es and it");
