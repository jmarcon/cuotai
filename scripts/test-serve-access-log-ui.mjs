#!/usr/bin/env node
// Browser-free UI smoke for PRD 0050 RF6.1/RF6.3: Settings › Integrations shows the access-log row beside the server
// rows with Off selected by default, the three modes, the size on disk and the Erase action (disabled while nothing is
// recorded); the demo backend mirrors the shell's refusal of an unknown mode, and every catalog translates the row.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { integrationHtml } = await import("../apps/desktop/ui/js/settings/integrations.js");
const { serveErrorText } = await import("../apps/desktop/ui/js/settings/common.js");
await i18n.initI18n?.();

let sv = await lib.serveStatus();
assert.equal(sv.access_log, "off", "off by default");
let html = integrationHtml(sv);
assert.match(html, /id="serve-access-log"/, "the access-log control is drawn");
assert.match(html, /data-value="off" aria-checked="true"/, "Off is selected");
for (const mode of ["errors", "all"]) assert.match(html, new RegExp(`data-value="${mode}" aria-checked="false"`));
assert.match(html, /data-serve-access-log-erase disabled/, "nothing to erase while nothing is recorded");
assert.match(html, new RegExp(i18n.t("settings.serve.accessLog.empty")));

sv = await lib.setServeAccessLog("errors");
assert.equal(sv.access_log, "errors");
html = integrationHtml(sv);
assert.match(html, /data-value="errors" aria-checked="true"/);
assert.doesNotMatch(html, /data-serve-access-log-erase disabled/, "a recorded log can be erased");
assert.match(html, /2 KiB on this computer/, "the size on disk is shown");

sv = await lib.eraseServeAccessLog();
assert.equal(sv.access_log_bytes, 0);
assert.match(integrationHtml(sv), /data-serve-access-log-erase disabled/);

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
