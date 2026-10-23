// Overflow / truncation / overlap check of the mockups (PRD 0022 i18n validation).
// Usage: node check.mjs [name-prefix ...] > ../i18n-check.json    (spawns python http.server and headless Chrome)
// Rules: text must never be clipped, ellipsized or overlap another text, EXCEPT elements marked allowed (account labels
// and names: .ac, .nm.tr, .cr .nm, .rt .t span, titles with a title attribute and class trunc).
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { here } from "./lib-paths.mjs";

const ROOT = join(here, "../../../../../..");
const PORT = process.env.MOCKUP_PORT ?? "8793", CDP = "9444";
const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const prefixes = process.argv.slice(2);
const screens = readFileSync(join(here, "screens.tsv"), "utf8").trim().split("\n").map((l) => l.split("\t")).filter(([n]) => !prefixes.length || prefixes.some((p) => n.startsWith(p)));

const server = spawn("python3", ["-m", "http.server", PORT, "--bind", "127.0.0.1", "--directory", ROOT], { stdio: "ignore" });
const chrome = spawn(CHROME, ["--headless=new", "--disable-gpu", `--remote-debugging-port=${CDP}`, "--user-data-dir=" + join(process.env.TMPDIR ?? "/tmp", "cuotai-check-profile"), "--hide-scrollbars", "--lang=en-US", "about:blank"], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const cleanup = () => { server.kill(); chrome.kill(); };
process.on("exit", cleanup);

const PAGE_JS = `(() => {
  const out = [];
  const vw = innerWidth;
  const sel = (e) => { const c = typeof e.className === "string" ? e.className.trim().split(/\\s+/).filter(Boolean).slice(0, 3).join(".") : ""; return e.tagName.toLowerCase() + (c ? "." + c : ""); };
  const allowedEl = (e) => e.matches(".ac, .nm.tr, .cr .nm, .rt .t > span, .ch .ac, .ttl.trunc, .ni .trunc") || (e.classList.contains("trunc") && e.hasAttribute("title"));
  const leaves = [];
  for (const e of document.querySelectorAll("body *")) {
    if (e.closest("svg")) continue;
    const cs = getComputedStyle(e);
    if (cs.display === "none" || cs.visibility === "hidden") continue;
    if (e.closest(".lmenu, .wm-name")) continue; // intentional overlay menu; visually hidden accessible name of the logotype
    const text = [...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.replace(/\\s+/g, " ").trim()).join(" ").trim();
    if (!text) continue;
    const ok = allowedEl(e);
    if (e.scrollWidth > e.clientWidth + 1 && (cs.textOverflow === "ellipsis" || cs.overflow !== "visible" || cs.overflowX !== "visible")) out.push({ kind: "truncated", ok, sel: sel(e), text: text.slice(0, 60), sw: e.scrollWidth, cw: e.clientWidth });
    const r = e.getBoundingClientRect();
    if (r.right > vw + 1 || r.left < -1) out.push({ kind: "outside-viewport", ok: false, sel: sel(e), text: text.slice(0, 60), right: Math.round(r.right) });
    // clipped by an ancestor with hidden overflow (horizontal)
    for (let p = e.parentElement; p && p !== document.body; p = p.parentElement) {
      const pcs = getComputedStyle(p);
      if (pcs.overflowX !== "visible" && p.scrollWidth <= p.clientWidth + 1) {
        const pr = p.getBoundingClientRect();
        if (r.right > pr.right + 1 && r.width > 0) { out.push({ kind: "clipped-by-parent", ok, sel: sel(e), text: text.slice(0, 60), parent: sel(p) }); break; }
      }
    }
    const range = document.createRange(); range.selectNodeContents(e);
    const rects = [...range.getClientRects()].filter((x) => x.width > 1 && x.height > 1);
    // ranges of the element's own text nodes only
    const clip = (x) => { // intersect with every ancestor that clips (overflow not visible), incl. the element itself
      let L = x.left, T = x.top, R = x.right, B = x.bottom;
      for (let p = e; p && p !== document.documentElement; p = p.parentElement) {
        const c = getComputedStyle(p);
        if (c.overflowX !== "visible" || c.overflowY !== "visible") {
          const q = p.getBoundingClientRect(); L = Math.max(L, q.left); R = Math.min(R, q.right);
          if (!p.matches(".pbody, .content, .dlg-b, body")) { T = Math.max(T, q.top); B = Math.min(B, q.bottom); } // scroll areas: judge the whole content
        }
      }
      return R - L > 1 && B - T > 1 ? { left: L, top: T, right: R, bottom: B } : null;
    };
    const own = []; for (const n of e.childNodes) if (n.nodeType === 3 && n.textContent.trim()) { const rg = document.createRange(); rg.selectNodeContents(n); for (const x of rg.getClientRects()) { if (x.width <= 1 || x.height <= 1) continue; const c = clip(x); if (c) own.push(c); } }
    if (own.length) leaves.push({ e, own, text, ok });
    const box = e.closest(".kpi, .chip, .btn, .opt, .call, .tc, .seg button, .field, .plan, .cr, .tab");
    if (box && !allowedEl(e)) { const br = box.getBoundingClientRect(); const over = own.find((x) => x.right > br.right + 1 || x.left < br.left - 1); if (over) out.push({ kind: "spills-container", ok: false, sel: sel(e) + " in " + sel(box), text: text.slice(0, 40) }); }
  }
  for (const s of document.querySelectorAll(".rc > .stl[data-fit]")) out.push({ kind: "status-line-yielded", ok: true, sel: sel(s), text: s.dataset.fit.slice(0, 80) });
  for (const s of document.querySelectorAll(".rc > .stl")) if (s.getBoundingClientRect().height > 22) out.push({ kind: "status-line-taller-than-one-line", ok: false, sel: sel(s), text: s.textContent.trim().slice(0, 50) });
  for (const b of document.querySelectorAll("button, .ib, .chip, .btn, .kbd, .sw")) { // controls without text (icon buttons) must stay inside the window too
    if (b.closest(".lmenu")) continue;
    const r = b.getBoundingClientRect();
    if (r.width && (r.right > vw + 1 || r.left < -1)) out.push({ kind: "control-outside-viewport", ok: false, sel: sel(b), text: (b.getAttribute("aria-label") || b.textContent || "").trim().slice(0, 40), right: Math.round(r.right) });
  }
  for (let i = 0; i < leaves.length; i++) for (let j = i + 1; j < leaves.length; j++) {
    const a = leaves[i], b = leaves[j];
    if (a.e.contains(b.e) || b.e.contains(a.e)) continue;
    if (a.e.closest(".pbody, .content, .dlg-b") !== b.e.closest(".pbody, .content, .dlg-b")) continue; // fixed bars vs scrolling body
    let hit = false;
    for (const x of a.own) { for (const y of b.own) { const w = Math.min(x.right, y.right) - Math.max(x.left, y.left), h = Math.min(x.bottom, y.bottom) - Math.max(x.top, y.top); if (w > 2 && h > 3) { hit = true; break; } } if (hit) break; }
    if (hit) out.push({ kind: "overlap", ok: false, sel: sel(a.e) + " x " + sel(b.e), text: a.text.slice(0, 30) + " | " + b.text.slice(0, 30) });
  }
  return out;
})()`;

async function main() {
  let targets;
  for (let i = 0; i < 40; i++) { try { targets = await (await fetch(`http://127.0.0.1:${CDP}/json/list`)).json(); if (targets.some((t) => t.type === "page")) break; } catch {} await sleep(250); }
  const ws = new WebSocket(targets.find((t) => t.type === "page").webSocketDebuggerUrl);
  await new Promise((r) => (ws.onopen = r));
  let id = 0; const pending = new Map(); const waiters = [];
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } else if (d.method) waiters.forEach((w) => w(d)); };
  const send = (method, params = {}) => new Promise((res) => { const i = ++id; pending.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
  const loaded = () => new Promise((res) => { const w = (d) => { if (d.method === "Page.loadEventFired") { waiters.splice(waiters.indexOf(w), 1); res(); } }; waiters.push(w); });
  await send("Page.enable");
  const result = {};
  for (const [name, w, h] of screens) {
    await send("Emulation.setDeviceMetricsOverride", { width: +w, height: +h, deviceScaleFactor: 1, mobile: false });
    const l = loaded();
    await send("Page.navigate", { url: `http://127.0.0.1:${PORT}/docs/plans/assets/0022/after/src/${name}.html` });
    await l; await sleep(350);
    const r = await send("Runtime.evaluate", { expression: PAGE_JS, returnByValue: true });
    result[name] = r.result?.result?.value ?? [{ kind: "error", ok: false, sel: JSON.stringify(r.result) }];
  }
  console.log(JSON.stringify(result, null, 1));
}
try { await main(); } finally { cleanup(); }
process.exit(0);
