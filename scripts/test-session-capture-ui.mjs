#!/usr/bin/env node
// Browser-free UI smoke for PRD 0048: the "Sign in and capture session" button of the account dialog. Every
// built-in catalog carries its texts, the demo backend falls back to the paste box, and the dialog wires the
// button only to slots the core marks capturable, fills the paste box from the outcome, runs the usual
// preview and test, and cancels a waiting window when the dialog closes. No live sign-in runs here.
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const KEYS = ["capture", "captureHint", "capturing", "captured", "captureCancelled", "captureTimeout",
  "captureUnavailable", "captureBusy"].map((k) => `dialog.session.${k}`);

// RF8.1: every catalog has every text, none empty and none left in English outside en-US.
const locales = (await readdir(ui("locales"))).filter((f) => f.endsWith(".json"));
assert.equal(locales.length, 14);
const en = JSON.parse(await readFile(ui("locales/en-US.json"), "utf8"));
for (const file of locales) {
  const catalog = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"));
  for (const key of KEYS) {
    assert.ok(typeof catalog[key] === "string" && catalog[key].trim(), `${file}: ${key}`);
    if (file !== "en-US.json") assert.notEqual(catalog[key], en[key], `${file}: ${key} is untranslated`);
  }
}
assert.equal(JSON.parse(await readFile(ui("locales/pt-BR.json"), "utf8"))["dialog.session.capture"], "Entrar e capturar sessão");

// The demo backend (browser preview) has no capture window: it answers "unavailable".
const storage = new Map();
globalThis.window = {};
globalThis.document = { hidden: false, documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} }, addEventListener: () => {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (k) => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, v) };
globalThis.fetch = async () => ({ ok: false, json: async () => ({}) });
const lib = await import("../apps/desktop/ui/js/lib.js");
assert.deepEqual(await lib.captureSession("xiaomi_mimo", "platform_session_env"), { status: "unavailable" });
assert.deepEqual(await lib.captureSession("alibaba_token_plan", "session_env"), { status: "unavailable" });
assert.deepEqual(await lib.captureSession("alibaba_coding_plan", "session_env", "cn"), { status: "unavailable" });
await lib.cancelSessionCapture();

// PRD 0048 RF3.4: the "Open {site}" link follows the region choice; another value keeps the default page.
const coding = { slot: "session_env", open_url: "https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=coding-plan#/efm/coding_plan",
  region_open_urls: [{ value: "cn", url: "https://bailian.console.aliyun.com/cn-beijing/?tab=model#/efm/coding_plan" }] };
assert.match(lib.sessionOpenLinkHtml(coding, "cn"), /data-session-url="https:\/\/bailian\.console\.aliyun\.com\/cn-beijing\//);
for (const region of [null, "intl", "other"]) {
  assert.match(lib.sessionOpenLinkHtml(coding, region), /data-session-url="https:\/\/modelstudio\.console\.alibabacloud\.com\//, String(region));
}
assert.match(lib.sessionOpenLinkHtml({ slot: "x", open_url: "https://platform.xiaomimimo.com/console/plan-manage" }), /platform\.xiaomimimo\.com/);

// The dialog's wiring, read from its source.
const dialog = await readFile(ui("js/account-dialog.js"), "utf8");
assert.match(dialog, /\$\{s\.capture \? `<span class="dlg-actions"><button type="button" class="btn" data-capture=/, "button only for capturable slots");
assert.match(dialog, /captureSession\(provider, slot, sessionRegion\(\)\)/, "the capture follows the region choice");
assert.match(dialog, /sessionOpenLinkHtml\(s, sessionRegion\(\)\)/, "the open link follows the region choice");
assert.match(dialog, /e\.target\?\.name === "f:region"[\s\S]{0,60}refreshOpenLinks\(\)/, "a region change updates the open link");
assert.match(dialog, /field\.value = outcome\.paste;[\s\S]{0,80}field\.dispatchEvent\(new Event\("input"\)\)/, "the paste box previews the capture");
assert.match(dialog, /#session-test"\)\?\.click\(\)/, "the capture is tested like a paste");
assert.match(dialog, /addEventListener\("close", \(\) => \{[\s\S]{0,300}if \(capturing\) cancelSessionCapture\(\)/, "closing the dialog cancels");
for (const status of ["cancelled", "timed_out"]) assert.ok(dialog.includes(`"${status}"`), status);
assert.ok(!/console\.(log|info|debug)\([^)]*paste/.test(dialog), "no paste is logged");
console.log("session capture UI: ok");
