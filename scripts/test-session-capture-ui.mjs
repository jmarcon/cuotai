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
  "captureUnavailable", "captureBusy",
  // PRD 0056: the shared Google window, the passkey limit and "Forget the Google sign-in".
  "captureSharedHint", "capturePasskeyHint", "captureForget", "captureForgotten", "captureNothingToForget",
  // PRD 0056 re-validation V-03: a shared window that times out or stops is hidden, not closed.
  "captureTimeoutShared", "captureCancelledShared",
].map((k) => `dialog.session.${k}`);

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
// PRD 0048 re-validation V-02: after a failed capture, the texts point to the paste box above the button, never below.
const BELOW = { "de.json": /unten/, "en-US.json": /below/i, "es.json": /abajo/, "fr.json": /dessous/, "hi-IN.json": /नीचे/,
  "id-ID.json": /di bawah/, "it.json": /sotto/, "ja.json": /下に/, "ko.json": /아래/, "pt-BR.json": /abaixo/, "ru.json": /ниже/,
  "tr.json": /aşağı/, "vi.json": /dưới/, "zh-CN.json": /下方/ };
assert.deepEqual(Object.keys(BELOW).sort(), [...locales].sort());
for (const file of locales) {
  const catalog = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"));
  for (const key of ["dialog.session.captureTimeout", "dialog.session.captureTimeoutShared", "dialog.session.captureUnavailable"]) {
    assert.doesNotMatch(catalog[key], BELOW[file], `${file}: ${key} still says "below"`);
  }
}
assert.match(en["dialog.session.captureTimeout"], /into the box above\.$/);
assert.match(en["dialog.session.captureUnavailable"], /into the box above instead\.$/);

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
// PRD 0056 RF2.4: the demo has no shared window to forget.
assert.equal(await lib.forgetSharedCapture(), false);

// PRD 0056 RF1.3/RF6: a shared Google capture explains the kept window and the passkey limit and offers "Forget";
// a private capture keeps the PRD 0048 hint; the region's capture decides (the mainland console has no Google).
const shared = { slot: "session_env", capture: true, capture_shared: "google",
  region_open_urls: [{ value: "cn", url: "https://bailian.console.aliyun.com/", capture_shared: null }] };
assert.equal(lib.captureSharedOf(shared, null), "google");
assert.equal(lib.captureSharedOf(shared, "intl"), "google");
assert.equal(lib.captureSharedOf(shared, "cn"), null);
assert.equal(lib.captureSharedOf({ slot: "x", capture: true }), null);
const sharedHint = lib.sessionCaptureHintHtml(shared, null);
assert.match(sharedHint, /data-capture-hint="session_env"/);
assert.match(sharedHint, /data-capture-forget/);
// No catalog is loaded here, so the texts render as their ids.
assert.match(sharedHint, /dialog\.session\.captureSharedHint[\s\S]*dialog\.session\.capturePasskeyHint[\s\S]*dialog\.session\.captureForget/);
assert.doesNotMatch(sharedHint, /dialog\.session\.captureHint\b/);
// PRD 0056 re-validation V-01/V-02: "Forget" is the design-system link button (an icon and a label) on its own row,
// outside the inline hints, with its own polite status line for the answer.
assert.match(sharedHint, /<\/small><div class="session-capture-forget"><button type="button" class="link" data-capture-forget><svg[^>]*>[\s\S]*?<\/svg><span>dialog\.session\.captureForget<\/span><\/button><small class="hint" data-capture-forget-status role="status" aria-live="polite"><\/small><\/div><\/div>$/);
assert.doesNotMatch(sharedHint, /<small[^>]*>(?:(?!<\/small>)[\s\S])*data-capture-forget>/, "Forget is not inside a hint");
// PRD 0016 re-validation (V-08): the two inline hints are separated by a space.
assert.match(sharedHint, /<\/small> <small class="hint">/);
const privateHint = lib.sessionCaptureHintHtml(shared, "cn");
assert.doesNotMatch(privateHint, /data-capture-forget|capturePasskeyHint|captureSharedHint/);
assert.match(privateHint, /dialog\.session\.captureHint\b/);
// The English texts say what the user sees: the passkey limit and the way out.
assert.match(en["dialog.session.capturePasskeyHint"], /Passkeys do not work in this window[\s\S]*Try another way/);
assert.match(en["dialog.session.captureSharedHint"], /30 minutes/);
// PRD 0056 RF6 (S-01): parking keeps only Google's cookies and erases every site's stored data, and the hint says so.
assert.match(en["dialog.session.captureSharedHint"], /keeps only your Google sign-in cookies[\s\S]*cookies and stored data are erased/);
for (const file of locales) {
  const hint = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"))["dialog.session.captureSharedHint"];
  assert.doesNotMatch(hint, /keeps only your Google sign-in,|own cookies are erased/, `${file}: the old shared-window hint`);
}
assert.equal(lib.sessionCaptureHintHtml({ slot: "x", capture: false }), "");
// PRD 0048 re-validation V-01: the hint is a block `div`, which no `.f > span` title rule can make bold.
for (const html of [sharedHint, privateHint]) assert.ok(html.startsWith('<div class="session-capture-hint"') && html.endsWith("</div>"), html);

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
assert.match(dialog, /\$\{s\.capture \? `<div class="dlg-actions"><button type="button" class="btn" data-capture=/, "button only for capturable slots");
// PRD 0048 re-validation (V-01, accessible name): the slot is a `div`, not a `label` around the buttons and hints, so
// the paste field is named by its title alone and described by its instructions and preview.
assert.doesNotMatch(dialog, /<label class="f">(?:(?!<\/label>)[\s\S])*session-paste/, "no label wraps a session paste field");
assert.match(dialog, /<span id="\$\{id\}-l">\$\{esc\(tl\(s\.label\)\)\}<\/span>/, "the slot title has its own id");
assert.equal(dialog.match(/class="session-paste" id="\$\{id\}" data-slot="\$\{esc\(s\.slot\)\}" aria-labelledby="\$\{id\}-l" aria-describedby="\$\{id\}-h \$\{id\}-p"/g)?.length, 2,
  "both paste fields (box and masked) are named by the title and described by the instructions and preview");
assert.match(dialog, /const id = `session-\$\{esc\(s\.slot\)\}`/, "the slot ids follow the slot");
assert.match(dialog, /captureSession\(provider, slot, sessionRegion\(\)\)/, "the capture follows the region choice");
assert.match(dialog, /sessionOpenLinkHtml\(s, sessionRegion\(\)\)/, "the open link follows the region choice");
assert.match(dialog, /e\.target\?\.name === "f:region"[\s\S]{0,60}refreshOpenLinks\(\)/, "a region change updates the open link");
assert.match(dialog, /field\.value = outcome\.paste;[\s\S]{0,80}field\.dispatchEvent\(new Event\("input"\)\)/, "the paste box previews the capture");
assert.match(dialog, /#session-test"\)\?\.click\(\)/, "the capture is tested like a paste");
assert.match(dialog, /addEventListener\("close", \(\) => \{[\s\S]{0,300}if \(capturing\) cancelSessionCapture\(\)/, "closing the dialog cancels");
for (const status of ["cancelled", "timed_out"]) assert.ok(dialog.includes(`"${status}"`), status);
// Re-validation V-02: Forget answers on its own row, never in the Test button's status line.
assert.match(dialog, /const bindForget[\s\S]{0,300}b\.closest\("\.session-capture-forget"\)\?\.querySelector\("\[data-capture-forget-status\]"\)/);
assert.doesNotMatch(dialog.match(/const bindForget[\s\S]*?\}\)\);/)[0], /#session-test-status/);
// Re-validation V-03: the shared texts follow the slot's shared sign-in for the chosen region.
assert.match(dialog, /const shared = captureSharedOf\(schema\.sessions\?\.find\(\(x\) => x\.slot === slot\), sessionRegion\(\)\) === "google";/);
assert.match(dialog, /t\(shared \? "dialog\.session\.captureCancelledShared" : "dialog\.session\.captureCancelled"\)/);
assert.match(dialog, /t\(shared \? "dialog\.session\.captureTimeoutShared" : "dialog\.session\.captureTimeout"\)/);
assert.match(en["dialog.session.captureTimeoutShared"], /window was hidden[\s\S]*into the box above\.$/);
assert.doesNotMatch(en["dialog.session.captureCancelledShared"], /closed/);
assert.ok(!/console\.(log|info|debug)\([^)]*paste/.test(dialog), "no paste is logged");
// PRD 0056: the hint comes from the shared helper, follows the region, and "Forget" calls the shell.
assert.match(dialog, /sessionCaptureHintHtml\(s, sessionRegion\(\)\)/, "the capture hint follows the region choice");
assert.match(dialog, /refreshOpenLinks\(\); refreshCaptureHints\(\);/, "a region change updates the capture hint");
assert.match(dialog, /await forgetSharedCapture\(\)\) \? t\("dialog\.session\.captureForgotten"\)/, "Forget reports its result");
assert.match(dialog, /bindForget\(sessionBox\)/, "Forget is wired on render");
console.log("session capture UI: ok");
