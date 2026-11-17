#!/usr/bin/env node
// PRD 0035 re-validation S-01: no text a language pack may change appears on a protected surface (the destructive
// confirmations, consent and disclosure, billed-query warnings, security warnings, sign-in and secrets, the update,
// first-run, backup and restore dialogs, the local-server, hooks, channels, vault, erase and privacy panels, and the
// notices about user content). Browser-free: every surface is built in `ui-test-env` with the demo's data, in French
// (so core texts resolve too), inside `i18n.traceTexts`, which records each id `t`/`te`/`tl` resolve after
// `desktopWording`. An id whose text lands inside a `data-protected-texts` root must be protected by
// `crates/cuotai-core/src/user_pack/policy.json`. Sections that live inside the account dialog's closures (sign-in,
// session paste, move to vault, secrets) and the extension consent steps are read from their source instead: every
// literal id there must be protected.
//
// Completeness: every dialog creation, `role="alertdialog"` and `askDialog(` in apps/desktop/ui/js either carries a
// `data-protected-texts` mark that this test walks, or is listed in NOT_PROTECTED with its reason; and every mark in
// the code is walked here.
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { installUiEnv } from "./ui-test-env.mjs";
import { isOverridable } from "./pack-policy.mjs";

const root = new URL("../", import.meta.url);
const ui = (path) => new URL(`apps/desktop/ui/${path}`, root);
const LOCALE = "fr";

// ---------- a stub DOM: enough for the builders, recording every HTML and text they produce ----------
const made = [];
function stub(tag = "div") {
  const node = {
    tagName: tag.toUpperCase(), children: [], dataset: {}, style: {}, attrs: {}, className: "", hidden: false, value: "",
    _html: "", _text: "", disabled: false, checked: false, open: false, type: "", isConnected: true,
    set innerHTML(v) { this._html = String(v); }, get innerHTML() { return this._html; },
    set textContent(v) { this._text = String(v ?? ""); }, get textContent() { return this._text; },
    setAttribute(k, v) { this.attrs[k] = String(v); }, getAttribute(k) { return this.attrs[k] ?? null; }, removeAttribute(k) { delete this.attrs[k]; },
    toggleAttribute() {}, hasAttribute(k) { return k in this.attrs; },
    append(...kids) { this.children.push(...kids); }, prepend(...kids) { this.children.unshift(...kids); }, appendChild(k) { this.children.push(k); return k; },
    replaceChildren(...kids) { this.children = kids; }, insertAdjacentHTML(_, html) { this._html += html; },
    addEventListener() {}, removeEventListener() {}, dispatchEvent() {}, focus() {}, blur() {}, click() {}, scrollIntoView() {},
    showModal() { this.open = true; }, show() { this.open = true; }, close() { this.open = false; }, remove() {}, animate() { return { finished: Promise.resolve() }; },
    querySelector: () => stub(), querySelectorAll: () => [], closest: () => null, contains: () => false, matches: () => false,
    getBoundingClientRect: () => ({ left: 0, top: 0, width: 0, height: 0 }),
    classList: { add() {}, remove() {}, toggle() {}, contains: () => false },
  };
  made.push(node);
  return node;
}
/** Every HTML and text under `node`, attributes included (titles and labels are rendered text too). */
function dump(node) {
  if (typeof node === "string") return node;
  if (!node || typeof node !== "object") return "";
  const mark = node.dataset?.protectedTexts ? ` data-protected-texts="${node.dataset.protectedTexts}"` : "";
  const attrs = Object.entries(node.attrs ?? {}).map(([k, v]) => ` ${k}="${v}"`).join("");
  return `<${(node.tagName ?? "div").toLowerCase()}${mark}${attrs} title="${node.title ?? ""}" placeholder="${node.placeholder ?? ""}">${node._html}${node._text}${(node.children ?? []).map(dump).join("")}</${(node.tagName ?? "div").toLowerCase()}>`;
}

installUiEnv({ locale: LOCALE });
Object.assign(document, {
  hidden: false, createElement: stub, createTextNode: (text) => text, body: stub("body"),
  getElementById: () => null, querySelector: () => null, querySelectorAll: () => [],
});
document.activeElement = document.body;
globalThis.MutationObserver = class { observe() {} disconnect() {} };
globalThis.getComputedStyle = () => ({ getPropertyValue: () => "" });
globalThis.CSS = { escape: (s) => s };
globalThis.requestAnimationFrame = (fn) => setTimeout(fn, 0);

const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
assert.equal(i18n.currentLocale(), LOCALE);
const catalog = (path) => readFile(new URL(path, root), "utf8").then(JSON.parse);
const texts = { ...(await catalog("apps/desktop/ui/locales/en-US.json")), ...(await catalog(`crates/cuotai-core/locales/${LOCALE}.json`)),
  ...(await catalog(`apps/desktop/ui/locales/${LOCALE}.json`)) };

const unesc = (s) => s.replace(/<[^>]*>/g, " ").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, "\"").replace(/&#39;/g, "'").replace(/&amp;/g, "&");
/** The `data-protected-texts` parts of `html`: each marked element up to its matching close tag, as text plus the
 *  attribute values (a title or an aria-label is rendered text). */
function markedParts(html) {
  const parts = [];
  const open = /<([a-z0-9]+)\b[^>]*\bdata-protected-texts="([^"]*)"[^>]*>/gi;
  for (let m; (m = open.exec(html));) {
    const tag = m[1].toLowerCase();
    let depth = 1, i = open.lastIndex;
    const re = new RegExp(`<(/?)${tag}\\b[^>]*>`, "gi");
    re.lastIndex = i;
    for (let n; depth && (n = re.exec(html));) { depth += n[1] ? -1 : 1; i = re.lastIndex; }
    const chunk = html.slice(m.index, depth ? html.length : i);
    const attrs = [...chunk.matchAll(/\b(?:title|aria-label|placeholder)="([^"]*)"/g)].map((a) => a[1]).join(" ");
    parts.push({ name: m[2], text: unesc(`${chunk} ${attrs}`).replace(/\s+/g, " ") });
  }
  return parts;
}
/** The literal pieces of an id's template (placeholders cut out), longest first. */
function pieces(id) {
  const v = texts[id];
  const forms = v && typeof v === "object" ? Object.values(v) : typeof v === "string" ? [v] : [];
  return forms.flatMap((f) => f.split(/\{\w+\}/)).map((s) => s.replace(/\s+/g, " ").trim()).filter((s) => s.length >= 2)
    .sort((a, b) => b.length - a.length);
}

const walked = new Set();
const problems = [];
/** Builds one surface (`build` returns HTML, or nodes it created) and checks every rendered id inside its marks. */
async function surface(name, build, { marks = [name] } = {}) {
  made.length = 0;
  let out;
  const ids = await i18n.traceTexts(async () => { out = await build(); });
  const html = [out, ...made].flat().map(dump).join("\n");
  const parts = markedParts(html);
  for (const mark of marks) {
    walked.add(mark);
    assert.ok(parts.some((p) => p.name === mark), `${name}: no data-protected-texts="${mark}" root was rendered`);
  }
  const inside = parts.filter((p) => marks.includes(p.name)).map((p) => p.text).join(" \u0000 ");
  const protectedHere = [...ids].filter((id) => !isOverridable(id));
  const shown = protectedHere.filter((id) => pieces(id)[0] && inside.includes(pieces(id)[0]));
  // A walk that finds no protected text on its surface walked nothing (a builder that stopped rendering).
  assert.ok(shown.length > 0, `${name}: no protected text found inside its marks`);
  if (process.env.CUOTAI_WALK_VERBOSE) console.log(`${name}: ${ids.size} ids traced, ${shown.length} protected shown, ${inside.length} chars`);
  const protectedTexts = new Set(protectedHere.map((id) => pieces(id)[0]).filter(Boolean));
  for (const id of ids) {
    if (!isOverridable(id)) continue;
    const [longest] = pieces(id);
    if (!longest || !inside.includes(longest)) continue;
    // The same words from a protected id rendered here (confirm.cancel beside common.cancel elsewhere) explain it.
    if (protectedTexts.has(longest)) continue;
    problems.push(`${name}: ${id} ("${longest}") is rendered on a protected surface but a pack may change it`);
  }
  return ids;
}

/** Source scan for closure-bound sections: every literal id inside the named function's body. */
async function sourceSurface(name, file, fnNames) {
  const src = await readFile(ui(`js/${file}`), "utf8");
  walked.add(name);
  for (const fn of fnNames) {
    const start = src.search(new RegExp(`(function ${fn}\\b|const ${fn} = )`));
    assert.ok(start >= 0, `${file}: ${fn} not found`);
    // To the next top-level-ish declaration at the same indentation.
    const indent = src.slice(src.lastIndexOf("\n", start) + 1, start);
    const rest = src.slice(start + 1);
    const end = rest.search(new RegExp(`\\n${indent}(async )?(function |const [a-zA-Z]+ = |let |export )`));
    const body = rest.slice(0, end < 0 ? undefined : end);
    for (const [, id] of body.matchAll(/\b(?:te|t|tl)\(\s*"([A-Za-z0-9_.-]+)"/g)) {
      if (isOverridable(id)) problems.push(`${name} (${file} ${fn}): ${id} is shown on a protected surface but a pack may change it`);
    }
    for (const [, prefix] of body.matchAll(/\b(?:te|t)\(\s*`([A-Za-z0-9_.-]+)\$\{/g)) {
      if (isOverridable(`${prefix}x`)) problems.push(`${name} (${file} ${fn}): ${prefix}… is shown on a protected surface but a pack may change it`);
    }
  }
}

// ---------- the walk ----------
const lib = await import("../apps/desktop/ui/js/lib.js");
const accountDialog = await import("../apps/desktop/ui/js/account-dialog.js");
for (const [label, opts] of [["plain", {}], ["vault", { usesOauth: true }], ["extension secrets", { usesOauth: true, secretsOf: "ext" }]]) {
  await surface("delete", () => { accountDialog.confirmDelete(0, "work@example.com", opts); });
  void label;
}
await sourceSurface("fingerprint", "account-dialog.js", ["confirmFingerprint"]);
await sourceSurface("external", "account-dialog.js", ["offerExternalReload"]);
await sourceSurface("login", "account-dialog.js", ["renderLoginPanel"]);
await sourceSurface("session", "account-dialog.js", ["renderSession"]);
await sourceSurface("move", "account-dialog.js", ["renderMove"]);
await sourceSurface("secret", "account-dialog.js", ["secretRowHtml"]);
await sourceSurface("extension-consent", "extensions-dialog.js", ["consent"]);
await sourceSurface("extension-secrets", "extensions-dialog.js", ["secrets"]);
await sourceSurface("pack-active", "settings/general.js", ["packActiveHtml"]);
// S-03: the "user pack · file" marker of the language selector.
await sourceSurface("pack-marker", "lib.js", ["packMarkHtml", "packMarkText"]);

// askDialog and each of its callers' words.
const common = await import("../apps/desktop/ui/js/settings/common.js");
for (const id of ["hooks-leave", "channel-remove", "cost-off", "serve-access-log-erase"]) {
  await surface(id, () => { common.askDialog(id, "", "", ""); });
}
for (const file of await jsFiles()) {
  const src = await readFile(ui(`js/${file}`), "utf8");
  for (const m of src.matchAll(/askDialog\(\s*"([a-z-]+)"/g)) {
    // The call's arguments: to the parenthesis that closes `askDialog(`.
    let depth = 0, end = m.index + "askDialog".length;
    for (; end < src.length; end++) {
      if (src[end] === "(") depth++;
      else if (src[end] === ")" && --depth === 0) break;
    }
    for (const [, id] of src.slice(m.index, end).matchAll(/\bte?\(\s*"([A-Za-z0-9_.-]+)"/g)) {
      if (isOverridable(id)) problems.push(`${m[1]} (${file}): ${id} is in a confirmation but a pack may change it`);
    }
  }
}

// The Settings panels and their inline confirmations, with the demo's data.
const files = await import("../apps/desktop/ui/js/settings/files.js");
const plan = await lib.vaultAudit();
plan.entries = [...(plan.entries ?? []), { key: "k", service: "cuotai", account: "x", status: "unused", label: "x" }];
await surface("vault", () => `<div data-protected-texts="vault">${files.vaultHtml(plan)}</div>`, { marks: ["vault", "vault-prune", "vault-one"] });
const erase = await import("../apps/desktop/ui/js/settings/erase.js");
const erasePlan = await lib.eraseAllPlan();
await surface("erase-all", () => `${erase.eraseHtml()}<div data-protected-texts="erase-all">${erase.erasePanelHtml(erasePlan)}${erase.partialHtml({ failed: ["x"] })}</div>`);
const data = await import("../apps/desktop/ui/js/settings/data.js");
await surface("privacy", async () => data.html({ settings: await lib.getSettings(), view: await lib.getView().then((v) => () => v) }), { marks: ["privacy", "history-erase"] });
const cost = await import("../apps/desktop/ui/js/settings/cost.js");
await surface("cost-erase", async () => cost.costHtml(await lib.costStatus(), await lib.costPrices(), null));
const integrations = await import("../apps/desktop/ui/js/settings/integrations.js");
const hooks = await import("../apps/desktop/ui/js/settings/hooks.js");
await surface("serve", async () => {
  const sv = { ...(await lib.serveStatus()), allow_browsers: false, token: true };
  return `${integrations.html()}<div data-protected-texts="serve">${integrations.integrationHtml(sv)}${integrations.dangerHtml(sv)}</div>`;
}, { marks: ["serve", "serve-browsers"] });
await surface("hooks", async () => {
  const hv = await lib.hooksStatus();
  return `<div data-protected-texts="hooks">${hooks.hooksHtml(hv, hv.rules ?? [], false)}</div>`;
});
const channels = await import("../apps/desktop/ui/js/settings/channels.js");
const cv = await lib.channelsView();
await surface("channels", () => `${channels.channelsHtml()}<div data-protected-texts="channels">${channels.listHtml(cv)}</div>`);
await surface("channel-editor", () => `<div data-protected-texts="channel-editor">${channels.formHtml(cv, null)}${channels.consentHtml({
  kind: "webhook", masked: "https://hooks.example.com/…", filters: cv.channels[0]?.filters_text ?? { kinds: [], accounts: "", names: "" },
  warning: null, example: "{}", sentence: { id: "channels.consent.sentence", params: {}, fallback: "x" } })}</div>`);
const extensions = await import("../apps/desktop/ui/js/extensions-dialog.js");
await surface("extensions", () => extensions.openExtensionsDialog());

// Update, first run, billed refresh, backup and restore.
const updater = await import("../apps/desktop/ui/js/updater.js");
await surface("update-consent", () => updater.updateConsentDialogHtml());
const firstRun = await import("../apps/desktop/ui/js/first-run.js");
await surface("first-run", () => firstRun.stepHtml({ updatePending: true }));
const metered = await import("../apps/desktop/ui/js/metered-refresh.js");
const view = await lib.getView();
await surface("billed-refresh", () => metered.billedConfirmHtml(view.reports.find((r) => r.metered) ?? view.reports[0]));
const backup = await import("../apps/desktop/ui/js/backup-dialog.js");
const { shellHtml } = await import("../apps/desktop/ui/js/dialog-shell.js");
const restore = (await lib.restorePlan()).plan;
await surface("backup", async () => shellHtml({ id: "b", title: i18n.t("backup.title"), protectedTexts: "backup",
  body: backup.backupBodyHtml(await lib.backupPreview(false), false) + backup.planBodyHtml(restore) + backup.nextStepsHtml(restore, true, true),
  foot: "" }));
await sourceSurface("backup", "backup-dialog.js", ["openBackupDialog", "openRestoreDialog"]);
// The banners of the main window, read from their lines.
await bannerLines();
// The update banner's states.
await sourceSurface("update", "updater.js", ["readyHtml", "failedHtml", "managedHtml", "downloadingHtml"]);

// ---------- completeness ----------
async function jsFiles(dir = "js", prefix = "") {
  const out = [];
  for (const entry of await readdir(ui(dir), { withFileTypes: true })) {
    if (entry.isDirectory()) out.push(...await jsFiles(`${dir}/${entry.name}`, `${prefix}${entry.name}/`));
    else if (entry.name.endsWith(".js")) out.push(`${prefix}${entry.name}`);
  }
  return out;
}
async function bannerLines() {
  const src = await readFile(ui("js/main.js"), "utf8");
  for (const line of src.split("\n").filter((l) => /data-protected-texts="[^"]+"/.test(l))) {
    const name = line.match(/data-protected-texts="([^"]+)"/)[1];
    walked.add(name);
    for (const [, id] of line.matchAll(/\bte?\(\s*`?"?([A-Za-z0-9_.-]+)"?/g)) {
      if (/^[A-Za-z]/.test(id) && id.includes(".") && texts[id] !== undefined && isOverridable(id)) problems.push(`${name} (main.js): ${id} is in a protected banner but a pack may change it`);
    }
  }
}
/** Dialogs that are not protected surfaces, and why. */
const NOT_PROTECTED = {
  "account-dialog.js:open": "the account form: its protected parts (sign-in, session, move, secrets) carry their own marks",
  "account-dialog.js:noticeAccountGone": "says an account is gone; asks nothing",
  "about-dialog.js": "About and libraries",
  "whats-new-dialog.js": "release notes",
  "report-dialog.js": "a problem report the user copies; nothing is sent",
  "details-dialog.js": "account details",
  "budgets-editor.js": "budget editor",
  "settings/cost.js:plans": "the plan price editor",
  "lib.js": "shared helpers; dialogs there are listed by their callers",
  "motion.js": "the showModal helper",
  "dialog-shell.js": "the shell; protected callers pass protectedTexts",
  "models-dialog.js": "model lists",
  "cost.js:openCostDetails": "a local cost source's details; its estimate notices are protected by id",
};
const marksInCode = new Set();
const unlisted = [];
for (const file of await jsFiles()) {
  const src = await readFile(ui(`js/${file}`), "utf8");
  for (const m of src.matchAll(/data-protected-texts="([a-z-]+)"|protectedTexts(?:\s*=|:)\s*"([a-z-]+)"|"data-protected-texts", "([a-z-]+)"/g)) {
    marksInCode.add(m[1] ?? m[2] ?? m[3]);
  }
  for (const [, id] of src.matchAll(/askDialog\(\s*"([a-z-]+)"/g)) marksInCode.add(id);
  const lines = src.split("\n");
  lines.forEach((line, i) => {
    if (!/createElement\("dialog"\)|el\("dialog"|role="alertdialog"|setAttribute\("role", "alertdialog"\)/.test(line)) return;
    const near = lines.slice(Math.max(0, i - 2), i + 4).join("\n");
    if (/data-protected-texts|protectedTexts/.test(near)) return;
    const fn = lines.slice(0, i + 1).reverse().find((l) => /^(export )?(async )?function \w+|^\s*(async )?function \w+/.test(l))?.match(/function (\w+)/)?.[1];
    if (NOT_PROTECTED[file] || NOT_PROTECTED[`${file}:${fn}`]) return;
    unlisted.push(`${file}:${i + 1} (${fn ?? "?"})`);
  });
}
assert.deepEqual(unlisted, [], "dialogs with no data-protected-texts mark and no NOT_PROTECTED reason");
const unwalked = [...marksInCode].filter((m) => !walked.has(m));
assert.deepEqual(unwalked, [], "marked surfaces this test does not walk");

// R-01 and the trade-off: under a pack for a language Cuotai ships, a protected text reads the shipped translation;
// under a pack for one it does not ship (nl), English.
i18n.registerLocale("nl", "Nederlands", { "tray.ok": "Alles goed" });
await i18n.setLocale("nl");
const en = await catalog("apps/desktop/ui/locales/en-US.json");
assert.equal(i18n.t("tray.ok"), "Alles goed");
assert.equal(i18n.t("confirm.cancel"), en["confirm.cancel"]);
assert.equal(i18n.t("dialog.deleteTitle"), en["dialog.deleteTitle"]);
i18n.registerLocale("fr-CA", "Français (Canada)", { "tray.ok": "Correct" });
await i18n.setLocale("fr-CA");
const fr = await catalog("apps/desktop/ui/locales/fr.json");
assert.equal(i18n.t("tray.ok"), "Correct");
assert.equal(i18n.t("dialog.deleteTitle"), fr["dialog.deleteTitle"], "fr-CA reads the shipped French for a protected text");
assert.equal(i18n.t("settings.page.general"), fr["settings.page.general"], "and for any text the pack leaves out");

assert.deepEqual(problems, [], `${problems.length} overridable ids on protected surfaces`);
console.log(`PASS: ${walked.size} protected surfaces walked; every text on them is protected; every dialog is marked or listed`);
void fileURLToPath;
