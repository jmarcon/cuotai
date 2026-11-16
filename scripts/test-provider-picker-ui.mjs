#!/usr/bin/env node
// Browser-free UI smoke: the provider picker, the cards and the details
// render no integration-status label (Official, Undocumented, Derived,
// Unavailable) in any built-in locale; that transparency lives in
// docs/providers.md. The picker receives the core's `pickable` flag instead,
// and the details show no "verified N months ago" chip either.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  documentElement: {
    lang: "",
    dataset: {},
    toggleAttribute: () => {},
  },
  addEventListener: () => {},
};
globalThis.matchMedia = () => ({
  matches: false,
  addEventListener: () => {},
});
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
await i18n.initI18n();

const LEVELS = ["official", "undocumented", "derived", "unavailable"];
const session = {
  level: { id: "access.credential.user_session", params: {}, fallback: "Sessão do usuário" },
  warning: { id: "user_session.warning", params: {}, fallback: "Sessão completa." },
};
const report = (level, withSession) => ({
  provider: "open_ai_api",
  account: "work",
  status: "ready",
  notes: [],
  notes_l10n: [],
  access: {
    level,
    label: { id: `access.level.${level}`, params: {}, fallback: level },
    summary: { id: "access.summary.open_ai_api", params: {}, fallback: "Resumo." },
    reason: null,
    explanation: null,
    session: withSession ? session : null,
    // PRD 0028 RF6.2 data the core still sends; the details never show it.
    verified_on: "2026-01-02",
    verification: { id: "access.verified_months", params: { months: 9 }, fallback: "Verificada há 9 meses." },
    verification_stale: true,
  },
});

const providers = await lib.listProviders();
assert.ok(providers.length > 0, "the demo lists providers");
for (const p of providers) {
  assert.equal(typeof p.pickable, "boolean", `${p.id}: pickable flag missing`);
  assert.equal(p.level, undefined, `${p.id}: the picker still receives an access level`);
}
assert.equal(providers.find((p) => p.id === "poolside")?.pickable, false, "a provider without any query is not pickable");
assert.equal(providers.find((p) => p.id === "perplexity")?.pickable, true, "a provider with one queried mode stays pickable");

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const labels = LEVELS.map((level) => i18n.tl({ id: `access.level.${level}`, params: {}, fallback: level }));
  for (const level of LEVELS) {
    for (const withSession of [false, true]) {
      const r = report(level, withSession);
      // The details call it with `full`; the cards without. Neither renders status.
      for (const html of [lib.accessHtml(r), lib.accessHtml(r, { full: true })]) {
        assert.ok(!html.includes("data-verified"), `${code}/${level}: verified chip rendered`);
        assert.ok(!html.includes(i18n.tl(r.access.verification)), `${code}/${level}: verification text rendered`);
        assert.ok(!html.includes(i18n.tl(r.access.summary)), `${code}/${level}: access summary rendered`);
        assert.ok(!html.includes("2026-01-02"), `${code}/${level}: verification date rendered`);
      }
      const html = lib.accessHtml(r, { full: true });
      for (const label of labels) {
        assert.ok(!html.includes(label), `${code}/${level}: access label "${label}" rendered: ${html}`);
      }
      assert.ok(!html.includes("data-access="), `${code}/${level}: access line rendered`);
      assert.equal(html.includes("data-session-notice"), withSession, `${code}/${level}: session notice`);
    }
  }
}
// PRD 0027 RF3: the catalog picker's markup, chips and empty state in every locale, with no status label; the demo
// search answers like the core's shape; the "Request a provider" address is the one the shell allow-lists.
const picker = await import("../apps/desktop/ui/js/provider-picker.js");
const KEYS = ["dialog.picker.search", "dialog.picker.clear", "dialog.picker.count", "dialog.picker.connect", "dialog.picker.metered",
  "dialog.picker.found", "dialog.picker.empty", "dialog.picker.request", "dialog.picker.change", "dialog.picker.otherWays",
  "dialog.detect.ask", "dialog.detect.check", "dialog.detect.notNow", "dialog.detect.dismiss", "settings.detect.on", "settings.detect.help"];
const entries = await lib.searchProviders("", "en-US");
assert.ok(entries.length > 0 && entries.every((e) => e.provider && e.method && e.hint && e.level === undefined), "demo catalog shape");
assert.ok(!entries.some((e) => e.provider === "poolside"), "a provider without any query is not offered");
assert.ok((await lib.searchProviders("CURSÓR", "en-US")).some((e) => e.provider === "cursor"), "case- and accent-insensitive demo search");
assert.deepEqual(await lib.searchProviders("zzzz-none", "en-US"), [], "no match is an empty list");
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const catalog = JSON.parse(await readFile(fileURLToPath(new URL(`apps/desktop/ui/locales/${code}.json`, root)), "utf8"));
  // `dialog.picker.count` is a plural object (C-10).
  for (const key of KEYS) assert.ok((typeof catalog[key] === "string" && catalog[key]) || catalog[key]?.other, `${code}: ${key} missing`);
  const html = picker.pickerHtml("prov-label") + picker.entryChips({ metered: true }, { kind: "cursor_app" });
  assert.ok(html.includes('role="listbox"') && html.includes('role="combobox"'), `${code}: listbox semantics`);
  assert.ok(html.includes(catalog["dialog.picker.metered"]) && html.includes(catalog["dialog.picker.found"]), `${code}: chips`);
  for (const level of LEVELS) {
    const label = i18n.tl({ id: `access.level.${level}`, params: {}, fallback: level });
    assert.ok(!html.includes(label), `${code}: status label "${label}" in the picker`);
  }
}
// PRD 0027 RF6.4: the Kimi CLI, Grok CLI and Zed detections show the found chip with the core hint and the brand logo.
await i18n.setLocale("en-US");
for (const [provider, kind, tool] of lib.DEMO_NATIVE_DETECT) {
  const chip = picker.entryChips({}, { kind, provider, hint: { id: "detect.hint", params: { tool }, fallback: `${tool} is installed on this device` } });
  assert.ok(chip.includes(`title="${tool} is installed on this device"`), `${kind}: hint ${chip}`);
  const mark = lib.logo(provider, 32);
  assert.ok(mark && !mark.includes(`>${provider}<`), `${kind}: brand logo`);
}
// Demo fixtures go through the catalogs: no pt-BR word leaks into the add-account dialog's texts (form schema,
// access draft, login refusal) or the demo accounts' notes when the language is en-US, es or it.
{
  // Words only Portuguese has (checked in every language) and words es/it share with it (checked in en-US only).
  const PT_ONLY = /(?<![\p{L}])(não|variável|demonstração|Organização|Serviço|indisponível|requisições|Região|Pessoal|momento atual)(?![\p{L}])/iu;
  const PT_SHARED = /(?<![\p{L}])(Modo|Opcional|credencial|Consulta|ausente|expirada|Informe|autorizado|Método|perfil|momento|navegador)(?![\p{L}])/iu;
  const PT = { test: (text) => PT_ONLY.test(text) || (i18n.currentLocale() === "en-US" && PT_SHARED.test(text)) };
  const l10n = (node, out = []) => {
    if (Array.isArray(node)) node.forEach((x) => l10n(x, out));
    else if (node && typeof node === "object") {
      if (typeof node.id === "string" && typeof node.fallback === "string") out.push(node);
      Object.values(node).forEach((x) => l10n(x, out));
    }
    return out;
  };
  const refusal = await lib.loginSubmitCode("x").catch((e) => e);
  const values = [...l10n(refusal)];
  const plain = [];
  for (const p of providers) {
    values.push(...l10n(await lib.formSchema(p.id)), ...l10n(await lib.draftAccess(p.id, "credential", {})));
  }
  for (const r of (await lib.getView()).reports) { values.push(...l10n(r.notes_l10n ?? [])); if (!r.notes_l10n?.length) plain.push(...r.notes); }
  assert.ok(values.length > 20, "the walk found the demo's localized texts");
  for (const code of ["en-US", "es", "it"]) {
    await i18n.setLocale(code);
    const leaks = [...new Set(values.filter((v) => PT.test(i18n.tl(v))).map((v) => `${v.id} -> "${i18n.tl(v)}"`))];
    leaks.push(...plain.filter((n) => PT.test(n)).map((n) => `plain note "${n}"`));
    assert.deepEqual(leaks, [], `${code}: Portuguese in the demo fixtures`);
  }
  await i18n.setLocale("en-US");
}
// PRD 0034 RF2: login lines prefer the core's catalog text (`l10n`, `instructions_l10n`) over the English string.
{
  const dialog = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/account-dialog.js", root)), "utf8");
  for (const use of ["tl(ev.instructions_l10n ?? ev.instructions)", "tl(ev.l10n ?? ev.text)", "tl(ev.l10n ?? ev.message)"]) {
    assert.ok(dialog.includes(use), `login events: ${use}`);
  }
  await i18n.setLocale("es");
  const line = i18n.tl({ id: "freshness.age.minutes", params: { n: 3 }, fallback: "{n} min ago" });
  assert.equal(line, "hace 3 min", "core catalog ids resolve in the dialog's language");
  await i18n.setLocale("en-US");
}
// PRD 0005/0027: the dialog keeps the core's provider order (no JS re-sort).
{
  const dialog = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/account-dialog.js", root)), "utf8");
  assert.ok(!/localeCompare/.test(dialog), "account-dialog.js does not re-sort the core's provider order");
  const list = await lib.listProviders();
  const pickable = list.filter((p) => p.pickable).map((p) => p.label.toLowerCase());
  assert.deepEqual(pickable, [...pickable].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0)), "demo list follows the core's lowercase-label order");
}
const about = await readFile(fileURLToPath(new URL("apps/desktop/src-tauri/src/about.rs", root)), "utf8");
assert.ok(about.includes(`"${lib.NEW_PROVIDER_URL}"`), "the new-provider address is allow-listed by open_external");
// PRD 0027 RF3.1: the "Billed per query" warning stays on the account card after creation, from the report's access
// descriptor (`metered`), with the same catalog text as the picker.
const mainSrc = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/main.js", root)), "utf8");
assert.match(mainSrc, /r\.access\?\.metered \? `<span class="mini warn" data-metered-chip>\$\{icon\("warn"\)\}<span class="mini-t">\$\{te\("dialog\.picker\.metered"\)\}/);
const bedrockAccess = (await lib.getView()).reports.map((r) => r.access?.metered);
assert.ok(bedrockAccess.every((m) => m === false), "no demo account is metered by default");
// PRD 0001 re-validation (L3): the desktop names providers as the core does (`list_providers`, then the core
// catalog's `provider.<slug>` in other languages); its own map keeps only the color and the mark.
await lib.loadProviderLabels();
const unnamedKiro = { provider: "kiro", account: "" };
assert.equal(lib.accountTitle(unnamedKiro), "Kiro (AWS)");
assert.equal(lib.brand("nvidia_nim").label, "NVIDIA NIM");
assert.equal(lib.brand("chat_gpt").label, "ChatGPT / Codex");
const libSrc = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/lib.js", root)), "utf8");
assert.doesNotMatch(libSrc, /B\("Kiro AI"|"Claude Code", "#/, "no desktop-only provider names");
const corePt = JSON.parse(await readFile(fileURLToPath(new URL("crates/cuotai-core/locales/pt-BR.json", root)), "utf8"));
await i18n.setLocale("pt-BR");
assert.equal(lib.brand("alibaba_cloud").label, corePt["provider.alibaba_cloud"]);
await i18n.setLocale("en-US");
assert.equal(lib.brand("alibaba_cloud").label, "Alibaba Cloud — billing");
// V-02 (PRD 0005 review): a search only a provider without any usage query answers shows the core's reason
// (`picker_hidden_notice`) and hides "Request a provider"; a search nothing answers keeps the request.
{
  const fake = () => {
    const el = { hidden: false, value: "", textContent: "", innerHTML: "", placeholder: "", dataset: {}, listeners: {},
      classList: { toggle: () => {} }, addEventListener: (type, fn) => { el.listeners[type] = fn; },
      setAttribute: () => {}, removeAttribute: () => {}, querySelectorAll: () => [], querySelector: () => null, focus: () => {} };
    return el;
  };
  const els = new Map();
  const root = { addEventListener: () => {}, querySelector: (sel) => { if (!els.has(sel)) els.set(sel, fake()); return els.get(sel); } };
  const handle = picker.mountPicker(root, { onChoose: () => {} });
  const search = async (query) => { root.querySelector("#picker-q").value = query; await handle.reload(); };
  await search("poolside");
  const text = root.querySelector("#picker-empty-text").textContent;
  assert.ok(!root.querySelector("#picker-empty").hidden, "the empty state shows");
  assert.ok(text.includes("Poolside") && !text.includes("{"), `the reason is shown: ${text}`);
  assert.equal(root.querySelector("#picker-request").hidden, true, "no request for a provider left out on purpose");
  await search("zzzz-none");
  assert.equal(root.querySelector("#picker-request").hidden, false, "a real miss keeps the request");
  assert.equal(root.querySelector("#picker-empty-text").textContent, i18n.t("dialog.picker.empty", { query: "zzzz-none" }));
  assert.equal(await lib.pickerHiddenNotice("openai", "en-US"), null, "an offered provider has no notice");
}
// R-02 / V-04 (PRD 0005 review): the "Save anyway" prompt is set in the warning role with the warn icon and cleared
// when a change withdraws it; the first submit relabels Save as "Save anyway".
{
  const dialogSrc = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/account-dialog.js", root)), "utf8");
  const prompt = dialogSrc.slice(dialogSrc.indexOf("function setSavePrompt"), dialogSrc.indexOf("async function renderAccess"));
  assert.match(prompt, /classList\.toggle\("warn", text != null\)/, "the prompt takes the warning role");
  assert.match(prompt, /icon\("warn"\)/, "the warn icon is the non-color cue");
  assert.match(dialogSrc, /if \(latest\.save_confirmation && !confirmUnavailable\) \{\s*confirmUnavailable = true;\s*setSavePrompt\(tl\(latest\.save_confirmation\)\);\s*ok\.textContent = t\("dialog\.saveAnyway"\);/,
    "the first submit of an Unavailable draft shows the core's prompt and relabels Save");
  assert.match(dialogSrc, /if \(confirmUnavailable\) setSavePrompt\(null\);\s*confirmUnavailable = false;/, "a change withdraws the prompt");
  assert.match(dialogSrc, /dlg\.querySelector\("#ok"\)\.textContent = t\("common\.save"\);/, "a change restores Save");
  const css = await readFile(fileURLToPath(new URL("apps/desktop/ui/css/dialog.css", root)), "utf8");
  assert.match(css, /\.err\.warn \{[^}]*color: var\(--warn\);/, "the warning role's color");
  for (const { code } of i18n.LOCALES) {
    const catalog = JSON.parse(await readFile(fileURLToPath(new URL(`apps/desktop/ui/locales/${code}.json`, root)), "utf8"));
    assert.ok(catalog["dialog.saveAnyway"], `${code}: dialog.saveAnyway`);
  }
}
// PRD 0027 re-validation C-01, C-10, C-11, C-12: a search result row reads "method · hint" (and the found source's
// hint), holds no control (focus stays in the search field), the count is plural-aware, the footer is one sentence,
// and Enter with nothing highlighted picks nothing.
{
  const pickerSrc = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/provider-picker.js", root)), "utf8");
  assert.doesNotMatch(pickerSrc, /\.map\(option\)/, "Array.map's third argument must not reach `grouped`");
  assert.match(pickerSrc, /maxlength="256"/, "the field stops at the core's query bound (C-09)");
  assert.match(pickerSrc, /id="picker-list" role="listbox" tabindex="-1"/, "the list is not a Tab stop");
  const found = { provider: "cursor", kind: "cursor_app", mode: "oauth",
    hint: { id: "detect.hint", params: { tool: "Cursor" }, fallback: "Cursor is installed on this device" },
    consent: { id: "detect.consent.cursor_import", params: {}, fallback: "x" } };
  const fake = () => {
    const el = { hidden: false, value: "", textContent: "", innerHTML: "", placeholder: "", dataset: {}, listeners: {},
      classList: { toggle: () => {} }, addEventListener: (type, fn) => { el.listeners[type] = fn; },
      setAttribute: () => {}, removeAttribute: () => {}, querySelectorAll: () => [], querySelector: () => null, focus: () => {} };
    return el;
  };
  const els = new Map();
  const host = { addEventListener: () => {}, querySelector: (sel) => { if (!els.has(sel)) els.set(sel, fake()); return els.get(sel); } };
  const chosen = [];
  const handle = picker.mountPicker(host, { onChoose: (entry) => chosen.push(entry.provider), detected: () => [found] });
  // Empty query: nothing highlighted, so Enter picks nothing.
  await handle.reload();
  host.querySelector("#picker-q").listeners.keydown({ key: "Enter", preventDefault: () => {} });
  assert.deepEqual(chosen, [], "Enter with nothing highlighted picks nothing");
  host.querySelector("#picker-q").value = "cursor";
  await handle.reload();
  const html = host.querySelector("#picker-list").innerHTML;
  const row = html.slice(html.indexOf('id="pick-0"'));
  const subtitle = row.slice(row.indexOf('class="pick-method"'), row.indexOf("</small>"));
  assert.ok(subtitle.includes(" · "), `a search result reads "method · hint": ${subtitle}`);
  assert.ok(subtitle.includes("Cursor is installed on this device"), "the found source's hint is visible, not a tooltip only");
  assert.ok(row.includes(i18n.t("dialog.picker.found")), "the found chip shows");
  assert.doesNotMatch(row.slice(0, row.indexOf("</div>")), /<button/, "the option row holds no control");
  host.querySelector("#picker-q").listeners.keydown({ key: "Enter", preventDefault: () => {} });
  assert.deepEqual(chosen, ["cursor"], "Enter picks the highlighted result");
  assert.equal(i18n.t("dialog.picker.count", { count: 1 }), "1 provider");
  assert.equal(i18n.t("dialog.picker.count", { count: 3 }), "3 providers");
  const foot = picker.pickerFoot();
  assert.match(foot, /^Not listed\? <a [^>]*data-extensions>Add a provider with an extension<\/a>$/, foot);
  for (const { code } of i18n.LOCALES) {
    const catalog = JSON.parse(await readFile(fileURLToPath(new URL(`apps/desktop/ui/locales/${code}.json`, root)), "utf8"));
    assert.ok(catalog["dialog.picker.notListed"].includes("{link}"), `${code}: notListed has {link}`);
    assert.equal(typeof catalog["dialog.picker.count"], "object", `${code}: the count is a plural object`);
    assert.ok(catalog["dialog.picker.count"].other, `${code}: count.other`);
  }
  // C-05 / C-01: the form's consent line shows only while the detected source's mode is open, is re-evaluated on
  // every mode switch, and carries the keyboard-reachable "Don't suggest this again".
  const dialogSrc = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/account-dialog.js", root)), "utf8");
  assert.match(dialogSrc, /const shown = Boolean\(offer\) && \(offer\.mode == null \|\| offer\.mode === authMode\);/);
  assert.match(dialogSrc, /authMode = e\.target\.value;\s*syncConsent\(\);/, "a mode switch re-evaluates the consent");
  assert.match(dialogSrc, /id="offer-consent"[^]*?data-offer-dismiss/, "the dismiss sits beside the consent line");
  // C-04: one clear button: the native search cancel is hidden in the picker only.
  const css = await readFile(fileURLToPath(new URL("apps/desktop/ui/css/dialog.css", root)), "utf8");
  assert.match(css, /\.picker-search input::-webkit-search-cancel-button \{[^}]*display: none;/);
}
// PRD 0027 re-validation C-07: the browser demo's search is a port of the core's. For every query of the core-generated
// table, in its language, the port returns the core's `(provider, mode)` list in the same order.
{
  const search = await import("../apps/desktop/ui/js/demo-search.js");
  const read = async (path) => JSON.parse(await readFile(fileURLToPath(new URL(path, root)), "utf8"));
  const catalog = await read("apps/desktop/ui/js/demo-catalog.json");
  const { cases } = await read("apps/desktop/tools/catalog-parity.json");
  const core = new Map();
  for (const { locale } of cases) {
    if (locale !== "en-US" && !core.has(locale)) core.set(locale, await read(`crates/cuotai-core/locales/${locale}.json`));
  }
  const resolve = (text, locale) => (locale === "en-US" ? text.fallback : search.resolveWith(core.get(locale), text));
  assert.ok(cases.length > 90, "the parity table covers the RF2 queries in several languages");
  for (const { query, locale, results } of cases) {
    const got = search.searchCatalog(catalog, query, locale, resolve).map((e) => [e.provider, e.mode]);
    assert.deepEqual(got, results, `${locale} ${JSON.stringify(query)}`);
  }
  assert.equal(search.fold("ｃｏｄｅｘ"), "codex");
  assert.equal(search.fold("KİMİ"), "kimi");
  assert.equal(search.fold("APIキー"), "api キー");
  // The demo answers through the port: "api key" lists every key mode, not a name filter's single hit.
  const demoKeys = await lib.searchProviders("api key", "en-US");
  assert.deepEqual(demoKeys.map((e) => [e.provider, e.mode]),
    cases.find((c) => c.query === "api key" && c.locale === "en-US").results);
  assert.ok(demoKeys.length > 10);
  assert.equal((await lib.searchProviders("gemini cli", "en-US"))[0]?.label, "Gemini CLI");
}
console.log("provider picker UI smoke: ok");
