#!/usr/bin/env node
// PRD 0050 RF1.4 (SEC-WEBVIEW): the three production pages under the REAL Content Security Policy. A static server on
// 127.0.0.1 serves the repository root and sends the policy of apps/desktop/src-tauri/tauri.conf.json as the
// `Content-Security-Policy` header of every HTML page, the way the Tauri webview enforces it; headless Chrome loads
// main, tray and Settings (the browser demo answers every command with its fixtures) and every
// `securitypolicyviolation` is a failure. Then a user theme that asks for `https://example.com/pixel` is inserted the
// way usercontent.js inserts one, and the policy must block that fetch (img-src 'self').
// Usage: node apps/desktop/tools/ui-shots/csp-smoke.mjs [--json]   Exit: 0 clean, 1 on a violation or a missed block.
// security-invariant: SEC-WEBVIEW
import { createReadStream, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, sep } from "node:path";
import { launch, ROOT } from "./driver.mjs";

const json = process.argv.includes("--json");
const conf = JSON.parse(readFileSync(join(ROOT, "apps/desktop/src-tauri/tauri.conf.json"), "utf8"));
const CSP = conf?.app?.security?.csp;
if (typeof CSP !== "string" || !CSP.includes("script-src")) {
  console.error("csp-smoke: tauri.conf.json has no string app.security.csp");
  process.exit(1);
}

const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8", ".json": "application/json; charset=utf-8", ".svg": "image/svg+xml", ".png": "image/png",
  ".woff2": "font/woff2", ".ico": "image/x-icon" };

/** The repository root on 127.0.0.1, with the app's CSP on every HTML answer. */
function serve() {
  const server = createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, "http://127.0.0.1").pathname);
    const file = normalize(join(ROOT, path));
    if (!file.startsWith(ROOT.endsWith(sep) ? ROOT : ROOT + sep)) { res.writeHead(403).end(); return; }
    let stat;
    try { stat = statSync(file); } catch { res.writeHead(404).end(); return; }
    if (!stat.isFile()) { res.writeHead(404).end(); return; }
    const type = TYPES[extname(file)] ?? "application/octet-stream";
    const headers = { "Content-Type": type, "Cache-Control": "no-store" };
    if (type.startsWith("text/html")) headers["Content-Security-Policy"] = CSP;
    res.writeHead(200, headers);
    createReadStream(file).pipe(res);
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok(server)));
}

// Installed by the DevTools protocol before any page script, so even a violation during the first parse is recorded.
const RECORDER = `(() => {
  window.__cspViolations = [];
  document.addEventListener("securitypolicyviolation", (e) => {
    window.__cspViolations.push({ directive: e.effectiveDirective || e.violatedDirective, blocked: e.blockedURI, sample: (e.sample || "").slice(0, 80), source: e.sourceFile });
  });
})();`;

const PAGES = [
  ["main", "index.html", 1200, 820],
  ["tray", "tray.html", 380, 600],
  ["settings", "settings.html?page=general", 640, 460],
];
const PIXEL = "https://example.com/pixel";

const server = await serve();
const port = server.address().port;
const ui = await launch({ port, serve: false });
await ui.send("Page.addScriptToEvaluateOnNewDocument", { source: RECORDER });
const problems = [];
const report = [];
try {
  for (const [name, target, w, h] of PAGES) {
    await ui.open(target, { w, h, scale: 1 });
    const state = await ui.evaluate(`({ violations: window.__cspViolations ?? null, rendered: document.body ? document.body.querySelectorAll("*").length : 0 })`);
    if (!state || state.violations === null) problems.push(`${name}: the violation recorder did not run`);
    else for (const v of state.violations) problems.push(`${name}: CSP violation ${v.directive} blocked ${v.blocked || "(inline)"} ${v.sample}`.trim());
    if (!state || state.rendered < 10) problems.push(`${name}: the page did not render (${state?.rendered ?? 0} elements)`);
    report.push({ page: name, violations: state?.violations ?? null, elements: state?.rendered ?? 0 });
  }
  // A user theme that tries to fetch a remote pixel, inserted as usercontent.js does (a <style> element).
  await ui.open("index.html", { w: 1200, h: 820, scale: 1 });
  const blocked = await ui.evaluate(`(async () => {
    const style = document.createElement("style");
    style.dataset.userTheme = "csp-smoke";
    style.textContent = ':root[data-theme="csp-smoke"] { --bg: #000; } body { background-image: url("${PIXEL}") !important; }';
    document.head.appendChild(style);
    const probe = document.createElement("div");
    probe.style.cssText = 'width:4px;height:4px;background-image:url("${PIXEL}")';
    document.body.appendChild(probe);
    getComputedStyle(probe).backgroundImage;
    for (let i = 0; i < 40 && !window.__cspViolations.some((v) => v.blocked.startsWith("${PIXEL}")); i++) await new Promise((r) => setTimeout(r, 50));
    return window.__cspViolations;
  })()`);
  const pixel = (blocked ?? []).filter((v) => v.blocked.startsWith(PIXEL));
  if (!pixel.length) problems.push(`theme: ${PIXEL} was not blocked by the policy`);
  else if (!pixel.every((v) => v.directive === "img-src")) problems.push(`theme: ${PIXEL} blocked by ${pixel.map((v) => v.directive)} instead of img-src`);
  const loaded = await ui.evaluate(`performance.getEntriesByType("resource").some((e) => e.name.startsWith("${PIXEL}") && e.responseEnd > 0 && e.transferSize > 0)`);
  if (loaded) problems.push(`theme: ${PIXEL} was fetched`);
  report.push({ page: "theme-pixel", blocked: pixel.length > 0 });
} finally {
  ui.close();
  server.close();
}

if (json) console.log(JSON.stringify({ csp: CSP, report, problems }, null, 2));
else {
  for (const r of report) console.log(`${r.page}: ${"violations" in r ? `${r.violations?.length ?? "?"} violation(s), ${r.elements} elements` : r.blocked ? "remote pixel blocked" : "NOT blocked"}`);
  for (const p of problems) console.log(`  - ${p}`);
  console.log(problems.length ? `csp-smoke: ${problems.length} problem(s)` : "csp-smoke: clean");
}
process.exit(problems.length ? 1 : 0);
