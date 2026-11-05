#!/usr/bin/env node
// Browser-free UI smoke for PRD 0040 RF7: the card's Live session block and
// the tray chip for 0, 1 and 6 sessions with the context at 50, 75 and 95 %
// (display-only roles, never a meter), the setting that turns them off, the
// feed banner, and the new desktop keys in every built-in catalog.
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

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

// 0 sessions: nothing.
assert.equal(lib.liveSessionHtml({}), "");
assert.equal(lib.liveChipHtml({}), "");

// 1 session at 50 %: one row, a neutral chip, the caveat as tooltip and caption, no meter.
const one = lib.liveSessionHtml({ live: live([session(50, "good")]) });
assert.equal((one.match(/data-live-row/g) ?? []).length, 1, one);
assert.match(one, /class="mini " data-live-ctx/);
assert.match(one, /title="Agent estimate at list price, not a charge\."/);
assert.match(one, /≈ \$0\.42 this session/);
assert.ok(!/role="meter"|class="bar/.test(one), "the context share is never drawn as a quota meter");

// 75 % warns and 95 % is bad, both display-only roles.
assert.match(lib.liveSessionHtml({ live: live([session(75, "warn")]) }), /class="mini warn" data-live-ctx/);
assert.match(lib.liveSessionHtml({ live: live([session(95, "bad")]) }), /class="mini bad" data-live-ctx/);

// 6 sessions: five rows and "and 1 more".
const six = lib.liveSessionHtml({ live: live([1, 2, 3, 4, 5].map((n) => session(n * 10, "good")), 1) });
assert.equal((six.match(/data-live-row/g) ?? []).length, 5);
assert.match(six, /and 1 more/);

// The tray chip mirrors the newest session.
assert.match(lib.liveChipHtml({ live: live([session(95, "bad")]) }), /class="mini bad" data-live-chip[^>]*><span class="mini-t">ctx 95%/);

// Settings › Dashboard › Show live agent session hides both.
await lib.setLiveSession(false);
assert.equal(lib.liveSessionShown(), false);
assert.equal(lib.liveSessionHtml({ live: live([session(50, "good")]) }), "");
assert.equal(lib.liveChipHtml({ live: live([session(50, "good")]) }), "");
await lib.setLiveSession(true);
assert.ok(lib.liveSessionHtml({ live: live([session(50, "good")]) }));

// Every new desktop key exists in every built-in catalog.
const KEYS = [
  "settings.liveSession.show", "settings.liveSession.help", "settings.kw.live-session",
  "settings.claudeCode.title", "settings.claudeCode.show", "settings.claudeCode.desc", "settings.claudeCode.privacy",
  "settings.claudeCode.copy", "settings.claudeCode.copied", "settings.claudeCode.chained", "settings.kw.claude-code",
];
const files = (await readdir(fileURLToPath(new URL("apps/desktop/ui/locales/", root)))).filter((f) => f.endsWith(".json"));
assert.equal(files.length, 14);
for (const file of files) {
  const catalog = await read(`apps/desktop/ui/locales/${file}`);
  for (const key of KEYS) assert.ok(typeof catalog[key] === "string" && catalog[key].trim(), `${file} lacks ${key}`);
}
console.log("live session UI: ok");
