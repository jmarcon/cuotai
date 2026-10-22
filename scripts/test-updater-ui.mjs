#!/usr/bin/env node
// Browser-free UI smoke for PRD 0018 RF3: render the real updater module with
// every built-in catalog and exercise the preference/policy switch boundary.
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
const updater = await import("../apps/desktop/ui/js/updater.js");
await i18n.initI18n();
await updater.initUpdates();
await new Promise((resolve) => setTimeout(resolve, 0));

const base = {
  enabled: true,
  automatic: true,
  disabled_reason: null,
  channel: "stable",
  current_version: "1.2.3",
  last_check: null,
  explanation_pending: false,
  phase: "available",
  version: "1.2.4",
  notes: "Fixes",
  date: "2026-10-22T12:00:00Z",
  can_install: true,
  upgrade_command: null,
  progress: null,
  dismissed: false,
  error: null,
  manual_url: "https://github.com/jmarcon/cuotai/releases/latest",
};

let expectedUpdateKeys;
for (const { code } of i18n.LOCALES) {
  const catalogPath = new URL(`apps/desktop/ui/locales/${code}.json`, root);
  const catalog = JSON.parse(await readFile(fileURLToPath(catalogPath), "utf8"));
  const updateKeys = Object.keys(catalog).filter((key) => key.startsWith("update.")).sort();
  expectedUpdateKeys ??= updateKeys;
  assert.deepEqual(updateKeys, expectedUpdateKeys, `${code}: updater catalog keys drifted`);

  await i18n.setLocale(code);
  const section = updater.updatesSectionHtml(base);
  assert.match(section, /id="update-automatic"/, `${code}: settings switch missing`);
  assert.ok(section.includes(catalog["update.title"]), `${code}: updater title did not render`);
  assert.ok(!section.includes("update."), `${code}: untranslated updater key rendered`);

  const banner = updater.updateBannerHtml();
  assert.match(banner, /data-update="available"/, `${code}: available banner missing`);
  assert.ok(banner.includes(catalog["update.now"]), `${code}: update action did not render`);

  const preferenceOff = updater.updatesSectionHtml({
    ...base,
    enabled: false,
    automatic: false,
    disabled_reason: "preference",
  });
  const preferenceSwitch = preferenceOff.match(/<input id="update-automatic"[^>]*>/)?.[0] ?? "";
  assert.ok(!preferenceSwitch.includes("disabled"), `${code}: preference-off switch cannot be re-enabled`);

  const policyOff = updater.updatesSectionHtml({
    ...base,
    enabled: false,
    disabled_reason: "env",
  });
  const policySwitch = policyOff.match(/<input id="update-automatic"[^>]*>/)?.[0] ?? "";
  assert.ok(policySwitch.includes("disabled"), `${code}: external policy switch must stay locked`);
  assert.ok(policyOff.includes("CUOTAI_UPDATER=off"), `${code}: environment override is not visible`);
}

console.log("PASS: updater Settings and banner render in pt-BR, en-US, es and it");
