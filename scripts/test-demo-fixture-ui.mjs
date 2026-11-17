#!/usr/bin/env node
// PRD 0022 RF18.2: the browser demo's `?fixture=six` swaps its accounts for the mockups' set (two Claude accounts,
// Codex, Cursor on extra usage, Kimi, a stale Groq, an Anthropic API account needing sign-in), example.com identities
// only; `&bedrock` adds the metered AWS Bedrock account (PRD 0027 RF3.1) and `?mono=XY` the first account's monogram
// override (DS-CMP-MONO-4). PRD 0036 re-validation (V-12): `?agg=0,1,12,40,exhausted,litellm` adds aggregator
// instances of every size, and the 9router and LiteLLM forms carry the core's descriptors.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
globalThis.location = { search: "?fixture=six&bedrock&mono=zq" };
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const lib = await import("../apps/desktop/ui/js/lib.js");
await lib.loadProviderLabels();
const c = await import("../apps/desktop/ui/js/components.js");

const { reports } = await lib.getView();
assert.deepEqual([...new Set((await lib.trayAccounts()).map((a) => a.provider_label))], ["Claude", "ChatGPT / Codex", "Cursor", "Kimi", "Groq", "Anthropic API", "AWS Bedrock"], "Menu bar shows the core's provider names, never provider ids");
assert.deepEqual(reports.map((r) => `${r.provider}:${r.account}`), [
  "claude:work@example.com", "claude:side@example.com", "chat_gpt:me@example.com", "cursor:dev@example.com",
  "kimi:ops@example.com", "groq:team@example.com", "anthropic_api:api@example.com", "aws_bedrock:billing@example.com",
]);
assert.ok(reports.every((r) => r.account.endsWith("@example.com")), "example.com identities only");
assert.equal(reports.find((r) => r.provider === "cursor").overage.state, "active", "Cursor is on extra usage");
assert.ok(reports.find((r) => r.provider === "groq").last_good, "Groq keeps a stale reading");
assert.equal(reports.find((r) => r.provider === "anthropic_api").status, "authentication_error");
assert.deepEqual(reports.filter((r) => r.access?.metered).map((r) => r.provider), ["aws_bedrock"], "only Bedrock is metered");
assert.ok(reports[0].binding && reports[0].metrics[0].pace, "the first Claude account has its binding and pace");
const monos = c.monograms(reports);
assert.equal(monos.get(lib.reportKey(reports[0])), "ZQ", "the override wins");
assert.equal(monos.get(lib.reportKey(reports[1])), "SI", "the sibling keeps its derived letters");

// A second copy of the demo module, loaded with the aggregator switches (the module reads `location` once).
globalThis.location = { search: "?agg=0,1,12,40,exhausted,litellm&aggtest=locked" };
const aggLib = await import("../apps/desktop/ui/js/lib.js?agg");
const aggView = await aggLib.getView();
const instances = aggView.reports.filter((r) => r.aggregator);
assert.deepEqual(instances.map((r) => `${r.provider}:${r.account}:${r.connections.length}`), [
  "nine_router:home:4", "nine_router:empty:0", "nine_router:solo:1", "nine_router:lab:12", "nine_router:fleet40:40", "nine_router:night:3", "litellm:gateway:4",
]);
for (const r of instances) {
  assert.equal(r.aggregator.connections, r.connections.length, `${r.account}: connection count`);
  assert.equal(new Set(r.connections.map((c) => c.account_key)).size, r.connections.length, `${r.account}: unique connection keys`);
  assert.ok(r.connections.every((c) => c.account_key.startsWith(`${r.provider}:${r.account}/`) && c.index === r.index), `${r.account}: keys and index`);
  for (const p of r.aggregator.pools) assert.ok(p.total > 0 && p.available + p.exhausted <= p.total, `${r.account}: pool counts`);
}
const byName = Object.fromEntries(instances.map((r) => [r.account, r]));
assert.deepEqual(byName.empty.aggregator.pools, [], "no connection, no pool");
assert.ok(byName.night.aggregator.pools.every((p) => p.exhausted === p.total && p.summary.id === "pool.exhausted"), "the exhausted pool");
const lab = byName.lab.connections;
assert.equal(lab.filter((c) => c.connection.duplicate_of === "claude:Account 2").length, 1, "one duplicate of a direct account");
assert.equal(lab.filter((c) => c.connection.enabled_upstream === false).length, 1, "one connection off in 9router");
assert.ok(byName.lab.aggregator.pools.every((p) => p.total <= 10), "the duplicate and the off connection take no part in pools");
assert.ok(byName.fleet40.connections.some((c) => c.account.length > 50), "the 40-connection instance has long labels");
assert.equal(byName.gateway.aggregator.kind, "LiteLLM proxy");
assert.deepEqual(byName.gateway.connections.map((c) => c.connection.upstream), ["openai", "anthropic", "azure", "deepinfra"]);
assert.ok(byName.gateway.connections.every((c) => c.metrics.every((m) => m.kind === "spend" && m.unit === "USD")), "LiteLLM budgets");
for (const provider of ["nine_router", "litellm"]) {
  const schema = await aggLib.formSchema(provider);
  assert.equal(schema.aggregator, true, `${provider}: an aggregator form`);
  assert.ok(schema.fields.every((f) => f.id === `field.${provider}.${f.key}` && f.label.id === `field.${provider}.${f.key}.label`), `${provider}: core field ids`);
  assert.deepEqual(await aggLib.visibleFields(provider, {}, "credential"), schema.fields.map((f) => f.key), `${provider}: every field visible`);
}
assert.deepEqual((await aggLib.formSchema("nine_router")).fields.map((f) => f.key),
  ["base_url", "allow_private_http", "include", "exclude", "hidden", "dedup", "show_both", "tls_ca_file", "tls_pin_sha256"]);
assert.deepEqual((await aggLib.formSchema("litellm")).secret_inputs.map((s) => [s.slot, s.required]), [["credential_env", true]]);
assert.equal((await aggLib.testAggregator({ provider: "nine_router", fields: [] })).message.id, "query.nine_router.locked", "`?aggtest=locked`");
console.log("PASS: six-account demo fixture with the Bedrock and monogram switches; aggregator instances and forms");
process.exit(0); // the shared modules keep a BroadcastChannel open
