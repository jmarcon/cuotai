#!/usr/bin/env node
// Browser-free UI smoke for PRD 0002 RF1.2: the "other config file" banner of the main window is dismissable
// (DS-CMP-MSG-2), shows a given (TUI path, app path) pair in one session only, stays hidden for that pair after a
// relaunch, and shows again for a new pair. Settings › Advanced › Files keeps the hint.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  body: { append: () => {} },
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: () => {},
};
globalThis.MutationObserver = class { observe() {} disconnect() {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = {
  getItem: (key) => storage.get(key) ?? null,
  setItem: (key, value) => storage.set(key, value),
};
globalThis.fetch = async (relative) => {
  try {
    const data = JSON.parse(await readFile(ui(relative), "utf8"));
    return { ok: true, json: async () => data };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
// Each import with a new query is a fresh module instance: a relaunch of the app with the same localStorage.
let launch = 0;
const relaunch = () => import(`../apps/desktop/ui/js/lib.js?launch=${++launch}`);

const pairA = { other_config: "/Users/demo/tui.toml", config_path: "/Users/demo/app.toml" };
const pairB = { other_config: "/Users/demo/other.toml", config_path: "/Users/demo/app.toml" };

// First launch: the banner shows and keeps showing across re-renders until dismissed.
let lib = await relaunch();
assert.equal(lib.otherConfigBanner(null), "");
assert.equal(lib.otherConfigBanner({ config_path: "/x.toml" }), "", "no record, no banner");
const text = lib.otherConfigBanner(pairA);
assert.match(text, /tui\.toml/);
assert.match(text, /app\.toml/);
assert.equal(lib.otherConfigBanner(pairA), text, "re-render keeps the banner in the same session");
// Switching files to another pair and back keeps an undismissed pair visible in its first session.
assert.match(lib.otherConfigBanner(pairB), /other\.toml/);
assert.equal(lib.otherConfigBanner(pairA), text, "switching back hid an undismissed pair");
lib.dismissOtherConfig(pairA);
assert.equal(lib.otherConfigBanner(pairA), "", "close button hides it");
assert.equal(lib.otherConfigNotice(pairA), text, "Settings › Files keeps the hint");

// Second launch, same pairs: suppressed. A new pair shows, but a hidden window does not record it as seen.
const pairC = { other_config: "/Users/demo/third.toml", config_path: "/Users/demo/app.toml" };
lib = await relaunch();
assert.equal(lib.otherConfigBanner(pairA), "", "same pair does not come back");
assert.equal(lib.otherConfigBanner(pairB), "", "a pair shown in an earlier session does not come back");
document.hidden = true;
assert.match(lib.otherConfigBanner(pairC), /third\.toml/);
lib = await relaunch();
assert.match(lib.otherConfigBanner(pairC), /third\.toml/, "a render in a hidden window counted as seen");
document.hidden = false;
assert.match(lib.otherConfigBanner(pairC), /third\.toml/, "a different pair shows again");

// Shown without a dismissal still counts: the next launch does not show it.
lib = await relaunch();
assert.equal(lib.otherConfigBanner(pairC), "");
assert.equal(lib.otherConfigBanner(pairA), "");

// The seen list is bounded and survives a corrupt or unavailable store.
for (let i = 0; i < 30; i++) lib.otherConfigBanner({ other_config: `/t/${i}.toml`, config_path: "/a.toml" });
assert.ok(JSON.parse(storage.get("cuotai.otherConfigSeen")).length <= 20, "seen list grows without bound");
storage.set("cuotai.otherConfigSeen", "{not json");
lib = await relaunch();
assert.notEqual(lib.otherConfigBanner(pairA), "", "corrupt store shows the banner");
const broken = globalThis.localStorage;
globalThis.localStorage = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
lib = await relaunch();
assert.notEqual(lib.otherConfigBanner(pairA), "", "blocked storage shows the banner instead of throwing");
globalThis.localStorage = broken;

// The main window renders it through the helper with a labelled close button and wires the dismissal.
const main = await readFile(ui("js/main.js"), "utf8");
assert.match(main, /\$\{otherBannerHtml\(\)\}/, "main window no longer renders the banner");
assert.match(main, /data-dismiss-other-config title="\$\{te\("common\.dismiss"\)\}" aria-label="\$\{te\("common\.dismiss"\)\}"/);
assert.match(main, /closest\("\[data-dismiss-other-config\]"\)\) \{ dismissOtherConfig\(store\.view\)/);
assert.doesNotMatch(main, /otherConfigNotice\(view\) \?/, "main window must use otherConfigBanner");
assert.match(main, /"visibilitychange", \(\) => \{ if \(!document\.hidden && otherConfigNotice\(store\.view\)\) render\(\); \}/,
  "showing the window must re-render so a pending banner is recorded");
for (const locale of ["pt-BR", "en-US", "es", "it"]) {
  const catalog = JSON.parse(await readFile(ui(`locales/${locale}.json`), "utf8"));
  assert.ok(catalog["config.other"] && catalog["common.dismiss"], `${locale}: missing banner strings`);
}
console.log("other-config banner smoke: ok");
