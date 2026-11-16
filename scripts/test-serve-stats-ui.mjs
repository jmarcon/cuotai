#!/usr/bin/env node
// Browser-free UI smoke for PRD 0031 RF11.2.5 (re-validation V-06): "Share usage statistics" without a token is not a
// native `disabled` switch (DS-CMP-BTN-5). It keeps its place in the tab order with `aria-disabled="true"`, its
// `aria-describedby` names a visible description saying why, and a saved "on" with the server off says the server
// must be on. Every catalog carries the new text, translated. The keyboard path (Space keeps the focus, refuses the
// blocked change) runs in a real browser: apps/desktop/tools/ui-shots/keyboard.mjs, step 11b.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { integrationHtml } = await import("../apps/desktop/ui/js/settings/integrations.js");
await i18n.initI18n?.();

/** The stats switch's tag and the description its `aria-describedby` points to, from freshly drawn markup. */
const statsRow = (sv) => {
  const html = integrationHtml(sv);
  const input = html.match(/<input id="serve-stats"[^>]*>/)?.[0];
  assert.ok(input, "the stats switch is drawn");
  const described = input.match(/aria-describedby="([^"]+)"/)?.[1];
  const desc = described ? html.match(new RegExp(`id="${described}">([^<]*)<`))?.[1] : undefined;
  return { input, desc };
};

const off = await lib.serveStatus();
assert.equal(off.token, false);
assert.equal(off.stats, false);
let { input, desc } = statsRow(off);
assert.doesNotMatch(input, /\sdisabled(\s|=|\/|>)/, "no native disabled: it would leave the tab order");
assert.match(input, /aria-disabled="true"/, "blocked without a token");
assert.equal(desc, i18n.t("settings.serve.statsNeedsToken"), "described by the visible reason");

// Stats saved on without a token: the switch can still be turned off, and says why it does not answer.
({ input, desc } = statsRow({ ...off, stats: true }));
assert.doesNotMatch(input, /aria-disabled/);
assert.equal(desc, i18n.t("settings.serve.statsNeedsToken"));

// A token and the server off: usable, and a saved "on" says the server must be on.
({ input, desc } = statsRow({ ...off, token: true }));
assert.doesNotMatch(input, /aria-disabled|\sdisabled/);
assert.equal(desc, undefined, "nothing to explain while off and not chosen");
({ input, desc } = statsRow({ ...off, token: true, stats: true }));
assert.equal(desc, i18n.t("settings.serve.statsNeedsServer"));

// Server on (it creates the token): no description.
const on = await lib.setServe(true, 6738, false, []);
({ input, desc } = statsRow(on));
assert.doesNotMatch(input, /aria-disabled|\sdisabled/);
assert.equal(desc, undefined);

const en = JSON.parse(readFileSync(new URL("../apps/desktop/ui/locales/en-US.json", import.meta.url), "utf8"));
for (const { code } of i18n.LOCALES) {
  const catalog = JSON.parse(readFileSync(new URL(`../apps/desktop/ui/locales/${code}.json`, import.meta.url), "utf8"));
  for (const key of ["settings.serve.statsNeedsToken", "settings.serve.statsNeedsServer"]) {
    assert.ok(catalog[key], `${code}: ${key}`);
    assert.match(catalog[key], /\/v1\/stats/, `${code}: ${key} names the route`);
    if (code !== "en-US") assert.notEqual(catalog[key], en[key], `${code}: ${key} translated`);
  }
}
console.log("serve stats: blocked switch focusable and described, server-off reason, catalogs OK");
