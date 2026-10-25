#!/usr/bin/env node
// PRD 0022 RF3-RF6: account rows, figure block, monograms, the status line and its overflow order, the state ring
// and the honest-data marks, rendered by the production component module in Node (no browser).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { installUiEnv, report } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const c = await import("../apps/desktop/ui/js/components.js");

// RF3.1: figure on line 1, the unit word below it (one block, unit never inline in rows)
const ok = report({ left: 22 });
const html = c.rowHtml(ok, { title: "Claude Code", sub: "work@example.com" });
assert.match(html, /<span class="fig"><b class="">22%<\/b><small>left<\/small><\/span>/, "figure block with the unit below");
// RF3.2: the whole row header is one button with aria-expanded and a complete accessible name
assert.match(html, /<button class="rh" data-open="claude::work@example.com" aria-expanded="false" aria-label="Claude Code, work@example.com, 22% left/);
assert.doesNotMatch(html.replace(/<button class="rh"[^>]*>/, ""), /<button/, "no nested interactive element in the row header");
// RF3.2 (DS-CMP-ROW-4): a fresh age is never repeated per row; an aging one is
assert.doesNotMatch(html, /Fresh/);
assert.match(c.rowHtml(report({ left: 40, freshness: "aging" }), { title: "x" }), /Aging · /);

// RF3.3: several accounts of a provider → the account label is the title (the caller passes it), monogram on tile
const two = [report({ account: "work@example.com", index: 0 }), report({ account: "side@example.com", index: 1 }), report({ provider: "kimi", account: "ops", index: 2 })];
assert.deepEqual([...c.repeatedProviders(two)], ["claude"]);
// RF3.4 (DS-CMP-MONO-1..3): only providers with two or more accounts; first two letters, collisions resolved
const monos = c.monograms(two);
assert.equal(monos.get("claude::work@example.com"), "WO");
assert.equal(monos.get("claude::side@example.com"), "SI");
assert.equal(monos.has("kimi::ops"), false, "a single account of a provider has no monogram");
const clash = c.monograms([report({ account: "Work Acme", index: 0 }), report({ account: "Work Apex", index: 1 })]);
assert.equal(clash.get("claude::Work Acme"), "WA");
assert.notEqual(clash.get("claude::Work Apex"), "WA", "collision gets another letter");
assert.match(c.tileHtml("claude", 32, "WO"), /class="mono-badge"[^>]*aria-hidden="true">WO</);
assert.doesNotMatch(c.tileHtml("claude", 32, ""), /mono-badge/);

// RF4.1: one status line with the binding label and the absolute reset at the right
assert.match(html, /<span class="stl"[^>]*><span class="stl-l"><svg class="ring pct bad"[^>]*>.*<span class="lbl fg">Week<\/span><\/span><span class="rst"/);
// RF4.2: the overflow order: compact label → drop detail → truncate; the reset is never measured or cut
const width = (p) => [...`${p.label}${p.detail ? ` · ${p.detail}` : ""}`].length * 7;
const longest = { label: "34 puntos por encima del ritmo", compact: "↑ 34 pts", detail: "se agota a las 18:53, 1d 7h antes del reinicio" };
assert.deepEqual(c.planFit(1000, longest, width), [], "fits: nothing yields");
assert.deepEqual(c.planFit(304, longest, width), ["compact", "drop"], "304 px: compact label, then the detail goes");
assert.deepEqual(c.planFit(304, { ...longest, compact: "" }, width), ["drop"]);
assert.deepEqual(c.planFit(40, { ...longest, compact: "" }, width), ["drop", "truncate"]);

// RF5.1: the ring is static, aria-hidden, one arc; icons replace it for non-quota states
const ring = c.ringHtml("pct", 22, 12);
assert.match(ring, /aria-hidden="true"/);
assert.equal((ring.match(/class="ra"/g) ?? []).length, 1, "one arc");
assert.doesNotMatch(ring, /animate|transition/);
assert.match(c.stateGlyph(c.accountState(report({ status: "authentication_error", left: null }))), /class="i sg accent"/);
assert.match(c.ringHtml("exhausted", 0, 12), /ring exhausted/);

// RF6.1: unknown is "—" and a hatched meter, never 0%; stale is dimmed; exhausted reads the word
const unknown = report({ left: null });
const unknownHtml = c.rowHtml(unknown, { title: "Kimi" });
assert.match(unknownHtml, /<b class="">—<\/b>/);
assert.match(unknownHtml, /class="mtr none"/);
assert.doesNotMatch(unknownHtml, />0%</, "no 0% for an unknown metric");
const stale = report({ status: "error", left: null, lastGood: { checked_at: new Date(Date.now() - 42 * 60e3).toISOString(), metrics: [report({ left: 74 }).metrics[0]] } });
const staleHtml = c.rowHtml(stale, { title: "Groq" });
assert.match(staleHtml, /class="fig stale"/);
assert.match(staleHtml, /class="mtr good stale"/);
assert.match(staleHtml, /Stale/);
assert.doesNotMatch(staleHtml, /class="pm/, "no pace mark on a stale meter");
const gone = c.rowHtml(report({ exhausted: true }), { title: "Cursor" });
assert.match(gone, /<b class="bad">Exhausted<\/b>/);
assert.doesNotMatch(gone, /<small>left<\/small>/, "a word figure has no unit line");

// every locale renders the unit word below the figure
for (const [locale, unit] of [["pt-BR", "disponível"], ["es", "disponible"], ["it", "disponibile"]]) {
  await i18n.setLocale(locale);
  assert.match(c.rowHtml(ok, { title: "x" }), new RegExp(`<small>${unit}</small>`));
}

// the status-line styles keep it to one line (DS-CMP-STL-5)
const css = await readFile(new URL("../apps/desktop/ui/css/components.css", import.meta.url), "utf8");
assert.match(css, /\.stl \{[^}]*height: 20px;[^}]*white-space: nowrap;/);

console.log("PASS: rows, figure block, monograms, status line fitting, state ring and honest marks");
