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
// PRD 0029 re-validation (V-01): what was typed for a peer comes back with that peer, never silently dropped.
assert.match(dialog, /const key = peer \? `\$\{provider\}:\$\{peer\.index\}` : null;\s*if \(key !== peerKey\) \{\s*if \(peerKey\) peerValues\.set\(peerKey, peerInput\.value\);\s*peerInput\.value = key \? peerValues\.get\(key\) \?\? label : "";/);
// V-04: "(optional)" only while the name is.
assert.match(dialog, /#name-label"\)\.textContent = nameNeeded \? t\("dialog\.name"\) : t\("dialog\.nameOptional"\)/);
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
assert.match(dialog, /t\("dialog\.intervalPlaceholderMetered", \{ default: bounds\.default_seconds, min: bounds\.effective_min \?\? bounds\.min \}\)/);
const bedrockSchema = await lib.formSchema("aws_bedrock");
assert.deepEqual(bedrockSchema.interval, { min: 300, max: 86400, default_seconds: 3600, effective_min: 900 });
assert.equal(i18n.t("dialog.intervalPlaceholderMetered", { default: 3600, min: 900 }), "empty = 3600 (min. 900)");
// PRD 0033 re-validation V-05: the real default and floor sit next to the field, and the advisory is linked to it.
assert.match(dialog, /aria-describedby="interval-hint cost-warn"/);
for (const file of locales) {
  const catalog = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"));
  const text = catalog["dialog.intervalHintMetered"];
  assert.ok(typeof text === "string" && ["{default}", "{effective}", "{min}"].every((p) => text.includes(p)), `${file}: dialog.intervalHintMetered`);
  if (file !== "en-US.json") assert.notEqual(text, en["dialog.intervalHintMetered"], `${file}: untranslated`);
}

// PRD 0036 re-validation V-13: a Test connection result carries the core's tone and field; the status is a polite live
// region, colored by its tone, and the field at fault points at it (aria-describedby) and is marked invalid on an error.
assert.match(dialog, /<p class="hint" id="aggregator-status" role="status" aria-live="polite"><\/p>/);
assert.match(dialog, /aggregatorStatus\.dataset\.tone = result\?\.tone \?\? "";/);
assert.match(dialog, /el\.setAttribute\("aria-describedby", \[\.\.\.ids, "aggregator-status"\]\.join\(" "\)\);/);
assert.match(dialog, /if \(result\.tone === "error"\) el\.setAttribute\("aria-invalid", "true"\);/);
assert.match(dialog, /if \(field\.startsWith\("params\."\)\) return dyn\.querySelector\(`\[name="f:\$\{CSS\.escape\(field\.slice\("params\."\.length\)\)\}"\]`\);/);
assert.match(dialog, /showAggregatorResult\(result, tl\(result\.message\)\);/);
const css = await readFile(ui("css/dialog.css"), "utf8");
for (const tone of ["ok", "warn", "error"]) assert.match(css, new RegExp(`#aggregator-status\\[data-tone="${tone}"\\]`));
// V-04: the aggregator form carries the core's 120 s floor and its sentence beside the interval field.
const routerSchema = await lib.formSchema("nine_router");
assert.equal(routerSchema.interval.min, 120);
assert.equal(routerSchema.interval_hint.id, "account_edit.interval_aggregator_hint");
assert.match(dialog, /: schema\?\.interval_hint \? tl\(schema\.interval_hint\) : "";/);
// V-11: a draft the core refuses during Test connection is shown at its field.
assert.match(dialog, /if \(parsed\?\.field\) setFieldError\(parsed\.field, errText\(parsed\)\);/);

// Re-validation S-03 (PRD 0036 RF2.3): a first sighting and a changed pin open different dialogs. The changed one
// shows the address and both fingerprints, focuses Cancel (the primary button), cancels on Enter and Escape, and
// replaces the pin only after "I checked the new fingerprint on the server" is ticked.
for (const file of locales) {
  const catalog = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"));
  for (const key of ["firstTitle", "firstBody", "trust", "changedTitle", "changedBody", "old", "new", "host", "checked", "replace"]) {
    const id = `confirm.certificate.${key}`;
    assert.ok(typeof catalog[id] === "string" && catalog[id].trim(), `${file}: ${id}`);
    if (file !== "en-US.json") assert.notEqual(catalog[id], en[id], `${file}: ${id} is untranslated`);
  }
  for (const gone of ["dialog.aggregator.trust", "dialog.aggregator.trustTitle", "dialog.aggregator.trustBody"]) assert.ok(!(gone in catalog), `${file}: ${gone} is retired`);
}
const opened = [];
function fakeElement(attrs = {}) {
  const listeners = {};
  return {
    ...attrs, dataset: { ...(attrs.dataset ?? {}) }, isConnected: true,
    addEventListener(type, fn) { (listeners[type] ??= []).push(fn); },
    fire(type, extra = {}) { const e = { type, target: this, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, ...extra }; for (const fn of listeners[type] ?? []) fn(e); return e; },
    click() { if (!this.disabled) this.fire("click"); },
    focus() { document.activeElement = this; },
  };
}
function fakeDialog() {
  const dlg = fakeElement({ className: "", open: false });
  let html = "", parts = {};
  Object.defineProperty(dlg, "innerHTML", {
    get: () => html,
    set(v) {
      html = String(v);
      parts = { choices: [...html.matchAll(/<button[^>]*data-choice="(\w+)"([^>]*)>/g)].map(([, choice, rest]) => fakeElement({ dataset: { choice }, disabled: /\bdisabled\b/.test(rest), parent: dlg })) };
      if (html.includes('id="cert-checked"')) parts.checked = fakeElement({ checked: false, parent: dlg });
    },
  });
  dlg.querySelector = (sel) => {
    const choice = sel.match(/^\[data-choice="(\w+)"\]$/);
    if (choice) return parts.choices.find((b) => b.dataset.choice === choice[1]) ?? null;
    return sel === "#cert-checked" ? parts.checked ?? null : null;
  };
  dlg.querySelectorAll = (sel) => (sel === "[data-choice]" ? parts.choices : []);
  dlg.showModal = () => { dlg.open = true; };
  dlg.close = () => { if (!dlg.open) return; dlg.open = false; dlg.fire("close"); };
  dlg.remove = () => {};
  dlg.contains = (el) => el?.parent === dlg;
  dlg.animate = () => ({ finished: Promise.resolve() });
  opened.push(dlg);
  return dlg;
}
Object.assign(document, { hidden: true, createElement: () => fakeDialog(), body: { append() {} }, getElementById: () => null, querySelector: () => null, querySelectorAll: () => [] });
document.activeElement = null;
globalThis.MutationObserver = class { observe() {} disconnect() {} };
globalThis.getComputedStyle = () => ({ getPropertyValue: () => "" });
globalThis.CSS = { escape: (s) => s, supports: () => true };
globalThis.requestAnimationFrame = (fn) => setTimeout(fn, 0);
const { confirmCertificate } = await import("../apps/desktop/ui/js/account-dialog.js");
const NEW = "AB:".repeat(31) + "AB", OLD = "CD:".repeat(31) + "CD";
const first = { state: "untrusted", host: "router.example.com", port: 443, fingerprint: NEW, previous: null };
const changed = { state: "changed", host: "router.example.com", port: 8443, fingerprint: NEW, previous: OLD };
const button = (dlg, choice) => dlg.querySelector(`[data-choice="${choice}"]`);
// First sighting: its own title, the address and the fingerprint; Trust and test is the primary button.
let answer = confirmCertificate(first);
let dlg = opened.at(-1);
assert.equal(dlg.dataset.certificate, "first");
assert.ok(dlg.innerHTML.includes(i18n.t("confirm.certificate.firstTitle")));
assert.match(dlg.innerHTML, /router\.example\.com:443/);
assert.ok(dlg.innerHTML.includes(NEW) && !dlg.innerHTML.includes(i18n.t("confirm.certificate.changedTitle")));
assert.match(dlg.innerHTML, /class="btn primary" data-choice="trust"/);
button(dlg, "trust").click();
assert.equal(await answer, "trust");
// Changed: another dialog, with both fingerprints; Cancel is primary and focused.
answer = confirmCertificate(changed);
dlg = opened.at(-1);
assert.equal(dlg.dataset.certificate, "changed");
assert.ok(dlg.innerHTML.includes(i18n.t("confirm.certificate.changedTitle")) && !dlg.innerHTML.includes(i18n.t("confirm.certificate.firstTitle")));
assert.ok(dlg.innerHTML.includes(OLD) && dlg.innerHTML.includes(NEW));
assert.match(dlg.innerHTML, /router\.example\.com:8443/);
assert.match(dlg.innerHTML, /class="btn primary" data-choice="cancel"/);
assert.doesNotMatch(dlg.innerHTML, /data-choice="trust"/);
assert.equal(document.activeElement, button(dlg, "cancel"), "Cancel has the initial focus");
// Replace does nothing until the box is ticked; Enter on the checkbox cancels.
assert.ok(button(dlg, "replace").disabled);
button(dlg, "replace").fire("click");
assert.ok(dlg.open, "an unticked Replace pins nothing");
const enter = dlg.fire("keydown", { key: "Enter", target: dlg.querySelector("#cert-checked") });
assert.ok(enter.defaultPrevented);
assert.equal(await answer, "cancel");
// Escape (the dialog's own close) cancels.
answer = confirmCertificate(changed);
opened.at(-1).close();
assert.equal(await answer, "cancel");
// Ticked, then Replace: the only way to replace the pin.
answer = confirmCertificate(changed);
dlg = opened.at(-1);
const box = dlg.querySelector("#cert-checked");
box.checked = true;
box.fire("change");
assert.ok(!button(dlg, "replace").disabled);
// Enter on the enabled Replace button is left to that button (a deliberate press).
assert.ok(!dlg.fire("keydown", { key: "Enter", target: button(dlg, "replace") }).defaultPrevented);
assert.ok(dlg.open, "Enter on Replace is left to the button itself");
button(dlg, "replace").click();
assert.equal(await answer, "replace");
// The dialog asks for the right thing per state: the test result's certificate, never a bare fingerprint.
assert.match(dialog, /const offer = result\.ok \? null : result\.certificate;/);
assert.match(dialog, /\["trust", "replace"\]\.includes\(await confirmCertificate\(offer\)\)/);
console.log("account dialog auth UI: ok");
