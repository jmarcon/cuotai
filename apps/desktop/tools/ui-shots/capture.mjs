#!/usr/bin/env node
// PRD 0022 RF18.2/RF18.3: captures the real desktop UI (browser demo) for every screen of screens.tsv, at 2x, into
// docs/plans/assets/0022/delivered/ (or --out DIR), with the same names as the mockups in assets/0022/after/.
// Usage: node apps/desktop/tools/ui-shots/capture.mjs [--out DIR] [name-prefix ...]
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { launch, ROOT } from "./driver.mjs";

const args = process.argv.slice(2);
const outAt = args.indexOf("--out");
const out = outAt >= 0 ? args.splice(outAt, 2)[1] : join(ROOT, "docs/plans/assets/0022/delivered");
const prefixes = args;
const screens = readFileSync(fileURLToPath(new URL("screens.tsv", import.meta.url)), "utf8").trim().split("\n")
  .map((l) => l.split("\t")).filter(([name]) => !prefixes.length || prefixes.some((p) => name.startsWith(p)));
mkdirSync(out, { recursive: true });

const ui = await launch();
try {
  for (const [name, page, w, h, theme, mode, locale, clicks = "", storage = ""] of screens) {
    const preset = Object.fromEntries(storage.split(";").filter(Boolean).map((kv) => kv.split("=")));
    await ui.open(page, { w: +w, h: +h, theme, mode, locale, storage: preset });
    if (clicks) await ui.click(...clicks.split("|"));
    await ui.shot(join(out, `${name}.png`));
    console.log(`${name}.png ${w}x${h}`);
  }
} finally {
  ui.close();
}
process.exit(0);
