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
  for (const key of KEYS) assert.ok(typeof catalog[key] === "string" && catalog[key], `${code}: ${key} missing`);
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
console.log("provider picker UI smoke: ok");
