#!/usr/bin/env node
// Browser-free UI smoke for PRD 0050 RF4.4: Settings › Integrations shows the trusted-origin editor beside the
// "allow browsers" switch, says when a token is needed, shows the one-time migration notice, and the demo backend
// (a mirror of the core's `serve::Origin` rules) refuses browser mode without a token or a valid origin, and the
// token's revocation while browsers are allowed.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { integrationHtml } = await import("../apps/desktop/ui/js/settings/integrations.js");
const { serveErrorText } = await import("../apps/desktop/ui/js/settings/common.js");
await i18n.initI18n?.();

const errorId = async (promise) => {
  try {
    await promise;
  } catch (e) {
    try { return JSON.parse(e).id; } catch { return e; }
  }
  assert.fail("the change was accepted");
};

// Without a token, browsers cannot be allowed, and the row says why before the switch is touched.
let sv = await lib.serveStatus();
assert.equal(sv.token, false);
let html = integrationHtml(sv);
assert.match(html, /id="serve-origins"/, "the origin editor is drawn");
assert.match(html, new RegExp(i18n.t("settings.serve.browsersNeedsToken").slice(0, 20)), "the token requirement is stated");
assert.equal(await errorId(lib.setServe(true, 6738, true, ["https://dash.example.com"])), "serve.error.browser_token_required");

await lib.generateServeToken();
assert.equal(await errorId(lib.setServe(true, 6738, true, [])), "serve.origin.error.empty");
assert.equal(await errorId(lib.setServe(true, 6738, true, ["*"])), "serve.origin.error.scheme");
assert.equal(await errorId(lib.setServe(true, 6738, true, ["http://dash.example.com"])), "serve.origin.error.scheme");
sv = await lib.serveStatus();
assert.equal(sv.allow_browsers, false, "nothing was persisted by a refused change");

sv = await lib.setServe(true, 6738, true, ["https://Dash.example.com/", ""]);
assert.equal(sv.allow_browsers, true);
assert.deepEqual(sv.allowed_origins, ["https://dash.example.com"]);
html = integrationHtml(sv);
assert.match(html, />https:\/\/dash\.example\.com<\/textarea>/, "the saved origins fill the editor");
assert.doesNotMatch(html, /data-serve-origins-notice/);

// The token cannot be revoked while browsers are allowed; the shell code has its own text.
assert.equal(await errorId(lib.revokeServeToken()), "browsers_need_token");
assert.notEqual(serveErrorText("browsers_need_token"), "browsers_need_token");

// The migration notice of an old broad browser mode.
html = integrationHtml({ ...sv, allow_browsers: false, origins_notice: true });
assert.match(html, /data-serve-origins-notice/);

// Every catalog translates the new texts.
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  for (const key of ["settings.serve.origins.label", "settings.serve.origins.help", "settings.serve.origins.save",
    "settings.serve.origins.notice", "settings.serve.browsersNeedsToken", "settings.error.browsers_need_token"]) {
    assert.notEqual(i18n.t(key), key, `${code}: ${key}`);
  }
}
console.log("serve origins: editor, token requirement, refusals and notice OK");
