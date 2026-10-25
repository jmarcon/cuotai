// PRD 0026 RF4: the Stats view. KPI cards (limit hits, average peak, extra spent), the daily peak bars, a row per
// account and the recent journal events, all from the core (`stats`): this module only draws. Honest data: a day without
// samples is a hatched ghost column, never a zero bar; unknown figures read "—" with the reason (DS-CMP-KPI-2); fewer
// than two days of data replaces the charts with one sentence (DS-CHT-11). Every chart hides from assistive technology,
// which reads its summary and an equivalent table (DS-CHT-10).
import { currentLocale, t, te, tl } from "./i18n.js";
import { STALE, brand, errText, esc, fmtNumber, getView, logo, openSettingsWindow, reportKey, stats } from "./lib.js";

const RANGES = ["days7", "days30"];
const REFRESH_MS = 30_000;
const DAY_MS = 864e5;
let uid = 0;

const pct = (share) => `${Math.round(share * 100)}%`;
const dayDate = (iso) => new Date(`${iso}T12:00:00`);
const dayName = (iso) => dayDate(iso).toLocaleDateString(currentLocale(), { weekday: "short", day: "numeric", month: "short" });
const clock = (iso) => new Date(iso).toLocaleTimeString(currentLocale(), { hour: "2-digit", minute: "2-digit" });

/** "Today", "Yesterday" or the date: the group header of the journal (DS-CMP-JRNL-1). */
function dayHeader(iso, now = new Date()) {
  const at = new Date(iso);
  const midnight = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const back = Math.round((midnight(now) - midnight(at)) / DAY_MS);
  return back === 0 ? t("stats.events.today") : back === 1 ? t("stats.events.yesterday")
    : at.toLocaleDateString(currentLocale(), { weekday: "short", day: "numeric", month: "short" });
}

// ---------- KPI cards (DS-CMP-KPI) ----------

const kpi = (labelKey, figure, caption, { unknown = false, id = "" } = {}) =>
  `<div class="kpi${unknown ? " unknown" : ""}" ${id ? `data-kpi="${id}"` : ""}><span class="kpi-label">${te(labelKey)}</span>
    <b class="kpi-figure">${esc(figure)}</b><span class="kpi-caption sub">${esc(caption)}</span></div>`;

/** The three cards. Insufficient data is "—" and the reason, never 0 (DS-CMP-KPI-2). */
export function kpisHtml(data) {
  const s = data.summary;
  const enough = s.coverage.kind !== "not_enough";
  const why = tl(data.not_enough);
  const days = s.range === "days30" ? 30 : 7;
  const hits = enough
    ? kpi("stats.kpi.hits", String(s.hits), t("stats.kpi.hits.caption", { days }), { id: "hits" })
    : kpi("stats.kpi.hits", "—", why, { unknown: true, id: "hits" });
  const avg = enough && s.average_peak != null
    ? kpi("stats.kpi.avg", pct(s.average_peak), t("stats.kpi.avg.caption", { days: s.daily.filter((d) => d.share != null).length }), { id: "avg" })
    : kpi("stats.kpi.avg", "—", why, { unknown: true, id: "avg" });
  // Extra spent lists one figure per currency; provider units are separate lines, never summed (DS-CMP-EXTRA-3).
  const extra = s.extra_spent?.length
    ? `<div class="kpi" data-kpi="extra"><span class="kpi-label">${te("stats.kpi.extra")}</span>${
      s.extra_spent.map((e) => `<b class="kpi-figure">${esc(fmtNumber(e.amount))} ${esc(e.unit)}</b>`).join("")
    }<span class="kpi-caption sub">${te("stats.kpi.extra.caption", { days })}</span></div>`
    : kpi("stats.kpi.extra", "—", t("stats.kpi.extra.none"), { unknown: true, id: "extra" });
  return `<div class="kpis">${hits}${avg}${extra}</div>`;
}

// ---------- daily bars (DS-CHT-5) ----------

/** Bars for a series of `{ day, share|null, is_today }`: equal width, today in the accent and labelled, `--bad` only
 *  at the limit, a gap as a hatched ghost column with "No data" in its tooltip. 0–100 axis, gridlines at 50 and 100. */
export function barsHtml(daily, { compact = false } = {}) {
  const n = daily.length;
  const every = n > 7 ? 5 : 1;
  const cols = daily.map((d, i) => {
    const name = dayName(d.day);
    const tip = d.share == null ? `${name}: ${t("stats.chart.noData")}` : `${name}: ${pct(d.share)}`;
    const label = d.is_today ? t("stats.chart.today") : (i % every === 0 || i === n - 1) && !compact ? String(dayDate(d.day).getDate()) : "";
    const cls = d.share == null ? "gap" : d.share >= 1 ? "full" : d.is_today ? "today" : "";
    const bar = d.share == null ? `<i class="bar ghost"></i>` : `<i class="bar" style="height:${(d.share * 100).toFixed(1)}%"></i>`;
    return `<li class="${cls}" title="${esc(tip)}">${bar}<span class="x-label">${esc(label)}</span></li>`;
  }).join("");
  return `<div class="bars${compact ? " compact" : ""}" aria-hidden="true"><span class="grid-line g100"><em>100</em></span><span class="grid-line g50"><em>50</em></span>
    <ul style="--n:${n}">${cols}</ul></div>`;
}

function chartHtml(data, id, { compact = false } = {}) {
  const s = data.summary;
  if (s.coverage.kind === "not_enough") return `<p class="sub st-empty">${te("stats.chart.empty")}</p>`;
  const withData = s.daily.filter((d) => d.share != null);
  const peak = Math.max(...withData.map((d) => d.share));
  const today = s.daily.find((d) => d.is_today)?.share;
  const alt = t("stats.chart.alt", { days: s.daily.length, peak: pct(peak), today: today == null ? t("stats.chart.noData") : pct(today) });
  const rows = s.daily.map((d) => `<tr><td>${esc(dayName(d.day))}</td><td>${d.share == null ? te("stats.chart.noData") : pct(d.share)}</td></tr>`).join("");
  return `<div role="img" aria-label="${esc(alt)}" aria-describedby="${id}-table">${barsHtml(s.daily, { compact })}</div>
    <table class="sr-only" id="${id}-table"><caption>${esc(alt)}</caption>
      <thead><tr><th scope="col">${te("stats.table.day")}</th><th scope="col">${te("stats.table.peak")}</th></tr></thead><tbody>${rows}</tbody></table>`;
}

// ---------- by account (DS-CHT-7) ----------

function accountsHtml(data, view) {
  const s = data.summary;
  if (!s.accounts.length) return "";
  // `indexes` maps a row of the answer to its account index in the configuration (a scoped answer has one row).
  const reportOf = (a) => view?.reports.find((r) => r.index === data.indexes[a.account_index]);
  const rows = s.accounts.map((a) => {
    const r = reportOf(a);
    const minis = a.days.map((d, i) => {
      const last = i === a.days.length - 1;
      if (d.share == null) return `<i class="mini-bar gap${last ? " last" : ""}"></i>`;
      return `<i class="mini-bar${last ? (d.share >= 1 ? " last full" : " last") : ""}" style="height:${Math.max(6, d.share * 100).toFixed(0)}%"></i>`;
    }).join("");
    const avg = a.average_peak == null ? t("stats.accounts.noData") : t("stats.accounts.avg", { pct: pct(a.average_peak) });
    const summary = `${avg} · ${t("stats.accounts.hits", { count: a.hits })}`;
    const name = r ? `${brand(r.provider).label} · ${a.name}` : a.name;
    return `<li class="st-account"><span class="st-who">${r ? logo(r.provider, 24) : ""}<span class="st-name" title="${esc(name)}">${esc(a.name)}</span></span>
      <span class="mini-bars" aria-hidden="true">${minis}</span><span class="st-sum${a.hits ? " bad" : ""}">${esc(summary)}</span>
      <span class="sr-only">${esc(`${a.name}: ${summary}`)}</span></li>`;
  }).join("");
  return `<section class="st-section"><h3 class="micro">${te("stats.accounts.title")}</h3><ul class="st-list">${rows}</ul></section>`;
}

// ---------- recent events (DS-CMP-JRNL-1) ----------

function eventsHtml(data) {
  const rows = [];
  let group = null;
  for (const ev of data.events) {
    const header = dayHeader(ev.occurred_at);
    if (header !== group) { rows.push(`<li class="st-day-head micro" role="presentation">${esc(header)}</li>`); group = header; }
    const who = ev.account ?? tl(data.removed_account);
    rows.push(`<li class="st-event"><div class="st-event-main"><span class="st-event-text">${esc(tl(ev.summary))}</span><span class="sub">${esc(who)}</span></div>
      <time class="st-time" datetime="${esc(ev.occurred_at)}">${esc(clock(ev.occurred_at))}</time></li>`);
  }
  const off = data.recording ? "" : `<p class="sub">${te("stats.recording.off")}</p>`;
  return `<section class="st-section"><h3 class="micro">${te("stats.events.title")}</h3>${off}${
    rows.length ? `<ul class="st-list events">${rows.join("")}</ul>` : `<p class="sub st-empty-line">${te("stats.events.empty")}</p>`}</section>`;
}

// ---------- footnote (DS-CMP-STATS-3) ----------

function footnoteHtml(data) {
  const s = data.summary;
  const partial = s.coverage.kind === "partial" ? ` ${te("stats.footer.partial", { have: s.coverage.days_with_data, total: s.daily.length })}` : "";
  const unknown = s.unknown_limits ? ` ${te("stats.footer.unknown", { count: s.unknown_limits })}` : "";
  return `<p class="sub st-foot">${esc(tl(data.history_kept))}${partial}${unknown}
    <button type="button" class="link" data-stats-history-settings>${te("stats.footer.settings")}</button></p>`;
}

/** The view for `data` (one `stats` answer). `compact` is the popover drill-in: the three cards and the 7-day bars. */
export function statsBodyHtml(data, view, id, { compact = false } = {}) {
  if (compact) return `${kpisHtml(data)}<section class="st-section"><h3 class="micro">${te("stats.chart.title")}</h3>${chartHtml(data, id, { compact })}</section>`;
  return `${kpisHtml(data)}
    <section class="st-section"><h3 class="micro">${te("stats.chart.title")}</h3>${chartHtml(data, id)}</section>
    ${accountsHtml(data, view)}${eventsHtml(data)}${footnoteHtml(data)}`;
}

/** The Stats view of a window: scope select + 7 d / 30 d toggle (DS-CMP-STATS-2), then `statsBodyHtml`. It loads
 *  from the core on mount, on a scope or range change and whenever `update` sees a newer view; an answer for another
 *  generation is discarded. The host calls `update(view)` on every render and `attach(slot)` after it. */
export function mountStats({ compact = false, onOpenFull = null } = {}) {
  const id = `stats-${++uid}`;
  const el = document.createElement("section");
  el.className = `stats${compact ? " compact" : ""}`;
  el.setAttribute("aria-labelledby", `${id}-title`);
  el.innerHTML = `<div class="st-head"><h2 id="${id}-title">${te("stats.title")}</h2>
      ${compact ? "" : `<label class="st-scope"><span class="sr-only">${te("stats.scope.label")}</span><select data-stats-scope></select></label>`}
      <div class="seg" role="group" aria-label="${te("stats.range.label")}">
        ${RANGES.map((r) => `<button type="button" data-stats-range="${r}">${te(`stats.range.${r}`)}</button>`).join("")}
      </div></div>
    <p class="st-status sub" role="status" aria-live="polite"></p>
    <div class="st-body"></div>
    ${compact && onOpenFull ? `<button type="button" class="link st-open" data-stats-open>${te("stats.openFull")}</button>` : ""}`;
  const status = el.querySelector(".st-status");
  const body = el.querySelector(".st-body");
  const select = el.querySelector("[data-stats-scope]");
  let range = compact ? "days7" : "days7", scope = "all", data = null, error = null, epoch = 0, stamp = null, generation = null, view = null, at = 0;
  if (compact) el.querySelector(".seg").hidden = true;

  /** The select lists "All accounts", then each account (DS-CMP-STATS-2); a scope that vanished falls back to all. */
  function fillScope() {
    if (!select) return;
    const accounts = view?.reports ?? [];
    if (scope !== "all" && !accounts.some((r) => reportKey(r) === scope)) scope = "all";
    select.innerHTML = `<option value="all">${te("stats.scope.all")}</option>${accounts.map((r) =>
      `<option value="${esc(reportKey(r))}">${esc(`${brand(r.provider).label} · ${r.account}`)}</option>`).join("")}`;
    select.value = scope;
  }
  function paint() {
    for (const b of el.querySelectorAll("[data-stats-range]")) b.setAttribute("aria-pressed", String(b.dataset.statsRange === range));
    status.textContent = error ?? (!data ? t("stats.loading") : "");
    body.innerHTML = !data || error ? "" : statsBodyHtml(data, view, id, { compact });
  }
  async function load() {
    const e = ++epoch;
    at = Date.now();
    const account = scope === "all" ? null : view?.reports.find((r) => reportKey(r) === scope)?.index ?? null;
    try {
      const d = await stats(range, account, generation);
      if (e !== epoch) return;
      data = d; error = null;
    } catch (x) {
      if (e !== epoch) return;
      if (x === STALE) return resync();
      data = null; error = errText(x);
    }
    paint();
  }
  async function resync() {
    const v = await getView().catch(() => null);
    if (v) update(v);
  }
  /** The stamp moves with a new reading, an erasure or the recording switch (`history_epoch`). */
  function update(next) {
    if (next?.generation == null) return;
    view = next;
    fillScope();
    const mark = `${next.generation}:${next.history_epoch}:${next.finished_at}:${next.reports.length}`;
    if (mark === stamp && Date.now() - at < REFRESH_MS) return;
    if (next.generation !== generation) { data = null; paint(); }
    generation = next.generation; stamp = mark;
    load();
  }
  el.addEventListener("click", (e) => {
    const r = e.target.closest("[data-stats-range]");
    if (r) {
      if (r.dataset.statsRange === range) return;
      range = r.dataset.statsRange; data = null; paint(); load();
      return;
    }
    if (e.target.closest("[data-stats-history-settings]")) return openSettingsWindow({ page: "data" });
    if (e.target.closest("[data-stats-open]")) onOpenFull?.();
  });
  select?.addEventListener("change", () => { scope = select.value; data = null; paint(); load(); });
  paint();
  return {
    el, update,
    focused: () => (el.contains(document.activeElement) ? document.activeElement : null),
    attach(slot, focus) { if (slot && el.parentNode !== slot) slot.append(el); focus?.focus?.(); },
    dispose() { epoch++; el.remove(); },
  };
}

