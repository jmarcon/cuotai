#!/usr/bin/env node
// Browser-free UI smoke for PRD 0025 RF4, RF5 and RF6: Settings › Menu bar renders the four style cards, the colored
// switch and the account list in every built-in catalog (with search keywords); keyboard reordering moves one place;
// the preview never draws an unknown share as a fill; the popover splits the menu-bar accounts from the others; and the
// first-run step lists what it turns on and asks the update question separately.
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
const menubar = await import("../apps/desktop/ui/js/settings/menubar.js");
const firstRun = await import("../apps/desktop/ui/js/first-run.js");
await i18n.initI18n();

// Catalog parity: every new key in the four catalogs.
const catalogs = {};
for (const { code } of i18n.LOCALES) catalogs[code] = JSON.parse(await readFile(ui(`locales/${code}.json`), "utf8"));
const ours = Object.keys(catalogs["en-US"]).filter((k) => /^(settings\.menubar|settings\.setup|settings\.page\.menubar|firstrun\.)/.test(k));
assert.ok(ours.length > 20, "menu-bar and first-run keys missing");
for (const [code, cat] of Object.entries(catalogs)) for (const k of ours) assert.ok(cat[k], `${code}: ${k} missing`);

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const settings = await lib.getSettings();
  const out = menubar.html({ settings });
  for (const style of menubar.STYLES) assert.ok(out.includes(`data-value="${style}"`), `${code}: ${style} card missing`);
  assert.match(out, /role="radiogroup" id="tray-style"/);
  assert.match(out, /aria-checked="true"[^>]*>\s*<span class="opt-preview" data-tray-preview="tightest_percent"/, `${code}: Tightest % must be the default`);
  assert.match(out, /id="tray-colored"[^>]*role="switch"|role="switch"[^>]*id="tray-colored"|id="tray-colored" type="checkbox" role="switch"/);
  assert.ok(!/id="tray-colored"[^>]*checked/.test(out), `${code}: color is off by default`);
  // The rows carry the catalog's search keywords (`settings.kw.<row>`) like every other Settings row.
  assert.ok(out.includes(`data-kw="${lib.esc(catalogs[code]["settings.kw.tray-style"])}"`), `${code}: search keywords missing`);
  assert.ok(!out.includes("data-keywords"), `${code}: no ad-hoc keyword attribute`);
  assert.ok(!/settings\.menubar\./.test(out), `${code}: untranslated key`);
}
await i18n.setLocale("en-US");

// RF4.2: keyboard reordering moves one place and stops at the ends.
assert.deepEqual(menubar.moved(["a", "b", "c"], "b", -1), ["b", "a", "c"]);
assert.deepEqual(menubar.moved(["a", "b", "c"], "c", 1), ["a", "b", "c"]);
const accounts = [{ key: "a", name: "Work", provider_label: "Claude" }, { key: "b", name: "Home", provider_label: "Gemini" }];
const custom = menubar.accountsHtml(accounts, ["b"], false);
assert.ok(custom.indexOf('data-key="b"') < custom.indexOf('data-key="a"'), "the menu-bar accounts come first, in their order");
assert.match(custom, /Not in menu bar/);
assert.match(custom, /data-grip/);
assert.ok(!menubar.accountsHtml(accounts, ["a", "b"], true).includes("data-grip"), "Automatic has no handles");

// RF4.1 / DS-CMP-MTR-5: an unknown share draws no fill; an exhausted one is an outline; the title is escaped text.
const meters = menubar.previewHtml({ style: "meters", items: [{ remaining: null }, { remaining: 0, exhausted: true }, { remaining: 0.004, level: "bad" }] }, false);
assert.match(meters, /mb-bar unknown"><span style="width:0%"/);
assert.match(meters, /mb-bar out"><span style="width:0%"/);
assert.match(meters, /mb-bar bad"><span style="width:6%"/, "a sliver below 1 %");
assert.match(menubar.previewHtml({ style: "tightest_percent", items: [], title: "<b>", badge: "outage" }, false), /&lt;b&gt;.*|mb-badge outage/);

// RF6: the step lists the recommended notices, names the Advanced features without turning them on, and asks the
// update question with its own buttons only while it is unanswered.
const step = firstRun.stepHtml({ updatePending: true });
assert.match(step, /data-first-on/);
assert.match(step, /data-first-later/);
assert.match(step, /data-first-update-yes/);
assert.match(step, /data-first-update-no/);
assert.ok(!firstRun.stepHtml({ updatePending: false }).includes("data-first-update-yes"));
// RF6.5: a managed install gets the package manager's command instead of the question.
const managed = firstRun.stepHtml({ updatePending: false, managed: { reason: "homebrew", command: "brew upgrade --cask cuotai" } });
assert.match(managed, /brew upgrade --cask cuotai/);
assert.match(managed, /Homebrew/);
assert.ok(!managed.includes("data-first-update-yes"), "nothing is asked");
assert.equal(firstRun.due({ onboarding: 0, onboarding_version: 1 }, { account_count: 1 }), true);
assert.equal(firstRun.due({ onboarding: 1, onboarding_version: 1 }, { account_count: 3 }), false);
assert.equal(firstRun.due({ onboarding: 0, onboarding_version: 1 }, { account_count: 0 }), false, "the step waits for the first account");

// RF2.3/RF4.3: the popover reads the selection by stable id and keeps its view in desktop.json.
const tray = await readFile(ui("js/tray.js"), "utf8");
assert.match(tray, /r\.account_id === k/);
assert.ok(!tray.includes("localStorage.setItem(VIEW_KEY"), "the popover view no longer lives in localStorage");

console.log("PASS: Settings › Menu bar, the popover split and the first-run step render in pt-BR, en-US, es and it");
