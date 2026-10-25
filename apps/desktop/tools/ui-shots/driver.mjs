// PRD 0022 RF18.2: a dev-only driver for the desktop UI's browser demo. It serves the repository root on 127.0.0.1
// (Python 3's static server), starts headless Chrome with a fresh profile and talks to it over the DevTools
// protocol: window size, scale, color scheme, reduced motion, the demo's language and theme, clicks and an
// expression to evaluate. No network beyond 127.0.0.1, no credential, no app file is edited: the browser demo
// answers every command with its own fixtures (`demo` in apps/desktop/ui/js/lib.js, example.com identities).
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = resolve(fileURLToPath(new URL("../../../../", import.meta.url)));
const CHROMES = {
  darwin: ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium"],
  linux: ["google-chrome", "chromium", "chromium-browser"],
  win32: ["C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"],
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** A port the OS reports free right now, so parallel runs never share a server (UI_SHOTS_PORT pins one). */
const freePort = () => new Promise((res, rej) => {
  const s = createServer();
  s.once("error", rej);
  s.listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => res(port)); });
});

/** Starts the server and the browser; returns `{ open, shot, evaluate, click, close }`. */
export async function launch({ port, cdp } = {}) {
  port ??= process.env.UI_SHOTS_PORT ? Number(process.env.UI_SHOTS_PORT) : await freePort();
  // Chrome picks its own DevTools port (0) and writes it to DevToolsActivePort in the fresh profile.
  cdp ??= process.env.UI_SHOTS_CDP ? Number(process.env.UI_SHOTS_CDP) : 0;
  const python = process.platform === "win32" ? "python" : "python3";
  const server = spawn(python, ["-m", "http.server", String(port), "--bind", "127.0.0.1", "--directory", ROOT], { stdio: "ignore" });
  const profile = mkdtempSync(join(tmpdir(), "cuotai-ui-shots-"));
  const chromePath = process.env.CHROME ?? (CHROMES[process.platform] ?? [])[0];
  const chrome = spawn(chromePath, ["--headless=new", "--disable-gpu", "--hide-scrollbars", `--remote-debugging-port=${cdp}`,
    `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "about:blank"], { stdio: "ignore" });
  const stop = () => { server.kill(); chrome.kill(); try { rmSync(profile, { recursive: true, force: true }); } catch {} };
  process.on("exit", stop);

  for (let i = 0; i < 80 && !cdp; i++) {
    const file = join(profile, "DevToolsActivePort");
    if (existsSync(file)) cdp = Number(readFileSync(file, "utf8").split("\n")[0]) || 0;
    if (!cdp) await sleep(100);
  }
  let targets = [];
  for (let i = 0; i < 80 && !targets.some((t) => t.type === "page"); i++) {
    try { targets = await (await fetch(`http://127.0.0.1:${cdp}/json/list`)).json(); } catch {}
    if (!targets.some((t) => t.type === "page")) await sleep(250);
  }
  const page = targets.find((t) => t.type === "page");
  if (!page) { stop(); throw new Error("headless Chrome did not start (set CHROME to its path)"); }
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let id = 0;
  const pending = new Map(), waiters = new Set();
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data);
    if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } else if (d.method) waiters.forEach((w) => w(d));
  };
  const send = (method, params = {}) => new Promise((res) => { const i = ++id; pending.set(i, res); ws.send(JSON.stringify({ id: i, method, params })); });
  const loaded = () => new Promise((res) => { const w = (d) => { if (d.method === "Page.loadEventFired") { waiters.delete(w); res(); } }; waiters.add(w); });
  await send("Page.enable");
  for (let i = 0; i < 40; i++) { try { await fetch(`http://127.0.0.1:${port}/`); break; } catch { await sleep(150); } }
  const base = `http://127.0.0.1:${port}/apps/desktop/ui/`;
  const evaluate = async (expression) => (await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true })).result?.result?.value;

  /** Opens `page` (tray.html, index.html, settings.html?page=…) at `w`×`h` with the demo's storage preset. */
  async function open(target, { w = 380, h = 600, scale = 2, theme = "dracula", mode = "dark", locale = "en-US", storage = {}, motion = "reduce", wait = 900 } = {}) {
    await send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: scale, mobile: false });
    await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: mode === "light" ? "light" : "dark" },
      { name: "prefers-reduced-motion", value: motion }] });
    let l = loaded();
    await send("Page.navigate", { url: `${base}locales/en-US.json` }); // same origin, to preset the storage
    await l;
    const preset = { "cuotai.theme": theme, "cuotai.themeMode": mode, "cuotai.locale": locale, ...storage };
    await evaluate(`localStorage.clear(); ${Object.entries(preset).map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(String(v))});`).join(" ")}`);
    l = loaded();
    await send("Page.navigate", { url: base + target });
    await l;
    await sleep(wait);
  }
  /** Clicks the first element matching each selector, in order. */
  async function click(...selectors) {
    for (const sel of selectors) {
      await evaluate(`document.querySelector(${JSON.stringify(sel)})?.click()`);
      await sleep(400);
    }
  }
  /** Writes a PNG of the viewport. */
  async function shot(file) {
    const r = await send("Page.captureScreenshot", { format: "png" });
    const { writeFileSync } = await import("node:fs");
    writeFileSync(file, Buffer.from(r.result.data, "base64"));
  }
  return { open, click, shot, evaluate, close: () => { ws.close(); stop(); } };
}
