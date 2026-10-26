#!/usr/bin/env node
// Browser-free UI smoke for PRD 0020 RF1 and RF2: the Settings window's page registry follows DS-CMP-SEG-5, its
// sidebar navigation (current page, Advanced disclosure, arrow keys) behaves, every section of the old 12-section
// dialog maps to a page, every control of the old dialog renders on its page in every built-in catalog, and the
// pages keep the RF2.2/RF3/RF5 limits (switches only, 8 rows, one-line descriptions, 4-word labels, word budget).
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

const long = [];
// Vietnamese spaces separate syllables, not words: a label or a page holds about twice as many space-separated tokens.
const SYLLABLE_FACTOR = { vi: 2 };
// Product and source names count as one word.
const names = [...(await lib.costStatus()).sources.map((x) => x.label), /^Cuotai \S+/];
const { PAGES, ADVANCED, LEGACY_SECTIONS, CONTROLS } = registry;
const ids = PAGES.map((p) => p.id);

// RF2.5 / DS-CMP-SEG-5: order, Advanced group and About at the bottom. 
const SEG5 = ["general", "dashboard", "menubar", "alerts", "data", "integrations", "cost", "extensions", "files", "about"];
assert.deepEqual(ids, SEG5.filter((id) => ids.includes(id)), "pages are out of the DS-CMP-SEG-5 order");
assert.deepEqual(ids.slice(0, 5), ["general", "dashboard", "menubar", "alerts", "data"], "everyday pages drifted");
assert.deepEqual(PAGES.filter((p) => p.group === ADVANCED).map((p) => p.id), ["integrations", "cost", "extensions", "files"], "Advanced group drifted");
assert.deepEqual(PAGES.filter((p) => p.bottom).map((p) => p.id), ["about"], "About must be the one page at the bottom");
assert.equal(new Set(ids).size, ids.length, "duplicate page id");
const sections = PAGES.flatMap((p) => p.sections);
assert.equal(new Set(sections).size, sections.length, "a section is listed on two pages");

// RF2 acceptance: the 12 sections of the old dialog (plus Updates) all land on a registered page, and the
// per-control checklist agrees with that map.
assert.deepEqual(Object.keys(LEGACY_SECTIONS).sort(), ["about", "appearance", "config", "cost", "extensions", "folder", "history", "integration",
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
// PRD 0025 RF5 acceptance: with Advanced collapsed, five everyday pages plus About.
assert.deepEqual(listed(nav), ["general", "dashboard", "menubar", "alerts", "data", "about"]);
assert.match(nav, /data-advanced-toggle aria-expanded="false"/);
assert.match(nav, /<nav class="set-nav" aria-label="/);
nav = registry.sidebarHtml("cost", false);
assert.deepEqual(listed(nav), ["general", "dashboard", "menubar", "alerts", "data", "cost", "about"], "the current Advanced page must stay listed");
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
  cost: async () => costHtml(await lib.costStatus(), await lib.costPrices(), await lib.costPlans()),
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
  // RF2.2/RF3/RF5: each page drawn with its async parts as the window shows it at first (sub-pages closed,
  // disclosures closed, popovers hidden).
  const filled = new Map();
  const fills = { cost: ["data-cost-body", "cost"], integrations: ["data-serve", "serve", "data-hooks", "hooks", "data-widgets", "widgets"] };
  let total = 0;
  for (const page of PAGES) {
    let out = rendered.get(page.id);
    const f = fills[page.id] ?? [];
    for (let i = 0; i < f.length; i += 2) {
      const state = drawn.get(f[i + 1]) ?? (await states[f[i + 1]]());
      out = out.replace(new RegExp(`<div ${f[i]}><p class="hint">[^<]*</p></div>`), `<div ${f[i]}>${state}</div>`);
      assert.ok(out.includes(state), `${code}/${page.id}: ${f[i]} not filled`);
    }
    filled.set(page.id, out);
    // RF3.1/RF3.2: no raw check box (only switches and membership checks), no old Apply/Save buttons (RF4.2).
    for (const box of out.match(/<input[^>]*type="checkbox"[^>]*>/g) ?? []) {
      assert.ok(/class="(sw|chk)"/.test(box), `${code}/${page.id}: raw check box ${box}`);
      if (box.includes('class="sw"')) assert.match(box, /role="switch"/, `${code}/${page.id}: switch without role`);
    }
    assert.ok(!/<label class="check"><input id=/.test(out), `${code}/${page.id}: hand-written setting row`);
    const shown = visible(out);
    // RF2.2: at most 8 rows before a disclosure or sub-page.
    const rows = shown.match(/class="set-row[" ]/g)?.length ?? 0;
    assert.ok(rows <= 8, `${code}/${page.id}: ${rows} rows shown`);
    // RF5.1: row descriptions fit one line (90 characters); the page text before any disclosure stays short.
    for (const [, desc] of shown.matchAll(/<span class="set-row-desc"[^>]*>([\s\S]*?)<\/span>\s*<\/div>/g)) {
      const text = plain(desc);
      assert.ok(text.length <= 90, `${code}/${page.id}: description over 90 characters: ${text}`);
    }
    // RF5.1: a row label has at most 4 words (a source or version name counts as one).
    for (const [, label] of shown.matchAll(/<span class="set-row-label"[^>]*>([\s\S]*?)<\/span>/g)) {
      const n = names.reduce((text, name) => text.replace(name, "X"), plain(label)).split(/\s+/).filter(Boolean).length;
      if (n > 4 * (SYLLABLE_FACTOR[code] ?? 1)) long.push(`${code}/${page.id}: ${plain(label)}`);
    }
    const words = plain(shown).split(/\s+/).filter(Boolean).length;
    assert.ok(words <= 150 * (SYLLABLE_FACTOR[code] ?? 1), `${code}/${page.id}: ${words} words shown before any disclosure`);
    total += words;
  }
  // Budget 450 -> 490 (audit #16: helper lines show as text; the setup row moved to About): the Startup group of mockup 50-settings-general (audit #37) adds one heading and one row.
  // PRD 0035 RF9.1: Hindi and Indonesian run longer than English in words (separate particles and
  // postpositions) but short per word, so their page-text budget counts 10 % more words.
  const wordy = { "hi-IN": 1.1, "id-ID": 1.1 }[code] ?? 1;
  assert.ok(total <= Math.round(490 * wordy) * (SYLLABLE_FACTOR[code] ?? 1), `${code}: ${total} words shown across the pages`);
  // PRD 0032 RF2.3/RF7.1: the quiet-hours row (with its search keywords) and its weekday editor, and the private
  // switch, render translated: seven weekday rows, labelled time fields, "Same every day", up to three intervals.
  const alertsPage = rendered.get("alerts");
  assert.ok(alertsPage.includes('data-row="quiet"') && /data-row="quiet"[^>]*data-kw="[^"]+"/.test(alertsPage), `${code}: quiet row or its keywords missing`);
  for (const id of ['id="quiet-on"', 'id="quiet-digest"', 'id="notify-private"', "data-quiet-days"]) assert.ok(alertsPage.includes(id), `${code}: ${id} missing from the Alerts page`);
  const { quietDaysHtml, quietSummary } = modules.get("alerts");
  const week = Object.fromEntries(["mon", "tue", "wed", "thu", "fri", "sat", "sun"].map((day) => [day, [{ from: "22:00", to: "08:00" }]]));
  week.mon.push({ from: "12:00", to: "13:00" }, { from: "15:00", to: "16:00" });
  const editor = quietDaysHtml({ enabled: true, digest: true, days: week });
  assert.equal(editor.match(/data-row="quiet-(mon|tue|wed|thu|fri|sat|sun)"/g)?.length, 7, `${code}: seven weekday rows`);
  assert.equal(editor.match(/type="time"/g)?.length, 2 * (3 + 6), `${code}: a labelled from/to pair per interval`);
  assert.ok(!editor.match(/<input type="time"(?![^>]*aria-label)/), `${code}: unlabelled time field`);
  assert.match(editor, /data-quiet-add="mon"[^>]*disabled/, `${code}: the fourth interval is refused`);
  assert.ok(editor.includes("data-quiet-same") && !/settings\.quiet\./.test(editor), `${code}: same-every-day shortcut or untranslated key`);
  assert.ok(quietSummary({ enabled: false, digest: true, days: {} }) !== quietSummary({ enabled: true, digest: true, days: {} }), `${code}: off and unset read differently`);
  console.log(`${code}: ${PAGES.length} pages, ${CONTROLS.length} controls rendered, ${total} words shown`);
}

/** The markup with what the window hides at first removed: popovers, closed disclosures' bodies, sub-pages and
 *  hidden blocks. */
function visible(html) {
  let out = html;
  const cut = (open, tag) => {
    for (let at = out.search(open); at >= 0; at = out.search(open)) {
      let depth = 0;
      const re = new RegExp(`<${tag}\\b|</${tag}>`, "g");
      re.lastIndex = at;
      for (let m = re.exec(out); m; m = re.exec(out)) {
        depth += m[0].startsWith("</") ? -1 : 1;
        if (depth === 0) { out = out.slice(0, at) + out.slice(re.lastIndex); break; }
      }
      if (depth !== 0) throw new Error(`unbalanced ${tag}`);
    }
  };
  cut(/<div class="set-pop"/, "div");
  cut(/<div class="set-subpage"/, "div");
  cut(/<div[^>]*\shidden>/, "div");
  out = out.replace(/(<details class="set-disc"[^>]*>[\s\S]*?<\/summary>)/g, "$1<!--closed-->");
  cut(/<!--closed--><div class="set-disc-body">/, "div");
  return out;
}
function plain(html) {
  return html.replace(/<[^>]+>/g, " ").replace(/&[a-z#0-9]+;/g, " ").replace(/\s+/g, " ").trim();
}

// Mockups 50-*: Advanced is a collapsible group label (not a page entry), General has the sliders icon, the
// "Open at login" row (audit #37) sits in General with search keywords in every catalog.
{
  const side = registry.sidebarHtml("general", false);
  assert.match(side, /class="set-nav-item set-nav-toggle set-nav-grouplabel"/, "Advanced is a group label");
  assert.match(side, /<ul class="set-nav-list set-nav-sub" id="set-nav-advanced"><\/ul>/, "closed group lists no page");
  assert.equal(registry.PAGES.find((p) => p.id === "general").icon, "sliders");
  assert.ok(registry.PAGES.find((p) => p.id === "general").sections.includes("startup"));
  const general = await import("../apps/desktop/ui/js/settings/general.js");
  for (const { code } of i18n.LOCALES) {
    await i18n.setLocale(code);
    const html = general.html({ settings: { motion: "system" } });
    assert.match(html, /<section class="set-sec" id="set-startup"/, `${code}: Startup group`);
    assert.match(html, /<input id="autostart" type="checkbox" role="switch" class="sw"[^>]*disabled/, `${code}: autostart switch waits for the system`);
    assert.match(html, /data-row="autostart" data-kw="[^"]{12,}"/, `${code}: autostart row has search keywords`);
    const catalog = JSON.parse(await readFile(ui(`locales/${code}.json`), "utf8"));
    for (const k of ["settings.autostart.label", "settings.autostart.desc", "settings.startup", "settings.error.autostart_failed", "report.intro"]) assert.ok(catalog[k], `${code}: ${k}`);
  }
  // "Show in Dock" (mockup 50-settings-general): only on macOS, beside Open at login, searchable, on by default.
  assert.ok(!general.html({ settings: { motion: "system" } }).includes('id="dock"'), "no Dock row off macOS");
  document.documentElement.dataset.platform = "mac";
  for (const { code } of i18n.LOCALES) {
    await i18n.setLocale(code);
    const html = general.html({ settings: { motion: "system" } });
    assert.match(html, /data-row="autostart"[\s\S]*data-row="dock" data-kw="[^"]{12,}"/, `${code}: Dock row after Open at login`);
    assert.match(html, /<input id="dock" type="checkbox" role="switch" class="sw" checked/, `${code}: shown by default`);
    assert.ok(!general.html({ settings: { motion: "system", show_in_dock: false } }).match(/id="dock"[^>]*checked/), `${code}: hidden choice`);
  }
  delete document.documentElement.dataset.platform;
  await lib.setPreference("show_in_dock", false);
  assert.equal((await lib.getSettings()).show_in_dock, false);
  await assert.rejects(lib.setPreference("show_in_dock", "no"));
  await lib.setPreference("show_in_dock", true);
  await i18n.setLocale("en-US");
  // The demo answers like the shell: the system's state comes back from the write.
  assert.equal(await lib.autostartStatus(), false);
  assert.equal(await lib.autostartSet(true), true);
  assert.equal(await lib.autostartStatus(), true);
  assert.equal(await lib.autostartSet(false), false);
  // The three dialogs share the one shell (title, ×, body, footer).
  const { shellHtml } = await import("../apps/desktop/ui/js/dialog-shell.js");
  const shell = shellHtml({ id: "x-title", title: "A <b>", body: "<p>b</p>", foot: "<button>ok</button>" });
  assert.match(shell, /<h2 id="x-title">A &lt;b&gt;<\/h2>/);
  assert.match(shell, /data-shell-close/);
  assert.match(shell, /<div class="dlg-shell-foot"><button>ok<\/button><\/div>/);
  const { stepHtml } = await import("../apps/desktop/ui/js/first-run.js");
  assert.match(stepHtml({ updatePending: true }), /dlg-shell-head[\s\S]*dlg-shell-body[\s\S]*first-run-update[\s\S]*dlg-shell-foot[\s\S]*data-first-on/);
}
assert.deepEqual(long, [], "row labels over 4 words");
console.log("PASS: Settings window pages follow DS-CMP-SEG-5, the sidebar navigates, every old dialog control renders on its page, and the rows keep the RF3/RF5 limits in pt-BR, en-US, es and it");
