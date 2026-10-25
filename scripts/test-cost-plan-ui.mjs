#!/usr/bin/env node
// Browser-free UI smoke for PRD 0030 RF1 and RF6: the plan price and the typed rate (browser demo of the core's
// `cost_plans`), the plan line and the converted amounts on the cost card and details, the "Add plan price" link, and
// the Settings rows with their search keywords. Nothing is summed across currencies and no plan means no line.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const storage = new Map();
globalThis.window = { open: () => {} };
globalThis.document = { hidden: false, body: { append: () => {} }, documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} }, addEventListener: () => {} };
globalThis.MutationObserver = class { observe() {} disconnect() {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (k) => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, String(v)) };
globalThis.fetch = async (relative) => {
  try {
    const data = JSON.parse(await readFile(ui(relative), "utf8"));
    return { ok: true, json: async () => data };
  } catch { return { ok: false, json: async () => ({}) }; }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { costCardHtml, costDetailHtml, convertedText } = await import("../apps/desktop/ui/js/cost.js");
const { costHtml } = await import("../apps/desktop/ui/js/settings/cost.js");
const { rowsOf } = await import("../apps/desktop/ui/js/settings/search.js");
await i18n.initI18n();
await i18n.setLocale("en-US");

// The demo's Claude Code source is on, so the card has numbers.
const status = await lib.costStatus();
const claude = status.sources.find((s) => s.source === "claude_code");
await lib.costEnable("claude_code", claude.disclosure.digest);
const load = async () => {
  const usage = await lib.costUsage();
  const plans = await lib.costPlans();
  return { summary: { ...usage, plans: plans.values, display: plans.display, planAccounts: plans.accounts }, plans };
};

// No plan: no line, and the details offer to add one.
let { summary, plans } = await load();
const account = plans.accounts.find((a) => a.source === "claude_code");
assert.ok(account, "the demo has an account of the Claude Code tool");
const source = () => summary.sources.find((s) => s.source === "claude_code");
assert.ok(!costCardHtml(source(), summary).includes("data-cost-plan"), "no plan, no line");
assert.ok(!costCardHtml(source(), summary).includes("data-cost-display"), "no rate, no converted line");
assert.match(costDetailHtml(source(), summary), /data-cost-plan-add/);

// A plan in USD gives a ratio line; the link is gone.
await lib.costSetPlan(account.id, 100, "usd", 15);
({ summary, plans } = await load());
assert.equal(plans.accounts.find((a) => a.id === account.id).plan.currency, "USD");
assert.match(costCardHtml(source(), summary), /data-cost-plan/);
assert.ok(!costDetailHtml(source(), summary).includes("data-cost-plan-add"));
assert.equal(plans.values.length, 1);
assert.ok(plans.values[0].ratio > 0, "same currency divides");

// A plan in BRL without a typed rate shows both side by side and no ratio; with the rate it is "at your rate".
await lib.costSetPlan(account.id, 550, "BRL", null);
({ summary, plans } = await load());
assert.equal(plans.values[0].ratio, null);
await lib.costSetDisplay("brl", 5.5);
({ summary, plans } = await load());
assert.equal(plans.display.code, "BRL");
assert.equal(plans.display.stale, false);
assert.ok(plans.values[0].ratio > 0 && plans.values[0].at_your_rate);
const card = costCardHtml(source(), summary);
assert.match(card, /data-cost-display/);
assert.match(card, /lc-fx-amount/);
assert.match(convertedText(10, plans.display), /55\.00/);
assert.equal(convertedText(null, plans.display), null);
assert.equal(convertedText(10, null), null);

// Settings: a USD plan converted with the typed rate is an estimate, marked with ≈; a plan already in the
// second currency is shown as typed, with no mark.
await lib.costSetPlan(account.id, 20, "USD", null);
await i18n.setLocale("en-US");
const converted = costHtml(await lib.costStatus(), await lib.costPrices(), await lib.costPlans());
assert.match(converted, /≈ 110 BRL at your rate/);
await lib.costSetPlan(account.id, 550, "BRL", null);
assert.ok(!costHtml(await lib.costStatus(), await lib.costPrices(), await lib.costPlans()).includes("at your rate"));

// Refusals come from the core as its own texts.
await assert.rejects(lib.costSetPlan(account.id, 0, "USD", null), /invalid_plan/);
await assert.rejects(lib.costSetPlan(account.id, 10, "US", null), /invalid_plan/);
await assert.rejects(lib.costSetPlan(account.id, 10, "USD", 29), /invalid_plan/);
await assert.rejects(lib.costSetDisplay("USD", 5), /invalid_rate/);
await assert.rejects(lib.costSetDisplay("BRL", 0), /invalid_rate/);

// Settings: one searchable row per account and one for the rate, inside the "Plans" sub-page, in every catalog.
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const html = costHtml(await lib.costStatus(), await lib.costPrices(), await lib.costPlans());
  const rows = rowsOf(html);
  assert.ok(rows.some((r) => r.key === "cost-plans" && r.kw), `${code}: the plans sub-page row is searchable`);
  assert.ok(rows.some((r) => r.key === `cost-plan-${account.id}` && r.kw), `${code}: the plan row is searchable`);
  assert.ok(rows.some((r) => r.key === "cost-display" && r.kw), `${code}: the rate row is searchable`);
  assert.match(html, /data-cost-plan-save=/);
  assert.match(html, /data-cost-display-save/);
  assert.ok(!/settings\.cost\.(plan|display|plans)\b/.test(html), `${code}: untranslated text`);
}

// Clearing both leaves nothing behind.
await lib.costSetPlan(account.id, null, "USD", null);
await lib.costSetDisplay(null, 0);
({ summary, plans } = await load());
assert.equal(plans.values.length, 0);
assert.equal(plans.display, null);
// PRD 0022 RF6.1 (DS-CMP-HON-2): a priced local cost is an estimate and carries "≈" on the card tiles and the daily
// bars; no usage, an unknown price and "< $0.01" do not.
const { approxText } = await import("../apps/desktop/ui/js/cost.js");
await i18n.setLocale("en-US");
const tok = (n) => ({ input: n, output: 0, cache_read: 0, cache_write: 0, cache_write_1h: 0 });
const none = tok(0);
assert.equal(approxText({ tokens: tok(10), unknown_tokens: none, total: 1.5 }), "≈$1.50");
assert.equal(approxText({ tokens: tok(10), unknown_tokens: none, total: null }), "cost unknown");
assert.equal(approxText({ tokens: none, unknown_tokens: none, total: 0 }), "no usage");
assert.equal(approxText({ tokens: tok(10), unknown_tokens: none, total: 0.001 }), "< $0.01");
assert.equal(approxText({ tokens: tok(10), unknown_tokens: none, cost: 2 }), "≈$2.00", "a day");
{ const tilesHtml = costCardHtml(source(), summary);
for (const m of tilesHtml.matchAll(/class="lc-cost[^"]*">([^<]*)</g)) assert.ok(/^(≈|<|no usage|cost unknown)/.test(m[1]), `tile ${m[1]}`);
const daysHtml = costDetailHtml(source(), summary);
for (const m of daysHtml.matchAll(/class="lc-bar[^"]*"[^>]*title="([^"]*)"/g)) assert.ok(!/: \$(?!0\.00)/.test(m[1]), `bar tip without ≈: ${m[1]}`); }
assert.match(costDetailHtml(source(), summary), /title="[^"]*: ≈\$/, "a priced day carries ≈");

console.log("PASS: plan price and display currency (PRD 0030 RF1/RF6): lines, converted amounts, add link, Settings rows and refusals");
