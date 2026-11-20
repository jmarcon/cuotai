#!/usr/bin/env node
// PRD 0022 RF7/RF8: the production tray renders its overview header with the Cuotai mark button that opens the
// main window and the filter tabs (no logotype text, no view toggle, sort, language or refresh-all), the footer with Settings, Open, the last refresh's age and the
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
// DS-CMP-HEAD-2: the mark button first (named, with a tooltip, a native button in the Tab order), then the tab list.
const home = /^<button class="icon pop-home" type="button" data-open-main aria-label="([^"]+)" title="([^"]+)"><span class="wm-c" aria-hidden="true"><\/span><\/button>/.exec(header);
assert.ok(home, `the overview header starts with the mark button: ${header.slice(0, 200)}`);
assert.equal(home[1], "Open the Cuotai window", "accessible name");
assert.equal(home[2], home[1], "the tooltip repeats the name");
const tabs = header.slice(home[0].length);
assert.match(tabs, /^<div class="tabs" role="tablist"/, "then the tab list");
assert.equal(tabs.replace(/<div class="tabs" role="tablist"[\s\S]*<\/div>$/, "").trim(), "", "nothing else in the header");
assert.doesNotMatch(tabs, /wm-c|wm-ai|data-view|data-sort-toggle|lang-btn|data-refresh-all/, "no logotype text, view toggle, sort, language or refresh-all");
assert.doesNotMatch(header, /tabindex="-1"[^>]*data-open-main|data-open-main[^>]*tabindex="-1"/, "the mark button is reachable by keyboard");
assert.match(header, /role="tab" data-tab="all" aria-selected="true" tabindex="0"/, "All is selected by default");
assert.match(header, /class="tst" aria-hidden="true"/, "a provider needing attention has a badge");

const footer = /<footer class="pop-foot">([\s\S]*?)<\/footer>/.exec(rendered)?.[1] ?? "";
assert.match(footer, /data-open-settings aria-keyshortcuts="(Meta|Control)\+Comma"/);
assert.doesNotMatch(footer, /data-open-main/, "the main window opens from the header mark and the tray menu, not the footer (DS-CMP-FOOT-1)");
assert.match(footer, /data-refresh-all aria-label="Refresh all"/);
assert.doesNotMatch(rendered, /alert-chip/, "no attention chips in the footer (DS-CMP-FOOT-3)");
assert.ok((rendered.match(/class="pop-notice/g) ?? []).length <= 1, "one notice at a time (DS-CMP-FOOT-4)");
assert.match(rendered, /<article class="row" data-row=/, "the list is the default view");
assert.doesNotMatch(rendered, /class="tiles"/);

// The mark opens the main window through the command the tray menu's Open item uses (`show_main_window`), which the
// tray may call: it is not one of PRD 0020's window-scoped commands, and it navigates nothing (PRD 0022 nav guard).
const trayJs = await readFile(new URL("../apps/desktop/ui/js/tray.js", import.meta.url), "utf8");
assert.match(trayJs, /if \(el\.closest\("\[data-open-main\]"\)\) return openMainWindow\(\);/, "the click opens the main window");
const libJs = await readFile(new URL("../apps/desktop/ui/js/lib.js", import.meta.url), "utf8");
assert.match(libJs, /export const openMainWindow = \(\) => isDesktop && T\.core\.invoke\("show_main_window"\);/);
const libRs = await readFile(new URL("../apps/desktop/src-tauri/src/lib.rs", import.meta.url), "utf8");
assert.match(libRs, /"account" \| "open" => show_main_window\(app\.clone\(\)\)/, "the tray menu's Open uses the same command");
assert.match(libRs, /\n\s+show_main_window,\n/, "the command is registered");
const scopes = await readFile(new URL("../apps/desktop/src-tauri/src/window_scope.rs", import.meta.url), "utf8");
// PRD 0050 re-validation S-02: every command is classified; this one sits in ANY_WINDOW, never in a scope.
const scoped = scopes.slice(scopes.indexOf("pub(crate) const SCOPED"), scopes.indexOf("pub(crate) const ANY_WINDOW"));
const anyWindow = scopes.slice(scopes.indexOf("pub(crate) const ANY_WINDOW"));
assert.ok(scoped.length > 0 && anyWindow.length > 0, "the window-scope tables are found");
assert.doesNotMatch(scoped, /"show_main_window"/, "the command is not refused to the tray");
assert.match(anyWindow, /"show_main_window"/, "the command is classified for any window");
const i18nDir = new URL("../apps/desktop/ui/locales/", import.meta.url);
const { readdir } = await import("node:fs/promises");
const catalogs = (await readdir(i18nDir)).filter((f) => /^[a-z]{2}(-[A-Z]{2})?\.json$/.test(f));
assert.equal(catalogs.length, 14, "the 14 built-in catalogs");
for (const file of catalogs) {
  const catalog = JSON.parse(await readFile(new URL(file, i18nDir), "utf8"));
  assert.ok(typeof catalog["tray.openWindow"] === "string" && catalog["tray.openWindow"].length > 3, `${file}: tray.openWindow`);
}

const main = await readFile(new URL("../apps/desktop/ui/js/main.js", import.meta.url), "utf8");
const topbar = /<header class="topbar"[\s\S]*?<\/header>/.exec(main)?.[0] ?? "";
assert.doesNotMatch(topbar, /themeSwitchHtml|langMenuHtml/, "theme and language left the main topbar (RF7.4)");
assert.match(topbar, /wordmark\(/, "the logotype stays in the main topbar (RF7.3)");
assert.match(topbar, /btn outline tb-add/, "Add account is outlined");

// PRD 0054 RF5.1 (re-validation V-02): the no-tray notice is a status, not dismissable, with a space after its icon,
// and names the Linux cause on Linux, in every catalog.
{
  const i18n = await import("../apps/desktop/ui/js/i18n.js");
  const { trayUnavailableHtml } = await import("../apps/desktop/ui/js/tray-notice.js");
  for (const file of catalogs) {
    const code = file.replace(/\.json$/, "");
    const catalog = JSON.parse(await readFile(new URL(file, i18nDir), "utf8"));
    await i18n.setLocale(code);
    const mac = trayUnavailableHtml({ tray_unavailable: true }, "MacIntel");
    assert.match(mac, /role="status"/, `${code}: a status notice`);
    assert.doesNotMatch(mac, /<button/, `${code}: no dismiss button`);
    assert.ok(mac.includes("</svg> <span>"), `${code}: a space after the icon`);
    assert.ok(mac.includes(i18n.te("tray.unavailable")) && catalog["tray.unavailable"], `${code}: the tray.unavailable text`);
    const linux = trayUnavailableHtml({ tray_unavailable: true }, "Linux x86_64");
    assert.ok(linux.includes(i18n.te("tray.unavailableLinux")) && catalog["tray.unavailableLinux"], `${code}: the Linux text`);
    assert.equal(trayUnavailableHtml({}, "Linux"), "", `${code}: nothing with a tray`);
  }
  await i18n.setLocale("en-US");
}

console.log("PASS: tray header holds the open-window mark and the tabs, footer and notices follow DS-CMP-FOOT, list is the default view");
process.exit(0); // the shared modules keep a BroadcastChannel open
