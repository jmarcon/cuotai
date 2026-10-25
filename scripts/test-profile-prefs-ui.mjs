#!/usr/bin/env node
// Browser-free UI smoke for three desktop.json-backed behaviours:
//  - PRD 0019 RF7.7: "What's new" opens by itself after a version change (the shell decides, the UI asks once),
//    with a Settings > About switch (on by default) and search keywords in every catalog;
//  - PRD 0022 MONO-4: the monogram override of the account form (at most two characters, validated, sent with the
//    save, stored per account id);
//  - PRD 0022 RF15.1: a profile that existed before this build keeps its theme even when web storage was cleared.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  body: { append: () => {} },
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: () => {},
  querySelector: () => null,
};
globalThis.MutationObserver = class { observe() {} disconnect() {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.localStorage = {
  getItem: (key) => storage.get(key) ?? null,
  setItem: (key, value) => storage.set(key, value),
};
globalThis.fetch = async (relative) => {
  try {
    return { ok: true, json: async () => JSON.parse(await readFile(ui(relative), "utf8")) };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

/** A fresh copy of the modules, as a window opened with `search` in its address would load them. */
async function load(search) {
  globalThis.location = { search };
  return import(`../apps/desktop/ui/js/lib.js?${encodeURIComponent(search)}`);
}
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();

// ---- PRD 0019 RF7.7 ------------------------------------------------------------------------------------------------
{
  const quiet = await load("");
  assert.equal((await quiet.getSettings()).whats_new_due, false, "nothing is due without a version change");
  const lib = await load("?whats-new");
  let s = await lib.getSettings();
  assert.equal(s.whats_new_due, true, "a version change makes the notes due");
  assert.equal(s.whats_new_auto, true, "the switch is on by default");
  await lib.whatsNewSeen();
  assert.equal((await lib.getSettings()).whats_new_due, false, "shown once, not offered again");
  await lib.setWhatsNewAuto(false);
  assert.equal((await lib.getSettings()).whats_new_auto, false);
  await lib.setWhatsNewAuto(true);
  assert.equal((await lib.getSettings()).whats_new_auto, true);

  // The About page draws the switch (on by default, a real switch) and every catalog has its texts and keywords.
  const about = await import("../apps/desktop/ui/js/settings/about.js");
  for (const { code } of i18n.LOCALES) {
    await i18n.setLocale(code);
    const html = about.html({ settings: { version: "1.2.3", whats_new_auto: true }, update: null });
    assert.match(html, /<input id="whats-new-auto" type="checkbox" role="switch" class="sw" checked/, `${code}: switch on by default`);
    assert.match(html, /data-row="whats-new-auto" data-kw="[^"]{12,}"/, `${code}: search keywords`);
    assert.ok(!/settings\.whatsnew\./.test(html), `${code}: untranslated key`);
    assert.ok(!about.html({ settings: { version: "1.2.3", whats_new_auto: false }, update: null }).match(/id="whats-new-auto"[^>]*checked/), `${code}: switched off`);
  }
  await i18n.setLocale("en-US");

  // The main window opens the notes by itself: the first-run step's entry point asks, once per session.
  const firstRun = await readFile(ui("js/first-run.js"), "utf8");
  assert.match(firstRun, /import \{ maybeShowWhatsNew \}/);
  assert.match(firstRun, /void maybeShowWhatsNew\(\);/);
  const dialog = await readFile(ui("js/whats-new-dialog.js"), "utf8");
  assert.match(dialog, /if \(checked\) return false;/, "asked once per session");
  assert.match(dialog, /!settings\?\.whats_new_due \|\| settings\.whats_new_auto === false/, "due and the switch decide");
  assert.match(dialog, /querySelector\("dialog\[open\]"\)/, "another dialog is never covered");
}

// ---- PRD 0022 MONO-4 -----------------------------------------------------------------------------------------------
{
  const lib = await load("");
  for (const ok of ["", "WK", " w ", "é9", "日本", "🙂"]) assert.equal(lib.validMonogram(ok), true, `valid: ${JSON.stringify(ok)}`);
  for (const bad of ["abc", "a b", "a\u0007", "​​x", "🙂🙂🙂"]) assert.equal(lib.validMonogram(bad), false, `invalid: ${JSON.stringify(bad)}`);
  await assert.rejects(lib.setAccountMonogram("acc_demo00000000", "abc"), (e) => e === "invalid_monogram");
  await lib.setAccountMonogram("acc_demo00000000", "WK");
  assert.equal((await lib.getSettings()).monograms.acc_demo00000000, "WK", "stored per account id");
  await lib.setAccountMonogram("acc_demo00000000", "");
  assert.deepEqual((await lib.getSettings()).monograms, {}, "blank clears the override");
  // The form reads the override, the save sends it, and the shell refuses what the form lets through.
  const form = await lib.getAccount(0);
  assert.equal(form.monogram, "", "no override: derived from the name");
  assert.match(form.account_id, /^acc_/);
  await lib.saveAccount({ index: null, provider: "claude", name: "Home", fields: [], interval: "", group: "", monogram: "HM" });
  assert.equal(globalThis.window.__lastSaveMonogram, "HM");
  await assert.rejects(lib.saveAccount({ index: null, provider: "claude", name: "Other", fields: [], interval: "", group: "", monogram: "toolong" }), /invalid_monogram/);
  const dlg = await readFile(ui("js/account-dialog.js"), "utf8");
  assert.match(dlg, /name="monogram"/, "the account form has the field");
  assert.match(dlg, /monogram: String\(data\.get\("monogram"\) \?\? ""\)\.trim\(\)/, "the draft carries it");
  assert.match(dlg, /validMonogram\(new FormData\(form\)\.get\("monogram"\)\)/, "the form checks it before saving");
  for (const { code } of i18n.LOCALES) {
    const catalog = JSON.parse(await readFile(ui(`locales/${code}.json`), "utf8"));
    for (const key of ["dialog.monogram", "dialog.monogramPlaceholder", "dialog.monogramHint", "dialog.monogram.invalid", "desktop.error.invalid_monogram"]) {
      assert.ok(catalog[key], `${code}: ${key}`);
    }
  }
}

// ---- PRD 0022 RF15.1 -----------------------------------------------------------------------------------------------
{
  // The web storage was cleared: the pre-paint script saw a new install (Cuotai), but the shell says the profile
  // existed before this build, so the theme the user never chose is written as Dracula, once.
  storage.clear();
  const lib = await load("?profile-existed");
  assert.equal((await lib.getSettings()).profile_existed, true);
  assert.equal((await lib.getSettings()).theme, null);
  await lib.syncPreferences({ boot: true });
  assert.equal((await lib.getSettings()).theme, "dracula", "an existing profile keeps Dracula");
  // A stored choice is never overwritten.
  await lib.setPreference("theme", "win98");
  await lib.syncPreferences({ boot: true });
  assert.equal((await lib.getSettings()).theme, "win98");
  // A new install keeps the new default (nothing is written).
  storage.clear();
  const fresh = await load("?fresh-install");
  assert.equal((await fresh.getSettings()).profile_existed, false);
  await fresh.syncPreferences({ boot: true });
  assert.notEqual((await fresh.getSettings()).theme, "dracula", "a new install keeps the default theme");
}
console.log("PASS: What's new after a version change, monogram override and the existing-profile theme");
