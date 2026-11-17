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

// PRD 0037 re-validation (V-09): `?eleven` swaps the accounts for the ElevenLabs set, with the core's shapes and ids.
globalThis.location = { search: "?eleven" };
const elLib = await import("../apps/desktop/ui/js/lib.js?eleven");
const el = await elLib.getView();
const byAccount = Object.fromEntries(el.reports.map((r) => [r.account.split("@")[0], r]));
assert.deepEqual(el.reports.map((r) => r.account), ["voice", "armed", "active", "capped", "unlimited", "nolimit", "free", "renewal", "scope", "drift", "eu", "us"]
  .map((name) => `${name}@example.com`), "the ElevenLabs set, example.com identities only");
assert.ok(el.reports.every((r) => r.provider === "eleven_labs"), "ElevenLabs accounts only");
assert.deepEqual(["armed", "active", "capped", "unlimited"].map((k) => byAccount[k].overage.state), ["armed", "active", "cap_reached", "active"], "extra-usage states");
assert.equal(byAccount.voice.overage, undefined, "a plan without extra usage has no overage");
const extra = byAccount.unlimited.metrics.find((m) => m.billing === "overage" && m.kind === "usage");
assert.ok(extra.unlimited && extra.limit == null && extra.uncapped, "the extra-credit counter states no cap");
assert.ok(byAccount.capped.metrics.some((m) => m.billing === "overage" && m.limit > 0 && m.used >= m.limit), "an extra-credit counter at its cap");
assert.equal(byAccount.nolimit.binding, null, "no binding without a reported limit");
assert.ok(byAccount.nolimit.notes_l10n.some((n) => n.id === "note.eleven_labs.limit_missing"));
const zero = byAccount.free.metrics[0];
assert.deepEqual([zero.limit, zero.remaining, zero.state, byAccount.free.class, byAccount.free.binding.reason], [0, 0, "exhausted", "exhausted", "exhausted"], "a zero limit is exhausted (RF3)");
assert.ok(byAccount.renewal.binding.renewal_due && byAccount.renewal.binding.blocked_until == null, "a reset past at the reading is a renewal due (RF4)");
assert.equal(byAccount.renewal.metrics[0].resets_label.id, "metric.renewal_due");
assert.deepEqual([byAccount.scope.status, byAccount.scope.notes_l10n[0].id], ["authentication_error", "note.eleven_labs.scope_missing"]);
assert.deepEqual([byAccount.drift.status, byAccount.drift.drift.paths[0].path], ["error", "character_count"], "the changed response names its path");
// One status per source (RF9): the EU page has the incident, the US page none; each links its own sub-page.
assert.deepEqual([byAccount.eu.incident.source, byAccount.eu.incident.state, byAccount.eu.incident.page_url], ["eu", "partial_outage", "https://status.elevenlabs.io/eu"]);
assert.deepEqual([byAccount.us.incident.source, byAccount.us.incident.state, byAccount.us.incident.incident, byAccount.us.incident.page_url],
  ["united-states", "operational", null, "https://status.elevenlabs.io/united-states"]);
assert.ok(el.reports.filter((r) => r !== byAccount.eu).every((r) => r.incident.state === "operational"), "the EU incident stays on the EU account");
// Every text the set shows is a core id the shipped catalogs carry (the labels, units and notes the adapter writes).
const { readFileSync } = await import("node:fs");
const core = JSON.parse(readFileSync(new URL("../crates/cuotai-core/locales/de.json", import.meta.url), "utf8"));
const ids = new Set(el.reports.flatMap((r) => [...(r.notes_l10n ?? []).map((n) => n.id), r.retry_note?.id, ...r.metrics.flatMap((m) => [m.label_text?.id, m.unit_text?.id])]).filter(Boolean));
for (const id of ids) assert.ok(id in core, `core catalog id ${id}`);
const elSchema = await elLib.formSchema("eleven_labs");
assert.deepEqual([elSchema.credential_kind, elSchema.default_env, elSchema.secret_inputs[0].key_page], ["env", "ELEVENLABS_API_KEY", "https://elevenlabs.io/app/api/api-keys"]);
assert.deepEqual(elSchema.fields.map((f) => [f.id, f.label.id]), [["field.eleven_labs.region", "field.eleven_labs.region.label"]]);
assert.deepEqual(elSchema.fields[0].options.map((o) => o.id), ["default", "us", "eu", "in", "sg"].map((v) => `field.eleven_labs.region.option.${v}`));
for (const o of elSchema.fields[0].options) assert.ok(o.label.id in core, `core catalog id ${o.label.id}`);
assert.deepEqual(await elLib.visibleFields("eleven_labs", {}, "credential"), ["region"]);

// PRD 0038 re-validation: `?openrouter` swaps the accounts for the OpenRouter set, with the adapter's shapes and ids.
globalThis.location = { search: "?openrouter" };
const orLib = await import("../apps/desktop/ui/js/lib.js?openrouter");
const or = await orLib.getView();
const orBy = Object.fromEntries(or.reports.map((r) => [r.account.split("@")[0], r]));
assert.deepEqual(or.reports.map((r) => r.account), ["key", "nolimit", "balance", "negative", "limited"].map((name) => `${name}@example.com`),
  "the OpenRouter set, example.com identities only");
assert.ok(or.reports.every((r) => r.provider === "open_router"), "OpenRouter accounts only");
const keyLimit = orBy.key.metrics[0];
assert.deepEqual([keyLimit.label_text.id, keyLimit.unit, keyLimit.limit, keyLimit.window], ["metric.label.key_limit", "USD", 50, "monthly"], "the key view is in USD");
assert.ok(orBy.key.metrics.some((m) => m.label_text.id === "metric.label.free_models_today" && m.unit_text.id === "metric.unit.requisitions"));
assert.equal(orBy.key.binding.index, 0, "a key with a limit binds on it");
assert.ok(!orBy.nolimit.metrics.some((m) => m.label_text.id === "metric.label.key_limit"), "a key without a limit has no limit metric");
const balanceOf = (r) => r.metrics.find((m) => m.kind === "balance");
assert.deepEqual([balanceOf(orBy.balance).remaining, balanceOf(orBy.negative).remaining], [37.25, -3.4], "a positive and a negative balance");
for (const r of [orBy.balance, orBy.negative]) {
  assert.equal(r.binding, null, "a balance has no gauge, so no binding");
  assert.ok(r.metrics.every((m) => m.unit === "créditos" && m.unit_text.id === "metric.unit.credits"), "management-key amounts are credits, no `$`");
  assert.ok(r.notes_l10n.some((n) => n.id === "note.open_router.credits_unit"));
}
assert.ok(orBy.negative.notes_l10n.some((n) => n.id === "note.open_router.low_balance" && n.params.threshold === "5"));
const byOrigin = orBy.balance.metrics.filter((m) => m.label_text.id.startsWith("metric.label.spend_30d"));
assert.equal(byOrigin.length, 3, "spend by provider, with Others");
assert.ok(byOrigin.every((m) => m.period_text.id === "metric.period.last_30_days_utc" && m.resets_at == null), "the period line, no reset");
assert.deepEqual([orBy.limited.status, orBy.limited.notes_l10n[0].id], ["rate_limited", "query.open_router.rate_limited"]);
assert.equal(orBy.limited.retry_note?.id, "query.retry_after_until", "the core's wait line");
const orIds = new Set(or.reports.flatMap((r) => [...(r.notes_l10n ?? []).map((n) => n.id), r.retry_note?.id, ...r.metrics.flatMap((m) => [m.label_text?.id, m.unit_text?.id, m.period_text?.id])]).filter(Boolean));
for (const id of orIds) assert.ok(id in core, `core catalog id ${id}`);
console.log("PASS: six-account demo fixture with the Bedrock and monogram switches; aggregator instances and forms; the ElevenLabs set and form; the OpenRouter set");
process.exit(0); // the shared modules keep a BroadcastChannel open
