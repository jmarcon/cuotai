#!/usr/bin/env node
// Browser-free UI smoke for PRD 0040 re-validation H-1: an agent-fed reading
// (Claude Code, Codex CLI) read just now but written 2 hours ago is aged from
// the core's `measured_at` on every desktop surface: the age chip and its
// tooltip, the row's status line, the menu-bar tooltip, and a preserved
// reading's age, never "now" beside "Stale". In en-US and pt-BR.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: () => {},
};
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
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
const lib = await import("../apps/desktop/ui/js/lib.js");
const tray = await import("../apps/desktop/ui/js/tray-menu.js");
const { statusParts } = await import("../apps/desktop/ui/js/components.js");
await i18n.initI18n();

const now = Date.now();
const iso = (msAgo) => new Date(now - msAgo).toISOString();
const twoHours = 2 * 3600 * 1000;
// Read now, written 2 h ago: the core says `freshness: "stale"` and gives the agent's time as `measured_at`.
const fed = {
  provider: "claude", account: "work", status: "ready", source: "feed:claude_code", notes: [], metrics: [],
  checked_at: iso(0), measured_at: iso(twoHours), freshness: "stale", clock_adjusted: false, last_good: null,
};
// A failed query keeping a fed reading: the preserved one's own time.
const kept = {
  ...fed, status: "error", freshness: null, checked_at: iso(0), measured_at: iso(0),
  last_good: { checked_at: iso(60 * 1000), measured_at: iso(twoHours), clock_adjusted: false, metrics: [] },
};
// A report from before the field: the query time stays the age.
const legacy = { ...fed, measured_at: undefined, freshness: "fresh", checked_at: iso(3 * 60 * 1000) };

for (const code of ["en-US", "pt-BR"]) {
  await i18n.setLocale(code);
  const nowWord = i18n.t("time.now");
  const twoH = lib.fmtAge(iso(twoHours));
  assert.notEqual(twoH, nowWord, code);
  assert.equal(lib.shownAt(fed), fed.measured_at, code);
  assert.equal(lib.shownAt(kept), kept.last_good.measured_at, code);
  assert.equal(lib.shownAt(legacy), legacy.checked_at, `${code}: falls back to checked_at`);

  const chip = lib.ageText(fed);
  assert.equal(chip, `${i18n.t("freshness.stale")} · ${twoH}`, `${code}: ${chip}`);
  assert.ok(!chip.includes(nowWord), `${code}: never "now" beside a stale label: ${chip}`);
  const html = lib.ageHtml(fed);
  const measuredClock = lib.fmtReset(fed.measured_at);
  assert.ok(html.includes(lib.esc(measuredClock)), `${code}: the tooltip names the measurement time: ${html}`);
  assert.ok(html.includes(`data-freshness="stale"`), code);

  const preserved = lib.ageText(kept);
  assert.equal(preserved, `${i18n.t("freshness.stale")} · ${twoH}`, `${code}: ${preserved}`);

  const parts = statusParts(fed);
  assert.ok(!String(parts.detail ?? "").includes(nowWord), `${code}: row detail ${parts.detail}`);

  const tip = tray.trayTooltip([fed]);
  assert.ok(tip.includes(twoH), `${code}: the menu-bar tooltip ages the reading: ${tip}`);
  assert.ok(!tip.includes(nowWord), `${code}: ${tip}`);
}
console.log("stale feed UI: ok");
