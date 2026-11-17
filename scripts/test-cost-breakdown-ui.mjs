#!/usr/bin/env node
// Browser-free UI smoke for PRD 0042 RF6: the Days · Models · Sessions · Blocks · Projects tabs of Settings › Local
// cost (browser demo of the core's `cost_breakdown`) render empty, partial and full, with the KPI header ("—" when
// unknown, never 0), inline labels, the hidden project left out of the total (and the note on every tab) and the active block's live figures;
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
// PRD 0020 re-validation R-06: a name with "&", "<" or '"' is escaped once in the placeholder and the accessible name.
{
  const data = await lib.costBreakdown("session");
  const named = { ...data, sessions: data.sessions.map((s, i) => (i ? s : { ...s, label: null, name: { id: "x.none", params: {}, fallback: 'R&D <"x">' } })) };
  const html = breakdownPanelHtml("session", named);
  assert.match(html, /placeholder="R&amp;D &lt;&quot;x&quot;&gt;"/, "the placeholder is escaped once");
  assert.ok(!html.includes("&amp;amp;"), "a name is escaped twice");
}
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
// PRD 0042 M-05: every tab says how many hidden projects its figures leave out, in the core's words.
for (const by of TABS) {
  const html = breakdownPanelHtml(by, await lib.costBreakdown(by));
  assert.match(html, /data-cost-hidden-note>1 hidden project is not counted in these figures\./, `${by}: the hidden note`);
}
{
  const summary = { ...(await lib.costUsage()), hidden_note: (await lib.costBreakdown("day")).hidden_note };
  const sc = summary.sources.find((s) => s.source === "claude_code");
  assert.match(costCardHtml(sc, summary), /lc-foot">[^<]*<br\/><span data-cost-hidden-note>1 hidden project/, "the card foot says it");
}
await lib.costSetHidden(first.project, false);
for (const by of TABS) assert.ok(!breakdownPanelHtml(by, await lib.costBreakdown(by)).includes("data-cost-hidden-note"), `${by}: no note when none is hidden`);

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

// PRD 0042 re-validation H-01/M-08: a half-unpriced live block keeps its unknown cost on the card and the Blocks tab
// (the core's texts, never money the desktop formats), projects tokens only, and a block without a burn rate says why.
{
  const loc = (id, params, fallback) => ({ id, params, fallback });
  const partialLine = loc("cost.live.line_partial", { cost: { id: "amount.money", params: { value: "1.8", currency: "USD" }, fallback: "$1.80" }, unknown: "600K", rate: "120K" },
    "This block ≈ $1.80 + unknown cost of 600K tokens · 120K tok/min");
  const tokens = loc("cost.live.projection_tokens", { tokens: "30M", at: "16:00" },
    "At this rate ≈ 30M tokens by 16:00; most of them have no known price, so no cost is projected");
  const half = { ...usage, live: usage.live.map((l) => ({ ...l, line: partialLine, projection: tokens, projected_block_cost: null,
    unknown_tokens: 600000, projected_tokens: 30000000, projected_unknown_tokens: 15000000, projection_kind: "tokens" })) };
  const card = liveLineHtml(claude, half);
  assert.match(card, /\+ unknown cost of 600K tokens/, "the card line names the unknown cost");
  assert.match(card, /no cost is projected/, "the tooltip carries the tokens projection");
  const page = await lib.costBreakdown("block");
  const live = page.live.map((l) => ({ ...l, line: partialLine, projection: tokens, projected_block_cost: null, projection_kind: "tokens" }));
  const tab = breakdownPanelHtml("block", { ...page, live });
  assert.match(tab, /\+ unknown cost of 600K tokens/, "the Blocks tab names the unknown cost");
  assert.match(tab, /data-cost-projection>At this rate ≈ 30M tokens/);
  const why = loc("cost.live.rate_few", { min: "3", minutes: "10", count: "2" }, "Burn rate: needs at least 3 requests in the last 10 minutes; 2 so far.");
  const waiting = page.live.map((l) => ({ ...l, line: null, projection: null, burn_rate_tokens_per_min: null, rate_missing: why }));
  const slow = breakdownPanelHtml("block", { ...page, live: waiting });
  assert.match(slow, /data-cost-rate>Burn rate: needs at least 3 requests in the last 10 minutes; 2 so far\./, "the core's reason");
  assert.ok(!slow.includes("data-cost-projection"), "no projection without a rate");
  assert.equal(liveLineHtml(claude, { ...usage, live: waiting }), "", "the card stays quiet without a rate");
}

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

// V-01 (DS-CMP-HON-2): the folded row and the tray line put "≈" only before a priced amount.
{
  const { costLineHtml, costSummary } = await import("../apps/desktop/ui/js/cost.js");
  await i18n.setLocale("en-US");
  const zero = { input: 0, output: 0, cache_read: 0, cache_write: 0, cache_write_1h: 0 };
  const some = { ...zero, input: 1000 };
  const est = (tokens, total) => ({ tokens, total, unknown_tokens: total == null ? tokens : zero });
  const line = (today, period) => {
    const sc = { ...claude, usage: { today, yesterday: today, period } };
    return { v: costSummary(sc, usage), html: costLineHtml(sc, usage) };
  };
  for (const [today, label] of [[est(zero, null), "no usage"], [est(some, null), "unknown"], [est(some, 0.001), "sub-cent"]]) {
    const { v, html } = line(today, today);
    assert.ok(!v.today.includes("≈") && !v.period.includes("≈"), `${label}: no ≈ in ${JSON.stringify(v)}`);
    assert.ok(!html.includes("≈"), `${label}: no ≈ in the tray line ${html}`);
  }
  const { v } = line(est(some, 1.5), est(some, 12));
  assert.ok(v.today.startsWith("≈") && v.period.startsWith("≈"), `a priced amount keeps ≈: ${JSON.stringify(v)}`);
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
