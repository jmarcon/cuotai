// PRD 0006 RF5.2: the recorded history in the account details. Per metric with samples in the span, a compact area
// chart (24 h, 7 days, 30 days) with the last point highlighted and a marker at each reset, all placed by the core
// (`account_history`): this module only draws. The chart is hidden from assistive technology, which reads the
// equivalent table (window by window: start, end, initial, final and peak use) instead.
import { currentLocale, t, te } from "./i18n.js";
import { STALE, accountHistory, errText, esc, fmtValue, getView, reportKey } from "./lib.js";

const RANGES = ["day", "week", "month"];
// viewBox of the chart; the SVG stretches to the width, so only the paths live in it (markers are HTML).
const W = 300, H = 60;
let uid = 0;

const when = (iso) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleString(currentLocale(), { dateStyle: "short", timeStyle: "short" });
};
const pct = (fraction) => `${(fraction * 100).toFixed(2)}%`;
const n = (v) => v.toFixed(2);

/** One closed area and one line per run of measured buckets: a gap (no sample) breaks both, never reads zero. */
function chartPaths(heights) {
  const count = heights.length;
  let area = "", line = "";
  for (let i = 0; i < count; i++) {
    if (heights[i] == null) continue;
    let j = i;
    while (j + 1 < count && heights[j + 1] != null) j++;
    // Each bucket at its centre; a run reaches the outer edges of its first and last buckets, so a lone bucket
    // stays visible as a short plateau.
    const pts = [[(i / count) * W, (1 - heights[i]) * H]];
    for (let k = i; k <= j; k++) pts.push([((k + 0.5) / count) * W, (1 - heights[k]) * H]);
    pts.push([((j + 1) / count) * W, (1 - heights[j]) * H]);
    const d = pts.map(([x, y]) => `${n(x)},${n(y)}`).join("L");
    line += `M${d}`;
    area += `M${n(pts[0][0])},${H}L${d}L${n(pts[pts.length - 1][0])},${H}Z`;
    i = j;
  }
  return { area, line };
}

const summary = (m) => m.trend
  ? t("trend.summary", { min: fmtValue(m.trend.min, m.unit), max: fmtValue(m.trend.max, m.unit), last: fmtValue(m.trend.last.used, m.unit) })
  : t("trend.noUsed");

function tableHtml(m) {
  const head = ["start", "end", "initial", "final", "peak", "peakAt"].map((c) => `<th scope="col">${te(`trend.table.${c}`)}</th>`).join("");
  const rows = m.windows.map((w) => `<tr><td>${esc(when(w.started_at))}</td><td>${esc(when(w.ended_at))}</td>
    <td>${esc(fmtValue(w.initial_used, m.unit))}</td><td>${esc(fmtValue(w.final_used, m.unit))}</td>
    <td>${esc(fmtValue(w.peak_used, m.unit))}</td><td>${esc(when(w.peak_at))}</td></tr>`).join("");
  return `<table class="sr-only"><caption>${esc(m.label)}: ${esc(summary(m))} ${te("trend.table.resets", { count: m.chart.resets.length })}</caption>
    <thead><tr>${head}</tr></thead><tbody>${rows}</tbody></table>`;
}

function metricHtml(m, view) {
  const { area, line } = chartPaths(m.chart.heights);
  const end = m.chart.last;
  return `<figure class="hist-metric">
    <figcaption><span class="hist-label">${esc(m.label)}</span><span class="sub">${esc(summary(m))}</span></figcaption>
    <div class="hist-chart" aria-hidden="true">
      <svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" focusable="false">${m.unit === "%" ? `<path class="hist-grid" d="M0,0H${W}M0,${H / 2}H${W}"/>` : ""}<path class="hist-area" d="${area}"/><path class="hist-line" d="${line}"/></svg>
      ${m.chart.resets.map((r) => `<i class="hist-reset" style="left:${pct(r.x)}" title="${te("trend.reset", { when: when(r.at) })}"></i>`).join("")}
      ${end ? `<i class="hist-end" style="left:${pct(end.x)};top:${pct(1 - end.y)}"></i>` : ""}
    </div>
    <div class="hist-axis sub" aria-hidden="true"><span>${esc(when(view.from))}</span>
      ${m.chart.resets.length ? `<span class="hist-key"><i></i>${te("trend.resetKey")}</span>` : ""}<span>${esc(when(view.to))}</span></div>
    ${tableHtml(m)}
  </figure>`;
}

/** History section of account `key` (`provider::name`) at `index` of configuration `generation`. Read from the core
 *  on mount, on a span change and whenever `stamp` (the reading on screen, an erasure, the recording switch) moves;
 *  an answer for another identity (old generation/index) is discarded, as the model catalog does. The host calls
 *  `update` on every view, `focused()` before redrawing and `attach(root, focus)` after. */
export function mountHistory({ key, index, generation, stamp }) {
  const id = `trend-${++uid}`;
  const el = document.createElement("section");
  el.className = "history";
  el.setAttribute("aria-labelledby", id);
  el.innerHTML = `<div class="history-head"><h3 id="${id}">${te("trend.title")}</h3>
      <div class="history-ranges" role="group" aria-label="${te("trend.range.label")}">
        ${RANGES.map((r) => `<button type="button" data-range="${r}">${te(`trend.range.${r}`)}</button>`).join("")}
      </div></div>
    <p class="history-status sub" role="status" aria-live="polite"></p>
    <div class="history-body"></div>`;
  const status = el.querySelector(".history-status");
  const body = el.querySelector(".history-body");
  let range = "day", view = null, error = null, loading = false, epoch = 0;

  /** The status line: loading, the failure, the explained empty state, or that recording is off. */
  function showStatus() {
    for (const b of el.querySelectorAll("[data-range]")) b.setAttribute("aria-pressed", String(b.dataset.range === range));
    const off = view && !view.recording ? t("trend.off") : "";
    status.textContent = loading && !view ? t("trend.loading")
      : error ?? (view && !view.metrics.length ? `${t(`trend.empty.${range}`)} ${off || t("trend.emptyHelp")}` : off);
  }
  /** Draws the answer at once: charts have no entrance (PRD 0022 RF11.4, DS-CHT-2). */
  function paint() {
    showStatus();
    body.innerHTML = error || !view ? "" : view.metrics.map((m) => metricHtml(m, view)).join("");
  }

  async function load() {
    const e = ++epoch;
    loading = true;
    showStatus();
    try {
      const v = await accountHistory(index, generation, range);
      if (e !== epoch) return;
      view = v; error = null;
    } catch (x) {
      if (e !== epoch) return;
      loading = false;
      if (x === STALE) return resync();
      view = null; error = errText(x);
    }
    loading = false;
    paint();
  }
  /** The configuration changed before the UI saw the new view: find the same account again. */
  async function resync() {
    const v = await getView().catch(() => null);
    const r = v?.reports.find((x) => reportKey(x) === key);
    if (r) update(r.index, v.generation, stamp);
  }
  function update(i, g, s) {
    if (i === index && g === generation && s === stamp) return;
    if (i !== index || g !== generation) { view = null; paint(); } // another identity: never show its old answer
    index = i; generation = g; stamp = s;
    load();
  }

  el.querySelector(".history-ranges").addEventListener("click", (e) => {
    const b = e.target.closest("[data-range]");
    if (!b || b.dataset.range === range) return;
    range = b.dataset.range;
    view = null;
    paint();
    load();
  });
  load();
  return {
    key, el, update,
    focused: () => (el.contains(document.activeElement) ? document.activeElement : null),
    attach(root, focus) { root.querySelector("[data-history-slot]")?.append(el); focus?.focus(); },
    dispose() { epoch++; el.remove(); },
  };
}
