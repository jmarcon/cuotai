#!/usr/bin/env node
// The public screenshots in docs/assets/screenshots/ that README.md, docs/ and site/ share, from the browser demo's
// made-up accounts. Names and pixel sizes are fixed: site/index.html declares them. Usage, from the repository root:
//   node apps/desktop/tools/ui-shots/docs-shots.mjs [--out DIR] [name-prefix ...]
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { launch, ROOT } from "./driver.mjs";

const args = process.argv.slice(2);
const outAt = args.indexOf("--out");
const out = outAt >= 0 ? args.splice(outAt, 2)[1] : join(ROOT, "docs/assets/screenshots");
const wanted = (name) => !args.length || args.some((p) => name.startsWith(p));
const THEMES = [...readFileSync(join(ROOT, "apps/desktop/ui/js/themes.js"), "utf8").matchAll(/\{ id: "([a-z0-9]+)", label:/g)].map((m) => m[1]);
const MAIN = "index.html?update=fresh"; // the demo's update banner is not part of the picture
mkdirSync(out, { recursive: true });

const ui = await launch();
const dashboard = async (name, mode) => {
  await ui.open(MAIN, { w: 1200, h: 800, scale: 1.5, theme: "dracula", mode });
  await ui.shot(join(out, `${name}.png`));
};
/** 840 × 907: the dialog at the window's minimum width, clipped to its box. */
async function details() {
  const scale = 840 / 488, height = 907 / scale;
  await ui.open(MAIN, { w: 520, h: 690, scale, theme: "dracula", mode: "dark" }); // the height that gives the dialog this box
  await ui.click("[data-details]");
  const box = JSON.parse(await ui.evaluate(`(() => { const r = document.querySelector("dialog.details").getBoundingClientRect(); return JSON.stringify({ x: r.x, y: r.y, h: r.height }); })()`));
  console.log(`details dialog ${box.h} high, clip ${height}`);
  await ui.shot(join(out, "desktop-details.png"), { x: box.x, y: box.y, width: 488, height, scale: 1 });
}
/** 1500 × 800: the main window and the tray popover in the rings view, side by side on a plain backdrop. */
async function hero() {
  const dir = mkdtempSync(join(tmpdir(), "cuotai-hero-"));
  const png = (name) => `data:image/png;base64,${readFileSync(join(dir, name)).toString("base64")}`;
  try {
    await ui.open(MAIN, { w: 1012, h: 676, scale: 2, theme: "dracula", mode: "dark" });
    await ui.shot(join(dir, "main.png"));
    await ui.open("tray.html?update=fresh", { w: 380, h: 600, scale: 2, theme: "dracula", mode: "dark", storage: { "cuotai.trayView": "rings" } });
    await ui.shot(join(dir, "tray.png"));
    await ui.open(MAIN, { w: 1500, h: 800, scale: 1, theme: "dracula", mode: "dark", wait: 100 });
    await ui.evaluate(`(() => {
      document.body.outerHTML = '<body style="margin:0;height:800px;overflow:hidden;background:linear-gradient(180deg,#262838,#171821)">'
        + '<img style="position:absolute;left:50px;top:62px;width:1012px;height:676px;border:1px solid #44475a;border-radius:18px" src="${png("main.png")}">'
        + '<img style="position:absolute;left:1090px;top:100px;width:380px;height:600px;border-radius:18px;box-shadow:0 12px 40px #0008" src="${png("tray.png")}"></body>';
    })()`);
    await new Promise((r) => setTimeout(r, 500));
    await ui.shot(join(out, "desktop-hero.png"));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

try {
  if (wanted("desktop-hero")) await hero();
  if (wanted("desktop-dashboard")) await dashboard("desktop-dashboard", "dark");
  if (wanted("desktop-dashboard-light")) await dashboard("desktop-dashboard-light", "light");
  if (wanted("desktop-details")) await details();
  for (const theme of THEMES) for (const mode of ["dark", "light"]) {
    const name = `theme-${theme}-${mode}`;
    if (!wanted(name)) continue;
    await ui.open(MAIN, { w: 1000, h: 560, scale: 1, theme, mode });
    await ui.shot(join(out, `${name}.png`));
    console.log(`${name}.png`);
  }
} finally {
  ui.close();
}
process.exit(0);
