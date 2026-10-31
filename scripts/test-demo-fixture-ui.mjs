#!/usr/bin/env node
// PRD 0022 RF18.2: the browser demo's `?fixture=six` swaps its accounts for the mockups' set (two Claude accounts,
// Codex, Cursor on extra usage, Kimi, a stale Groq, an Anthropic API account needing sign-in), example.com identities
// only; `&bedrock` adds the metered AWS Bedrock account (PRD 0027 RF3.1) and `?mono=XY` the first account's monogram
// override (DS-CMP-MONO-4).
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
globalThis.location = { search: "?fixture=six&bedrock&mono=zq" };
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const lib = await import("../apps/desktop/ui/js/lib.js");
const c = await import("../apps/desktop/ui/js/components.js");

const { reports } = await lib.getView();
assert.deepEqual([...new Set((await lib.trayAccounts()).map((a) => a.provider_label))], ["Claude Code", "OpenAI Codex", "Cursor", "Kimi", "Groq", "Anthropic API", "AWS Bedrock"], "Menu bar shows brand labels, never provider ids");
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
console.log("PASS: six-account demo fixture with the Bedrock and monogram switches");
process.exit(0); // the shared modules keep a BroadcastChannel open
