#!/usr/bin/env node
// PRD 0020 RNF (accessibility): an automated axe-core pass over the real UI in the browser demo, on the tray, the
// main window and the Settings pages, in light and dark. It complements, and never replaces, the manual
// screen-reader pass. axe-core is fetched from cdnjs at a pinned version and verified against a pinned SHA-256
// before it runs in the page (nothing is vendored and nothing else leaves 127.0.0.1).
// Usage: node apps/desktop/tools/ui-shots/a11y.mjs [--json] [--all]   Exit: 0 clean, 1 with serious/critical
// violations (`--all` also fails on moderate/minor ones), 3 when axe-core could not be fetched or verified.
import { createHash } from "node:crypto";
import { launch } from "./driver.mjs";

const AXE_URL = "https://cdnjs.cloudflare.com/ajax/libs/axe-core/4.10.2/axe.min.js";
const AXE_SHA256 = "b511cd9dec01c76f4b2ad1723b66b6db37d4c2eb4ed199076e1829d9ee7b75e3";
const args = process.argv.slice(2);
const json = args.includes("--json");
const failOn = args.includes("--all") ? ["minor", "moderate", "serious", "critical"] : ["serious", "critical"];
const PAGES = [
  ["tray", "tray.html", 380, 600],
  ["main", "index.html", 1000, 760],
  ["settings-general", "settings.html?page=general", 760, 560],
  ["settings-dashboard", "settings.html?page=dashboard", 760, 560],
  ["settings-alerts", "settings.html?page=alerts", 760, 560],
  ["settings-menubar", "settings.html?page=menubar", 760, 560],
];
// Contrast is judged per theme by contrast_gate.py against the theme contract; axe's own color-contrast rule is
// reported too, but it cannot see through gradients and image backgrounds, so it stays advisory (listed, not failing).
// en-US in both modes, then the languages whose scripts or lengths stress the layout and the names (dark only).
const RUNS = [["en-US", "dark"], ["en-US", "light"], ...["de", "ru", "ko", "hi-IN", "id-ID", "tr", "vi", "zh-CN", "fr", "ja"].map((code) => [code, "dark"])];
const ADVISORY = new Set(["color-contrast"]);

let source;
try {
  const res = await fetch(AXE_URL);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  source = Buffer.from(await res.arrayBuffer());
  const sha = createHash("sha256").update(source).digest("hex");
  if (sha !== AXE_SHA256) throw new Error(`checksum mismatch (${sha})`);
} catch (e) {
  console.error(`a11y: axe-core unavailable (${e.message}); not run`);
  process.exit(3);
}
const axeSource = source.toString("utf8");

const ui = await launch();
const report = {};
let failing = 0;
try {
  for (const [locale, mode] of RUNS) {
    for (const [name, page, w, h] of PAGES) {
      await ui.open(page, { w, h, scale: 1, mode, locale, theme: mode === "light" ? "github" : "dracula" });
      await ui.evaluate(axeSource);
      const result = await ui.evaluate(`axe.run(document, { resultTypes: ["violations"] }).then((r) => r.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.slice(0, 3).map((n) => n.target.join(" ")) })))`);
      const found = result ?? [{ id: "error", impact: "critical", help: "axe did not run", nodes: [] }];
      const bad = found.filter((v) => failOn.includes(v.impact) && !ADVISORY.has(v.id));
      failing += bad.length;
      const tag = locale === "en-US" ? mode : `${mode} ${locale}`;
      report[`${name}.${tag.replace(" ", ".")}`] = found;
      if (!json) {
        console.log(`${bad.length ? "FAIL" : "ok  "} ${name} ${tag}: ${bad.length} violation(s), ${found.length - bad.length} advisory`);
        for (const v of found) console.log(`       ${bad.includes(v) ? "" : "(advisory) "}${v.impact} ${v.id}: ${v.help} -> ${v.nodes.join(" | ")}`);
      }
    }
  }
} finally {
  ui.close();
}
if (json) console.log(JSON.stringify(report, null, 1));
console.error(`ui-shots a11y: ${failing} violation(s)`);
process.exit(failing ? 1 : 0);
