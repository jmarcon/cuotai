#!/usr/bin/env node
// Browser-free UI smoke for PRD 0045 RF6.1: the details' Breakdown section (DS-CMP-SUB rows, a meter only for a row
// with a cap, the "Others" row and the truncation note, both sort orders) and the card's "Top: …" line, rendered from
// the backend's view in every built-in catalog without an untranslated key. Fixtures: 0, 5 and 26 rows.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const read = async (relative) => JSON.parse(await readFile(fileURLToPath(new URL(relative, root)), "utf8"));
const storage = new Map();
const registry = await read("apps/desktop/ui/locales/en-US.json");
const locales = ["en-US", "pt-BR", "es", "it", "fr", "de", "ja", "ko", "zh-CN", "ru", "tr", "vi", "id-ID", "hi-IN"];
globalThis.window = {
  __TAURI__: {
    core: {
      invoke: async (_command, { locale }) => (locale === "en-US" ? {} : read(`crates/cuotai-core/locales/${locale}.json`)),
    },
  },
};
const listeners = [];
globalThis.document = {
  hidden: false,
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: (type, fn) => { if (type === "click") listeners.push(fn); },
};
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
const { detailHtml } = await import("../apps/desktop/ui/js/detail.js");
const { breakdownHtml, orderOf } = await import("../apps/desktop/ui/js/breakdown.js");
await i18n.initI18n();
assert.ok(registry["breakdown.title"], "the English catalog has the section title");

const text = (id, fallback, params = {}) => ({ id, params, fallback });
const verbatim = (label) => ({ id: label, params: {}, fallback: label });
const spend = (used) => ({ label: "Gasto no mês", label_text: text("metric.label.month_spend", "Spending in the month"), used, limit: null, remaining: null, unit: "USD", kind: "spend", window: "monthly", state: "available", uncapped: true, ratio: null, resets_at: null });
const capped = (used, limit) => ({ label: "Limite da chave", label_text: text("metric.label.key_limit", "Key limit"), used, limit, remaining: limit - used, unit: "USD", kind: "quota", window: "monthly", state: "available", uncapped: false, ratio: (limit - used) / limit, resets_at: "2099-11-01T00:00:00Z", resets_label: text("resets.pair", "Resets 00:00 · in 3d", {}) });
const row = (id, label, figure, extra = {}) => ({ scope: "key", id, label: verbatim(label), others: false, figure, metrics: [figure], notes: [], ...extra });
const report = (breakdown) => ({
  provider: "open_router", account: "Org", status: "ready", class: "available", binding: null, metrics: [spend(1)], notes: [], notes_l10n: [],
  usage_alerts: [], checked_at: new Date().toISOString(), freshness: "fresh", clock_adjusted: false, last_good: null, pending: false,
  access: null, incident: null, session_expiry: null, identity: { email: null, badges: [], copy: null }, overage: null, scope_blocks: [],
  ...(breakdown ? { breakdown } : {}),
});

// 0 rows: no section at all.
assert.equal(breakdownHtml(report(null)), "");
assert.ok(!detailHtml(report(null)).includes("data-breakdown"));

// 5 rows: one with a cap draws a meter; the others their amount; a disabled key carries its note.
const five = {
  rows: [
    row("a", "Proj-A", spend(412)),
    row("b", "Proj-B", capped(92, 100)),
    row("c", "Proj-C", spend(30)),
    row("d", "ci …3456", spend(5), { notes: [text("note.breakdown.disabled", "Disabled on the provider: it cannot be used until it is enabled again.")] }),
    row("e", "Proj-E", spend(1)),
  ],
  by_percent: [1, 0, 2, 3, 4],
  top: text("breakdown.top", "Top: Proj-A $412 · Proj-B $92", { items: "Proj-A $412 · Proj-B $92" }),
  truncated: null,
};
for (const locale of locales) {
  await i18n.setLocale?.(locale);
  const html = detailHtml(report(five), { eye: true });
  assert.ok(html.includes("data-breakdown"), locale);
  assert.equal((html.match(/data-breakdown-row=/g) ?? []).length, 5, locale);
  assert.equal((html.match(/class="gauge"/g) ?? []).length, 1, `${locale}: only the capped row draws a meter`);
  assert.ok(html.includes("8%"), `${locale}: the capped row shows its remaining share`);
  assert.ok(html.includes("data-breakdown-sort=\"percent\""), locale);
  assert.ok(!/breakdown\.(title|sort)/.test(html), `${locale}: no untranslated key`);
  assert.ok(!html.includes("@"), `${locale}: no address`);
}
await i18n.setLocale?.("en-US");
assert.deepEqual(orderOf(five, "percent"), [1, 0, 2, 3, 4]);
assert.deepEqual(orderOf(five, "spend"), [0, 1, 2, 3, 4]);
assert.deepEqual(orderOf({ rows: five.rows, by_percent: [0] }, "percent"), [0, 1, 2, 3, 4], "a malformed order falls back to spend");

// 26 rows: 24 named rows, the "Others" row last and the truncation note.
const many = {
  rows: [
    ...Array.from({ length: 24 }, (_, i) => row(`k${i}`, `Key ${i}`, spend(100 - i))),
    row("others", "Others (2)", spend(3), { others: true, label: text("breakdown.others", "Others (2)", { count: "2" }) }),
  ],
  by_percent: Array.from({ length: 25 }, (_, i) => i),
  top: null,
  truncated: text("breakdown.truncated", "Showing the top 24 of 26; the rest are summed in Others.", { shown: "24", total: "26" }),
};
const html = breakdownHtml(report(many));
assert.equal((html.match(/data-breakdown-row=/g) ?? []).length, 25);
assert.ok(html.includes("brk-row others"));
assert.ok(html.includes("Showing the top 24 of 26"));

// The sort toggle re-orders the section's rows by their position in the chosen order.
const node = (dataset) => ({ dataset, attrs: {}, setAttribute(k, v) { this.attrs[k] = v; } });
const rows = [node({ breakdownRow: "a", so: "0", po: "1" }), node({ breakdownRow: "b", so: "1", po: "0" })];
const list = { children: rows.slice(), append(n) { this.children = this.children.filter((x) => x !== n).concat(n); } };
const buttons = [node({ breakdownSort: "spend" }), node({ breakdownSort: "percent" })];
const section = { querySelector: () => list, querySelectorAll: () => buttons };
const click = { closest: (sel) => (sel === "[data-breakdown-sort]" ? click : section), dataset: { breakdownSort: "percent" } };
for (const fn of listeners) fn({ target: click });
assert.deepEqual(list.children.map((n) => n.dataset.breakdownRow), ["b", "a"]);
assert.equal(buttons[1].attrs["aria-pressed"], "true");

// The card shows "Top: …" as one line with the sub-limits element: Detailed shows it, Standard leaves it out.
const { report: cardReport } = await import("./ui-test-env.mjs");
const components = await import("../apps/desktop/ui/js/components.js");
const density = await import("../apps/desktop/ui/js/density.js");
const card = { ...cardReport({ provider: "open_router", account: "Org" }), breakdown: five };
density.applyRowLayout({ preset: "detailed" });
assert.match(components.rowHtml(card, { title: "OpenRouter" }), /data-breakdown-top>Top: Proj-A \$412 · Proj-B \$92</);
density.applyRowLayout({ preset: "standard" });
assert.ok(!components.rowHtml(card, { title: "OpenRouter" }).includes("data-breakdown-top"));
density.applyRowLayout({ preset: "detailed" });
assert.ok(!components.rowHtml(cardReport({}), { title: "Claude" }).includes("data-breakdown-top"), "no line without a breakdown");

console.log("breakdown UI: ok");
