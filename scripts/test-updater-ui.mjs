#!/usr/bin/env node
// Browser-free UI smoke for PRD 0018 RF3 and PRD 0022 RF17.2/RF17.4:
// render the production Settings About page and updater banner with every built-in catalog.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// A UTC-midnight release must keep its published day even west of UTC.
process.env.TZ = "America/Los_Angeles";

const root = new URL("../", import.meta.url);
const consentMode = process.argv[2] === "--consent" ? process.argv[3] : null;
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
globalThis.location = { search: consentMode === "managed" ? "?update=managed" : consentMode === "yes" ? "?update=consent&update-consent-race" : consentMode ? "?update=consent" : "" };
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
const aboutPage = await import("../apps/desktop/ui/js/settings/about.js");
await i18n.initI18n();
await updater.initUpdates();
await new Promise((resolve) => setTimeout(resolve, 0));

if (consentMode) {
  assert.ok(["close", "no", "yes", "managed"].includes(consentMode), `unknown consent mode: ${consentMode}`);
  document.hidden = true;
  const controls = new Map(
    ["[data-update-consent-close]", "[data-update-consent-no]", "[data-update-consent-yes]", "[data-update-consent-error]"]
      .map((selector) => [selector, {
        disabled: false,
        focused: false,
        onclick: null,
        textContent: "",
        focus() { this.focused = true; document.activeElement = this; },
      }]),
  );
  let dialogHtml = "";
  let removed = false;
  const closeListeners = [];
  const dialog = {
    open: false,
    isConnected: true,
    className: "",
    setAttribute: () => {},
    set innerHTML(html) { dialogHtml = html; },
    querySelector: (selector) => controls.get(selector) ?? null,
    addEventListener(type, callback) { if (type === "close") closeListeners.push(callback); },
    showModal() { this.open = true; },
    close() {
      this.open = false;
      for (const callback of closeListeners.splice(0)) callback();
    },
    remove() { removed = true; this.isConnected = false; },
    contains: () => false,
  };
  document.body = { append: (node) => assert.equal(node, dialog) };
  document.createElement = (tag) => {
    assert.equal(tag, "dialog");
    return dialog;
  };

  if (consentMode === "managed") {
    assert.equal(updater.updateState().consent_pending, false, "managed installs must not be asked");
    assert.equal(updater.updateState().enabled, false, "managed installs must keep internal checks off");
    assert.equal(await updater.promptForUpdateConsent(), false, "managed installs opened the consent dialog");
    assert.equal(dialog.open, false, "managed consent dialog unexpectedly opened");
    await assert.rejects(lib.updaterCheckNow(), (error) => error === "updater_off");
    console.log("PASS: updater consent stays hidden and requests stay off for managed installs");
    process.exit(0);
  }

  assert.equal(updater.updateState().consent_pending, true, "fresh profile must await an answer");
  assert.equal(updater.updateState().enabled, false, "fresh profile must keep checks disabled");
  await assert.rejects(lib.updaterCheckNow(), (error) => error === "updater_consent_required");
  assert.equal(await updater.promptForUpdateConsent(), true, "pending answer must open the dialog");
  assert.match(dialogHtml, /data-update-consent-yes/, "Yes action missing");
  assert.match(dialogHtml, /data-update-consent-no/, "No action missing");
  assert.ok(controls.get("[data-update-consent-no]").focused, "No must receive cautious initial focus");

  if (consentMode === "close") {
    controls.get("[data-update-consent-close]").onclick();
    await new Promise((resolve) => setTimeout(resolve, 0));
    assert.equal(updater.updateState().consent_pending, true, "closing must not answer");
    assert.equal(updater.updateState().enabled, false, "closing must keep every check off");
    await assert.rejects(lib.updaterCheckNow(), (error) => error === "updater_consent_required");
    assert.equal(await updater.promptForUpdateConsent(), false, "closing must not reopen in the same launch");
  } else {
    controls.get(`[data-update-consent-${consentMode}]`).onclick();
    await new Promise((resolve) => setTimeout(resolve, 0));
    const state = updater.updateState();
    assert.equal(state.consent_pending, false, `${consentMode}: answer was not recorded`);
    assert.equal(state.automatic, consentMode === "yes", `${consentMode}: automatic choice differs`);
    assert.equal(state.enabled, consentMode === "yes", `${consentMode}: request gate differs`);
    if (consentMode === "yes") {
      assert.equal(state.phase, "available", "newer updater event was overwritten by consent response");
      assert.match(updater.updateBannerHtml(), /data-update="available"/, "available update disappeared after consent");
    }
    if (consentMode === "no") {
      await assert.rejects(lib.updaterCheckNow(), (error) => error === "updater_off");
    }
  }
  assert.equal(dialog.open, false, `${consentMode}: dialog did not close`);
  assert.equal(removed, true, `${consentMode}: closed dialog was not removed`);
  console.log(`PASS: updater consent ${consentMode} path uses the production dialog and request gate`);
  process.exit(0);
}

// PRD 0020 RF2.5: Updates lives on the Settings window's About page; render that production page module.
const settingsPageHtml = async () => aboutPage.html({ settings: await lib.getSettings(), update: await lib.updaterStatus() });

const base = {
  enabled: true,
  automatic: true,
  disabled_reason: null,
  channel: "stable",
  current_version: "1.2.3",
  last_check: null,
  consent_pending: false,
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
  assert.deepEqual(headings, [catalog["update.title"]], `${code}: the About page must render exactly one updater heading`);
  renderedHeadings.push(`${code}: ${headings[0]}`);
  assert.match(section, /id="update-automatic"/, `${code}: settings switch missing`);
  assert.ok(!section.includes("update."), `${code}: untranslated updater key rendered`);
  const consent = updater.updateConsentDialogHtml();
  assert.ok(consent.includes(catalog["update.consent.title"]), `${code}: consent question missing`);
  assert.ok(consent.includes(lib.esc(catalog["update.consent.body"])), `${code}: consent disclosure missing`);
  assert.ok(consent.includes(lib.esc(catalog["update.consent.no"])), `${code}: No action missing`);
  assert.ok(consent.includes(lib.esc(catalog["update.consent.yes"])), `${code}: Yes action missing`);
  assert.match(consent, /data-update-consent-close[^>]*aria-label=/, `${code}: close action lacks an accessible name`);
  assert.ok(
    consent.indexOf("data-update-consent-no") < consent.indexOf("data-update-consent-yes"),
    `${code}: primary Yes action must be last`,
  );

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
  const pending = updater.updatesSectionHtml({
    ...base,
    enabled: false,
    automatic: false,
    consent_pending: true,
  });
  const pendingSwitch = pending.match(/<input id="update-automatic"[^>]*>/)?.[0] ?? "";
  assert.ok(pendingSwitch.includes("disabled"), `${code}: unanswered switch must stay locked`);
  assert.ok(pending.includes(catalog["update.consent.pending"]), `${code}: pending answer is not visible`);

  const policyOff = updater.updatesSectionHtml({
    ...base,
    enabled: false,
    disabled_reason: "env",
  });
  const policySwitch = policyOff.match(/<input id="update-automatic"[^>]*>/)?.[0] ?? "";
  assert.ok(policySwitch.includes("disabled"), `${code}: external policy switch must stay locked`);
  assert.ok(policyOff.includes("CUOTAI_UPDATER=off"), `${code}: environment override is not visible`);
}

const consentSmokes = [];
for (const mode of ["close", "no", "yes", "managed"]) {
  const child = spawnSync(process.execPath, [fileURLToPath(import.meta.url), "--consent", mode], {
    encoding: "utf8",
  });
  assert.equal(child.status, 0, `${mode} consent smoke failed:\n${child.stdout}\n${child.stderr}`);
  consentSmokes.push(child.stdout.trim());
}
console.log(consentSmokes.join("\n"));

console.log(`Rendered updater headings: ${renderedHeadings.join(" | ")}`);
console.log(`Rendered release dates: ${renderedDates.join(" | ")}`);
console.log("PASS: updater Settings renders one heading and a localized release date in pt-BR, en-US, es and it");
