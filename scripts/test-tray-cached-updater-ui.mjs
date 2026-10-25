#!/usr/bin/env node
// Browser-free UI smoke for PRD 0022 RF17.1: load the production tray module
// while the browser demo returns an already-known updater status.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const repository = new URL("../", import.meta.url);
const storage = new Map();
let rendered = "";

const trayRoot = {
  addEventListener: () => {},
  querySelector: () => null,
  querySelectorAll: () => [],
  set innerHTML(html) { rendered = html; },
  get innerHTML() { return rendered; },
};

globalThis.window = {};
globalThis.document = {
  hidden: true,
  activeElement: null,
  body: {},
  documentElement: {
    lang: "",
    dataset: {},
    toggleAttribute: () => {},
  },
  addEventListener: () => {},
  getElementById: (id) => {
    assert.equal(id, "root", "tray module requested an unexpected root element");
    return trayRoot;
  },
};
globalThis.matchMedia = () => ({
  matches: false,
  addEventListener: () => {},
});
globalThis.addEventListener = () => {};
globalThis.setInterval = () => 0;
// The browser demo syncs settings over a BroadcastChannel, which would keep
// Node alive after the smoke; the tray needs no cross-window sync here.
globalThis.BroadcastChannel = undefined;
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
    const path = new URL(`apps/desktop/ui/${relative}`, repository);
    const data = JSON.parse(await readFile(fileURLToPath(path), "utf8"));
    return { ok: true, json: async () => data };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

await import("../apps/desktop/ui/js/tray.js");
await new Promise((resolve) => setTimeout(resolve, 0));

assert.match(rendered, /<div class="pop">/, "production tray did not render its popover");
assert.match(rendered, /data-update-notice/, "cached updater status did not render the tray update notice");
assert.match(rendered, /0\.72\.0/, "tray update notice omitted the cached version");

console.log("Rendered production tray with cached updater status: update notice for 0.72.0");
console.log("PASS: cached updater state renders the tray without a module-load crash");
