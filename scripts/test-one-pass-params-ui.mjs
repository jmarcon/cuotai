#!/usr/bin/env node
// Browser-free UI test for the PRD 0034 re-validation (S-02): catalog params are substituted in one pass. A param value
// that carries `{name}` stays literal, with a catalog template and on the fallback path (the core's interpolated
// English, or a fixture's raw template), in plain text and in HTML.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "en-US" });
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const { durationText } = await import("../apps/desktop/ui/js/lib.js");
await i18n.initI18n();

// A catalog template (`agg.tightest` in the UI's en-US catalog: "Tightest: {name} · {pct}% left").
{
  const note = { id: "agg.tightest", params: { name: "Team {pct}", pct: "40" }, fallback: "Tightest: Team {pct} · 40% left" };
  assert.equal(i18n.tl(note), "Tightest: Team {pct} · 40% left");
  assert.equal(i18n.t("agg.tightest", { name: "Team {pct}", pct: "40" }), "Tightest: Team {pct} · 40% left");
}
console.log("PASS: a catalog template substitutes its params once");

// No catalog template: the core's fallback is already interpolated and is shown as it is.
{
  const note = { id: "demo.not.in.catalog", params: { label: "Team {code}", code: "<b>x</b>" }, fallback: "Row Team {code} failed" };
  assert.equal(i18n.tl(note), "Row Team {code} failed");
  assert.equal(i18n.tle(note), "Row Team {code} failed");
  // A nested note whose own value carries a placeholder of the outer note.
  const inner = { id: "demo.inner", params: { who: "{code}" }, fallback: "by {code}" };
  const outer = { id: "demo.outer", params: { by: inner, code: "x" }, fallback: "Changed by {code}" };
  assert.equal(i18n.tl(outer), "Changed by {code}");
}
console.log("PASS: an interpolated fallback is not substituted again");

// No catalog template: a fixture's raw template is substituted once, and the value's braces stay literal.
{
  const note = { id: "demo.raw.template", params: { name: "{n} & co", n: "3" }, fallback: "{name} has {n}" };
  assert.equal(i18n.tl(note), "{n} & co has 3");
  assert.equal(i18n.tle(note), "{n} &amp; co has 3");
}
console.log("PASS: a raw fallback template is substituted once");

// The demo's durations pass the raw English pattern, so `tl` is their only substitution, in every language.
assert.equal(durationText(2 * 3600 + 5 * 60), "2h 5m");
await i18n.setLocale("de");
assert.equal(durationText(2 * 3600 + 5 * 60), "2\u00a0Std. 5\u00a0Min.");
await i18n.setLocale("en-US");
console.log("PASS: durations read in one pass");
