#!/usr/bin/env node
// Browser-free UI smoke for PRD 0033 re-validation V-06 and L-2: a per-account refresh the user triggers on a
// metered account (AWS Bedrock) confirms the billed query first and invokes nothing on Cancel, other accounts and
// "Refresh all" never ask, the core's refusal (`note.metered.refused`, nested clocks) becomes a translated toast, the
// browser demo refuses a second refresh within 15 minutes, and a metered draft's "Test connection" carries the billed
// hint. The new strings exist, translated, in all 14 desktop catalogs.
import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { installUiEnv } from "./ui-test-env.mjs";

const ui = (path) => fileURLToPath(new URL(`../apps/desktop/ui/${path}`, import.meta.url));

// ---- catalogs ----
const KEYS = ["refresh.billed.title", "refresh.billed.body", "refresh.billed.query", "dialog.test.billed"];
const files = (await readdir(ui("locales"))).filter((f) => f.endsWith(".json"));
assert.equal(files.length, 14, "14 desktop catalogs");
const en = JSON.parse(await readFile(ui("locales/en-US.json"), "utf8"));
for (const file of files) {
  const catalog = JSON.parse(await readFile(ui(`locales/${file}`), "utf8"));
  for (const key of KEYS) {
    assert.ok(typeof catalog[key] === "string" && catalog[key].trim(), `${file}: ${key} missing`);
    if (file !== "en-US.json") assert.notEqual(catalog[key], en[key], `${file}: ${key} is untranslated`);
  }
  assert.ok(catalog["refresh.billed.title"].includes("{account}"), `${file}: title placeholder`);
  for (const key of ["refresh.billed.body", "dialog.test.billed"]) assert.match(catalog[key], /0[.,]01/, `${file}: ${key} names the price`);
}

// ---- behaviour (the demo backend, with the metered demo account) ----
installUiEnv({ locale: "pt-BR" });
globalThis.location = { search: "?metered" };
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const metered = await import("../apps/desktop/ui/js/metered-refresh.js");
await i18n.initI18n();

const view = await lib.getView();
const bedrock = view.reports.find((r) => r.provider === "aws_bedrock");
const other = view.reports.find((r) => r.provider !== "aws_bedrock" && !r.connection && !r.access?.metered);
assert.ok(bedrock && other, "the demo has the metered account and another one");
assert.equal(bedrock.access?.metered, true, "the metered account's descriptor says metered");

assert.equal(metered.needsBilledConfirm(bedrock), true);
assert.equal(metered.needsBilledConfirm(other), false);
assert.equal(metered.needsBilledConfirm({ ...bedrock, connection: { key: "k" } }), false, "a connection never asks");
assert.equal(metered.needsBilledConfirm(undefined), false);

const spy = () => {
  const calls = { confirm: 0, run: 0, toasts: [] };
  return {
    calls,
    opts: (answer) => ({ confirm: async () => { calls.confirm++; return answer; }, toast: (text) => calls.toasts.push(text) }),
    run: (result) => async () => { calls.run++; return result; },
  };
};

// Cancel invokes nothing.
{
  const s = spy();
  assert.equal(await metered.manualRefresh(bedrock, s.run(null), s.opts(false)), null);
  assert.deepEqual([s.calls.confirm, s.calls.run, s.calls.toasts.length], [1, 0, 0], "cancel: one question, no refresh, no toast");
}
// Another account never asks.
{
  const s = spy();
  await metered.manualRefresh(other, s.run(null), s.opts(false));
  assert.deepEqual([s.calls.confirm, s.calls.run, s.calls.toasts.length], [0, 1, 0], "a non-metered account refreshes without a question");
}
// "Query now": the demo's first refresh queries, a second within 15 minutes is refused with the core's note.
{
  const s = spy();
  const run = () => { s.calls.run++; return lib.refreshAccount(bedrock.index); };
  assert.equal(await metered.manualRefresh(bedrock, run, s.opts(true)), null, "the first refresh queries");
  assert.equal(s.calls.toasts.length, 0, "no toast when the refresh ran");
  const note = await metered.manualRefresh(bedrock, run, s.opts(true));
  assert.equal(note?.id, "note.metered.refused", "the second refresh within 15 minutes is refused");
  assert.equal(note.params.queried.id, "at.time");
  assert.equal(note.params.next.id, "at.time");
  assert.deepEqual([s.calls.confirm, s.calls.run, s.calls.toasts.length], [2, 2, 1]);
  const text = s.calls.toasts[0];
  assert.match(text, /^Não consultado: a última consulta cobrada ao AWS Cost Explorer foi às .+; a próxima é possível às .+\.$/, "the toast is the core note in the UI language");
  assert.doesNotMatch(text, /[{}]/, "the nested clocks are resolved");
  assert.doesNotMatch(text, /T\d\d:\d\d/, "never an ISO time");
}
// The other accounts of the demo are never refused.
assert.equal(await lib.refreshAccount(other.index), null);

// The confirmation: the billed wording, Cancel first, "Query now" the primary and last.
{
  const html = metered.billedConfirmHtml(bedrock);
  const plain = html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
  assert.match(plain, /Consultar .+ agora\?/);
  assert.match(plain, /consulta cobrada ao AWS Cost Explorer \(cerca de US\$ 0,01\)/);
  const cancel = html.indexOf('data-billed="cancel"'), query = html.indexOf('data-billed="query"');
  assert.ok(cancel > 0 && query > cancel, "Cancel comes before Query now");
  assert.match(html, /class="btn primary" data-billed="query">Consultar agora</);
}

// ---- L-2: the billed hint of "Test connection" ----
const draftBedrock = await lib.draftAccess("aws_bedrock", null, [], []);
const draftOther = await lib.draftAccess("claude", null, [], []);
assert.equal(metered.draftIsMetered(draftBedrock), true, "a Bedrock draft is metered");
assert.equal(metered.draftIsMetered(draftOther), false, "another draft is not");
assert.equal(metered.draftIsMetered({ metered: false, cost_advisory: { id: "x" } }), false, "an explicit flag wins");
assert.match(metered.billedTestHint("h"), /^<small class="hint billed-hint" id="h">Consulta cobrada \(cerca de US\$ 0,01\)<\/small>$/);

// ---- wiring (source) ----
const dialog = await readFile(ui("js/account-dialog.js"), "utf8");
assert.match(dialog, /id="session-test"\$\{draftMetered \? ' aria-describedby="session-test-billed"' : ""\}/, "the button is described by the hint");
assert.match(dialog, /\$\{draftMetered \? billedTestHint\("session-test-billed"\) : ""\}/);
assert.match(dialog, /const metered = draftIsMetered\(access\);/);
for (const [file, attr] of [["js/main.js", "refresh"], ["js/tray.js", "refreshOne"]]) {
  const src = await readFile(ui(file), "utf8");
  assert.match(src, /const refreshOne = \(index\) => manualRefresh\(store\.view\?\.reports\?\.find\(\(r\) => r\.index === index\), \(\) => store\.refreshOne\(index\)\);/, `${file}: per-account refresh confirms`);
  assert.match(src, new RegExp(`if \\(one\\) return refreshOne\\(Number\\(one\\.dataset\\.${attr}\\)\\);`), `${file}: the button goes through it`);
  assert.doesNotMatch(src, /manualRefresh\([^)]*store\.refresh\(\)/, `${file}: Refresh all never asks`);
}
const libSrc = await readFile(ui("js/lib.js"), "utf8");
assert.match(libSrc, /s\.refreshOne = \(i\) => refreshAccount\(i\)\.then\(async \(note\) => \{ if \(!isDesktop\) await getView\(\)\.then\(set\); return note \?\? null; \}\)/);

console.log("metered refresh UI: ok");
