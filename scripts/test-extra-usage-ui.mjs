#!/usr/bin/env node
// Browser-free UI smoke for PRD 0023 RF7: paid extra usage (overage) on the
// card chip, the card row and the details, in every built-in locale, for an
// active, capped, unknown, armed and disabled state. The core owns the texts
// (`overage.*`); the UI only formats the figure.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const read = async (relative) => JSON.parse(await readFile(fileURLToPath(new URL(relative, root)), "utf8"));
const storage = new Map();
const coreFiles = { "pt-BR": "pt-BR", es: "es", it: "it" }; // en-US uses the English fallback of each text
globalThis.window = {
  __TAURI__: {
    core: {
      invoke: async (_command, { locale }) => (coreFiles[locale] ? read(`crates/cuotai-core/locales/${coreFiles[locale]}.json`) : {}),
    },
  },
};
globalThis.document = { hidden: false, documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} }, addEventListener: () => {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) };
globalThis.fetch = async (relative) => {
  try {
    return { ok: true, json: async () => read(`apps/desktop/ui/${relative}`) };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { detailHtml, metricCardHtml } = await import("../apps/desktop/ui/js/detail.js");
await i18n.initI18n();

const text = (id, fallback, params = {}) => ({ id, params, fallback });
const meter = { label: "5h", state: "available", uncapped: false, no_cap: false, ratio: 0.4, used: 60, limit: 100, unit: "%", kind: "quota", window: "window_5h", resets_at: null, pace: null };
const extraMetric = (over) => ({
  label: "Extra usage", state: "available", uncapped: true, no_cap: false, ratio: null, used: 6.25, limit: 50, remaining: null,
  unit: "USD", kind: "spend", window: "monthly", resets_at: null, pace: null, billing: "overage", ...over,
});
const report = (overage, metrics = [meter, extraMetric({})]) => ({
  provider: "claude", account: "Work", status: "ready", class: "available", binding: null, metrics, overage, notes: [], notes_l10n: [],
  usage_alerts: [], checked_at: new Date().toISOString(), freshness: "fresh", clock_adjusted: false, last_good: null, pending: false,
  access: null, incident: null, session_expiry: null, identity: { email: null, badges: [], copy: null },
});
const cycle = "2026-11-01T00:00:00Z";
const money = { unit: "USD", currency: "USD", cycle_ends_at: cycle };
const cases = {
  active: report({
    state: "active", spent: 6.25, cap: 50, ...money,
    text: text("overage.on_extra", "On extra · $6.25", { amount: "$6.25" }),
    summary: text("overage.on_extra", "On extra · $6.25", { amount: "$6.25" }), caveat: null,
  }),
  capped: report({
    state: "cap_reached", spent: 50, cap: 50, ...money,
    text: text("overage.cap_reached", "Extra budget used up"),
    summary: text("overage.limit_hit_on_extra", "Limit hit · on extra $50.00", { amount: "$50.00" }), caveat: null,
  }, [meter, extraMetric({ used: 50 })]),
  unknown: report({
    state: "unknown", spent: null, cap: 50, unit: "USD", currency: "USD", cycle_ends_at: null,
    text: text("overage.unknown", "Extra usage amount unknown"), summary: text("overage.unknown", "Extra usage amount unknown"), caveat: null,
  }, [meter, extraMetric({ used: null })]),
  armed: report({
    state: "armed", spent: 0, cap: 50, ...money,
    text: text("overage.armed", "Extra usage on · nothing spent"), summary: text("overage.armed", "Extra usage on · nothing spent"), caveat: null,
  }, [meter, extraMetric({ used: 0 })]),
  disabled: report({
    state: "disabled", spent: null, cap: null, unit: null, currency: null, cycle_ends_at: null,
    text: text("overage.disabled", "Extra usage off"), summary: text("overage.disabled", "Extra usage off"), caveat: null,
  }, [meter]),
  units: report({
    state: "active", spent: 12.5, cap: 50, unit: "unid. provedor", currency: null, cycle_ends_at: cycle,
    text: text("overage.on_extra", "On extra · 12.5 provider units", { amount: text("overage.amount.provider_units", "12.5 provider units", { value: "12.5" }) }),
    summary: text("overage.on_extra", "On extra · 12.5 provider units", { amount: text("overage.amount.provider_units", "12.5 provider units", { value: "12.5" }) }),
    caveat: text("overage.units_caveat", "Amounts are in provider units, not money."),
  }, [meter, extraMetric({ unit: "unid. provedor", kind: "quota", used: 12.5 })]),
};

const expected = {
  "en-US": { chip: "On extra · $6.25", off: "Extra usage off", unknown: "Extra usage amount unknown", caveat: "Amounts are in provider units, not money.", title: "Extra usage" },
  es: { chip: "En extra · $6.25", off: "Uso extra desactivado", unknown: "Importe del uso extra desconocido", caveat: "Los importes están en unidades del proveedor, no en dinero.", title: "Uso extra" },
  it: { chip: "Extra in uso · $6.25", off: "Uso extra disattivato", unknown: "Importo dell'uso extra sconosciuto", caveat: "Gli importi sono in unità del provider, non in denaro.", title: "Uso extra" },
  "pt-BR": { chip: "No extra · $6.25", off: "Uso extra desligado", unknown: "Valor do uso extra desconhecido", caveat: "Valores em unidades do provedor, não em dinheiro.", title: "Uso extra" },
};
const flat = (value) => value.replace(/[\u00a0\u202f]/g, " ");
const rowOf = (code, template) => {
  const usd = (value) => new Intl.NumberFormat(code, { style: "currency", currency: "USD" }).format(value);
  return flat(template.replace("{amount}", usd(6.25)).replace("{cap}", usd(50)));
};
const rowTemplates = { "en-US": "{amount} extra of {cap}", es: "{amount} extra de {cap}", it: "{amount} extra su {cap}", "pt-BR": "{amount} extra de {cap}" };
// `&amp;` last, so an escaped entity such as `&amp;#39;` is not decoded twice.
const decode = (html) => flat(html).replaceAll("&#39;", "'").replaceAll("&quot;", '"').replaceAll("&amp;", "&");

for (const { code } of i18n.LOCALES) {
  if (!expected[code]) continue;
  const want = { ...expected[code], row: rowOf(code, rowTemplates[code]) };
  await i18n.setLocale(code);

  // Billing states: the warning chip carries the core summary; the row is a figure, never a meter.
  const active = decode(lib.overageChipHtml(cases.active));
  assert.ok(active.includes("data-overage-chip") && active.includes(`>${want.chip}<`), `${code}: active chip ${active}`);
  assert.ok(active.includes('class="mini warn"'), `${code}: warning role`);
  const row = decode(lib.overageLineHtml(cases.active));
  assert.ok(row.includes(want.row), `${code}: active row ${row}`);
  assert.ok(!row.includes("class=\"bar"), `${code}: the row is not a meter`);
  assert.ok(lib.overageChipHtml(cases.capped).includes("data-overage-chip"), `${code}: capped chip`);

  // No chip unless money is billing; unknown shows the core words and "—", never $0.
  for (const quiet of ["unknown", "armed", "disabled"]) {
    assert.equal(lib.overageChipHtml(cases[quiet]), "", `${code}: ${quiet} has no chip`);
  }
  assert.equal(lib.overageLineHtml(cases.disabled), "", `${code}: disabled has no row`);
  assert.equal(lib.overageLineHtml(cases.armed), "", `${code}: armed has no row`);
  assert.ok(decode(lib.overageLineHtml(cases.unknown)).includes(want.unknown), `${code}: unknown row`);
  const unknownDetail = decode(detailHtml(cases.unknown));
  assert.ok(unknownDetail.includes(">—<"), `${code}: unknown amount is a dash`);
  assert.ok(!unknownDetail.includes("$0"), `${code}: unknown amount is never $0`);

  // Details: a section for every state, with the cap and the cycle end.
  const detail = decode(detailHtml(cases.active));
  assert.ok(detail.includes("data-overage-detail") && detail.includes(`<h3>${want.title}</h3>`), `${code}: details section`);
  assert.ok(decode(detailHtml(cases.disabled)).includes(want.off), `${code}: disabled details text`);
  assert.ok(decode(detailHtml(cases.armed)).includes("data-overage-detail"), `${code}: armed details`);

  // The overage metric is a textual figure in the list, not a meter that implies "left".
  const metricHtml = decode(metricCardHtml(cases.active, cases.active.metrics[1]));
  assert.ok(metricHtml.includes(want.row), `${code}: metric row ${metricHtml}`);
  assert.ok(!metricHtml.includes("class=\"bar"), `${code}: overage metric has no bar`);
  assert.ok(!metricHtml.includes('class="pct'), `${code}: overage metric has no left%`);

  // Claude keeps provider units and says so: no $ sign anywhere.
  const units = decode(detailHtml(cases.units) + lib.overageChipHtml(cases.units) + lib.overageLineHtml(cases.units));
  assert.ok(units.includes(want.caveat), `${code}: units caveat`);
  assert.ok(!units.includes("$"), `${code}: provider units never show a dollar sign`);

  // Overage never decides the card summary or the sort order.
  assert.equal(lib.worstRemaining(cases.active), 40, `${code}: worst remaining ignores overage`);
  console.log(`${code}: ${want.chip}; ${want.row}`);
}

assert.equal(lib.summaryOf(report(null, [extraMetric({})])).kind, "none", "an account with only overage metrics has no meter");
console.log("PASS: extra usage renders chip, row and details in pt-BR, en-US, es and it; overage never becomes a meter");
