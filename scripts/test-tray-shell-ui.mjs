#!/usr/bin/env node
// PRD 0022 RF7/RF8: the production tray renders its overview header with the filter tabs only (no logotype, no
// view toggle, sort, language or refresh-all), the footer with Settings, Open, the last refresh's age and the
// refresh button (no attention chips), one notice at a time, and the list view by default. The main topbar no
// longer carries the theme or language switches. Browser-free; the demo backend supplies the view.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
let rendered = "";
const trayRoot = {
  addEventListener: () => {},
  querySelector: () => null,
  querySelectorAll: () => [],
  contains: () => false,
  set innerHTML(html) { rendered = html; },
  get innerHTML() { return rendered; },
};
globalThis.document.getElementById = () => trayRoot;
globalThis.setInterval = () => 0;
globalThis.CSS = { escape: (s) => s };

await import("../apps/desktop/ui/js/tray.js");
for (let i = 0; i < 20 && !/class="row/.test(rendered); i++) await new Promise((r) => setTimeout(r, 10));

const header = /<header class="pop-head">([\s\S]*?)<\/header>/.exec(rendered)?.[1] ?? "";
assert.match(header, /^<div class="tabs" role="tablist"/, "the overview header starts with the tab list");
assert.equal(header.replace(/<div class="tabs" role="tablist"[\s\S]*<\/div>$/, "").trim(), "", "nothing but the tab list in the header");
assert.doesNotMatch(header, /wm-c|data-view|data-sort-toggle|lang-btn|data-refresh-all/, "no logotype, view toggle, sort, language or refresh-all");
assert.match(header, /role="tab" data-tab="all" aria-selected="true" tabindex="0"/, "All is selected by default");
assert.match(header, /class="tst" aria-hidden="true"/, "a provider needing attention has a badge");

const footer = /<footer class="pop-foot">([\s\S]*?)<\/footer>/.exec(rendered)?.[1] ?? "";
assert.match(footer, /data-open-settings aria-keyshortcuts="(Meta|Control)\+Comma"/);
assert.match(footer, /data-open-main/);
assert.match(footer, /data-refresh-all aria-label="Refresh all"/);
assert.doesNotMatch(rendered, /alert-chip/, "no attention chips in the footer (DS-CMP-FOOT-3)");
assert.ok((rendered.match(/class="pop-notice/g) ?? []).length <= 1, "one notice at a time (DS-CMP-FOOT-4)");
assert.match(rendered, /<article class="row" data-row=/, "the list is the default view");
assert.doesNotMatch(rendered, /class="tiles"/);

const main = await readFile(new URL("../apps/desktop/ui/js/main.js", import.meta.url), "utf8");
const topbar = /<header class="topbar"[\s\S]*?<\/header>/.exec(main)?.[0] ?? "";
assert.doesNotMatch(topbar, /themeSwitchHtml|langMenuHtml/, "theme and language left the main topbar (RF7.4)");
assert.match(topbar, /wordmark\(/, "the logotype stays in the main topbar (RF7.3)");
assert.match(topbar, /btn outline tb-add/, "Add account is outlined");

console.log("PASS: tray header holds the tabs only, footer and notices follow DS-CMP-FOOT, list is the default view");
process.exit(0); // the shared modules keep a BroadcastChannel open
