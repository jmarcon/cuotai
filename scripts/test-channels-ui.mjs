#!/usr/bin/env node
// PRD 0044 RF5 acceptance: Settings › Alerts › Notification channels escapes a channel's label and the delivery time
// exactly once (an "&" is shown as "&", never as "&amp;"), and the Alerts page wires the channel list once, so Send
// test posts once and Remove asks once. PRD 0044 re-validation (V-04, V-08, V-10): the log line reads the kind in the
// person's language, a webhook is lowercase mid-sentence, and the default filters read as one "All kinds" item.
// Re-validation V-13/V-12: at the maximum, Add stays focusable with its reason
// (`aria-disabled`, a tooltip and a hint), and Slack and Discord rows carry different icons.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "en-US" });
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const { consentHtml, listHtml } = await import("../apps/desktop/ui/js/settings/channels.js");
const { channelConsent } = await import("../apps/desktop/ui/js/lib.js");

const channel = {
  id: "0123456789abcdef", kind: "webhook", label: "R&D <ops>", host: "hooks.example.com", enabled: true,
  filters_text: { kinds: [], accounts: { id: "x", params: {}, fallback: "of every account" }, severity: null, names: { id: "y", params: {}, fallback: "names hidden" } },
  failing_long: true, failing_status: 500,
  last: { outcome: "failed", status: 500, at: "2026-11-04T12:00:00Z" },
};
const html = listHtml({ channels: [channel], log: [] });
assert.ok(html.includes("R&amp;D &lt;ops&gt;"), "the label is escaped");
assert.ok(!html.includes("&amp;amp;") && !html.includes("&amp;lt;"), "and escaped only once");

// V-10: the default filters (every kind but the digest) are one item; the digest is named when it is on too.
const DEFAULT_KINDS = ["reset", "threshold", "exhausted", "recovered", "projected", "overage", "floor", "session_expiring", "limit_changed"];
const draft = (kinds) => ({ kind: "webhook", label: "Ops", sign: false, filters: { kinds, accounts: [], min_severity: "info", include_names: null } });
const consent = await channelConsent(draft(DEFAULT_KINDS), "https://hooks.example.com/x");
assert.deepEqual(consent.filters.kinds.map((k) => i18n.tl(k)), ["All kinds"], "the default filters read as one item");
const withDigest = await channelConsent(draft([...DEFAULT_KINDS, "digest"]), "https://hooks.example.com/x");
assert.deepEqual(withDigest.filters.kinds.map((k) => i18n.tl(k)), ["All kinds", "Weekly digest"], "the digest is still named");
const three = await channelConsent(draft(["threshold", "exhausted", "floor"]), "https://hooks.example.com/x");
assert.equal(three.filters.kinds.length, 3, "any other set lists its kinds");

// V-08: "webhook" is lowercase inside the consent title; the standalone label keeps "Webhook".
const title = consentHtml(consent);
assert.ok(title.includes("Send notifications to this webhook address?"), "the consent title writes webhook in lowercase");
assert.ok(html.includes("Webhook · hooks.example.com"), "the list line starts with the standalone label");

// V-04: the log line and the Send-test reason read in the person's language (the core's labels).
const label = (id, fallback) => ({ id, params: {}, fallback });
const logView = {
  channels: [channel], max: 5,
  log: [{ at: "2026-11-04T12:00:00Z", channel: channel.id, kind: "threshold", outcome: "failed", attempt: 3, error: "network",
    kind_label: label("channels.kind.threshold", "Thresholds"), error_label: label("channels.error_category.network", "network error") }],
};
await i18n.setLocale("de");
const german = listHtml(logView);
assert.ok(german.includes("Schwellen"), "the log kind is German");
assert.ok(german.includes("Netzwerkfehler"), "the log error is German");
assert.ok(!/· threshold ·|network</.test(german), "no raw slug or category");
await i18n.setLocale("en-US");
const { icon } = await import("../apps/desktop/ui/js/lib.js");
const five = ["webhook", "ntfy", "slack", "discord", "ntfy"].map((kind, i) => ({ ...channel, id: `${i}`.repeat(16), kind, label: `C${i}`, failing_long: false }));
const full = listHtml({ channels: five, log: [], max: 5 });
const add = full.match(/<button[^>]*data-channel-add[^>]*>/)?.[0] ?? "";
assert.ok(add.includes('aria-disabled="true"') && add.includes("is-disabled") && add.includes('title="At most 5 channels.'), "Add at the maximum explains itself");
assert.ok(!/\sdisabled[\s>]/.test(add), "and stays focusable");
assert.ok(full.includes("data-channels-max") && full.includes("At most 5 channels. Remove one to add another."), "the hint says why");
const open = listHtml({ channels: five.slice(0, 4), log: [], max: 5 }).match(/<button[^>]*data-channel-add[^>]*>/)?.[0] ?? "";
assert.ok(!open.includes("aria-disabled") && !open.includes("is-disabled"), "below the maximum, Add is a plain button");
const rowIcon = (id) => full.split(`data-row="channel-${id}"`)[1]?.match(/<svg[\s\S]*?<\/svg>/)?.[0];
assert.equal(rowIcon("2".repeat(16)), icon("hash"), "Slack has the hash icon");
assert.equal(rowIcon("3".repeat(16)), icon("message"), "Discord has the speech bubble icon");
assert.notEqual(rowIcon("2".repeat(16)), rowIcon("3".repeat(16)), "Slack and Discord rows differ");

const alerts = await readFile(new URL("../apps/desktop/ui/js/settings/alerts.js", import.meta.url), "utf8");
assert.equal(alerts.match(/^\s*bindChannels\(/gm)?.length, 1, "the channel list is bound once");

console.log("channels UI: ok");
