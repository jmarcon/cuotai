#!/usr/bin/env node
// PRD 0022 RF18.2: the browser demo's `?fixture=six` swaps its accounts for the mockups' set (two Claude accounts,
// Codex, Cursor on extra usage, Kimi, a stale Groq, an Anthropic API account needing sign-in), example.com identities
// only; `&bedrock` adds the metered AWS Bedrock account (PRD 0027 RF3.1) and `?mono=XY` the first account's monogram
// override (DS-CMP-MONO-4). PRD 0036 re-validation (V-12): `?agg=0,1,12,40,exhausted,litellm` adds aggregator
// instances of every size, and the 9router and LiteLLM forms carry the core's descriptors. PRD 0040 re-validation
// (V-10): `?feed=`, `?live=N` and `?feed-banner` give the agent-fed accounts, the live sessions and the unbound banner.
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

// PRD 0040 re-validation (V-10): `?feed=` puts agent-fed accounts first, in the backend's shapes; `?live=N` gives the
// first fed Claude account N sessions (five shown, then "and N more"); `?feed-banner` the unbound Claude Code banner.
globalThis.location = { search: "?feed=fresh,stale,absent,invalid,one-window&live=7&feed-banner" };
const feedLib = await import("../apps/desktop/ui/js/lib.js?feed");
const feedView = await feedLib.getView();
const fedBy = Object.fromEntries(feedView.reports.slice(0, 6).map((r) => [r.account.split("@")[0], r]));
assert.deepEqual(Object.keys(fedBy), ["fed", "hybrid", "stale", "waiting", "codex", "one-window"], "the fed accounts come first, example.com only");
const noteIds = (r) => r.notes_l10n.map((n) => n.id);
assert.deepEqual([fedBy.fed.source, fedBy.fed.status, fedBy.fed.freshness, fedBy.fed.metrics.length], ["feed:claude_code", "ready", "fresh", 2]);
assert.deepEqual(noteIds(fedBy.fed), ["note.claude_code.fed"]);
assert.deepEqual([fedBy.fed.access.level, fedBy.fed.access.summary.id, fedBy.fed.access.evidence.url],
  ["official", "access.summary.claude_code", "https://code.claude.com/docs/en/statusline"], "the Claude Code feed's own access entry");
assert.deepEqual([fedBy.hybrid.access.summary.id, noteIds(fedBy.hybrid)], ["access.summary.claude_code_or_api", ["note.claude_code.answered_by_api"]],
  "a hybrid account that fell back says the query answered");
assert.equal(fedBy.stale.freshness, "stale");
assert.ok(Date.now() - Date.parse(fedBy.stale.measured_at) >= 119 * 60e3, "the stale reading was measured two hours ago");
assert.ok(Date.now() - Date.parse(fedBy.stale.checked_at) < 60e3, "and queried now");
assert.deepEqual([fedBy.waiting.status, fedBy.waiting.metrics.length, noteIds(fedBy.waiting)], ["needs_configuration", 0, ["note.claude_code.no_feed"]],
  "no reading yet: needs setup, never a zero");
assert.deepEqual([fedBy.codex.provider, fedBy.codex.source, fedBy.codex.status, noteIds(fedBy.codex), fedBy.codex.access.summary.id],
  ["chat_gpt", "feed:codex_cli", "error", ["query.schema"], "access.summary.codex_cli"], "a Codex CLI log in an unknown shape");
assert.deepEqual(fedBy["one-window"].metrics.map((m) => m.window), ["window_5h"], "one window: the week is absent, not zero");
assert.ok(noteIds(fedBy["one-window"]).includes("note.claude_code.window_absent"));
const liveBlock = fedBy.fed.live;
assert.equal(liveBlock.sessions.length, 5, "five sessions shown");
assert.deepEqual([liveBlock.more.id, liveBlock.more.params.n], ["feed.live.more", 2], "and 2 more");
assert.deepEqual(liveBlock.sessions.slice(0, 3).map((x) => [x.context.params.pct, x.level]), [["50", "good"], ["75", "warn"], ["95", "bad"]],
  "the context at 50, 75 and 95 % in their display roles");
assert.deepEqual([liveBlock.chip, liveBlock.chip_level], [liveBlock.sessions[0].context, "good"], "the tray chip is the newest session");
assert.ok(Object.values(fedBy).filter((r) => r !== fedBy.fed).every((r) => !r.live), "only one account reads the live sessions");
assert.deepEqual(feedView.feed_banners.map((b) => [b.source, b.provider, b.query_mode, b.text.id, b.action.id]),
  [["claude_code", "claude", "claude_code", "feed.banner.claude_code", "feed.banner.add"]]);
const feedIds = new Set(feedView.reports.slice(0, 6).flatMap((r) => [...noteIds(r), r.access.summary.id, ...(r.live?.sessions ?? []).flatMap((x) => [x.context.id, x.cost.id])]));
for (const id of [...feedIds, "feed.live.title", "feed.live.caveat", "feed.live.more", "feed.banner.claude_code", "feed.banner.add"]) assert.ok(id in core, `core catalog id ${id}`);
// The Claude form: Source and Live session with the core's ids; the feed mode asks for no credential (`feed_only`) and
// hides Live session, which only a query-mode account binds.
const claudeSchema = await feedLib.formSchema("claude");
assert.deepEqual(claudeSchema.fields.map((f) => f.key), ["query_mode", "feed"]);
assert.deepEqual(claudeSchema.fields[0].options.map((o) => o.value), ["api", "claude_code", "claude_code_or_api"]);
for (const id of claudeSchema.fields.flatMap((f) => [f.label.id, f.help.id, ...f.options.map((o) => o.label.id)])) assert.ok(id in core, `core catalog id ${id}`);
assert.deepEqual(claudeSchema.feed_only, { key: "query_mode", choice: 1 });
assert.deepEqual(await feedLib.visibleFields("claude", {}, "credential"), ["query_mode", "feed"]);
assert.deepEqual(await feedLib.visibleFields("claude", { query_mode: 1 }, "credential"), ["query_mode"]);
assert.deepEqual(await feedLib.visibleFields("claude", { query_mode: 2 }, "credential"), ["query_mode"]);
// `?live=1` without a fed account gives the first Claude account one session; `?live=0` and no flag give none.
for (const [search, count] of [["?live=1", 1], ["?live=0", 0], ["", 0]]) {
  globalThis.location = { search };
  const liveLib = await import(`../apps/desktop/ui/js/lib.js?live${count}${search.length}`);
  const liveView = await liveLib.getView();
  assert.equal(liveView.reports.reduce((n, r) => n + (r.live?.sessions.length ?? 0), 0), count, `${search || "no flag"}: ${count} session(s)`);
  assert.deepEqual(liveView.feed_banners, [], "no banner without `?feed-banner`");
  assert.ok(!liveView.reports.some((r) => r.source.startsWith("feed:")), "no fed account without `?feed`");
}
// PRD 0041 re-validation (F-23, F-25): `?budget` gives the budget, floor and allowance states, and the demo's
// `check_budgets` answers like the core's: text rows read as typed, each problem at its field with its rule and a note
// naming the field's label, and each row's metric unit from the account's reading.
globalThis.location = { search: "?budget" };
const budgetLib = await import("../apps/desktop/ui/js/lib.js?budget");
const budgetView = await budgetLib.getView();
assert.deepEqual(budgetView.reports.slice(0, 4).map((r) => `${r.provider}:${r.account}`), [
  "open_ai_api:budget@example.com", "open_ai_api:budget-low@example.com", "open_router:floor@example.com", "kimi:floor-bad@example.com",
]);
const [spendAcc, lowAcc, floorAcc, badAcc] = budgetView.reports;
const budgets = spendAcc.metrics.filter((m) => m.derived);
assert.equal(budgets.length, 4, "four budgets on the spend account");
assert.ok(budgets.every((m) => m.kind === "quota" && m.budget_badge?.id === "budget.badge" && m.derived_from), "budgets are the person's quotas");
assert.ok(budgets.some((m) => m.state === "exhausted" && !m.allowance), "a budget over its limit has no allowance");
assert.ok(budgets.some((m) => m.partial === true && m.derived_from.partial === true && m.used != null), "a partial budget keeps a lower-bound used");
assert.ok(budgets.some((m) => m.unit === "EUR" && m.used == null), "a budget in another currency is not counted");
assert.deepEqual(spendAcc.notes_l10n.map((n) => n.id), ["note.budget.partial", "note.budget.unit_mismatch_named"]);
assert.deepEqual(spendAcc.notes_l10n[1].params, { budget: "Euro cap", metric_unit: "USD", budget_unit: "EUR" });
assert.ok(spendAcc.metrics.some((m) => !m.derived && m.budget_draft), "a metric offers Set a budget…");
assert.equal(lowAcc.metrics[0].allowance.id, "pace.allowance.budget_day_less", "the less-than allowance");
assert.deepEqual(floorAcc.floors.map((f) => [f.label, f.amount, f.role, !!f.chip]), [["Saldo", 10, "warn", true], ["Saldo", 5, "ok", false]], "two floors on one balance");
assert.equal(badAcc.floors[0].role, "bad");
assert.ok(floorAcc.metrics.some((m) => m.offers_floor), "a balance offers Set a floor…");
const row = (amount, unit = "USD", target = "spend") => ({ target, amount, unit, period: "monthly", start: null, filter: "", label: "", row: null });
const spendIndex = budgetView.reports.indexOf(spendAcc);
let check = await budgetLib.checkBudgets(spendIndex, "open_ai_api", [row("1.000,50"), row("1,000"), row("40", "EUR"), row("")], [{ target: "any", amount: "1.500", unit: "", filter: "" }], "de");
assert.deepEqual(check.problems.map((p) => [p.field, p.rule]), [
  ["budgets[1].amount", "amount_ambiguous"], ["budgets[3].amount", "amount"], ["floors[0].amount", "amount_ambiguous"], ["budgets[2].unit", "unit_mismatch"],
]);
assert.equal(check.problems[0].note.params.field.id, "budget.field.amount", "the note names the field's label");
assert.deepEqual([check.problems[0].note.params.whole, check.problems[0].note.params.decimal], ["1000", "1"]);
assert.deepEqual(check.problems[2].note.params.decimal, "1,5", "the readings are written for the dialog's language");
assert.deepEqual(check.units, ["USD", "USD", "USD", "USD"], "each row's metric unit from the reading");
check = await budgetLib.checkBudgets(null, "open_ai_api", [row("50", "")], [], "en-US");
assert.deepEqual([check.problems, check.units], [[], [null]], "a new account has no reading");
const suggestion = await budgetLib.budgetSuggestion(spendIndex, "Requests today");
assert.equal(suggestion.budget.kind, "usage");
assert.equal(suggestion.reference.id, "budget.reference.last_30_days");
await assert.rejects(budgetLib.budgetSuggestion(spendIndex, "Nope"), "an unknown metric is the core's invalid index");
console.log("PASS: six-account demo fixture with the Bedrock and monogram switches; aggregator instances and forms; the ElevenLabs set and form; the OpenRouter set; the agent-fed accounts, live sessions, banner and Claude form; the budget, floor and allowance set and the budget check");
process.exit(0); // the shared modules keep a BroadcastChannel open
