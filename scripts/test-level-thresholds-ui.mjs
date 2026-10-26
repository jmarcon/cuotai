#!/usr/bin/env node
// DS-ID-5: the desktop level() uses the core thresholds (good >= 70, warn >= 30); the core test
// `level_thresholds_are_70_and_30_and_match_the_desktop_ui` pins the same numbers to `tray::Level`.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
const { level } = await import("../apps/desktop/ui/js/lib.js");
for (const [pct, lv] of [[100, "good"], [70, "good"], [69.9, "warn"], [30, "warn"], [29.9, "bad"], [0, "bad"], [null, "unknown"]]) {
  assert.equal(level(pct), lv, `level(${pct})`);
}
console.log("level thresholds ok");
