#!/usr/bin/env node
// Browser-free UI smoke for PRD 0012 RF5 (2026-11-10 re-validation): the desktop local server always requires its
// token. Turning it on creates one (demo mirror of `Integration::apply`), revoking turns the server off, Settings
// and the main window show the one-time notice of a server that used to run without a token, and every catalog
// carries the new texts and drops the tokenless one.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { integrationHtml } = await import("../apps/desktop/ui/js/settings/integrations.js");
await i18n.initI18n?.();

let sv = await lib.serveStatus();
assert.equal(sv.enabled, false);
assert.equal(sv.token, false);
sv = await lib.setServe(true, 6738, false, []);
assert.equal(sv.state, "on");
assert.equal(sv.token, true, "turning the server on creates its token");
let html = integrationHtml(sv);
assert.match(html, /data-serve-token-copy/);
assert.doesNotMatch(html, /data-serve-token-notice/);

sv = await lib.revokeServeToken();
assert.equal(sv.token, false);
assert.equal(sv.enabled, false, "revoking turns the server off");
assert.equal(sv.state, "off");

html = integrationHtml({ ...sv, state: "on", enabled: true, token: true, url: "http://127.0.0.1:6738", token_notice: true });
assert.match(html, /data-serve-token-notice/, "Settings shows the token notice");
const main = readFileSync(new URL("../apps/desktop/ui/js/main.js", import.meta.url), "utf8");
assert.match(main, /data-serve-token-notice/, "the main window shows the token notice too");

// PRD 0012 re-validation V-02/V-06: a taken port gets one hint (the Cuotai-holder text or "choose another port here"),
// and the browsers row's help does not depend on the switch state.
const taken = { ...sv, state: "error", error: { id: "serve.error.port_taken", params: { port: 6738 }, fallback: "Port 6738 is in use." } };
assert.match(integrationHtml(taken), /Choose another port here\./);
const held = integrationHtml({ ...taken, cuotai_on_port: true });
assert.doesNotMatch(held, /Choose another port here\./, "the Cuotai-holder text is the only hint");
assert.match(held, /Another Cuotai server/);
const browsersHelp = (state) => integrationHtml({ ...state, state: "on", enabled: true, token: true, url: "http://127.0.0.1:6738" })
  .includes(i18n.t?.("settings.serve.browsersHelp") ?? "When on, only the web pages");
assert.ok(browsersHelp({ ...sv, allow_browsers: false }) && browsersHelp({ ...sv, allow_browsers: true, allowed_origins: ["https://example.com"] }));

for (const { code } of i18n.LOCALES) {
  const catalog = JSON.parse(readFileSync(new URL(`../apps/desktop/ui/locales/${code}.json`, import.meta.url), "utf8"));
  assert.equal(catalog["settings.serve.token.off"], undefined, `${code}: the tokenless text is gone`);
  assert.equal(catalog["settings.serve.browsersOff"], undefined, `${code}: the state-bound browsers text is gone`);
  for (const key of ["settings.serve.token.notice", "settings.serve.token.revokeHelp", "settings.serve.privacy", "settings.serve.browsersHelp", "settings.serve.portHint"]) {
    assert.ok(catalog[key], `${code}: ${key}`);
    if (code !== "en-US") assert.notEqual(catalog[key], JSON.parse(readFileSync(new URL("../apps/desktop/ui/locales/en-US.json", import.meta.url), "utf8"))[key], `${code}: ${key} translated`);
  }
}
console.log("serve token: required on enable, revoke turns off, notice and catalogs OK");
