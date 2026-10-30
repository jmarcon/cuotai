#!/usr/bin/env node
// PRD 0022 RF14.2 (DS-MSG-14..16): the overflow check of the mockups, run against the REAL UI in the browser demo at
// the minimum sizes (tray 380×600, main 520 and 1200 wide, settings 640×460) in en-US, pt-BR, es, it and the en-XA
// pseudo-locale. It reports (a) text wider than its box with ellipsis or hidden overflow, (b) text or controls
// outside the window, (c) text clipped by an ancestor, (d) text spilling out of a chip, button, tab or card, (e)
// overlapping text, and (f) a status line taller than one line. Elements that may truncate by design (account labels
// and names, the status label after its fitting, one-line notices) are reported as allowed.
// Usage: node apps/desktop/tools/ui-shots/check.mjs [--json] [--selftest] [--zoom] [locale ...]   Exit: 0 clean, 1 with problems.
// `--selftest` forces a long string into a figure and a tab of the tray and passes only when the check catches it.
import { launch } from "./driver.mjs";

const args = process.argv.slice(2);
const json = args.includes("--json");
const selftest = args.includes("--selftest");
// `--zoom` (PRD 0020 RNF): the 200 % zoom pass. At 200 % a 1280×820 screen offers a 640×410 CSS viewport, so the main
// window and Settings are checked there (the tray is a fixed 380 px popover whose content does not reflow).
const zoom = args.includes("--zoom");
const locales = args.filter((a) => !a.startsWith("--"));
const LOCALES = locales.length ? locales : selftest ? ["en-US"] : ["en-US", "pt-BR", "es", "it", "de", "ru", "ko", "hi-IN", "id-ID", "tr", "vi", "zh-CN", "fr", "ja", "en-XA"];
const ZOOM_PAGES = [
  ["zoom200-main", "index.html", 640, 410, {}],
  ["zoom200-main-stats", "index.html", 640, 410, { "cuotai.openView": "stats" }],
  ["zoom200-settings-general", "settings.html?page=general", 640, 360, {}],
  ["zoom200-settings-dashboard", "settings.html?page=dashboard", 640, 360, {}],
  ["zoom200-settings-alerts", "settings.html?page=alerts", 640, 360, {}],
  ["zoom200-settings-menubar", "settings.html?page=menubar", 640, 360, {}],
];
const PAGES = zoom ? ZOOM_PAGES : [
  ["tray", "tray.html", 380, 600, {}],
  ["tray-rings", "tray.html", 380, 600, { "cuotai.trayView": "rings" }],
  ["tray-compact", "tray.html", 380, 600, { "cuotai.trayView": "compact" }],
  ["main-narrow", "index.html", 520, 820, {}],
  ["main", "index.html", 1200, 820, {}],
  ["settings-general", "settings.html?page=general", 640, 460, {}],
  ["settings-dashboard", "settings.html?page=dashboard", 640, 460, {}],
  ["settings-alerts", "settings.html?page=alerts", 640, 460, {}],
  ["settings-menubar", "settings.html?page=menubar", 640, 460, {}],
  ["main-stats", "index.html", 1200, 820, { "cuotai.openView": "stats" }],
  ["tray-stats", "tray.html", 380, 600, {}, "[data-open-stats]"],
  ["details-short", "index.html", 1200, 500, {}, "[data-details]"],
  ["details-narrow", "index.html", 520, 600, {}, "[data-details]"],
];
// DS-MSG-15: what may truncate (with its full text in a tooltip or the accessible name).
export const ALLOWED = [".r1 .nm", ".r1 .ac", ".crow .nm", ".tile-name b", ".tile-st span", ".who h2", ".who p", ".stl.truncated .lbl",
  ".xline span", ".pop-notice span", ".sum-top .lt", ".set-path", ".lc-title", ".models-meta", ".access-line", ".win .reset", ".pop-head h1",
  ".set-nav-item span", ".sum-note", ".tl-name", ".st-name", ".set-row-desc", "option", "select", ".fig.dual .wl.model"].join(", ");

const PAGE_JS = `(() => {
  const allowed = ${JSON.stringify(ALLOWED)};
  const out = [];
  const vw = innerWidth;
  const sel = (e) => { const c = typeof e.className === "string" ? e.className.trim().split(/\\s+/).filter(Boolean).slice(0, 3).join(".") : ""; return e.tagName.toLowerCase() + (c ? "." + c : ""); };
  const ok = (e) => !!e.closest(allowed);
  const leaves = [];
  // A modal dialog covers the page: only its own text can collide.
  const modal = document.querySelector("dialog.details[open]");
  for (const e of document.querySelectorAll("body *")) {
    if (e.closest("svg, .sr-only, .wm-name, [hidden], dialog:not([open]), .set-pop, .menu, .lang-menu")) continue;
    if (modal && !modal.contains(e)) continue;
    // the hidden part of a closed disclosure (its own text and every child but the summary)
    if (e.matches("details:not([open])") || (e.closest("details:not([open])") && !e.closest("summary"))) continue;
    const cs = getComputedStyle(e);
    if (cs.display === "none" || cs.visibility === "hidden") continue;
    if (cs.position === "absolute" && cs.clip.replace(/[^0-9]/g, "") === "0000") continue; // visually hidden
    const text = [...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.replace(/\\s+/g, " ").trim()).join(" ").trim();
    if (!text) continue;
    const r = e.getBoundingClientRect();
    if (!r.width) continue;
    if (e.scrollWidth > e.clientWidth + 1 && (cs.textOverflow === "ellipsis" || cs.overflowX !== "visible")) out.push({ kind: "truncated", ok: ok(e), sel: sel(e), text: text.slice(0, 60) });
    // The visible part of a box: intersected with every ancestor (and itself) that clips its overflow.
    let clipper = null;
    const clip = (x) => {
      let L = x.left, T = x.top, R = x.right, B = x.bottom;
      for (let p = e; p && p !== document.documentElement; p = p.parentElement) {
        const c = getComputedStyle(p);
        if (c.overflowX !== "visible" || c.overflowY !== "visible") {
          const q = p.getBoundingClientRect();
          if (R > q.right + 1 && !clipper) clipper = p;
          L = Math.max(L, q.left); R = Math.min(R, q.right);
          if (!p.matches(".pop-body, .set-content, body")) { T = Math.max(T, q.top); B = Math.min(B, q.bottom); } // scroll areas: all their content counts
        }
      }
      return R - L > 1 && B - T > 1 ? { left: L, top: T, right: R, bottom: B } : null;
    };
    const own = [];
    for (const n of e.childNodes) if (n.nodeType === 3 && n.textContent.trim()) { const rg = document.createRange(); rg.selectNodeContents(n); for (const x of rg.getClientRects()) { if (x.width <= 1 || x.height <= 1) continue; const c = clip(x); if (c) own.push(c); } }
    const seen = clip(r);
    if (seen && (seen.right > vw + 1 || seen.left < -1)) out.push({ kind: "outside-viewport", ok: false, sel: sel(e), text: text.slice(0, 60) });
    if (clipper && clipper !== e) out.push({ kind: "clipped-by-parent", ok: ok(e) || ok(clipper), sel: sel(e), text: text.slice(0, 60), parent: sel(clipper) });
    if (own.length) leaves.push({ e, own, text, ok: ok(e) });
    const box = e.closest(".schip, .mini, .btn, .tab, .opt-card, .seg [role=radio], .fig, .kbd, kbd");
    if (box && box !== e && !ok(e)) { const br = box.getBoundingClientRect(); if (own.some((x) => x.right > br.right + 1 || x.left < br.left - 1)) out.push({ kind: "spills-container", ok: false, sel: sel(e) + " in " + sel(box), text: text.slice(0, 40) }); }
  }
  for (const s of document.querySelectorAll(".rc > .stl")) if (s.getBoundingClientRect().height > 22) out.push({ kind: "status-line-taller-than-one-line", ok: false, sel: sel(s), text: s.textContent.trim().slice(0, 50) });
  for (const b of document.querySelectorAll("button, input, select")) {
    if (b.closest("[hidden], dialog:not([open]), .menu, .lang-menu, .set-pop")) continue;
    const r = b.getBoundingClientRect();
    if (r.width && (r.right > vw + 1 || r.left < -1)) out.push({ kind: "control-outside-viewport", ok: false, sel: sel(b), text: (b.getAttribute("aria-label") || b.textContent || "").trim().slice(0, 40) });
  }
  const scroller = (e) => e.closest(".pop-body, .set-content, .grid, main");
  for (let i = 0; i < leaves.length; i++) for (let j = i + 1; j < leaves.length; j++) {
    const a = leaves[i], b = leaves[j];
    if (a.e.contains(b.e) || b.e.contains(a.e) || scroller(a.e) !== scroller(b.e)) continue;
    const hit = a.own.some((x) => b.own.some((y) => Math.min(x.right, y.right) - Math.max(x.left, y.left) > 2 && Math.min(x.bottom, y.bottom) - Math.max(x.top, y.top) > 3));
    if (hit) out.push({ kind: "overlap", ok: false, sel: sel(a.e) + " x " + sel(b.e), text: a.text.slice(0, 30) + " | " + b.text.slice(0, 30) });
  }
  // The details dialog scrolls in its body only, never around it and never sideways.
  if (modal && (modal.scrollHeight > modal.clientHeight + 1 || modal.scrollWidth > modal.clientWidth + 1)) out.push({ kind: "nested-scroll", ok: false, sel: "dialog.details", text: modal.scrollWidth + "x" + modal.scrollHeight + " > " + modal.clientWidth + "x" + modal.clientHeight });
  if (document.documentElement.scrollWidth > vw + 1) out.push({ kind: "horizontal-scroll", ok: false, sel: "html", text: document.documentElement.scrollWidth + " > " + vw });
  return out;
})()`;

const ui = await launch();
const report = {};
let problems = 0;
try {
  for (const locale of LOCALES) {
    for (const [name, page, w, h, storage, click] of PAGES) {
      const target = locale === "en-XA" ? `${page}${page.includes("?") ? "&" : "?"}pseudo` : page;
      await ui.open(target, { w, h, scale: 1, locale: locale === "en-XA" ? "en-US" : locale, storage });
      if (click) await ui.click(click);
      if (selftest) {
        if (!page.startsWith("tray") || storage["cuotai.trayView"]) continue;
        await ui.evaluate(`(() => { const f = document.querySelector(".fig b"); if (f) f.textContent = "W".repeat(60); })()`);
      }
      const found = (await ui.evaluate(PAGE_JS)) ?? [{ kind: "error", ok: false, sel: "page", text: "evaluation failed" }];
      const bad = found.filter((f) => !f.ok);
      problems += bad.length;
      report[`${name}.${locale}`] = found;
      if (!json) console.log(`${bad.length ? "FAIL" : "ok  "} ${name} ${locale} (${w}×${h}): ${bad.length} problem(s), ${found.length - bad.length} allowed`);
      if (!json) for (const f of bad) console.log(`       ${f.kind}: ${f.sel} "${f.text}"`);
    }
  }
} finally {
  ui.close();
}
if (json) console.log(JSON.stringify(report, null, 1));
console.error(`ui-shots check: ${problems} problem(s)`);
if (selftest) {
  console.error(problems ? "selftest: the forced long string was caught" : "selftest FAILED: the forced long string went unnoticed");
  process.exit(problems ? 0 : 1);
}
process.exit(problems ? 1 : 0);
