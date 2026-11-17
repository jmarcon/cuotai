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

// Catalog parity: every new key in every built-in catalog.
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
const accounts = [{ key: "a", name: "Work", provider: "claude", provider_label: "Claude", remaining: 0.22 }, { key: "b", name: "Home", provider: "gemini", provider_label: "Gemini", remaining: 0.74, stale: true }];
const custom = menubar.accountsHtml(accounts, ["b"], false);
assert.ok(custom.indexOf('data-key="b"') < custom.indexOf('data-key="a"'), "the menu-bar accounts come first, in their order");
assert.match(custom, /Not in menu bar/);
assert.match(custom, /data-grip/);
assert.ok(!menubar.accountsHtml(accounts, ["a", "b"], true).includes("data-grip"), "Automatic has no handles");
// PRD 0025 re-validation V-08: an account switched off in the dashboard cannot be picked and says why.
const off = menubar.accountsHtml(accounts, ["a", "b"], true, new Set(["b"]));
assert.match(off, /data-tray-pick="b"[^>]*disabled/, "a hidden account's check is disabled");
assert.match(off, /Hidden from the dashboard/);
assert.ok(!/data-tray-pick="a"[^>]*disabled/.test(off), "the others stay enabled");

// RF2.3 (re-validation V-19): the popover lists the menu-bar accounts first, in its order; the rest apart; the demo
// view carries the account ids the split reads.
const reps = [{ account_id: "acc_a" }, { account_id: "acc_b" }, { account_id: "acc_c" }];
const split = lib.splitByMenuBarKeys(reps, ["acc_c", "acc_gone", "acc_a"]);
assert.deepEqual(split.inBar.map((r) => r.account_id), ["acc_c", "acc_a"]);
assert.deepEqual(split.rest.map((r) => r.account_id), ["acc_b"]);
assert.equal(lib.splitByMenuBarKeys(reps, []).rest.length, 0, "nothing selected: one list");
assert.equal(lib.splitByMenuBarKeys(reps, ["acc_a", "acc_b", "acc_c"]).rest.length, 0, "all in the bar: one list");
assert.ok((await lib.getView()).reports.every((r) => /^acc_demo\d{8}$/.test(r.account_id)), "the demo sends account ids");
// Re-validation V-05: with the shell's image the preview draws that image (a canvas), with the title beside it.
const drawn = menubar.previewHtml({ style: "icon_only", items: [], title: "22% · Claude", image: { width: 44, height: 44, template: true, rgba: "AAAA" } }, false);
assert.match(drawn, /<canvas class="mb-img"[^>]*data-template="1"/);
assert.match(drawn, /22% · Claude/);
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
const managed = firstRun.stepHtml({ updatePending: false, managed: { reason: "homebrew", command: "brew upgrade --cask cuotai-app" } });
assert.match(managed, /brew upgrade --cask cuotai-app/);
assert.match(managed, /Homebrew/);
assert.ok(!managed.includes("data-first-update-yes"), "nothing is asked");
assert.equal(firstRun.due({ onboarding: 0, onboarding_version: 1 }, { account_count: 1 }), true);
assert.equal(firstRun.due({ onboarding: 1, onboarding_version: 1 }, { account_count: 3 }), false);
assert.equal(firstRun.due({ onboarding: 0, onboarding_version: 1 }, { account_count: 0 }), false, "the step waits for the first account");

// PRD 0036 RF5.3 (V-05): the menu bar's items are units, as the core's `tray::summary_units` decides them. The demo
// 9router "home" is not an item; its active connections are (the "team" one, off in 9router, is not), keyed by the
// aggregator's id; its id picks them in a custom selection; "Aggregator connections in All: Hidden" drops them.
{
  const view = await lib.getView();
  const home = view.reports.find((r) => r.aggregator && r.account === "home");
  assert.ok(home, "the demo has its 9router instance");
  const items = await lib.trayAccounts();
  assert.ok(!items.some((a) => a.key === home.account_id), "an aggregator with active connections is not an item");
  const conns = items.filter((a) => a.parent === home.account_id);
  assert.deepEqual(conns.map((a) => a.name), ["work", "personal", "zhipu"], "its active connections are items, the off one is not");
  assert.ok(conns.every((a) => a.key.startsWith(`${home.account_id}/`)));
  assert.equal(conns.find((a) => a.name === "personal").exhausted, true);
  const picked = await lib.trayPreview("logos_percent", [home.account_id]);
  assert.equal(picked.items.length, 3, "the aggregator's id picks its connections");
  await lib.setAggregatorsInAll("hidden");
  assert.ok(!(await lib.trayAccounts()).some((a) => a.parent || a.key === home.account_id), "Hidden leaves the aggregators out");
  await lib.setAggregatorsInAll("grouped");
}

// RF2.3/RF4.3: the popover reads the selection by stable id and keeps its view in desktop.json.
const tray = await readFile(ui("js/tray.js"), "utf8");
assert.match(tray, /splitByMenuBarKeys\(/, "the popover splits through lib.js, which reads the stable id");
assert.match(await readFile(ui("js/lib.js"), "utf8"), /r\.account_id === k/);
assert.ok(!tray.includes("localStorage.setItem(VIEW_KEY"), "the popover view no longer lives in localStorage");

// PRD 0043 RF4.2: the 24-hour strip is a macOS style only; its preview scales each hour to the busiest one, keeps a
// gap faint (never a zero bar) and marks a reset; without history it shows faint columns only.
assert.deepEqual(menubar.stylesFor(false), menubar.STYLES);
assert.deepEqual(menubar.stylesFor(true), [...menubar.STYLES, "hourly_strip"]);
const hours = [{ consumption: 4 }, { consumption: 2, reset: true }, { consumption: null }, { consumption: 0 }];
const stripPreview = menubar.previewHtml({ style: "hourly_strip", items: [{ level: "bad" }] }, true, { rhythm: { buckets: hours } });
assert.match(stripPreview, /mb-strip colored bad/);
assert.match(stripPreview, /mb-col "><i style="height:100%"/);
assert.match(stripPreview, /mb-col  reset"><i style="height:50%"/);
assert.match(stripPreview, /mb-col gap"><i style="height:0%"/);
assert.match(stripPreview, /mb-col zero"><i style="height:0%"/);
const emptyStrip = menubar.previewHtml({ style: "hourly_strip", items: [] }, false, null);
assert.equal((emptyStrip.match(/mb-col gap/g) ?? []).length, 24);
// The popover strip sits under the account rows, collapsible, its state kept on this device.
assert.match(tray, /data-pop-strip/);
assert.match(tray, /rhythmChartHtml\(strip\.rhythm/);

console.log("PASS: Settings › Menu bar, the popover split and the first-run step render in every built-in language");
