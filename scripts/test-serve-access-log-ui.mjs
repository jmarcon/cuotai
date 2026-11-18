#!/usr/bin/env node
// Browser-free UI smoke for PRD 0050 RF6.1/RF6.3: Settings › Integrations shows the access-log row beside the server
// rows with Off selected by default, the three modes, the size on disk and the Erase action (unavailable while nothing
// is recorded: re-validation V-01, DS-CMP-BTN-5, focusable with `aria-disabled` and a tooltip saying why, and a click
// does nothing); the demo backend mirrors the shell's refusal of an unknown mode, and every catalog translates the row.
import assert from "node:assert/strict";
import { installUiEnv, markupRoot, parseHtml } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { bind, integrationHtml } = await import("../apps/desktop/ui/js/settings/integrations.js");
const { serveErrorText } = await import("../apps/desktop/ui/js/settings/common.js");
await i18n.initI18n?.();

/** The Erase log button of freshly drawn markup. */
const eraseButton = (sv) => parseHtml(integrationHtml(sv)).find((n) => "data-serve-access-log-erase" in n.attrs);
/** Unavailable the DS-CMP-BTN-5 way: no native `disabled`, `aria-disabled`, `.is-disabled` and the reason as tooltip. */
const assertUnavailable = (button) => {
  assert.ok(!("disabled" in button.attrs), "no native disabled: it would leave the tab order");
  assert.equal(button.attrs["aria-disabled"], "true");
  assert.ok(button.cls.includes("is-disabled"));
  assert.equal(button.attrs.title, i18n.t("settings.serve.accessLog.empty"), "the tooltip says why");
};
const assertAvailable = (button) => {
  for (const name of ["disabled", "aria-disabled", "title"]) assert.ok(!(name in button.attrs), name);
  assert.ok(!button.cls.includes("is-disabled"));
};

let sv = await lib.serveStatus();
assert.equal(sv.access_log, "off", "off by default");
let html = integrationHtml(sv);
assert.match(html, /id="serve-access-log"/, "the access-log control is drawn");
assert.match(html, /data-value="off" aria-checked="true"/, "Off is selected");
for (const mode of ["errors", "all"]) assert.match(html, new RegExp(`data-value="${mode}" aria-checked="false"`));
assertUnavailable(eraseButton(sv));
assert.match(html, new RegExp(i18n.t("settings.serve.accessLog.empty")));
{
  // A click on the unavailable button asks nothing and erases nothing: the confirmation dialog would need the DOM.
  const created = [];
  document.createElement = (tag) => { created.push(tag); throw new Error("a dialog was opened"); };
  const sec = markupRoot();
  const page = { querySelector: (sel) => (sel === "[data-serve]" ? sec : markupRoot()) };
  await bind(page, { changed: () => {} }).catch(() => {});
  const button = sec.querySelector("[data-serve-access-log-erase]");
  assert.equal(button.getAttribute("aria-disabled"), "true");
  await button.onclick({ currentTarget: button, target: button });
  assert.deepEqual(created, [], "no confirmation was opened");
  assert.equal(sec.querySelector("#serve-access-log-msg").textContent, "");
  delete document.createElement;
}

sv = await lib.setServeAccessLog("errors");
assert.equal(sv.access_log, "errors");
html = integrationHtml(sv);
assert.match(html, /data-value="errors" aria-checked="true"/);
assertAvailable(eraseButton(sv));
assert.match(html, /2 KiB on this computer/, "the size on disk is shown");

sv = await lib.eraseServeAccessLog();
assert.equal(sv.access_log_bytes, 0);
assertUnavailable(eraseButton(sv));

await assert.rejects(lib.setServeAccessLog("verbose"), (e) => e === "invalid_access_log");
assert.notEqual(serveErrorText("invalid_access_log"), "invalid_access_log", "the shell code has its own text");

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  for (const key of ["title", "help", "off", "errors", "all", "size", "empty", "erase", "eraseTitle", "eraseBody"]) {
    assert.notEqual(i18n.t(`settings.serve.accessLog.${key}`), `settings.serve.accessLog.${key}`, `${code}: ${key}`);
  }
  assert.notEqual(i18n.t("settings.error.invalid_access_log"), "settings.error.invalid_access_log", code);
}
console.log("serve access log: row, modes, size, erase and catalogs OK");
