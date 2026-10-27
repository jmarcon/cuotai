#!/usr/bin/env node
// Browser-free UI smoke for PRD 0047 G1: the desktop never shows an environment variable or a `params.*` name.
// Every core text that names one (in any core catalog) is either swapped by `desktop-wording.js` or listed below as
// never rendered by the desktop; the swapped wording and every desktop catalog are free of terminal names.
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

import { readdir } from "node:fs/promises";
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const { DESKTOP_WORDING, desktopAction } = await import("../apps/desktop/ui/js/desktop-wording.js");
const { noteText, sessionAction, causeText } = await import("../apps/desktop/ui/js/lib.js");
await i18n.initI18n();

const readJson = async (rel) => JSON.parse(await readFile(fileURLToPath(new URL(rel, root)), "utf8"));
const jsonFiles = async (rel) => (await readdir(fileURLToPath(new URL(rel, root)))).filter((f) => f.endsWith(".json"));
// Terminal wording: a variable or parameter name, or the word "variable" in the shipped languages.
const ENV_NAME = /_env\b|params\.|\$[A-Z_]{3,}/;
const ENV_WORD = /vari[aá]ve(l|is)|variable|variabile|environment|entorno|ambiente|umgebung|переменн|変数|变量|환경 ?변수|(?<!매개)변수|değişken|biến môi trường|पर्यावरण चर/i;
const terminalWording = (text) => ENV_NAME.test(text) || ENV_WORD.test(text);

// Core ids the desktop never renders (or renders on purpose), with the reason.
const NOT_RENDERED = [
  [/^field\..*_env\./, "a `*_env` field is terminal-only (`terminal_only_fields`), never drawn"],
  [/^field\.kiro\./, "Kiro auth-method help: the word \"variable\" in some catalogs names the terminal path only"],
  [/^credential_help\./, "the desktop draws `credential_help` only for an AWS profile (account-dialog.js)"],
  [/^auth_mode\.credential\.label$/, "the desktop labels the credential mode itself"],
  [/^login\.[a-z_]+\.oauth\.registration\.client_secret_env\./, "known gap: own OAuth client registration still asks for a variable name"],
  [/^detect\./, "the desktop device check uses its own consent (`dialog.detect.*`) and checks folders only (G1)"],
  [/^extensions\./, "an extension's manifest lists its own variables (PRD 0038)"],
  [/^access\.credential_hint\.extension$/, "extension access hint, same as above"],
];
const notRendered = (id) => NOT_RENDERED.some(([re]) => re.test(id));

const coreFiles = await jsonFiles("crates/cuotai-core/locales/");
const flagged = new Set();
for (const file of coreFiles) {
  const catalog = await readJson(`crates/cuotai-core/locales/${file}`);
  for (const [id, value] of Object.entries(catalog)) {
    const texts = typeof value === "string" ? [value] : value && typeof value === "object" ? Object.values(value) : [];
    if (texts.some((v) => typeof v === "string" && terminalWording(v))) flagged.add(id);
  }
}
assert.ok(flagged.has("query.router_keys.xiaomi_native_key_has_no_quota"), "the scan finds the Xiaomi native-key note");
const unmapped = [...flagged].filter((id) => !DESKTOP_WORDING[id] && !notRendered(id)).sort();
assert.deepEqual(unmapped, [], `core texts naming a variable reach the desktop unmapped:\n${unmapped.join("\n")}`);

// The desktop catalogs: every swap target exists everywhere and is GUI-safe; no value names a variable.
const uiFiles = await jsonFiles("apps/desktop/ui/locales/");
const targets = new Set(Object.values(DESKTOP_WORDING).map((e) => e.id));
for (const file of uiFiles) {
  const catalog = await readJson(`apps/desktop/ui/locales/${file}`);
  for (const id of [...targets, "card.connectSession", "dialog.session.connect"]) {
    assert.equal(typeof catalog[id], "string", `${file}: ${id} missing`);
    assert.ok(!terminalWording(catalog[id]), `${file}: ${id} names a variable: ${catalog[id]}`);
  }
  for (const [id, value] of Object.entries(catalog)) {
    if (typeof value === "string") assert.ok(!ENV_NAME.test(value), `${file}: ${id} names a variable or parameter: ${value}`);
  }
}
for (const entry of Object.values(DESKTOP_WORDING)) assert.ok(!terminalWording(entry.fallback), `fallback names a variable: ${entry.fallback}`);

// Rendering: the Xiaomi native-key note, in every locale, reads in plain words and offers "Connect session".
const xiaomi = {
  provider: "xiaomi_mimo", status: "configuration_error", paste_session: false,
  notes: ["Xiaomi MiMo: ... params.session_env ..."],
  notes_l10n: [{ id: "query.router_keys.xiaomi_native_key_has_no_quota", params: {}, fallback: "Xiaomi MiMo: ... params.session_env or params.platform_session_env." }],
};
assert.equal(desktopAction(xiaomi.notes_l10n[0]), "session");
assert.equal(sessionAction(xiaomi), "connect");
assert.equal(sessionAction({ ...xiaomi, paste_session: true }), "paste");
assert.equal(sessionAction({ ...xiaomi, status: "ready" }), null);
assert.equal(sessionAction({ ...xiaomi, notes_l10n: [{ id: "query.missing_param", params: { key: "region" }, fallback: "" }] }), null);
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const text = noteText(xiaomi, 0);
  assert.ok(text && !terminalWording(text), `${code}: ${text}`);
  assert.equal(causeText(xiaomi), text);
  for (const id of flagged) {
    if (notRendered(id)) continue;
    const shown = i18n.tl({ id, params: { key: "region", env: "X_ENV", name: "X_ENV", slot: "session_env", label: "L", hosts: "example.com" }, fallback: "params.x" });
    assert.ok(!terminalWording(shown), `${code}: ${id} renders terminal wording: ${shown}`);
  }
}
await i18n.setLocale("pt-BR");
assert.equal(noteText(xiaomi, 0), "Esta chave não mostra o consumo. Para ver a quota, conecte a sessão da sua conta Xiaomi.");
console.log("desktop wording: ok");
