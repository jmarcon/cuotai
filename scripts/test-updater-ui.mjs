#!/usr/bin/env node
// Browser-free UI smoke for PRD 0018 RF3 and PRD 0022 RF17.2/RF17.4:
// render the production Settings page and updater banner with every built-in catalog.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

// A UTC-midnight release must keep its published day even west of UTC.
process.env.TZ = "America/Los_Angeles";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  body: { append: () => {} },
  activeElement: null,
  documentElement: {
    lang: "",
    dataset: {},
    toggleAttribute: () => {},
  },
  addEventListener: () => {},
};
document.activeElement = document.body;
globalThis.MutationObserver = class {
  observe() {}
  disconnect() {}
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
const lib = await import("../apps/desktop/ui/js/lib.js");
const settings = await import("../apps/desktop/ui/js/settings.js");
await i18n.initI18n();
await updater.initUpdates();
await new Promise((resolve) => setTimeout(resolve, 0));
const SETTINGS_CAPTURED = Symbol("settings-captured");
let capturedSettingsHtml = "";
document.createElement = (tag) => {
  assert.equal(tag, "dialog", "Settings must create a dialog");
  return {
    open: false,
    className: "",
    addEventListener: () => {},
    showModal: () => {},
    set innerHTML(html) {
      capturedSettingsHtml = html;
      throw SETTINGS_CAPTURED;
    },
  };
};
const settingsPageHtml = async () => {
  capturedSettingsHtml = "";
  document.hidden = true;
  try {
    await settings.openSettings();
    assert.fail("Settings rendered without capturing its production HTML");
  } catch (error) {
    assert.equal(error, SETTINGS_CAPTURED, "Settings failed before rendering its production HTML");
  } finally {
    document.hidden = false;
  }
  return capturedSettingsHtml;
};

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
  date: "2026-10-22T00:00:00Z",
  can_install: true,
  upgrade_command: null,
  progress: null,
  dismissed: false,
  error: null,
  manual_url: "https://github.com/jmarcon/cuotai/releases/latest",
};

const expectedReleaseDates = {
  "pt-BR": "publicada em 22 de out. de 2026",
  "en-US": "released on Oct 22, 2026",
  es: "publicada el 22 oct 2026",
  it: "pubblicato il 22 ott 2026",
};

let expectedUpdateKeys;
const renderedHeadings = [];
const renderedDates = [];
for (const { code } of i18n.LOCALES) {
  const catalogPath = new URL(`apps/desktop/ui/locales/${code}.json`, root);
  const catalog = JSON.parse(await readFile(fileURLToPath(catalogPath), "utf8"));
  const updateKeys = Object.keys(catalog).filter((key) => key.startsWith("update.")).sort();
  expectedUpdateKeys ??= updateKeys;
  assert.deepEqual(updateKeys, expectedUpdateKeys, `${code}: updater catalog keys drifted`);

  await i18n.setLocale(code);
  assert.equal(lib.fmtDate(null), null, `${code}: absent release date must stay absent`);
  assert.equal(lib.fmtDate("not-a-date"), null, `${code}: invalid release date must stay absent`);
  const section = updater.updatesSectionHtml(base);
  const page = await settingsPageHtml();
  const headings = Array.from(page.matchAll(/<h3>([^<]*)<\/h3>/g), (match) => match[1])
    .filter((heading) => heading === catalog["update.title"]);
  assert.match(section, /^<section class="set-sec" data-updates>/, `${code}: updater section wrapper missing`);
  assert.deepEqual(headings, [catalog["update.title"]], `${code}: Settings must render exactly one updater heading`);
  renderedHeadings.push(`${code}: ${headings[0]}`);
  assert.match(section, /id="update-automatic"/, `${code}: settings switch missing`);
  assert.ok(!section.includes("update."), `${code}: untranslated updater key rendered`);

  const banner = updater.updateBannerHtml();
  assert.match(banner, /data-update="available"/, `${code}: available banner missing`);
  assert.ok(banner.includes(catalog["update.now"]), `${code}: update action did not render`);

  const renderedDate = banner.match(/<span class="hint">([^<]+)<\/span>/)?.[1] ?? "";
  assert.equal(renderedDate, expectedReleaseDates[code], `${code}: release date is not localized`);
  assert.doesNotMatch(banner, /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z/, `${code}: raw ISO release timestamp rendered`);
  const updateState = updater.updateState();
  const releaseDate = updateState.date;
  for (const date of [null, "not-a-date"]) {
    updateState.date = date;
    assert.doesNotMatch(
      updater.updateBannerHtml(),
      /<span class="hint">/,
      `${code}: ${date == null ? "absent" : "invalid"} release date rendered a label`,
    );
  }
  updateState.date = releaseDate;
  renderedDates.push(`${code}: ${renderedDate}`);
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

console.log(`Rendered updater headings: ${renderedHeadings.join(" | ")}`);
console.log(`Rendered release dates: ${renderedDates.join(" | ")}`);
console.log("PASS: updater Settings renders one heading and a localized release date in pt-BR, en-US, es and it");
