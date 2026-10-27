#!/usr/bin/env node
// Browser-free UI smoke: the "Create a key on <host>" link under a typed key in the account dialog. Every
// catalog carries the text with its {site} placeholder, the link carries only the core's HTTPS `key_page`,
// and the dialog renders it in the secret row and opens it through `openExternal` (the shell allow-list).
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const KEY = "dialog.secret.createKey";

const locales = (await readdir(ui("locales"))).filter((f) => f.endsWith(".json"));
assert.equal(locales.length, 14);
const en = JSON.parse(await readFile(ui("locales/en-US.json"), "utf8"));
for (const file of locales) {
  const catalog = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"));
  assert.ok(typeof catalog[KEY] === "string" && catalog[KEY].includes("{site}"), `${file}: ${KEY}`);
  if (file !== "en-US.json") assert.notEqual(catalog[KEY], en[KEY], `${file}: ${KEY} is untranslated`);
}
assert.equal(JSON.parse(await readFile(ui("locales/pt-BR.json"), "utf8"))[KEY], "Criar chave em {site}");

const storage = new Map();
globalThis.window = {};
globalThis.document = { hidden: false, documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} }, addEventListener: () => {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (k) => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, v) };
globalThis.fetch = async (relative) => {
  try {
    const data = JSON.parse(await readFile(ui(relative), "utf8"));
    return { ok: true, json: async () => data };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const { keyPageLinkHtml } = await import("../apps/desktop/ui/js/lib.js");
await i18n.initI18n();

const html = keyPageLinkHtml({ slot: "credential_env", key_page: "https://console.groq.com/keys" });
assert.match(html, /class="link key-page"/);
assert.match(html, /data-key-url="https:\/\/console\.groq\.com\/keys"/);
assert.match(html, /<span>Create a key on console\.groq\.com<\/span>/);
for (const bad of [undefined, null, "", "http://console.groq.com/keys", "javascript:alert(1)", "https://"]) {
  assert.equal(keyPageLinkHtml({ key_page: bad }), "", String(bad));
}
assert.equal(keyPageLinkHtml(undefined), "");
// An attribute-breaking address is escaped, never injected.
assert.doesNotMatch(keyPageLinkHtml({ key_page: 'https://a.example/"><script>' }), /"><script>/);

// The dialog renders the link in each secret row and opens it only through openExternal.
const dialog = await readFile(ui("js/account-dialog.js"), "utf8");
assert.match(dialog, /\$\{keyPageLinkHtml\(s\)\}/);
assert.match(dialog, /closest\("\[data-key-url\]"\)[\s\S]{0,160}openExternal\(keyPage\.dataset\.keyUrl\)/);
console.log("key page UI: ok");
