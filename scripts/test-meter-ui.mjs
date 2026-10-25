#!/usr/bin/env node
// PRD 0022 RF2: the continuous meter renders every state, carries the pace corridor at expected ± 5 points only on
// fresh meters (DS-CMP-MTR-6, -6b), the tick on 4 px meters (MTR-6a), never a zero for an unknown value (MTR-5),
// a sliver below 1 % (MTR-4), and exposes role="meter" with its value text (MTR-9).
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const { meterHtml, corridor } = await import("../apps/desktop/ui/js/components.js");

const band = (html) => /class="pm" style="left:([\d.]+)%;width:([\d.]+)%"/.exec(html);

// fresh good / warn / bad, each with a corridor at expected ± 5
for (const [pct, lv] of [[82, "good"], [45, "warn"], [12, "bad"]]) {
  const html = meterHtml(pct, { expected: 38 });
  assert.match(html, new RegExp(`class="mtr ${lv}"`), `level ${lv}`);
  assert.match(html, /role="meter" aria-label="[^"]+" aria-valuemin="0" aria-valuemax="100" aria-valuenow="\d+"/);
  const m = band(html);
  assert.ok(m, "fresh meter with a pace has a corridor");
  assert.equal(Number(m[1]), 33);
  assert.equal(Number(m[2]), 10);
}
// pace ahead / reserve / on pace only move the fill relative to the band; the band is the same geometry
assert.deepEqual(corridor(97), { center: 97, left: 92, width: 8 }, "clamped at 100");
assert.deepEqual(corridor(2), { center: 2, left: 0, width: 7 }, "clamped at 0");
assert.equal(corridor(null), null);

// no pace mark for stale, unknown, exhausted and overage (DS-CMP-MTR-6b)
for (const state of ["stale", "unknown", "exhausted", "overage"]) {
  const html = meterHtml(state === "unknown" ? null : 40, { state, expected: 38 });
  assert.doesNotMatch(html, /class="pm/, `${state} meter has no pace mark`);
}
// unknown: hatched track, no value, "No data" (never 0%)
const unknown = meterHtml(null, { expected: 38 });
assert.match(unknown, /class="mtr none"/);
assert.doesNotMatch(unknown, /aria-valuenow/);
// PRD 0020 RNF (axe): a value-less meter is an image named "No data", not a meter without aria-valuenow
assert.match(unknown, /role="img" aria-label="No data"/);
assert.doesNotMatch(unknown, /role="meter"/);
// every meter has an accessible name: the caller's metric name, else its value text (axe aria-meter-name)
assert.match(meterHtml(40, { name: "Weekly" }), /aria-label="Weekly"/);
assert.match(meterHtml(40), /aria-label="40% left"/);
assert.doesNotMatch(unknown, /0%/);
// stale: dimmed treatment class
assert.match(meterHtml(40, { state: "stale" }), /class="mtr warn stale"/);
// exhausted: empty bad track
assert.match(meterHtml(0, { state: "exhausted" }), /class="mtr bad"[^>]*aria-valuenow="0"/);
// a sliver below 1 %, never a full bar below 100 %
assert.match(meterHtml(0.4), /class="mf" style="width:1%"/);
assert.doesNotMatch(meterHtml(99.7), /width:100%/);
// the 4 px form: a tick at the exact expected share
assert.match(meterHtml(40, { thin: true, expected: 38 }), /class="pm tick" style="left:38%"/);
// translated value text
await i18n.setLocale("pt-BR");
assert.match(meterHtml(22), /aria-valuetext="22% disponível"/);

console.log("PASS: continuous meters, corridor geometry, honest unknown and stale states");
process.exit(0); // the shared modules keep a BroadcastChannel open
