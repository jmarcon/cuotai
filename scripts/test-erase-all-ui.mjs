#!/usr/bin/env node
// Browser-free UI smoke for PRD 0052 RF5.3 and RF7.2: Settings › Data & privacy › Erase all Cuotai data lists the
// complete ordered inventory (what is erased and what is kept), the accounts-file box starts off, the confirm button
// stays disabled until the exact word is typed, a partial erase lists only ids with Try again and Quit, and the main
// window shows the core's clock-skew advice as a banner while the clocks disagree.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const erase = await import("../apps/desktop/ui/js/settings/erase.js");
const data = await import("../apps/desktop/ui/js/settings/data.js");
await i18n.initI18n?.();

// The row closes the collapsed "Export and erase" group of the Data & privacy page.
const page = data.html({ settings: { history_days: 90, history_days_min: 7, history_days_max: 365 }, view: () => null });
const group = page.slice(page.indexOf('data-disc="history-data"'));
assert.ok(group.indexOf('data-row="erase-all"') > group.indexOf('data-row="erase"'), "after the history's own erase");
const rowHtml = erase.eraseHtml();
assert.match(rowHtml, /data-row="erase-all"/);
assert.match(rowHtml, /data-erase-all\b/);
assert.match(rowHtml, /role="alertdialog"[^>]*hidden/, "the confirmation starts closed");

// The inventory: every item of the plan, in its order, erased ones first and the kept accounts file apart.
const plan = await lib.eraseAllPlan();
assert.ok(plan.word && plan.items.length >= 4);
let html = erase.erasePanelHtml(plan);
const list = (attr) => (html.match(new RegExp(`<ul class="hook-runs" ${attr}>([\\s\\S]*?)</ul>`)) ?? [])[1] ?? "";
const erased = plan.items.filter((i) => i.erase);
const kept = plan.items.filter((i) => !i.erase);
for (const item of erased) assert.match(list("data-erase-list"), new RegExp(item.id.replace(".", "\\.")), item.id);
for (const item of kept) {
  assert.match(list("data-erase-kept"), new RegExp(item.id.replace(".", "\\.")), item.id);
  assert.doesNotMatch(list("data-erase-list"), new RegExp(item.id.replace(".", "\\.")));
}
const order = [...list("data-erase-list").matchAll(/<small>([^<]+)<\/small>/g)].map((m) => m[1]);
assert.deepEqual(order, erased.map((i) => i.id), "the core's order is kept");
assert.match(html, /<input type="checkbox" class="chk" data-erase-config\s*\/>/, "the accounts-file box starts off");
assert.match(html, /data-erase-all-confirm disabled/, "the confirm button starts disabled");
assert.ok(html.includes(i18n.te("settings.erase.word", { word: plan.word })), "the word to type is shown");

// Ticking the box moves the kept items to the erased list.
html = erase.erasePanelHtml(plan, true);
assert.doesNotMatch(html, /data-erase-kept/);
for (const item of plan.items) assert.match(list("data-erase-list"), new RegExp(item.id.replace(".", "\\.")));
assert.match(html, /data-erase-config checked/);

// Only the exact word (spaces around it ignored, as Rust checks) enables the button.
assert.equal(erase.wordMatches("", plan.word), false);
assert.equal(erase.wordMatches(plan.word.toLowerCase(), plan.word), false);
assert.equal(erase.wordMatches(`${plan.word}!`, plan.word), false);
assert.equal(erase.wordMatches(` ${plan.word} `, plan.word), true);
assert.equal(erase.wordMatches("anything", ""), false);

// The shell refuses a wrong word with the core's text; a partial erase lists ids, never paths.
try {
  await lib.eraseAllData(false, "nope");
  assert.fail("a wrong word was accepted");
} catch (e) {
  assert.equal(JSON.parse(e).id, "privacy.erase.word_mismatch");
}
const partial = erase.partialHtml({ complete: false, failed: ["history.db", "cost.index"] });
assert.match(partial, /history\.db, cost\.index/);
assert.match(partial, /data-erase-retry/);
assert.match(partial, /data-erase-quit/);
{
  // Text outside the tags only: split on tag boundaries instead of stripping.
  const text = partial.split("<").map((part) => part.slice(part.indexOf(">") + 1)).join(" ");
  assert.doesNotMatch(text, /[/\\~]/, "no path in the failure");
}

// RF7.2: the clock-skew banner shows the core's advice and nothing while the clocks agree.
const skew = { id: "network.clock_ahead", params: { minutes: 12 }, fallback: "This computer's clock is about {minutes} min ahead of the providers' servers." };
const banner = lib.clockSkewBannerHtml({ clock_skew: skew });
assert.match(banner, /class="banner bad" role="status" data-clock-skew/);
assert.match(banner, /12/);
assert.equal(lib.clockSkewBannerHtml({ clock_skew: null }), "");
assert.equal(lib.clockSkewBannerHtml(null), "");

// Every catalog translates the new texts.
const KEYS = ["label", "desc", "help", "open", "listErased", "listKept", "includeConfig", "irreversible", "word", "confirm",
  "partial", "retry", "quit", "done"].map((k) => `settings.erase.${k}`);
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  for (const key of KEYS) assert.notEqual(i18n.t(key), key, `${code}: ${key}`);
  assert.match(i18n.t("settings.erase.word", { word: "X" }), /X/, `${code}: the word placeholder`);
  assert.match(i18n.t("settings.erase.partial", { ids: "Y" }), /Y/, `${code}: the ids placeholder`);
}
console.log("erase-all UI: ok");
