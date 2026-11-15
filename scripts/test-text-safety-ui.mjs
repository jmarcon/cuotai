#!/usr/bin/env node
// Browser-free UI test for the PRD 0022 re-validation (S-01, S-02, S-03): catalogs (a user language pack included)
// and core texts are untrusted, so every HTML-safe helper escapes the whole text exactly once, and no catalog text
// reaches a markup sink raw.
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "en-US" });
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();

const POISON = `<b data-pwn>x</b> & "q" 'a'`;
const SAFE = "&lt;b data-pwn&gt;x&lt;/b&gt; &amp; &quot;q&quot; &#39;a&#39;";
const noRawMarkup = (html, what) => {
  assert.doesNotMatch(html, /<b data-pwn>/, `${what}: no raw markup`);
  assert.doesNotMatch(html, /&amp;(lt|gt|amp|quot|#39);/, `${what}: no double escape`);
};

// 1. A core text whose id no catalog knows: the English fallback (already interpolated) is used, in en-US always.
const coreNote = (id) => ({ id, params: { rule: POISON }, fallback: `Hook ${POISON}: done` });
for (const code of ["en-US", "pt-BR", i18n.PSEUDO]) {
  if (code === i18n.PSEUDO) globalThis.location = { search: "?pseudo" };
  await i18n.setLocale(code);
  const out = i18n.tle(coreNote("demo.core.unknown"));
  noRawMarkup(out, `tle fallback in ${code}`);
  if (code !== i18n.PSEUDO) assert.equal(out, `Hook ${SAFE}: done`, `tle fallback in ${code}`);
  assert.equal(i18n.tl(coreNote("demo.core.unknown"), {}, true), out, "tl(…, true) is tle");
}
globalThis.location = { search: "" };
console.log("ok core fallback escaped once in en-US, pt-BR and en-XA");

// 2. A poisoned catalog (a user pack covering a UI id and a core id): the template and the params are escaped once.
await i18n.setLocale("en-US");
i18n.registerLocale("xx", "Poison", {
  "sort.custom": `Custom ${POISON}`,
  "hooks.run.succeeded": `${POISON} {rule} ok`,
  "card.nextRefresh": `Next ${POISON} {time}`,
});
await i18n.setLocale("xx");
assert.equal(i18n.te("sort.custom"), `Custom ${SAFE}`, "te escapes a poisoned template");
assert.equal(i18n.t("sort.custom"), `Custom ${POISON}`, "t stays plain text (for properties and textContent)");
const viaCatalog = i18n.tle({ id: "hooks.run.succeeded", params: { rule: POISON }, fallback: "unused" });
assert.equal(viaCatalog, `${SAFE} ${SAFE} ok`, "tle escapes a poisoned catalog template and its params once");
// A nested core note keeps a single escape (PRD 0001 L5).
const nested = i18n.tle({ id: "hooks.run.succeeded", params: { rule: { id: "demo.inner", params: {}, fallback: POISON } }, fallback: "unused" });
assert.equal(nested, `${SAFE} ${SAFE} ok`, "a nested note is escaped once");
// Plain strings and absent values.
assert.equal(i18n.tle(POISON), SAFE);
assert.equal(i18n.tle(null), "");
assert.equal(i18n.tl({ id: "demo.core.unknown", params: {}, fallback: POISON }), POISON, "tl without escape stays plain");
await i18n.setLocale("en-US");
console.log("ok poisoned catalog templates and params escaped once");

// 3. Static guards on the sinks the re-validation found (S-02, S-03, glyph hardening).
const ui = new URL("../apps/desktop/ui/js/", import.meta.url);
const src = async (path) => readFile(fileURLToPath(new URL(path, ui)), "utf8");
const lib = await src("lib.js");
const sortMenu = lib.slice(lib.indexOf("export function sortMenuHtml"), lib.indexOf("// ---------- notice categories"));
assert.doesNotMatch(sortMenu, />\$\{label\(/, "the sort menu never splices a raw catalog label into markup");
assert.match(sortMenu, /textContent = label\(m\)/, "the sort menu sets its labels with textContent");
assert.match(await src("components.js"), /<span class="pg">\$\{esc\(d\.glyph\)\}<\/span>/, "the pace glyph is escaped");
assert.match(await src("settings/files.js"), /<small>\$\{te\("settings\.vault\.created", \{ date \}\)\}<\/small>/, "no esc(te(…))");

// No escaped text goes to a property, setAttribute or an outer esc() anywhere in the UI (they do not decode entities).
const files = [];
const walk = async (dir) => {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const url = new URL(entry.name + (entry.isDirectory() ? "/" : ""), dir);
    if (entry.isDirectory()) await walk(url);
    else if (entry.name.endsWith(".js")) files.push(url);
  }
};
await walk(ui);
const DOUBLE = /\.(title|textContent|placeholder|value|innerText|ariaLabel|label)\s*=\s*(te|tle)\(|setAttribute\([^)]*,\s*(te|tle)\(|esc\((te|tle)\(/;
for (const url of files) {
  const lines = (await readFile(fileURLToPath(url), "utf8")).split("\n");
  lines.forEach((line, i) => assert.doesNotMatch(line, DOUBLE, `${fileURLToPath(url)}:${i + 1} double-escapes`));
}
console.log(`ok no raw catalog label in the sort menu and no double escape in ${files.length} UI modules`);
