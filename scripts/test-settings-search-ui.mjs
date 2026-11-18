#!/usr/bin/env node
// Browser-free UI smoke for PRD 0020 RF6 and RF8/RF9: the search index is read from the rows the pages draw (RF6.3),
// so every row is searchable by label, description and keywords in every built-in catalog, accents do not matter,
// results group by page, an empty result offers to clear the field; the responsive and accessibility hooks of the
// shell (page selector, tooltips, search landmark) and the style rules of RF8/RF9 are in place.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const storage = new Map();
globalThis.window = { open: () => {} };
// The rows of every system render: macOS adds Show in Dock (mockup 50-settings-general).
globalThis.document = { hidden: false, body: { append: () => {} }, documentElement: { lang: "", dataset: { platform: "mac" }, toggleAttribute: () => {} }, addEventListener: () => {} };
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
const { PAGES, sidebarHtml } = await import("../apps/desktop/ui/js/settings/registry.js");
const searchModule = await import("../apps/desktop/ui/js/settings/search.js");
const { buildIndex, forget, fold, resultsHtml, rowsOf, search } = searchModule;
await i18n.initI18n();

const view = await lib.getView();
const used = new Set();
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  forget();
  const catalog = JSON.parse(await readFile(ui(`locales/${code}.json`), "utf8"));
  const ctx = { settings: await lib.getSettings(), update: await lib.updaterStatus(), view: () => view, changed: () => {}, rerender: () => {} };
  const rows = await buildIndex(ctx);
  assert.ok(rows.length >= 40, `${code}: only ${rows.length} rows indexed`);
  assert.equal(new Set(rows.map((r) => `${r.page}/${r.key}`)).size, rows.length, `${code}: a row is indexed twice`);
  for (const page of PAGES) assert.ok(rows.some((r) => r.page === page.id), `${code}: no row of ${page.id} is searchable`);
  // RF6.3: a row cannot exist without being searchable: it has a label and keywords in the active language.
  for (const r of rows) {
    assert.ok(r.label, `${code}/${r.page}/${r.key}: no label`);
    assert.ok(r.kw, `${code}/${r.page}/${r.key}: no keywords (settings.kw.${r.key})`);
    assert.ok(!/settings\.|common\./.test(r.label + r.desc), `${code}/${r.key}: untranslated text`);
    used.add(r.key.startsWith("cost-src-") ? "cost-src" : r.key.startsWith("cost-plan-") ? "cost-plan" : r.key.startsWith("cost-opt-") ? "cost-opt" : r.key);
  }
  // Every keyword key of the catalog belongs to a row that exists.
  for (const key of Object.keys(catalog).filter((k) => k.startsWith("settings.kw."))) {
    assert.ok(used.has(key.slice("settings.kw.".length)), `${code}: ${key} matches no row`);
  }
  // Matching: all words must occur (label, description or keywords), accents and case do not matter.
  const word = (r, i) => fold(r.label).split(/\s+/).filter((w) => w.length > 2)[i] ?? fold(r.label);
  const sample = rows.find((r) => r.key === "refresh");
  assert.ok(search(rows, word(sample, 0).toUpperCase()).some((r) => r.key === "refresh"), `${code}: refresh not found by its label`);
  assert.deepEqual(search(rows, "   "), []);
  assert.deepEqual(search(rows, "zzqxjv"), []);
  // PRD 0036 RF8: the aggregator row is found by either aggregator's name.
  for (const q of ["9router", "LiteLLM", "litellm"]) {
    assert.ok(search(rows, q).some((r) => r.key === "agg-in-all"), `${code}: "${q}" does not find agg-in-all`);
  }
  const hits = search(rows, "dark");
  if (code === "en-US") assert.ok(hits.some((r) => r.key === "mode"), "en-US: dark finds the mode row");
  const grouped = resultsHtml(search(rows, fold(sample.label).split(" ")[0]));
  assert.match(grouped, /class="set-result"/);
  assert.match(grouped, /role="status"/);
  assert.match(grouped, /<section class="set-sec set-results" aria-label="/);
}
// RF6 acceptance (PRD 0020 re-validation R-04, V-06): each term finds its row, the best match first; the page of the
// best match leads; "sound" has no row.
{
  const { rankOf } = searchModule;
  for (const [code, q, first] of [["en-US", "port", "port"], ["en-US", "retention", "cost-retention-row"], ["en-US", "token", "token"],
    ["en-US", "theme", "theme"], ["pt-BR", "tema", "theme"], ["pt-BR", "porta", "port"], ["de", "Port", "port"]]) {
    await i18n.setLocale(code); forget();
    const rows = await buildIndex({ settings: await lib.getSettings(), update: await lib.updaterStatus(), view: () => view, changed: () => {}, rerender: () => {} });
    const hits = search(rows, q);
    assert.equal(hits[0]?.key, first, `${code}: "${q}" lists ${hits.slice(0, 3).map((r) => r.key)} first`);
    if (q === "retention") assert.ok(hits.some((r) => r.key === "days"), "retention also finds the history days");
    if (code === "en-US" && q === "theme") assert.equal(search(rows, "sound").length, 0, "sound has no row");
  }
  const row = { head: "port", text: "port serve local" };
  assert.equal(rankOf(row, ["port"]), 0);
  assert.equal(rankOf({ head: "portable mode", text: "portable mode" }, ["port"]), 1);
  assert.equal(rankOf({ head: "server port", text: "server port" }, ["port"]), 2);
  assert.equal(rankOf({ head: "export", text: "export" }, ["port"]), 3);
  assert.equal(rankOf({ head: "language", text: "language portugues" }, ["port"]), 5);
}
// Accents: "configuracao" finds pt-BR "configuração" keywords; the empty state offers to clear.
await i18n.setLocale("pt-BR");
forget();
{
  const rows = await buildIndex({ settings: await lib.getSettings(), update: await lib.updaterStatus(), view: () => view, changed: () => {}, rerender: () => {} });
  assert.ok(search(rows, "intervalo").some((r) => r.key === "refresh"));
  assert.ok(search(rows, "animacoes").some((r) => r.key === "motion"), "accent-free search");
  const none = resultsHtml([]);
  assert.match(none, /data-search-clear/);
  assert.match(none, /role="status"/);
}
// rowsOf reads label, description, keywords and the `find` fallback straight from the row markup.
const sample = '<div class="set-row" data-row="a" data-kw="x y"><span class="set-row-label" id="a-label">A &amp; B</span><span class="set-row-desc" id="a-desc">d</span></div>'
  + '<div class="set-row" data-row="b" data-kw="" data-find="Found"><span class="set-row-label" id="b-label"></span></div>';
assert.deepEqual(rowsOf(sample), [{ key: "a", label: "A & B", desc: "d", kw: "x y" }, { key: "b", label: "Found", desc: "", kw: "" }]);

// RF8: the sidebar offers a tooltip per page for the icon rail and a page selector for narrow widths.
const nav = sidebarHtml("cost", true);
assert.match(nav, /<select class="set-select set-nav-select" data-page-select aria-label="/);
assert.equal((nav.match(/<option /g) ?? []).length, PAGES.length);
assert.match(nav, /data-page="general"[\s\S]*?title="/);
assert.match(nav, /<option value="cost" selected>/);
// PRD 0020 re-validation V-12: the Advanced toggle carries a tooltip too, since the rail hides its label.
assert.match(nav, /data-advanced-toggle[^>]*title="[^"]+"/);
const win = await readFile(ui("js/settings/window.js"), "utf8");
assert.match(win, /role="search"/);
assert.match(win, /aria-label="\$\{te\("settings\.search\.label"\)\}"/);
assert.match(win, /close_settings_window/, "Ctrl/Cmd+W and Esc close the window");

// RF8.4/RF9: the rail at 720, the selector at 560, tokens only, no literal colors, the visible focus ring is kept.
const css = await readFile(ui("css/settings.css"), "utf8");
assert.match(css, /@media \(max-width: 719px\)/);
assert.match(css, /@media \(max-width: 559px\)/);
assert.match(css, /\.set-content \.set-sec \{[^}]*border-radius: var\(--radius-lg\)/, "group cards use --radius-lg");
assert.match(css, /grid-template-columns: 220px /, "sidebar of about 220 px");
assert.match(css, /max-width: 640px/);
assert.match(css, /min-height: 52px/);
assert.ok(!/#[0-9a-fA-F]{3,8}\b/.test(css.replace(/\/\*[\s\S]*?\*\//g, "")), "settings.css must use theme roles, not literal colors");
// Only headings focused by script (tabindex -1) drop their ring; every control keeps it.
assert.ok(!/outline:\s*(none|0)\b/.test(css.replace(/(\.set-title|\.set-subhead h2):focus \{[^}]*\}/g, "")), "a focus ring was removed");
for (const token of ["--motion-base", "--motion-fast"]) assert.ok(css.includes(token), `${token} unused`);
assert.ok(!/animation[^;]*\b\d+m?s\b/.test(css.replace(/\/\*[\s\S]*?\*\//g, "")), "durations must come from the motion tokens");
// PRD 0020 re-validation: descriptions wrap (V-01); every field and select uses the field tokens (V-02); a
// disabled field dims (V-04); the membership check is a 20 px disc with a check glyph, not a dot (V-03).
assert.match(css, /\.set-row-desc \{[^}]*overflow-wrap: anywhere/);
assert.ok(!/\.set-row-desc \{[^}]*(nowrap|ellipsis)/.test(css), "row descriptions must wrap, never truncate (DS-MSG-15)");
assert.match(css, /\.set-window :where\(textarea, select, input[^)]*\)\) \{[^}]*background: var\(--input-bg\);[^}]*color: var\(--fg\)/);
assert.match(css, /\.set-window :where\([^{]*\):disabled \{[^}]*opacity: 0\.45/);
// PRD 0045 re-validation (V-03): the check lives in components.css, which every window loads (the account dialog too).
const components = await readFile(ui("css/components.css"), "utf8");
assert.match(components, /\.chk \{[^}]*width: 20px;[^}]*height: 20px/);
assert.match(components, /\.chk:checked::after \{[^}]*border: solid var\(--accent-fg\)/);
assert.ok(!/radial-gradient/.test(css) && !/\.chk[^{]*\{[^}]*radial-gradient/.test(components), "the check is a glyph, not a radio dot");
const check = await readFile(new URL("../apps/desktop/tools/ui-shots/check.mjs", import.meta.url), "utf8");
assert.ok(!check.includes('".set-row-desc"'), "row descriptions must not be on the overflow check's allowed list");

// PRD 0043 re-validation V-06: Send a test digest follows Send a test notification. With reset notices, usage alerts
// and the projected alert off, both tests are disabled and the digest test explains why; one switch on enables both.
{
  await i18n.setLocale("en-US");
  const alerts = await import("../apps/desktop/ui/js/settings/alerts.js");
  const base = await lib.getSettings();
  const page = (notify, alertsOn, lead) => alerts.html({ settings: { ...base, notify: { ...base.notify, enabled: notify }, alerts: { ...base.alerts, enabled: alertsOn, projected_lead_minutes: lead } } });
  const button = (html, attr) => html.match(new RegExp(`<button[^>]*${attr}[^>]*>`))?.[0] ?? "";
  const off = page(false, false, null);
  assert.match(button(off, "data-notify-test"), /disabled/, "every notice off: the notification test is disabled");
  assert.match(button(off, "data-digest-test"), /disabled/, "every notice off: the digest test is disabled");
  assert.ok(off.includes(`id="digest-test-info" role="note" hidden>${i18n.t("settings.insights.digestTestOff")}<`), "the digest test explains why");
  for (const [n, a, l] of [[true, false, null], [false, true, null], [false, false, 60]]) {
    assert.doesNotMatch(button(page(n, a, l), "data-digest-test"), /disabled/, `notify=${n} alerts=${a} lead=${l}: the digest test is enabled`);
  }
  const source = await readFile(ui("js/settings/alerts.js"), "utf8");
  assert.match(source, /setDigestTestAvailable\(root, on\.checked \|\| alertsOn\.checked \|\| projectedOn\.checked\)/, "a switch change updates the digest test");
}

console.log("PASS: every row is searchable in pt-BR, en-US, es and it; results group by page; the responsive shell and style rules hold");
process.exit(0);
