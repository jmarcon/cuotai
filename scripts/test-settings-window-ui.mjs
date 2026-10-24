#!/usr/bin/env node
// Browser-free UI smoke for PRD 0020 RF1 and RF2: the Settings window's page registry follows DS-CMP-SEG-5, its
// sidebar navigation (current page, Advanced disclosure, arrow keys) behaves, every section of the old 12-section
// dialog maps to a page, and every control of the old dialog renders on its page in every built-in catalog.
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const storage = new Map();
const opened = [];
globalThis.window = { open: (url, name) => opened.push([url, name]) };
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
const registry = await import("../apps/desktop/ui/js/settings/registry.js");
const { integrationHtml, widgetsHtml } = await import("../apps/desktop/ui/js/settings/integrations.js");
const { hooksHtml } = await import("../apps/desktop/ui/js/settings/hooks.js");
const { costHtml } = await import("../apps/desktop/ui/js/settings/cost.js");
await i18n.initI18n();

const { PAGES, ADVANCED, LEGACY_SECTIONS, CONTROLS } = registry;
const ids = PAGES.map((p) => p.id);

// RF2.5 / DS-CMP-SEG-5: order, Advanced group and About at the bottom. Menu bar and Extensions join later.
const SEG5 = ["general", "dashboard", "menubar", "alerts", "data", "integrations", "cost", "extensions", "files", "about"];
assert.deepEqual(ids, SEG5.filter((id) => ids.includes(id)), "pages are out of the DS-CMP-SEG-5 order");
assert.deepEqual(ids.slice(0, 4), ["general", "dashboard", "alerts", "data"], "everyday pages drifted");
assert.deepEqual(PAGES.filter((p) => p.group === ADVANCED).map((p) => p.id), ["integrations", "cost", "files"], "Advanced group drifted");
assert.deepEqual(PAGES.filter((p) => p.bottom).map((p) => p.id), ["about"], "About must be the one page at the bottom");
assert.equal(new Set(ids).size, ids.length, "duplicate page id");
const sections = PAGES.flatMap((p) => p.sections);
assert.equal(new Set(sections).size, sections.length, "a section is listed on two pages");

// RF2 acceptance: the 12 sections of the old dialog (plus Updates) all land on a registered page, and the
// per-control checklist agrees with that map.
assert.deepEqual(Object.keys(LEGACY_SECTIONS).sort(), ["about", "appearance", "config", "cost", "folder", "history", "integration",
  "language", "notifications", "order", "privacy", "refresh", "updates"], "old dialog sections drifted");
for (const [section, pages] of Object.entries(LEGACY_SECTIONS)) {
  for (const page of pages) assert.ok(ids.includes(page), `${section} maps to unknown page ${page}`);
  const controlled = [...new Set(CONTROLS.filter((c) => c.section === section).map((c) => c.page))].sort();
  assert.deepEqual(controlled, [...pages].sort(), `${section}: controls and section map disagree`);
}

// RF2.4 / RF10.4: one module per page, each with html() and bind(), each under about 300 lines.
const modules = new Map();
for (const page of PAGES) {
  const mod = await page.load();
  assert.equal(typeof mod.html, "function", `${page.id}: no html()`);
  assert.equal(typeof mod.bind, "function", `${page.id}: no bind()`);
  modules.set(page.id, mod);
}
for (const file of await readdir(ui("js/settings"))) {
  const lines = (await readFile(ui(`js/settings/${file}`), "utf8")).split("\n").length;
  assert.ok(lines <= 300, `js/settings/${file} has ${lines} lines`);
}

// Sidebar: the current page is marked once, Advanced pages only while the group is open (except the current one),
// the toggle says whether it is expanded, and the arrow keys wrap.
const marked = (html) => html.match(/aria-current="page"/g)?.length ?? 0;
const listed = (html) => [...html.matchAll(/data-page="([a-z-]+)"/g)].map((m) => m[1]);
let nav = registry.sidebarHtml("general", false);
assert.equal(marked(nav), 1);
assert.deepEqual(listed(nav), ["general", "dashboard", "alerts", "data", "about"]);
assert.match(nav, /data-advanced-toggle aria-expanded="false"/);
assert.match(nav, /<nav class="set-nav" aria-label="/);
nav = registry.sidebarHtml("cost", false);
assert.deepEqual(listed(nav), ["general", "dashboard", "alerts", "data", "cost", "about"], "the current Advanced page must stay listed");
nav = registry.sidebarHtml("files", true);
assert.deepEqual(listed(nav), ids);
assert.match(nav, /data-page="files"\s+aria-current="page"/);
assert.match(nav, /aria-expanded="true"/);
const entries = ["a", "b", "c"];
assert.equal(registry.nextEntry(entries, "a", "ArrowDown"), "b");
assert.equal(registry.nextEntry(entries, "a", "ArrowUp"), "c");
assert.equal(registry.nextEntry(entries, "c", "ArrowDown"), "a");
assert.equal(registry.nextEntry(entries, "b", "Home"), "a");
assert.equal(registry.nextEntry(entries, "a", "End"), "c");
assert.equal(registry.nextEntry(entries, "a", "Tab"), null);
// Restoring: the requested page, else the remembered one, else General; anchors find their page.
assert.equal(registry.resolvePage("nope", "data").id, "data");
assert.equal(registry.resolvePage(null, undefined).id, "general");
assert.equal(registry.pageOfAnchor("hooks").id, "integrations");
assert.equal(registry.pageOfAnchor("cost").id, "cost");
assert.equal(registry.pageOfAnchor("updates").id, "about");
assert.equal(registry.pageOfAnchor("nope"), null);

// Settings is not modal, so the accounts can change under the history's account list: the page is drawn again
// on a new configuration generation, and one account's export or erasure carries the generation of its index.
const data = modules.get("data");
assert.equal(data.viewChanged({ generation: 2 }, { generation: 1 }), true);
assert.equal(data.viewChanged({ generation: 2 }, { generation: 2 }), false);
const live = (await lib.getView()).generation;
await assert.rejects(lib.historyErase(0, live + 1), (e) => e === lib.STALE, "an erase of another generation must be refused");
await assert.rejects(lib.historyExport(0, null, "csv", "/tmp/x.csv"), (e) => e === lib.STALE, "an export without its generation must be refused");
assert.equal(await lib.historyExport(null, null, "csv", "/tmp/x.csv"), 42, "every account needs no generation");
assert.equal(await lib.historyErase(0, live), 7);

// RF1.5: the browser demo opens settings.html in one named window, with the page and anchor in the query.
await lib.openSettingsWindow({ page: "cost", anchor: "cost" });
await lib.openSettingsWindow();
assert.deepEqual(opened, [["settings.html?page=cost&anchor=cost", "cuotai-settings"], ["settings.html", "cuotai-settings"]]);

// RF1.1/RF1.4: the page exists, loads the shell, and the window is in the capability (and nothing else changed).
const html = await readFile(ui("settings.html"), "utf8");
assert.match(html, /<script type="module" src="js\/settings\/window\.js"><\/script>/);
assert.match(html, /href="css\/settings\.css"/);
const index = await readFile(ui("index.html"), "utf8");
const bootstrap = (page) => page.split("<script>", 2)[1].split("</script>", 1)[0];
assert.equal(bootstrap(html), bootstrap(index), "settings.html must apply language and theme before first paint like index.html");
const capability = JSON.parse(await readFile(fileURLToPath(new URL("apps/desktop/src-tauri/capabilities/default.json", root)), "utf8"));
assert.deepEqual(capability.windows, ["main", "tray", "settings"]);
// RF10.1 (as far as RF1 needs): the old dialog and its event are gone.
for (const file of await readdir(ui("js"))) {
  if (!file.endsWith(".js")) continue;
  const text = await readFile(ui(`js/${file}`), "utf8");
  assert.ok(!text.includes('"./settings.js"'), `${file} still imports the old dialog`);
  assert.ok(!text.includes('"open-settings"'), `${file} still listens to open-settings`);
}

// Every control of the old dialog renders on its page, in every built-in catalog, with no raw key shown.
const view = await lib.getView();
const states = {
  cost: async () => costHtml(await lib.costStatus(), await lib.costPrices()),
  serve: async () => integrationHtml(await lib.serveStatus()),
  hooks: async () => { const hv = await lib.hooksStatus(); return hooksHtml(hv, hv.rules ?? [], false); },
  widgets: async () => widgetsHtml(await lib.widgetsStatus()),
};
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const catalog = JSON.parse(await readFile(ui(`locales/${code}.json`), "utf8"));
  const ctx = { settings: await lib.getSettings(), update: await lib.updaterStatus(), view: () => view, changed: () => {}, rerender: () => {} };
  const nav = registry.sidebarHtml("general", true);
  for (const page of PAGES) {
    assert.ok(catalog[page.label], `${code}: ${page.label} missing`);
    assert.ok(nav.includes(lib.esc(catalog[page.label])), `${code}: ${page.id} label not in the sidebar`);
  }
  assert.ok(catalog["settings.page.advanced"] && catalog["settings.nav.label"] && catalog["settings.display.title"], `${code}: shell keys missing`);
  const rendered = new Map();
  for (const page of PAGES) {
    const out = modules.get(page.id).html(ctx);
    assert.ok(!/settings\.(page|nav|display)\./.test(out), `${code}/${page.id}: untranslated key`);
    for (const section of page.sections) {
      assert.ok(out.includes(`id="set-${section}"`) || out.includes(`data-section="${section}"`) || out.includes(`data-${section}`),
        `${code}/${page.id}: no anchor for ${section}`);
    }
    rendered.set(page.id, out);
  }
  const drawn = new Map();
  for (const control of CONTROLS) {
    const out = control.async
      ? (drawn.get(control.async) ?? drawn.set(control.async, await states[control.async]()).get(control.async))
      : rendered.get(control.page);
    assert.ok(out.includes(control.selector), `${code}: ${control.section} control ${control.selector} missing on ${control.page}`);
    if (!control.async) {
      for (const [other, html] of rendered) {
        if (other !== control.page && !control.selector.startsWith("data-")) assert.ok(!html.includes(control.selector), `${code}: ${control.selector} also on ${other}`);
      }
    }
  }
  console.log(`${code}: ${PAGES.length} pages, ${CONTROLS.length} controls rendered`);
}

console.log("PASS: Settings window pages follow DS-CMP-SEG-5, the sidebar navigates, and every old dialog control renders on its page in pt-BR, en-US, es and it");
