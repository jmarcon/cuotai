// Generates the static mockup HTML files in this directory plus screens.tsv (name, width, height).
// English is the unsuffixed set; pt-BR, es and it are rendered for the key screens (I18N_SET) with the suffix ".<loc>".
// Usage: node build.mjs [moduleName ...]   (default: every module)
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { here } from "./lib.mjs";
import "./strings.mjs";
import { LOCS, setLoc, usage } from "./i18n.mjs";

export const I18N_SET = ["01-tray-overview", "03-tray-account-claude-expanded", "05-tray-detail-cursor-extra", "10-main-overview", "15-main-narrow", "31-account-add-catalog", "50-settings-general", "50-settings-general-language", "50-settings-menubar", "50-settings-alerts", "70-stats"];
const mods = process.argv.slice(2);
const all = ["tray", "main", "dialogs", "settings", "stats"];
const list = [];
for (const loc of LOCS) {
  setLoc(loc);
  for (const m of mods.length ? mods : all) {
    const mod = await import(`./${m}.mjs`);
    for (const s of mod.screens()) {
      if (loc !== "en-US" && !I18N_SET.includes(s.name)) continue;
      const name = loc === "en-US" ? s.name : `${s.name}.${loc}`;
      writeFileSync(join(here, `${name}.html`), s.html);
      list.push(`${name}\t${s.w}\t${s.h}`);
    }
  }
}
// Cuotai pastel theme variants (dark and light) of the two overview screens: same HTML, other theme link and tokens
const THEMED = ["01-tray-overview", "10-main-overview"];
if (!mods.length) for (const base of THEMED) for (const mode of ["dark", "light"]) {
  const src = readFileSync(join(here, `${base}.html`), "utf8");
  const html = src.replace(/data-theme="dracula" data-mode="dark"/, `data-theme="cuotai" data-mode="${mode}"`).replace('<link rel="stylesheet" href="common.css" />', '<link rel="stylesheet" href="themes/cuotai.css" />\n<link rel="stylesheet" href="common.css" />');
  if (html === src) throw new Error("theme variant did not apply to " + base);
  const name = `${base}.cuotai${mode === "light" ? "-light" : ""}`;
  writeFileSync(join(here, `${name}.html`), html);
  list.push(`${name}\t${base.startsWith("01") ? "380\t600" : "1200\t820"}`);
}
if (!mods.length) writeFileSync(join(here, "screens.tsv"), list.join("\n") + "\n");
// which wording came from where, for AFTER.md
const rep = {};
for (const [loc, m] of usage) { rep[loc] = { catalog: [], core: [], rust: [], draft: [] }; for (const [id, v] of m) rep[loc][v.src].push(id + (v.key ? `=${v.key}` : "")); }
writeFileSync(join(here, "i18n-usage.json"), JSON.stringify(rep, null, 1));
console.log(list.length + " screens");
