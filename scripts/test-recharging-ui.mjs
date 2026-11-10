#!/usr/bin/env node
// Browser-free UI smoke for PRD 0010 RF4 (re-validation R-02, R-03, V-02..V-04): which accounts join the
// "Recharging" group, the order inside each list, and the stale card and row wording. An account usable now on
// paid extra usage (overage `armed` or `active`) is not recharging; `unknown` stays in (usability not confirmed).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = { open: () => {} };
globalThis.document = { hidden: false, body: { append: () => {} }, documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} }, addEventListener: () => {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
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
const components = await import("../apps/desktop/ui/js/components.js");
await i18n.initI18n();

const now = new Date().toISOString();
const base = (name, extra = {}) => ({
  provider: "claude", account: name, status: "ready", class: "available", checked_at: now, freshness: "fresh",
  metrics: [], notes: [], notes_l10n: [], binding: null, last_good: null, pending: false, ...extra,
});
const exhausted = (name, extra = {}) => base(name, { class: "exhausted", ...extra });
const blocked = (name) => base(name, { binding: { index: 0, reason: "exhausted", used_ratio: 1, blocked_until: null } });
const overage = (state) => ({ state, spent: 1, cap: 10, currency: "USD" });

const cases = [
  ["exhausted joins", exhausted("a"), true],
  ["blocked joins", blocked("b"), true],
  ["available stays out", base("c"), false],
  ["a failure with a kept reading stays out", base("d", { status: "error", last_good: base("d") }), false],
  ["overage active stays out", exhausted("e", { overage: overage("active") }), false],
  ["overage armed stays out", exhausted("f", { overage: overage("armed") }), false],
  ["overage cap reached joins", exhausted("g", { overage: overage("cap_reached") }), true],
  ["overage disabled joins", exhausted("h", { overage: overage("disabled") }), true],
  ["overage unknown joins", exhausted("i", { overage: overage("unknown") }), true],
];
for (const [what, report, expected] of cases) assert.equal(lib.isRecharging(report), expected, what);

// Off by default: one list, in the given order.
const list = [exhausted("1"), base("2"), blocked("3"), base("4"), exhausted("5", { overage: overage("active") })];
assert.equal(lib.groupingRecharging(), false);
assert.deepEqual(lib.splitRecharging(list).map((l) => l.map((r) => r.account)), [["1", "2", "3", "4", "5"], []]);
// On: the manual order is kept inside each list.
lib.setGroupRecharging(true);
assert.deepEqual(lib.splitRecharging(list).map((l) => l.map((r) => r.account)), [["2", "4", "5"], ["1", "3"]]);
lib.setGroupRecharging(false);

// V-04: a stale row names the cause in its tooltip and accessible name; a restored reading still waiting has none.
const stale = base("work", { status: "error", notes: ["Network unreachable"], last_good: base("work", { checked_at: new Date(Date.now() - 42 * 60000).toISOString() }) });
const st = components.accountState(stale);
assert.equal(st.kind, "stale");
assert.ok(components.statusParts(stale, st).full.includes("Network unreachable"), "tooltip carries the cause");
assert.ok(components.rowName(stale, st).includes("Network unreachable"), "row name carries the cause");
const waiting = { ...stale, status: "ready", pending: true, notes: [] };
const wst = components.accountState(waiting);
assert.equal(wst.kind, "stale");
assert.ok(!components.statusParts(waiting, wst).full.includes(i18n.t("card.waiting")), "waiting is no cause");

console.log("recharging UI: ok");
