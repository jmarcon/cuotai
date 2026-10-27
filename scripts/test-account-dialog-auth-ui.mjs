#!/usr/bin/env node
// Browser-free UI smoke for the account dialog's peer name and authentication section: the unnamed peer's
// field is prefilled from the peer itself and explained by a hint, a provider with a web-session slot never
// shows "no credential to enter", the session box reads as an alternative, and a missing CLI links its
// official install page (the core's `install`, opened through `openExternal`).
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));

const locales = (await readdir(ui("locales"))).filter((f) => f.endsWith(".json"));
assert.equal(locales.length, 14);
const en = JSON.parse(await readFile(ui("locales/en-US.json"), "utf8"));
for (const file of locales) {
  const catalog = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"));
  for (const key of ["dialog.peerHint", "dialog.session.orIntro"]) {
    assert.ok(typeof catalog[key] === "string" && catalog[key].trim(), `${file}: ${key}`);
    if (file !== "en-US.json") assert.notEqual(catalog[key], en[key], `${file}: ${key} is untranslated`);
  }
  assert.ok(catalog["dialog.session.orIntro"].includes("{provider}"), `${file}: orIntro placeholder`);
}
const pt = JSON.parse(await readFile(ui("locales/pt-BR.json"), "utf8"));
assert.match(pt["dialog.peerHint"], /^Você já tem uma conta deste provedor sem nome/);

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
const { installLinkHtml } = await import("../apps/desktop/ui/js/lib.js");
await i18n.initI18n();

const url = "https://www.alibabacloud.com/help/en/model-studio/cli/installation";
const html = installLinkHtml({ install: { label: { id: "login.install.bl", fallback: "Install the bl CLI" }, url } });
assert.match(html, /data-key-url="https:\/\/www\.alibabacloud\.com\/help\/en\/model-studio\/cli\/installation"/);
assert.match(html, /Install the bl CLI/);
for (const bad of [undefined, null, {}, { install: { url: "http://x.example" } }, { install: { url: "javascript:alert(1)" } }]) {
  assert.equal(installLinkHtml(bad), "", JSON.stringify(bad));
}

const dialog = await readFile(ui("js/account-dialog.js"), "utf8");
// Peer: prefilled whenever the peer changes (never only on a visibility flip), with a placeholder and a hint.
assert.match(dialog, /const key = peer \? `\$\{provider\}:\$\{peer\.index\}` : null;\s*if \(key !== peerKey\) peerInput\.value = peer \? label : "";/);
assert.doesNotMatch(dialog, /Boolean\(peer\) !== peerShown/);
assert.match(dialog, /peerInput\.placeholder = label;/);
assert.match(dialog, /#peer-hint"\)\.textContent = t\("dialog\.peerHint"\)/);
// credNone is never shown beside a session slot.
assert.match(dialog, /\(schema\.sessions \?\? \[\]\)\.length \? "" : te\("dialog\.credNone"\)/);
// The session box introduces itself as the alternative when another way to connect exists.
assert.match(dialog, /const alternative = slots\.length && \(modes\(\)\.length \|\| schema\.credential_kind !== "none"\);/);
assert.match(dialog, /te\("dialog\.session\.orIntro", \{ provider: providerLabel\(\) \}\)/);
// The install link sits beside each mode and opens through the shell allow-list handler.
assert.equal((dialog.match(/\$\{installLinkHtml\(m\)\}/g) ?? []).length, 2);
assert.match(dialog, /closest\("\[data-key-url\]"\)[\s\S]{0,160}openExternal\(keyPage\.dataset\.keyUrl\)/);
console.log("account dialog auth UI: ok");
