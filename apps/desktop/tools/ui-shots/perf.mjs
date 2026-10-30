#!/usr/bin/env node
// PRD 0020 RNF (performance) and PRD 0022 (render budget): two probes against the real UI in the browser demo.
//   * Settings open time: from the start of the navigation to the first Settings page rendered (sidebar and the
//     first row present), budget 300 ms (PRD 0020 RNF: "opens in under 300 ms on the reference machine").
//   * Render time: the main-thread work of an interaction (expanding an account, opening Stats): the click handler,
//     its render and the forced style and layout of the new DOM, budget 16 ms (it must fit in one frame at 60 Hz).
//     The time to the next presented frame is printed beside it; it adds the wait for the display's vsync (0 to
//     16.7 ms, whatever the work), so it cannot be held to a one-frame budget and stays informational.
// A browser demo on a CI runner is slower and noisier than the reference machine, so each probe takes the median of
// several runs and the script is advisory by default: it prints the numbers and exits 0 unless `--strict` is given.
// The render probes are held to their budget on a sample that noise cannot decide. On a runner the same code ranges
// from 5 to 30 ms, in bursts, so five runs went over a 16 ms budget with a true median of 12 ms: each probe discards
// one printed warm-up run (the first click of a page's code, cold, took 20 ms there), takes many more runs, and a
// probe over budget is measured once more; the median of both batches decides, so a real regression still fails.
// Usage: node apps/desktop/tools/ui-shots/perf.mjs [--json] [--strict] [--runs N] [--open-budget MS] [--frame-budget MS]
import { launch } from "./driver.mjs";

const args = process.argv.slice(2);
const flag = (name, dflt) => { const i = args.indexOf(name); return i >= 0 ? Number(args[i + 1]) : dflt; };
const json = args.includes("--json");
const strict = args.includes("--strict");
const RUNS = flag("--runs", 15);
const OPEN_BUDGET = flag("--open-budget", 300);
const FRAME_BUDGET = flag("--frame-budget", 16);
const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

const ui = await launch();
const results = [];
try {
  // Settings open time: poll inside the page (performance.now() counts from the start of the navigation).
  const openTimes = [];
  for (let i = 0; i < RUNS; i++) {
    await ui.open("settings.html?page=general", { w: 760, h: 560, scale: 1, wait: 0 });
    const t = await ui.evaluate(`new Promise((res) => {
      const done = () => document.querySelector(".set-nav-item") && document.querySelector(".set-row") ? res(performance.now()) : requestAnimationFrame(done);
      done();
    })`);
    if (typeof t === "number") openTimes.push(t);
  }
  results.push({ probe: "settings-open", unit: "ms", budget: OPEN_BUDGET, runs: openTimes.map((t) => Math.round(t)), median: openTimes.length ? Math.round(median(openTimes)) : null });

  // Render time: the click, its render and a forced style + layout (the work), then the next presented frame (two
  // rAFs: the first runs before paint, the second after it; informational, it includes the vsync wait).
  const frame = async (name, page, w, h, selector) => {
    const times = [], frames = [];
    const sample = async (runs) => {
      for (let i = 0; i < runs; i++) {
        await ui.open(page, { w, h, scale: 1, motion: "reduce" });
        const t = await ui.evaluate(`new Promise((res) => {
          const el = document.querySelector(${JSON.stringify(selector)});
          if (!el) return res(null);
          const t0 = performance.now();
          el.click();
          document.body.getBoundingClientRect();
          const work = performance.now() - t0;
          requestAnimationFrame(() => requestAnimationFrame(() => res([work, performance.now() - t0])));
        })`);
        if (Array.isArray(t)) { times.push(t[0]); frames.push(t[1]); }
      }
    };
    await sample(1);
    const warmup = times.shift();
    frames.shift();
    await sample(RUNS);
    const remeasured = times.length > 0 && median(times) > FRAME_BUDGET;
    if (remeasured) await sample(RUNS);
    const r1 = (x) => Math.round(x * 10) / 10;
    results.push({ probe: name, unit: "ms", budget: FRAME_BUDGET, warmup: warmup == null ? null : r1(warmup), remeasured, runs: times.map(r1), median: times.length ? r1(median(times)) : null, frame_median: frames.length ? r1(median(frames)) : null });
  };
  await frame("tray-expand-account", "tray.html", 380, 600, "[data-open]");
  await frame("tray-open-stats", "tray.html", 380, 600, "[data-open-stats]");
} finally {
  ui.close();
}
let over = 0;
for (const r of results) {
  const bad = r.median == null || r.median > r.budget;
  if (bad) over++;
  if (!json) console.log(`${bad ? "OVER" : "ok  "} ${r.probe}: median ${r.median} ${r.unit} (budget ${r.budget}; runs ${r.runs.join(", ")})${r.frame_median != null ? `; next frame ${r.frame_median} ms` : ""}${r.warmup != null ? `; warm-up ${r.warmup} ms not counted` : ""}${r.remeasured ? "; over budget at first, measured again" : ""}`);
}
if (json) console.log(JSON.stringify(results, null, 1));
console.error(`ui-shots perf: ${over} probe(s) over budget${strict ? "" : " (advisory)"}`);
process.exit(strict && over ? 1 : 0);
