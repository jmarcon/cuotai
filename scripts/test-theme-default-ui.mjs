#!/usr/bin/env node
// PRD 0022 RF15.1: Cuotai is the default theme for new installs only. Runs the production pre-paint script of every
// window: a new profile gets Cuotai; an existing profile without a stored theme keeps Dracula (written once); a
// stored choice is never changed; the migration runs once.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";

const ui = new URL("../apps/desktop/ui/", import.meta.url);
const { DEFAULT_THEME } = await import("../apps/desktop/ui/js/themes.js");
assert.equal(DEFAULT_THEME, "cuotai");

async function prePaint(page, initial, { app = false } = {}) {
  const html = await readFile(new URL(page, ui), "utf8");
  assert.match(html, /<html lang="en-US" data-theme="cuotai"/, `${page}: static default`);
  const script = /<script>([\s\S]*?)<\/script>/.exec(html)[1];
  const storage = new Map(Object.entries(initial));
  const localStorage = {
    getItem: (k) => storage.get(k) ?? null,
    setItem: (k, v) => storage.set(k, String(v)),
    key: (i) => [...storage.keys()][i] ?? null,
    get length() { return storage.size; },
  };
  const attrs = new Set();
  const timers = [];
  const documentElement = { lang: "", dataset: {}, setAttribute: (k) => attrs.add(k), removeAttribute: (k) => attrs.delete(k) };
  vm.runInNewContext(script, {
    localStorage, document: { documentElement }, navigator: { languages: ["en-US"], language: "en-US" },
    matchMedia: () => ({ matches: true }), JSON, setTimeout: (fn, ms) => timers.push([fn, ms]),
    ...(app ? { window: { __TAURI__: {} } } : {}),
  });
  return { theme: documentElement.dataset.theme, storage, pending: attrs.has("data-theme-pending"), timers };
}

for (const page of ["tray.html", "index.html", "settings.html"]) {
  const fresh = await prePaint(page, {});
  assert.equal(fresh.theme, "cuotai", `${page}: new profile gets Cuotai`);
  assert.equal(fresh.storage.get("cuotai.theme"), undefined, "nothing is written for a new profile");
  assert.equal(fresh.storage.get("cuotai.themeDefaultMigrated"), "1");

  const existing = await prePaint(page, { "cuotai.locale": "pt-BR" });
  assert.equal(existing.theme, "dracula", `${page}: an existing profile keeps Dracula`);
  assert.equal(existing.storage.get("cuotai.theme"), "dracula");

  const chosen = await prePaint(page, { "cuotai.theme": "win98", "cuotai.locale": "es" });
  assert.equal(chosen.theme, "win98", `${page}: a stored choice is never changed`);

  // Empty storage in the app window: the first paint waits (hidden, at most 600 ms) for the shell, which knows
  // whether the profile existed; with stored keys, or in the browser demo, nothing waits.
  const appFresh = await prePaint(page, {}, { app: true });
  assert.ok(appFresh.pending, `${page}: an app window with empty storage waits for the shell`);
  assert.equal(appFresh.timers[0]?.[1], 600);
  appFresh.timers[0][0]();
  assert.ok(!fresh.pending && !(await prePaint(page, { "cuotai.locale": "es" }, { app: true })).pending, `${page}: no wait otherwise`);

  const once = await prePaint(page, { "cuotai.locale": "it", "cuotai.themeDefaultMigrated": "1" });
  assert.equal(once.theme, "cuotai", `${page}: the migration runs once`);
}
console.log("PASS: Cuotai for new installs, Dracula kept for existing profiles, stored choices untouched, runs once");
