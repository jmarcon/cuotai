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
// PRD 0001 re-validation (M3): the RF11.3 substitution note sits under the replaced field's row (or the Authentication
// choice when that field is hidden), never in an advisory box at the end of the form.
assert.doesNotMatch(dialog, /id="choice-notice"/);
assert.match(dialog, /notices\.push\(\{ key: resolved\.key, text: tl\(resolved\.notice\) \}\)/);
assert.match(dialog, /dynFields\.querySelector\(`\[data-field="f:\$\{CSS\.escape\(key\)\}"\]`\)\s*\?\? dynAuth\.querySelector\(`\[name="o:\$\{CSS\.escape\(key\)\}"\]`\)\?\.closest\("\.f"\)\s*\?\? dynAuth\.querySelector\("fieldset\.auth"\) \?\? dynFields;/);
assert.match(dialog, /note\.className = "hint choice-notice";\s*note\.setAttribute\("role", "status"\);/);
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
// PRD 0001 re-validation (C1): every command aimed at an existing account carries its stable id, and both
// reloads find the account again by that id (closing with a notice when it is gone) instead of reusing the
// index of the view from before the reload.
for (const file of locales) {
  const catalog = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"));
  assert.ok(typeof catalog["dialog.accountGone"] === "string" && catalog["dialog.accountGone"].trim(), `${file}: dialog.accountGone`);
  if (file !== "en-US.json") assert.notEqual(catalog["dialog.accountGone"], en["dialog.accountGone"], `${file}: dialog.accountGone is untranslated`);
}
assert.match(dialog, /accountId = \(await getAccount\(index\)\)\?\.account_id \?\? null;/);
assert.match(dialog, /await reloadConfig\(\);\s*if \(accountId != null\) \{\s*index = await accountIndex\(accountId\);\s*if \(index == null\) \{\s*await noticeAccountGone\(\);\s*return;/);
assert.match(dialog, /const view = await reloadConfig\(\);[\s\S]{0,200}const found = await accountIndex\(accountId\);\s*if \(found == null\) \{\s*dlg\.close\(\);\s*await noticeAccountGone\(\);\s*return;\s*\}\s*index = found;/);
assert.match(dialog, /index, accountId, provider, name:/);
assert.match(dialog, /loginStart\(\{ provider, index, accountId, mode: mode\.id/);
// PRD 0054 RF6.1: `LoginStartInput` is camelCase, so the draft's program path travels as `cliPath`.
assert.match(dialog, /loginStart\(\{[^}]*\bcliPath \}\)/);
assert.doesNotMatch(dialog, /cli_path: cliPath/);
assert.match(dialog, /deleteAccount\(index, btn\.dataset\.purge === "1", accountId\)/);
assert.match(dialog, /parsed\.id === "edit_error\.conflict" \|\| parsed\.id === "edit_error\.account_moved"/);
const main = await readFile(ui("js/main.js"), "utf8");
assert.equal((main.match(/accountId: accountIdAt\(index\)/g) ?? []).length, 2, "both removal paths bind the account id");
const lib = await import("../apps/desktop/ui/js/lib.js");
assert.equal(await lib.accountIndex("acc_demo00000001"), 1);
assert.equal(await lib.accountIndex("acc_gone"), null);
// PRD 0001 re-validation (L2): the interval placeholder follows the core's bounds of the provider (`schema.interval`),
// so a provider billed per query shows its own default and minimum.
for (const file of locales) {
  const catalog = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"));
  const text = catalog["dialog.intervalPlaceholderMetered"];
  assert.ok(typeof text === "string" && text.includes("{default}") && text.includes("{min}"), `${file}: dialog.intervalPlaceholderMetered`);
  if (file !== "en-US.json") assert.notEqual(text, en["dialog.intervalPlaceholderMetered"], `${file}: untranslated`);
}
assert.match(dialog, /placeholder="\$\{esc\(intervalPlaceholder\(\)\)\}"/);
assert.match(dialog, /async function renderDynamic\(\) \{\s*form\.querySelector\("\[name=interval\]"\)\.placeholder = intervalPlaceholder\(\);/);
assert.match(dialog, /t\("dialog\.intervalPlaceholderMetered", \{ default: bounds\.default_seconds, min: bounds\.min \}\)/);
const bedrockSchema = await lib.formSchema("aws_bedrock");
assert.deepEqual(bedrockSchema.interval, { min: 300, max: 86400, default_seconds: 3600 });
assert.equal(i18n.t("dialog.intervalPlaceholderMetered", { default: 3600, min: 300 }), "provider default 3600 (min. 300)");
console.log("account dialog auth UI: ok");
