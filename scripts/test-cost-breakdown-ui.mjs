#!/usr/bin/env node
// Browser-free UI smoke for PRD 0042 RF6: the Days · Models · Sessions · Blocks · Projects tabs of Settings › Local
// cost (browser demo of the core's `cost_breakdown`) render empty, partial and full, with the KPI header ("—" when
// unknown, never 0), inline labels, the hidden project left out of the total and the active block's live figures;
// the card's "This block" line is present only while the active block has a burn rate. Every built-in locale renders
// without raw catalog keys.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const ui = (path) => fileURLToPath(new URL(`apps/desktop/ui/${path}`, root));
const storage = new Map();
globalThis.window = { open: () => {} };
globalThis.document = { hidden: false, body: { append: () => {} }, documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} }, addEventListener: () => {} };
globalThis.MutationObserver = class { observe() {} disconnect() {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (k) => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, String(v)) };
globalThis.fetch = async (relative) => {
  try {
    const data = JSON.parse(await readFile(ui(relative), "utf8"));
    return { ok: true, json: async () => data };
  } catch { return { ok: false, json: async () => ({}) }; }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { costCardHtml, liveLineHtml } = await import("../apps/desktop/ui/js/cost.js");
const { breakdownHtml, breakdownPanelHtml } = await import("../apps/desktop/ui/js/settings/cost-breakdown.js");
const { costHtml } = await import("../apps/desktop/ui/js/settings/cost.js");
await i18n.initI18n();
await i18n.setLocale("en-US");

const TABS = ["day", "model", "session", "block", "project"];
const raw = /settings\.costBy\.|cost\.live\.tip|\{[a-z_]+\}/;

// Empty: every source off. Each tab renders its KPI header with "—" and the empty sentence; nothing reads as $0.
for (const by of TABS) {
  const html = breakdownPanelHtml(by, await lib.costBreakdown(by));
  assert.match(html, /data-cost-kpis/, `${by}: KPI header`);
  assert.match(html, /data-cost-by-empty/, `${by}: empty state`);
  assert.match(html, /lc-kpi unknown/, `${by}: unknown total`);
  assert.ok(!/\$0\.00/.test(html), `${by}: no zero cost`);
}

// Partial: one source on. Full: both.
const status = await lib.costStatus();
const enable = async (id) => lib.costEnable(id);
await enable("claude_code");
const partial = breakdownPanelHtml("session", await lib.costBreakdown("session"));
await enable("codex_cli");
for (const by of TABS) {
  const html = breakdownPanelHtml(by, await lib.costBreakdown(by));
  assert.ok(!html.includes("data-cost-by-empty"), `${by}: rows when on`);
  assert.ok(!raw.test(html), `${by}: raw key or placeholder in ${html.match(raw)?.[0]}`);
}
const full = breakdownPanelHtml("session", await lib.costBreakdown("session"));
assert.ok(full.split("data-session=").length > partial.split("data-session=").length, "full lists more sessions than partial");

// Inline labels: a session label and a project label round-trip through the core's answer.
const sessions = await lib.costBreakdown("session");
await lib.costSetLabel("session", sessions.sessions[0].session, "Refactor");
assert.match(breakdownPanelHtml("session", await lib.costBreakdown("session")), /value="Refactor"/);
await assert.rejects(lib.costSetLabel("session", sessions.sessions[0].session, "x".repeat(49)));
let projects = await lib.costBreakdown("project");
const first = projects.projects[0];
await lib.costSetLabel("project", first.project, "Client work");
assert.match(breakdownPanelHtml("project", await lib.costBreakdown("project")), /value="Client work"/);

// Hidden project: listed, flagged, left out of the total.
const before = projects.total.requests;
await lib.costSetHidden(first.project, true);
projects = await lib.costBreakdown("project");
const hidden = breakdownPanelHtml("project", projects);
assert.match(hidden, /lc-hidden/);
assert.match(hidden, /aria-pressed="true"/);
assert.ok(projects.total.requests < before, "a hidden project leaves the total");
await lib.costSetHidden(first.project, false);

// Blocks: the active block first with its live figures and the local-estimate wording.
const blocks = breakdownPanelHtml("block", await lib.costBreakdown("block"));
assert.match(blocks, /data-cost-live="claude_code"/);
assert.match(blocks, /lc-active/);
assert.match(blocks, /data-cost-projection/);

// The card line: present while the active block has a burn rate, absent without one.
const usage = await lib.costUsage();
const claude = usage.sources.find((s) => s.source === "claude_code");
assert.match(costCardHtml(claude, usage), /data-cost-live="claude_code"/);
assert.match(liveLineHtml(claude, usage), /title="Local estimate of the current 5-hour block/);
const noRate = { ...usage, live: usage.live.map((l) => ({ ...l, line: null, projection: null })) };
assert.equal(liveLineHtml(claude, noRate), "");
assert.ok(!costCardHtml(claude, { ...usage, live: [] }).includes("data-cost-live"));

// S-02: an unreadable index shows its note in the card and the tray line, never nothing (as when off).
{
  const { costLineHtml } = await import("../apps/desktop/ui/js/cost.js");
  const note = { id: "cost.view.unreadable", params: { source: "Claude Code" }, fallback: "Claude Code: the local cost index could not be read; erase it in Settings › Local cost or with --cost-erase." };
  const unreadable = { ...claude, state: "unreadable", note, usage: null };
  const card = costCardHtml(unreadable, usage);
  assert.match(card, /lc-note bad/);
  assert.match(card, /could not be read/);
  assert.ok(!card.includes("lc-tiles"), "no tiles without usage");
  assert.match(costLineHtml(unreadable, usage), /cost-line bad/);
  assert.equal(costCardHtml({ ...claude, state: "off", usage: null }, usage), "");
}

// Every built-in locale renders the sub-page, the tabs and the source options without raw keys.
const view = await lib.costStatus();
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const page = costHtml(view, null, null) + breakdownHtml();
  assert.ok(!/settings\.costBy\.|settings\.cost\.more/.test(page), `${code}: raw key in the page`);
  for (const by of TABS) {
    const html = breakdownPanelHtml(by, await lib.costBreakdown(by));
    assert.ok(!raw.test(html), `${code}/${by}: raw key or placeholder ${html.match(raw)?.[0]}`);
  }
}
console.log("PASS: local cost sessions, blocks and projects (PRD 0042 RF6): tabs empty/partial/full, labels, hidden project, live block and the card line in every locale");
