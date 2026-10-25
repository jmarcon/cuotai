#!/usr/bin/env node
// Browser-free UI smoke for PRD 0036 RF8: the 9router pieces of the desktop (instance header, pool lines, the
// grouped row's status line, connection chips and the details blocks) render from the backend's view in every
// built-in catalog, with the core's pool sentences, no untranslated key, no raw URL path and no e-mail.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: () => {},
};
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = {
  getItem: (key) => storage.get(key) ?? null,
  setItem: (key, value) => storage.set(key, value),
};
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
const agg = await import("../apps/desktop/ui/js/aggregator.js");
await i18n.initI18n();

const view = await lib.getView();
const router = view.reports.find(agg.isAggregator);
assert.ok(router, "the demo has a 9router instance");
const connections = agg.connectionsOf(router);
assert.equal(connections.length, 4);
// Connections keep the aggregator's index (actions act on it) and their own key.
assert.ok(connections.every((c) => c.index === router.index && c.account_key.startsWith("nine_router:home/")));
assert.equal(new Set(connections.map(lib.reportKey)).size, 4, "connection keys are unique");
assert.ok(!view.reports.some((r) => connections.some((c) => lib.reportKey(c) === lib.reportKey(r))), "no key shared with an account");
// Lookups reach connections; an unknown upstream keeps its slug and a neutral tile.
const all = agg.withConnections(view.reports);
assert.equal(all.length, view.reports.length + 4);
const glm = connections.find((c) => c.connection.upstream === "glm");
assert.equal(glm.provider, "glm");
assert.equal(agg.upstreamName(glm), "glm");
// Groups follow the upstreams, each with its pool when the core has one.
const groups = agg.groupsOf(router);
assert.deepEqual(groups.map((g) => g.upstream), ["claude", "codex", "glm"]);
assert.equal(groups[0].items.length, 2);
assert.ok(groups[0].pool && !groups[1].pool);
assert.equal(agg.poolFigure(router), "1/3");
// The off connection never takes part.
assert.equal(connections.filter(agg.activeConnection).length, 3);

const expected = {
  "en-US": { pool: "Claude · 1 of 2 available", off: "Off in 9router", tight: "Tightest: Claude Code · work" },
  es: { pool: "Claude · 1 de 2 disponibles", off: "Desactivada en 9router", tight: "La más ajustada: Claude Code · work" },
  it: { pool: "Claude · 1 di 2 disponibili", off: "Disattivata in 9router", tight: "La più stretta: Claude Code · work" },
  "pt-BR": { pool: "Claude · 1 de 2 disponíveis", off: "Desligada no 9router", tight: "Mais apertada: Claude Code · work" },
};
const untranslated = /\b(agg|settings\.agg)\.[a-zA-Z.]+/;
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const want = expected[code];
  const pool = agg.poolLineHtml(router.aggregator.pools[0]);
  // Pool sentences are core texts: the browser-free run reads the core's catalog files like the demo does (PRD 0022
  // RF18.2); pt-BR has no file and shows the core's Portuguese fallback.
  assert.ok(pool.includes(want.pool), `${code}: ${pool}`);
  const info = agg.instanceInfoHtml(router);
  assert.ok(info.includes("127.0.0.1:20128") && info.includes("0.5.109"), `${code}: instance info`);
  assert.ok(info.includes('data-pool-each="nine_router:home"'), `${code}: alert mode switch`);
  assert.ok(info.includes(`data-agg-dashboard="${router.index}"`), `${code}: open dashboard`);
  assert.ok(!/\/dashboard|\/api\//.test(info), `${code}: no URL path on screen`);
  const off = connections.find((c) => c.connection.enabled_upstream === false);
  const chips = agg.connectionChips(off, { via: true });
  if (want) assert.ok(chips.includes(want.off), `${code}: ${chips}`);
  const details = agg.connectionInfoHtml(connections[0], router.index);
  const line = agg.statusLine(router);
  if (want) assert.ok(line.text.startsWith(want.tight), `${code}: ${line.text}`);
  const head = agg.instanceHeadHtml(router);
  const name = agg.accessibleName(router);
  for (const [what, html] of [["info", info], ["chips", chips], ["details", details], ["line", line.text], ["head", head], ["name", name]]) {
    assert.ok(!untranslated.test(html), `${code}/${what}: untranslated key in ${html}`);
    assert.ok(!/@/.test(html), `${code}/${what}: an e-mail on screen`);
  }
  console.log(`${code}: ${line.text} · ${agg.poolFigure(router)} ${i18n.t("agg.available")}`);
}

// RF8.3: the "All" choice is per device and falls back to grouped.
assert.equal(agg.aggregatorsInAll(), "grouped");
agg.setAggregatorsInAll("listed");
assert.equal(agg.aggregatorsInAll(), "listed");
agg.setAggregatorsInAll("nonsense");
assert.equal(agg.aggregatorsInAll(), "grouped");

console.log("PASS: 9router instance, pools, connections and details render in pt-BR, en-US, es and it without raw keys, paths or e-mails");
