#!/usr/bin/env node
// Browser-free UI smoke for PRD 0025 RF9 and PRD 0022 RF8.2-RF8.3 (DS-CMP-DENS-1..5): each preset maps to its
// element values, a switch change selects Custom, no layout hides the figure or the meter, the accessible name
// keeps every element, Minimal shows one status by priority, rows.window applies only when separate, and the
// Row display sub-page renders translated in the four catalogs.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const storage = new Map();
globalThis.window = { open: () => {} };
globalThis.document = {
  hidden: false,
  body: { append: () => {} },
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: () => {},
};
globalThis.MutationObserver = class { observe() {} disconnect() {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (k) => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, v), removeItem: (k) => storage.delete(k) };
globalThis.fetch = async (relative) => {
  try {
    const data = JSON.parse(await readFile(ui(relative), "utf8"));
    return { ok: true, json: async () => data };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const d = await import("../apps/desktop/ui/js/density.js");
const c = await import("../apps/desktop/ui/js/components.js");
const rowdisplay = await import("../apps/desktop/ui/js/settings/rowdisplay.js");
await i18n.initI18n();

// DS-CMP-DENS-1: the preset values.
const std = d.resolve({ preset: "standard" }).elements;
assert.equal(std.sublimits, false);
assert.equal(std.age, false);
assert.ok(d.ELEMENTS.filter((k) => !["sublimits", "age"].includes(k)).every((k) => std[k]), "Standard shows the rest");
assert.ok(d.ELEMENTS.every((k) => d.resolve({ preset: "detailed" }).elements[k]), "Detailed shows everything");
const min = d.resolve({ preset: "minimal" });
assert.ok(min.one, "Minimal shows one status");
assert.deepEqual(d.ELEMENTS.filter((k) => min.elements[k]), ["monogram", "status", "runs_out", "returns", "incident"]);
assert.equal(d.resolve(null).preset, "standard", "absent = Standard");
assert.equal(d.resolve({ preset: "huge" }).preset, "standard");
// RF9.2: a switch change selects Custom from the current values; a preset resets the switches.
const custom = d.toggled({ preset: "minimal" }, "unit", true);
assert.equal(custom.preset, "custom");
assert.equal(custom.elements.unit, true);
assert.equal(custom.elements.plan, false, "starts from the preset's values");
assert.deepEqual(d.choosePreset("detailed"), { preset: "detailed" });
assert.deepEqual(d.toggled({ preset: "standard" }, "figure", false), { preset: "standard" }, "the figure is not a switch");
// RF9.5: rows.window only with the separate switch.
const rows = { tray: { preset: "minimal" }, window: { preset: "detailed" }, separate: false };
assert.equal(d.layoutFor(rows, "window").preset, "minimal");
assert.equal(d.layoutFor({ ...rows, separate: true }, "window").preset, "detailed");
assert.equal(d.layoutFor({ ...rows, separate: true }, "tray").preset, "minimal");
// DS-CMP-DENS-3: Minimal's priority table.
const cases = [
  [{ incident: true, stale: true, exhausted: true, runsOut: true, pace: true }, "incident"],
  [{ stale: true, exhausted: true, runsOut: true, pace: true }, "stale"],
  [{ exhausted: true, runsOut: true, pace: true }, "exhausted"],
  [{ runsOut: true, pace: true }, "runs_out"],
  [{ pace: true }, "pace"],
  [{}, null],
];
for (const [facts, kind] of cases) assert.equal(d.oneStatus(facts), kind, JSON.stringify(facts));

// RF9.3/RF9.4: every combination keeps the figure and the meter, and the accessible name stays whole.
const view = await lib.getView();
const r = view.reports.find((x) => x.status === "ready" && x.binding);
const name = (html) => html.match(/aria-label="([^"]*)"/)[1];
d.applyRowLayout({ preset: "detailed" });
const full = name(c.rowHtml(r, { title: "T", sub: "S" }));
for (const layout of [{ preset: "standard" }, { preset: "minimal" }, { preset: "custom", elements: Object.fromEntries(d.ELEMENTS.map((k) => [k, false])) }]) {
  d.applyRowLayout(layout);
  const html = c.rowHtml(r, { title: "T", sub: "S" });
  assert.match(html, /class="fig/, `${layout.preset}: figure`);
  assert.match(html, /role="meter"/, `${layout.preset}: meter`);
  assert.equal(name(html), full, `${layout.preset}: accessible name`);
  // The CSS hook never names the figure or the meter.
  assert.ok(!/fig|meter/.test(document.documentElement.dataset.rowsHide), layout.preset);
}
d.applyRowLayout({ preset: "minimal" });
assert.equal((c.rowHtml(r, { title: "T" }).match(/class="stl[ "]/g) ?? []).length <= 1, true, "one status line");
assert.ok(!c.rowHtml(r, { title: "T" }).includes("xline"), "Minimal has no extra line");
d.applyRowLayout({ preset: "standard" });

// The sub-page in the four catalogs.
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const out = rowdisplay.rowsHtml({ tray: { preset: "custom", elements: { unit: false } }, separate: true });
  for (const k of d.ELEMENTS) assert.ok(out.includes(`data-rows-el="${k}"`), `${code}: ${k} switch`);
  assert.ok(out.includes('id="rows-scope"'), `${code}: scope choice when separate`);
  assert.ok(!/settings\.rows\./.test(out), `${code}: untranslated key`);
  assert.ok(/data-row="rows"[^>]*data-kw="[^"]+"/.test(out), `${code}: search keywords`);
}
console.log("density: presets, Custom, priority, accessible names and the sub-page OK");
