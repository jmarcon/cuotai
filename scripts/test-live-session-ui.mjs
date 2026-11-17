#!/usr/bin/env node
// Browser-free UI smoke for PRD 0040 RF7: the card's Live session block and
// the tray chip for 0, 1 and 6 sessions with the context at 50, 75 and 95 %
// (display-only roles, never a meter) and the setting that turns them off,
// asserted on the parsed elements; and the new desktop keys in every built-in
// catalog.
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { hasAttr, hasClass, parseHtml } from "./ui-test-env.mjs";

const root = new URL("../", import.meta.url);
const read = async (relative) => JSON.parse(await readFile(fileURLToPath(new URL(relative, root)), "utf8"));
const storage = new Map();
globalThis.window = { __TAURI__: { core: { invoke: async () => ({}) }, event: { emit: async () => {}, listen: async () => () => {} } } };
globalThis.document = { hidden: false, documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} }, addEventListener: () => {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) };
globalThis.fetch = async (relative) => {
  try {
    return { ok: true, json: async () => read(`apps/desktop/ui/${relative}`) };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

await import("../apps/desktop/ui/js/i18n.js").then((i18n) => i18n.initI18n());
const lib = await import("../apps/desktop/ui/js/lib.js");

const text = (id, fallback, params = {}) => ({ id, params, fallback });
const session = (ctx, level) => ({
  model_name: "Opus",
  context: text("feed.live.ctx", `ctx ${ctx}%`, { pct: String(ctx) }),
  level,
  cost: text("feed.live.cost", "≈ $0.42 this session", { usd: "0.42" }),
  written_at: new Date().toISOString(),
});
const live = (sessions, more = null) => ({
  title: text("feed.live.title", "Live session"),
  caveat: text("feed.live.caveat", "Agent estimate at list price, not a charge."),
  more: more ? text("feed.live.more", `and ${more} more`, { n: more }) : null,
  chip: sessions[0]?.context ?? null,
  chip_level: sessions[0]?.level ?? null,
  sessions,
});

// The block and the chip are asserted as elements (parsed from the rendered markup), never as markup strings.
const block = (r) => {
  const tree = parseHtml(lib.liveSessionHtml(r));
  const group = tree.find(hasAttr("data-live"));
  return { tree, group, rows: tree.all(hasAttr("data-live-row")) };
};
const ctxOf = (row) => row.find(hasAttr("data-live-ctx"));
const chipOf = (r) => parseHtml(lib.liveChipHtml(r)).find(hasAttr("data-live-chip"));

// 0 sessions: nothing at all.
assert.equal(block({}).tree.children.length, 0);
assert.equal(block({ live: live([]) }).tree.children.length, 0);
assert.equal(chipOf({}), null);

// 1 session at 50 %: one row inside a labelled group, a neutral context chip, the cost with the caveat as its tooltip
// and as a caption, and no meter anywhere: the context share is not a quota.
{
  const { tree, group, rows } = block({ live: live([session(50, "good")]) });
  assert.ok(group, "the block is one element");
  assert.equal(group.attrs.role, "group");
  assert.equal(group.attrs["aria-label"], "Live session");
  assert.equal(group.find(hasClass("ls-title")).text(), "Live session");
  assert.equal(rows.length, 1);
  assert.ok(rows.every((row) => row.closest((n) => n === group)), "the rows sit inside the block");
  assert.equal(rows[0].find(hasClass("ls-model")).text(), "Opus");
  const ctx = ctxOf(rows[0]);
  assert.deepEqual(ctx.cls, ["mini"], "50 % has no warning role");
  assert.equal(ctx.text(), "ctx 50%");
  const cost = rows[0].find(hasClass("ls-cost"));
  assert.equal(cost.text(), "≈ $0.42 this session");
  assert.equal(cost.attrs.title, "Agent estimate at list price, not a charge.");
  assert.equal(group.find(hasClass("ls-caveat")).text(), "Agent estimate at list price, not a charge.");
  assert.equal(group.find(hasClass("ls-more")), null, "no 'and N more' for one session");
  assert.equal(tree.find((n) => n.attrs.role === "meter" || n.cls.includes("bar")), null, "never drawn as a quota meter");
}

// 75 % warns and 95 % is bad, both display-only roles on the chip.
assert.deepEqual(ctxOf(block({ live: live([session(75, "warn")]) }).rows[0]).cls, ["mini", "warn"]);
assert.deepEqual(ctxOf(block({ live: live([session(95, "bad")]) }).rows[0]).cls, ["mini", "bad"]);

// A session without a cost has no cost element and no caveat caption.
{
  const { group, rows } = block({ live: live([{ ...session(50, "good"), cost: null }]) });
  assert.equal(rows[0].find(hasClass("ls-cost")), null);
  assert.equal(group.find(hasClass("ls-caveat")), null);
}

// 6 sessions: the core sends five and "and 1 more"; five rows, newest first, and the "more" line after them.
{
  const { group, rows } = block({ live: live([50, 75, 95, 40, 30].map((n) => session(n, n >= 90 ? "bad" : n >= 70 ? "warn" : "good")), 1) });
  assert.equal(rows.length, 5);
  assert.deepEqual(rows.map((row) => ctxOf(row).text()), ["ctx 50%", "ctx 75%", "ctx 95%", "ctx 40%", "ctx 30%"]);
  assert.equal(group.find(hasClass("ls-more")).text(), "and 1 more");
}

// The tray chip mirrors the newest session, its role and the block's title as tooltip.
{
  const chip = chipOf({ live: live([session(95, "bad")]) });
  assert.deepEqual(chip.cls, ["mini", "bad"]);
  assert.equal(chip.text(), "ctx 95%");
  assert.equal(chip.attrs.title, "Live session");
}

// Settings › Dashboard › Show live agent session hides both.
await lib.setLiveSession(false);
assert.equal(lib.liveSessionShown(), false);
assert.equal(block({ live: live([session(50, "good")]) }).group, null);
assert.equal(chipOf({ live: live([session(50, "good")]) }), null);
await lib.setLiveSession(true);
assert.ok(block({ live: live([session(50, "good")]) }).group);

// Every new desktop key exists in every built-in catalog.
const KEYS = [
  "settings.liveSession.show", "settings.liveSession.help", "settings.kw.live-session",
  "settings.claudeCode.title", "settings.claudeCode.desc", "settings.claudeCode.privacy",
  "settings.claudeCode.copy", "settings.claudeCode.copied", "settings.claudeCode.chained", "settings.kw.claude-code",
];
const files = (await readdir(fileURLToPath(new URL("apps/desktop/ui/locales/", root)))).filter((f) => f.endsWith(".json"));
assert.equal(files.length, 14);
for (const file of files) {
  const catalog = await read(`apps/desktop/ui/locales/${file}`);
  for (const key of KEYS) assert.ok(typeof catalog[key] === "string" && catalog[key].trim(), `${file} lacks ${key}`);
}
console.log("live session UI: ok");
