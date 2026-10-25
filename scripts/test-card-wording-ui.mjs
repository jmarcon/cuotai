#!/usr/bin/env node
// Browser-free UI smoke for PRD 0022 RF17.7: render the remaining-share
// figure with every built-in locale and keep UI-owned catalog keys in parity.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  documentElement: {
    lang: "",
    dataset: {},
    toggleAttribute: () => {},
  },
  addEventListener: () => {},
};
globalThis.matchMedia = () => ({
  matches: false,
  addEventListener: () => {},
});
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
const { metricCardHtml } = await import("../apps/desktop/ui/js/detail.js");
await i18n.initI18n();

const expected = {
  "pt-BR": {
    unit: "disponível",
    levels: {
      good: "75% disponível: com folga (70% ou mais).",
      warn: "Atenção: 50% disponível (entre 30% e 70%).",
      bad: "Crítico: só 20% disponível (menos de 30%).",
    },
    rateLimit: "Rate limit: 42% disponível",
    sortAsc: "Disponível: menor primeiro",
    sortDesc: "Disponível: maior primeiro",
  },
  "en-US": {
    unit: "left",
    levels: {
      good: "75% left: plenty remains (70% or more).",
      warn: "Attention: 50% left (between 30% and 70%).",
      bad: "Critical: only 20% left (under 30%).",
    },
    rateLimit: "Rate limit: 42% left",
    sortAsc: "Left: lowest first",
    sortDesc: "Left: highest first",
  },
  es: {
    unit: "disponible",
    levels: {
      good: "75% disponible: con margen (70% o más).",
      warn: "Atención: 50% disponible (entre 30% y 70%).",
      bad: "Crítico: solo 20% disponible (menos de 30%).",
    },
    rateLimit: "Rate limit: 42% disponible",
    sortAsc: "Disponible: menor primero",
    sortDesc: "Disponible: mayor primero",
  },
  it: {
    unit: "disponibile",
    levels: {
      good: "75% disponibile: ne resta in abbondanza (70% o più).",
      warn: "Attenzione: 50% disponibile (tra 30% e 70%).",
      bad: "Critico: solo 20% disponibile (meno di 30%).",
    },
    rateLimit: "Limite di frequenza: 42% disponibile",
    sortAsc: "Disponibile: prima il minore",
    sortDesc: "Disponibile: prima il maggiore",
  },
};

const coreCatalog = JSON.parse(await readFile(
  fileURLToPath(new URL("crates/cuotai-core/locales/pt-BR.json", root)),
  "utf8",
));
const coreKeys = new Set(Object.keys(coreCatalog));
let expectedUiKeys;

const metric = {
  label: "5h",
  state: "available",
  uncapped: false,
  no_cap: false,
  ratio: 0.75,
  used: 25,
  limit: 100,
  unit: "%",
  resets_at: null,
  pace: null,
};
const report = {
  provider: "open_ai_api",
  account: "work",
  binding: null,
  metrics: [metric],
};

for (const { code } of i18n.LOCALES) {
  const catalogPath = new URL(`apps/desktop/ui/locales/${code}.json`, root);
  const catalog = JSON.parse(await readFile(fileURLToPath(catalogPath), "utf8"));
  const uiKeys = Object.keys(catalog).filter((key) => !coreKeys.has(key)).sort();
  expectedUiKeys ??= uiKeys;
  assert.deepEqual(uiKeys, expectedUiKeys, `${code}: UI-owned catalog keys drifted`);

  const wording = expected[code];
  assert.equal(catalog["card.available"], wording.unit, `${code}: card.available drifted`);

  await i18n.setLocale(code);
  for (const [tone, ratio] of [["good", 0.75], ["warn", 0.5], ["bad", 0.2]]) {
    const rendered = metricCardHtml(report, { ...metric, ratio });
    assert.ok(
      rendered.includes(`<small>${wording.unit}</small>`),
      `${code}/${tone}: remaining-share unit did not render`,
    );
    assert.ok(
      rendered.includes(`title="${wording.levels[tone]}"`),
      `${code}/${tone}: remaining-share level sentence did not render`,
    );
  }
  assert.equal(
    i18n.t("card.rateLimit", { pct: 42 }),
    wording.rateLimit,
    `${code}: rate-limit composition drifted`,
  );
  assert.equal(i18n.t("sort.pct-asc"), wording.sortAsc, `${code}: ascending sort composition drifted`);
  assert.equal(i18n.t("sort.pct-desc"), wording.sortDesc, `${code}: descending sort composition drifted`);

  // PRD 0032 RF8.2: a level crossed while Cuotai was closed shows its own line in the details, in the language;
  // a metric without it shows none.
  const crossed = metricCardHtml(report, { ...metric, ratio: 0.08, alert: { muted: false, active: true, missed: 90 } });
  assert.ok(crossed.includes("data-missed"), `${code}: missed line did not render`);
  const line = crossed.match(/data-missed><svg[\s\S]*?<\/svg><span>([^<]*)<\/span>/)?.[1];
  assert.equal(line, i18n.t("alerts.missed", { percent: 90 }), `${code}: missed line text`);
  assert.ok(line && !line.includes("{") && /90/.test(line), `${code}: missed line not translated: ${line}`);
  assert.ok(!metricCardHtml(report, { ...metric, alert: { muted: false, active: false } }).includes("data-missed"), `${code}: marker without a crossing`);

  console.log(`${code}: 75% ${wording.unit}; ${wording.rateLimit}`);
}

console.log("PASS: remaining-share wording renders in pt-BR, en-US, es and it; UI catalog keys match");
