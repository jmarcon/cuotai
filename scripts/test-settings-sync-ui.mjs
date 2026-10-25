#!/usr/bin/env node
// Browser-free UI smoke for PRD 0020 RF7: the preferences that used to live in each window's localStorage (theme,
// mode, language, animations, account order, grouping) are read from the shell's settings, a change made in one
// window reaches the others through `syncPreferences`, an invalid value is refused, and the old `storage` events
// are gone. The browser demo's settings store stands in for desktop.json.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = { open: () => {} };
globalThis.document = { hidden: false, body: { append: () => {} }, documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} }, addEventListener: () => {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
const storageListeners = [];
globalThis.addEventListener = (type, fn) => { if (type === "storage") storageListeners.push(fn); };
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (k) => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, String(v)) };
globalThis.fetch = async (relative) => {
  try {
    const data = JSON.parse(await readFile(fileURLToPath(new URL(`apps/desktop/ui/${relative}`, root)), "utf8"));
    return { ok: true, json: async () => data };
  } catch { return { ok: false, json: async () => ({}) }; }
};

const lib = await import("../apps/desktop/ui/js/lib.js");
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();

// A change made elsewhere (the shell's store changed under this window) is applied and reported by key.
storage.set("cuotai.themeMode", "light");
storage.set("cuotai.sort", "pct-asc");
storage.set("cuotai.group-recharging", "on");
storage.set("cuotai.locale", "es");
storage.set("cuotai.motion", "off");
let changed = await lib.syncPreferences();
assert.deepEqual([...changed].sort(), ["group_recharging", "locale", "motion", "sort", "theme_mode"]);
assert.equal(lib.theme.mode, "light");
assert.equal(lib.currentSort(), "pct-asc");
assert.equal(lib.groupingRecharging(), true);
assert.equal(i18n.currentLocale(), "es");
assert.equal(document.documentElement.dataset.motion, "off");
assert.equal((await lib.syncPreferences()).size, 0, "nothing differs, so nothing is reported");

// This window's own changes go through the one command and keep the mirror in step.
lib.setSort("binding");
lib.setGroupRecharging(false);
await new Promise((r) => setTimeout(r, 0));
assert.equal(storage.get("cuotai.sort"), "binding");
assert.equal(storage.get("cuotai.group-recharging"), "off");
await assert.rejects(lib.setPreference("sort", "sideways"), /invalid_preference/);
await assert.rejects(lib.setPreference("theme", "no spaces"), /invalid_preference/);
await assert.rejects(lib.setPreference("nope", true), /invalid_preference/);

// RF7.2: nothing listens to `storage` events for these preferences any more.
lib.initOrderSync(() => {});
lib.initTheme(() => {});
for (const fn of storageListeners) {
  storage.set("cuotai.sort", "reset-asc");
  fn({ key: "cuotai.sort", newValue: "reset-asc" });
}
assert.equal(lib.currentSort(), "binding", "a storage event no longer moves the order");

console.log("PASS: preferences are read from the shell, applied by key, validated, and no longer follow storage events");
process.exit(0); // the demo channel of the listeners keeps the event loop open
