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
  de: { pool: "Claude · 1 von 2 verfügbar", off: "In 9router aus", tight: "Am knappsten: Claude Code · work" },
  ru: { pool: "Claude · доступно 1 из 2", off: "Выключено в 9router", tight: "Самый жёсткий: Claude Code · work" },
  ko: { pool: "Claude · 2개 중 1개 사용 가능", off: "9router에서 꺼짐", tight: "가장 빠듯함: Claude Code · work" },
  "hi-IN": { pool: "Claude · 2 में से 1 उपलब्ध", off: "9router में बंद", tight: "सबसे कम: Claude Code · work" },
  "id-ID": { pool: "Claude · 1 dari 2 tersedia", off: "Nonaktif di 9router", tight: "Paling ketat: Claude Code · work" },
  tr: { pool: "Claude · 2 içinden 1 kullanılabilir", off: "9router&#39;da kapalı", tight: "En sıkı: Claude Code · work" },
  vi: { pool: "Claude · 1 trên 2 khả dụng", off: "Tắt trong 9router", tight: "Chặt nhất: Claude Code · work" },
  "zh-CN": { pool: "Claude · 2 个中 1 个可用", off: "在 9router 中已关闭", tight: "最紧：Claude Code · work" },
  fr: { pool: "Claude · 1 sur 2 disponibles", off: "Désactivée dans 9router", tight: "Plus serrée : Claude Code · work" },
  ja: { pool: "Claude · 2 件中 1 件利用可能", off: "9router でオフ", tight: "最も厳しい：Claude Code · work" },
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

// PRD 0036 RF2.3/RF2.4: the standing warnings are persistent chips with the core's texts, most severe first;
// none without `connection_warnings`, and the details list them too.
assert.equal(agg.warningChipsHtml(router), "", "no warnings, no chips");
const warned = { ...router, connection_warnings: { exposed: true, private_http: true } };
const chipTexts = { "en-US": ["No password", "Unencrypted"], es: ["Sin contraseña", "Sin cifrado"], it: ["Senza password", "Non cifrato"], "pt-BR": ["Sem senha", "Sem criptografia"], de: ["Ohne Passwort", "Unverschlüsselt"], ru: ["Без пароля", "Без шифрования"], ko: ["비밀번호 없음", "암호화 없음"], "hi-IN": ["बिना पासवर्ड", "बिना एन्क्रिप्शन"], "id-ID": ["Tanpa kata sandi", "Tanpa enkripsi"], tr: ["Parola yok", "Şifresiz"], vi: ["Không mật khẩu", "Không mã hóa"] };
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const html = agg.warningChipsHtml(warned);
  const [exposed, http] = chipTexts[code];
  assert.ok(html.indexOf(exposed) >= 0 && html.indexOf(exposed) < html.indexOf(http), `${code}: ${html}`);
  assert.ok(!/aggregators\.chip/.test(html), `${code}: untranslated chip`);
  assert.ok(agg.instanceInfoHtml(warned).includes("data-instance-warnings"), `${code}: details warnings`);
}
assert.ok(!agg.warningChipsHtml({ ...router, connection_warnings: { exposed: false, private_http: true } }).includes('data-conn-warning="exposed"'));
await i18n.setLocale("en-US");

// PRD 0036 RF8.1: with aggregator tabs each aggregator has its own tab after the provider tabs (its tile, pool ring
// and connection count, named like the grouped row); it adds no provider tab of its own or of its upstreams.
const comp = await import("../apps/desktop/ui/js/components.js");
const tabs = comp.filterTabsHtml(view.reports, { aggregatorTabs: true, maxTabs: 99 });
const aggId = comp.aggregatorTabId(router);
assert.ok(tabs.includes(`data-tab="${aggId}"`), "the aggregator has its own tab");
assert.ok(!tabs.includes('data-tab="nine_router"'), "no provider tab for the aggregator");
assert.ok(tabs.indexOf(`data-tab="${aggId}"`) > tabs.lastIndexOf('data-tab="claude"'), "after the provider tabs");
assert.ok(tabs.includes(`<span class="cnt">${router.aggregator.connections}</span></button>`), "connection count");
assert.ok(comp.filterTabsHtml(view.reports).includes('data-tab="nine_router"'), "without the option, the old provider tab");
assert.deepEqual(view.reports.filter((r) => comp.inTabFilter(r, aggId, true)), [router]);
assert.ok(!view.reports.filter((r) => comp.inTabFilter(r, "nine_router", true)).length, "a provider tab never lists it");
const overflowTabs = comp.filterTabsHtml(view.reports, { aggregatorTabs: true, maxTabs: 2, moreOpen: true, selected: aggId });
assert.match(overflowTabs, new RegExp(`role="menuitemradio" aria-checked="true" data-tab="${aggId}"`), "overflow lists it in More");

// RF8.3: the "All" choice is per device and falls back to grouped.
assert.equal(agg.aggregatorsInAll(), "grouped");
agg.setAggregatorsInAll("listed");
assert.equal(agg.aggregatorsInAll(), "listed");
agg.setAggregatorsInAll("nonsense");
assert.equal(agg.aggregatorsInAll(), "grouped");

// PRD 0036 RF2.1/RF2.6: a vault password or key slot (`secret`) is a masked one-line field, an account-host slot
// lists no hosts, and Test connection sends the unsaved pastes with the draft.
const dialogSrc = await readFile(ui("js/account-dialog.js"), "utf8");
assert.match(dialogSrc, /s\.secret\s*\n?\s*\/\/[^\n]*\n\s*\? `<input type="password" class="session-paste" data-slot=/, "secret slots render as password fields");
assert.ok(dialogSrc.includes("${s.hosts.length ? `<small"), "no empty hosts line for account-host slots");
assert.ok(dialogSrc.includes("testAggregator({ ...draftInput(), sessions: pastes() })"), "Test connection carries the pastes");
assert.ok(!dialogSrc.includes('querySelectorAll("textarea[data-slot]")'), "pastes are read from every field kind");
// PRD 0036 RF4.4/RF8.5: per-connection Refresh and Hide; "Show in both" only for a duplicate (or one already shown).
const work = connections.find((c) => c.connection.label === "work");
const acts = agg.connectionActionsHtml(work, router.index);
assert.ok(acts.includes("data-conn-refresh") && acts.includes('data-conn-flag="hidden"') && acts.includes(`data-sub="${work.connection.sub}"`));
assert.ok(!acts.includes('data-conn-flag="show_both"'), "no duplicate, no Show in both");
const dup = { ...work, connection: { ...work.connection, duplicate_of: "claude:Account 2" } };
assert.ok(agg.connectionActionsHtml(dup, router.index).includes('data-conn-flag="show_both"'));
await lib.setConnectionFlag(router.index, "hidden", work.connection.sub, true);
assert.equal(agg.connectionsOf((await lib.getView()).reports.find(agg.isAggregator)).length, 3, "a hidden connection leaves the view");
await lib.setConnectionFlag(router.index, "hidden", work.connection.sub, false);
assert.equal(agg.connectionsOf((await lib.getView()).reports.find(agg.isAggregator)).length, 4);
assert.equal(work.plan, "Max", "the connection's plan rides on the view");

console.log("PASS: 9router instance, pools, connections and details render in pt-BR, en-US, es and it without raw keys, paths or e-mails");
