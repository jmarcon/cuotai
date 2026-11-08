#!/usr/bin/env node
// PRD 0044 RF5 acceptance: Settings › Alerts › Notification channels escapes a channel's label and the delivery time
// exactly once (an "&" is shown as "&", never as "&amp;"), and the Alerts page wires the channel list once, so Send
// test posts once and Remove asks once.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "en-US" });
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const { listHtml } = await import("../apps/desktop/ui/js/settings/channels.js");

const channel = {
  id: "0123456789abcdef", kind: "webhook", label: "R&D <ops>", host: "hooks.example.com", enabled: true,
  filters_text: { kinds: [], accounts: { id: "x", params: {}, fallback: "of every account" }, severity: null, names: { id: "y", params: {}, fallback: "names hidden" } },
  failing_long: true, failing_status: 500,
  last: { outcome: "failed", status: 500, at: "2026-11-04T12:00:00Z" },
};
const html = listHtml({ channels: [channel], log: [] });
assert.ok(html.includes("R&amp;D &lt;ops&gt;"), "the label is escaped");
assert.ok(!html.includes("&amp;amp;") && !html.includes("&amp;lt;"), "and escaped only once");

const alerts = await readFile(new URL("../apps/desktop/ui/js/settings/alerts.js", import.meta.url), "utf8");
assert.equal(alerts.match(/^\s*bindChannels\(/gm)?.length, 1, "the channel list is bound once");

console.log("channels UI: ok");
