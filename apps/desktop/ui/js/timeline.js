// PRD 0026 RF5: "Limits come back", the reset timeline (DS-CHT-6). One lane per account for its tightest limit, sorted
// by return time, from the core's `reset_timeline` (the held readings only: no query, no history). Lanes with a return
// beyond the range, and accounts whose return is unknown, are listed as text below and never plotted at an edge.
// This module only draws; the chart is hidden from assistive technology, which reads one named button per lane.
import { currentLocale, t, te } from "./i18n.js";
import { STALE, brand, errText, esc, fmtCountdown, getView, logo, reportKey, resetTimeline } from "./lib.js";

const RANGES = ["hours24", "days7"];
const REFRESH_MS = 30_000;
let uid = 0;

const clock = { hour: "2-digit", minute: "2-digit" };
const timeOf = (iso, range) => new Date(iso).toLocaleString(currentLocale(), range === "days7" ? { weekday: "short", ...clock } : clock);
const percentOf = (lane) => Math.round(lane.remaining_share * 100);

/** "Claude Work back at 19:30, in 1h 35m" (DS-CHT-6 accessible name). */
export function laneLabel(lane, range) {
  const when = timeOf(lane.returns_at, range);
  const left = lane.exhausted ? t("timeline.exhausted") : t("timeline.left", { pct: percentOf(lane) });
  return `${t("timeline.lane.aria", { name: lane.name, time: when, in: fmtCountdown(lane.returns_at) ?? "" })}. ${left}${lane.stale ? `. ${t("timeline.stale")}` : ""}`;
}

/** Several accounts of one provider show the account name next to the tile; one shows only the provider. */
const labelOf = (lane, lanes) =>
  lanes.filter((l) => l.provider === lane.provider).length > 1 ? lane.name : brand(lane.provider).label;

function laneHtml(lane, all, tl) {
  const span = new Date(tl.ends_at) - new Date(tl.now);
  const x = Math.min(1, Math.max(0, (new Date(lane.returns_at) - new Date(tl.now)) / span));
  const left = `${(x * 100).toFixed(2)}%`;
  const flip = x > 0.78 ? " flip" : "";
  return `<li class="tl-lane ${lane.level}${lane.stale ? " stale" : ""}">
    <button type="button" class="tl-who" data-timeline-account="${lane.account_index}" aria-label="${esc(laneLabel(lane, tl.range))}" title="${esc(laneLabel(lane, tl.range))}">
      ${logo(lane.provider, 14)}<span class="tl-name">${esc(labelOf(lane, all))}</span>
    </button>
    <div class="tl-track" aria-hidden="true"><i class="tl-line" style="width:${left}"></i><i class="tl-dot" style="left:${left}"></i>
      <span class="tl-time${flip}" style="left:${left}">${esc(timeOf(lane.returns_at, tl.range))}</span></div>
  </li>`;
}

/** Text lines for the returns beyond the range and for the unknown ones. */
function textGroup(titleKey, lanes, tl, withTime) {
  if (!lanes.length) return "";
  return `<div class="tl-text"><h4>${te(titleKey)}</h4><ul>${lanes.map((l) => `<li>${logo(l.provider, 14)}<span>${esc(`${brand(l.provider).label} · ${l.name}`)}</span>${
    withTime ? `<span class="tl-when">${esc(timeOf(l.returns_at, "days7"))}</span>` : ""}</li>`).join("")}</ul></div>`;
}

function chartHtml(tl) {
  if (!tl.lanes.length) return `<p class="sub tl-empty">${te("timeline.empty")}</p>`;
  return `<div class="tl-axis sub" aria-hidden="true"><span class="tl-now-label">${te("timeline.now")}</span><span>${esc(timeOf(tl.ends_at, tl.range))}</span></div>
    <ul class="tl-lanes">${tl.lanes.map((lane) => laneHtml(lane, tl.lanes, tl)).join("")}</ul>`;
}

/** The section: loads from the core on mount, on a range change and whenever `update` sees a newer view; an answer
 *  for another generation is discarded. The host calls `update(view)` on every render and `attach(slot)` after it.
 *  `onSelect(key, report)` runs when a lane is chosen (the host focuses that account). */
export function mountTimeline({ onSelect = () => {}, compact = false } = {}) {
  const id = `tl-${++uid}`;
  const el = document.createElement("section");
  el.className = `timeline${compact ? " compact" : ""}`;
  el.setAttribute("aria-labelledby", id);
  el.innerHTML = `<div class="tl-head"><h3 id="${id}">${te("timeline.title")}</h3>
      <div class="seg" role="group" aria-label="${te("timeline.range.label")}">
        ${RANGES.map((r) => `<button type="button" data-timeline-range="${r}">${te(`timeline.range.${r}`)}</button>`).join("")}
      </div></div>
    <p class="tl-status sub" role="status" aria-live="polite"></p>
    <div class="tl-body"></div>`;
  const status = el.querySelector(".tl-status");
  const body = el.querySelector(".tl-body");
  let range = "hours24", data = null, error = null, epoch = 0, stamp = null, generation = null, reports = [], at = 0;

  function paint() {
    for (const b of el.querySelectorAll("[data-timeline-range]")) b.setAttribute("aria-pressed", String(b.dataset.timelineRange === range));
    status.textContent = error ?? (!data ? t("stats.loading") : "");
    body.innerHTML = !data || error ? "" : `${chartHtml(data)}${textGroup("timeline.later", data.later, data, true)}${textGroup("timeline.unknown", data.unknown, data, false)}`;
  }
  async function load() {
    const e = ++epoch;
    at = Date.now();
    try {
      const d = await resetTimeline(range, generation);
      if (e !== epoch) return;
      data = d; error = null;
    } catch (x) {
      if (e !== epoch) return;
      if (x === STALE) return resync();
      data = null; error = errText(x);
    }
    paint();
  }
  /** The configuration changed before the UI saw the new view: read it again. */
  async function resync() {
    const v = await getView().catch(() => null);
    if (v) update(v);
  }
  /** `view` is the latest `get_view`; the timeline reads again when its stamp (reading times, generation) moves or
   *  the data is older than the refresh period (the Now line and countdowns follow the clock). */
  function update(view) {
    reports = view?.reports ?? [];
    const next = `${view?.generation}:${view?.finished_at}:${reports.length}`;
    if (view?.generation == null) return;
    if (next === stamp && Date.now() - at < REFRESH_MS) return;
    if (view.generation !== generation) { data = null; paint(); }
    generation = view.generation; stamp = next;
    load();
  }
  el.addEventListener("click", (e) => {
    const r = e.target.closest("[data-timeline-range]");
    if (r) {
      if (r.dataset.timelineRange === range) return;
      range = r.dataset.timelineRange; data = null; paint(); load();
      return;
    }
    const lane = e.target.closest("[data-timeline-account]");
    if (!lane) return;
    const report = reports.find((x) => x.index === Number(lane.dataset.timelineAccount));
    if (report) onSelect(reportKey(report), report);
  });
  paint();
  return {
    el, update,
    focused: () => (el.contains(document.activeElement) ? document.activeElement : null),
    attach(slot, focus) { if (slot && el.parentNode !== slot) slot.append(el); focus?.focus?.(); },
    dispose() { epoch++; el.remove(); },
  };
}
