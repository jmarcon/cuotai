#!/usr/bin/env node
// Browser-free UI smoke for PRD 0050 RF4.4: Settings › Integrations shows the trusted-origin editor beside the
// "allow browsers" switch, says when a token is needed, shows the one-time migration notice, and the demo backend
// (a mirror of the core's `serve::Origin` rules) refuses browser mode without a token or a valid origin, and the
// token's revocation while browsers are allowed. Re-validation V-02: without a token the switch carries
// `aria-disabled`, and toggling it says to generate a token instead of opening its warning.
import assert from "node:assert/strict";
import { installUiEnv, markupRoot, parseHtml } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { bind, integrationHtml } = await import("../apps/desktop/ui/js/settings/integrations.js");
const { serveErrorText } = await import("../apps/desktop/ui/js/settings/common.js");
await i18n.initI18n?.();

/** Binds the Integrations page into stand-in containers and returns the server group. Hooks and widgets bind into
 *  an empty stand-in too; their own failures there do not concern these checks. */
const mountServe = async () => {
  const sec = markupRoot();
  const danger = markupRoot();
  const page = { querySelector: (sel) => (sel === "[data-serve]" ? sec : sel === "[data-integrations-danger]" ? danger : markupRoot()) };
  await bind(page, { changed: () => {} }).catch(() => {});
  assert.ok(sec.querySelector("#serve-browsers"), "the server group was drawn");
  return sec;
};
/** Turns the browsers switch on the way a click does: the click handler first, then `change` unless prevented. */
const toggleOn = (input) => {
  let prevented = false;
  const event = { target: input, currentTarget: input, preventDefault: () => { prevented = true; } };
  input.checked = true;
  input.onclick?.(event);
  if (!prevented) input.onchange?.(event);
  return prevented;
};

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
// Re-validation V-02 (core order, mirrored by the demo): the token is named first, whatever the origins.
assert.equal(await errorId(lib.setServe(true, 6738, true, [])), "serve.error.browser_token_required");
assert.equal(await errorId(lib.setServe(true, 6738, true, ["*"])), "serve.error.browser_token_required");
const browsersInput = parseHtml(html).find((n) => n.attrs.id === "serve-browsers");
assert.equal(browsersInput.attrs["aria-disabled"], "true", "blocked without a token, like the stats switch");
assert.ok(!("disabled" in browsersInput.attrs), "no native disabled: it would leave the tab order");
{
  const sec = await mountServe();
  const input = sec.querySelector("#serve-browsers");
  assert.equal(toggleOn(input), true, "the blocked change is refused");
  assert.equal(input.checked, false, "the switch stays off");
  assert.equal(sec.querySelector("#serve-browsers-confirm").hidden, true, "the warning does not open without a token");
  assert.equal(sec.querySelector("#serve-browsers-msg").textContent, i18n.t("settings.serve.browsersNeedsToken"), "the row points to Generate token");
  assert.equal((await lib.serveStatus()).allow_browsers, false, "nothing was saved");
}
// Saved on without a token (an old state): turning it off is never blocked.
assert.doesNotMatch(integrationHtml({ ...sv, allow_browsers: true }).match(/<input id="serve-browsers"[^>]*>/)[0], /aria-disabled/);

await lib.generateServeToken();
{
  // With a token, the existing flow: the switch is usable and opens the warning to confirm.
  const sec = await mountServe();
  const input = sec.querySelector("#serve-browsers");
  assert.equal(input.getAttribute("aria-disabled"), null);
  assert.equal(toggleOn(input), false);
  assert.equal(input.checked, false, "nothing changes before the warning is confirmed");
  assert.equal(sec.querySelector("#serve-browsers-confirm").hidden, false, "the warning opens");
  assert.equal(sec.focused, sec.querySelector("[data-serve-browsers-confirm]"), "the confirmation takes the focus");
  assert.equal(sec.querySelector("#serve-browsers-msg").textContent, "");
}
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
