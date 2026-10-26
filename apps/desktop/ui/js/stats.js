// PRD 0026 RF4: the Stats view. KPI cards (limit hits, average peak, extra spent), the daily peak bars, a row per
// account and the recent journal events, all from the core (`stats`): this module only draws. Honest data: a day without
// samples is a hatched ghost column, never a zero bar; unknown figures read "—" with the reason (DS-CMP-KPI-2); fewer
// than two days of data replaces the charts with one sentence (DS-CHT-11). Every chart hides from assistive technology,
// which reads its summary and an equivalent table (DS-CHT-10).
import { currentLocale, t, te, tl } from "./i18n.js";
import { monograms, repeatedProviders, tileHtml } from "./components.js";
import { STALE, brand, errText, esc, fmtNumber, getView, icon, openSettingsWindow, reportKey, stats } from "./lib.js";

const RANGES = ["days7", "days30"];
const REFRESH_MS = 30_000;
const DAY_MS = 864e5;
let uid = 0;

const pct = (share) => `${Math.round(share * 100)}%`;
const dayDate = (iso) => new Date(`${iso}T12:00:00`);
const dayName = (iso) => dayDate(iso).toLocaleDateString(currentLocale(), { weekday: "short", day: "numeric", month: "short" });
const clock = (iso) => new Date(iso).toLocaleTimeString(currentLocale(), { hour: "2-digit", minute: "2-digit" });

/** "Today 10:44", "Yesterday 10:44" or "24 Oct 10:44": the time of a journal row (DS-CMP-JRNL-1). */
function eventWhen(iso, now = new Date()) {
  const at = new Date(iso);
  const midnight = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const back = Math.round((midnight(now) - midnight(at)) / DAY_MS);
  const day = back === 0 ? t("stats.events.today") : back === 1 ? t("stats.events.yesterday")
    : at.toLocaleDateString(currentLocale(), { day: "numeric", month: "short" });
  return `${day} ${clock(iso)}`;
}

/** The glyph and tone of a journal row, by event kind. */
const EVENT_ICON = {
  quota_threshold_crossed: ["arrowUp", "warn"], quota_exhausted: ["ban", "bad"], quota_recovered: ["check", "good"],
  quota_exhaustion_projected: ["arrowUp", "warn"], quota_reset: ["refresh", "good"], account_blocked: ["ban", "bad"],
  account_unblocked: ["check", "good"], refresh_failed: ["alert", "bad"], refresh_recovered: ["check", "good"],
  auth_required: ["key", "warn"], provider_incident: ["alert", "bad"], overage_started: ["coins", "warn"],
  overage_ended: ["coins", "muted"], session_expiring: ["clock", "warn"], quota_reset_soon: ["clock", "muted"],
  pool_exhausted: ["ban", "bad"], pool_recovered: ["check", "good"],
};

// ---------- KPI cards (DS-CMP-KPI) ----------

const kpi = (labelKey, figure, caption, { unknown = false, id = "" } = {}) =>
  `<div class="kpi${unknown ? " unknown" : ""}" ${id ? `data-kpi="${id}"` : ""}><span class="kpi-label">${te(labelKey)}</span>
    <b class="kpi-figure">${esc(figure)}</b><span class="kpi-caption sub">${esc(caption)}</span></div>`;

/** An amount in its unit: a currency code reads as money ("$6.25"), any other provider unit stays "amount unit". */
function money(amount, unit) {
  if (/^[A-Z]{3}$/.test(unit)) {
    try { return new Intl.NumberFormat(currentLocale(), { style: "currency", currency: unit }).format(amount); } catch { /* not a currency */ }
  }
  return `${fmtNumber(amount)} ${unit}`;
}

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
    ? kpi("stats.kpi.avg", `≈${pct(s.average_peak)}`, t("stats.kpi.avg.caption"), { id: "avg" })
    : kpi("stats.kpi.avg", "—", why, { unknown: true, id: "avg" });
  // Extra spent lists one figure per currency; provider units are separate lines, never summed (DS-CMP-EXTRA-3).
  // Summed from the sampled readings, so each amount is an estimate and carries "≈" (DS-CMP-HON-2).
  const extra = s.extra_spent?.length
    ? `<div class="kpi" data-kpi="extra"><span class="kpi-label">${te("stats.kpi.extra")}</span>${
      s.extra_spent.map((e) => { const text = `≈${money(e.amount, e.unit)}`; return `<b class="kpi-figure${text.length > 6 ? " long" : ""}">${esc(text)}</b>`; }).join("")
    }<span class="kpi-caption sub">${te("stats.kpi.extra.caption")}</span></div>`
    : kpi("stats.kpi.extra", "—", t("stats.kpi.extra.none"), { unknown: true, id: "extra" });
  return `<div class="kpis">${hits}${avg}${extra}</div>`;
}

// ---------- daily bars (DS-CHT-5) ----------

/** Bars for a series of `{ day, share|null, is_today }`: equal width, today in the accent and labelled, `--bad` only
 *  at the limit, a gap as a hatched ghost column with "No data" in its tooltip. 0–100 axis, gridlines at 50 and 100. */
export function barsHtml(daily) {
  const n = daily.length;
  const every = n > 7 ? 5 : 1;
  const cols = daily.map((d, i) => {
    const name = dayName(d.day);
    const tip = d.share == null ? `${name}: ${t("stats.chart.noData")}` : `${name}: ${pct(d.share)}`;
    const label = d.is_today ? t("stats.chart.today")
      : n <= 7 ? dayDate(d.day).toLocaleDateString(currentLocale(), { weekday: "short" })
      : i % every === 0 || i === n - 1 ? String(dayDate(d.day).getDate()) : "";
    const cls = d.share == null ? "gap" : d.share >= 1 ? "full" : d.is_today ? "today" : "";
    const bar = d.share == null ? `<i class="bar ghost"></i>` : `<i class="bar" style="height:${(d.share * 100).toFixed(1)}%"></i>`;
    // The value over a bar: today's, and a dash over a day without readings.
    const value = d.share == null ? `<span class="x-val" style="bottom:100%">—</span>`
      : d.is_today ? `<span class="x-val" style="bottom:calc(${(d.share * 100).toFixed(1)}% + 2px)">${esc(pct(d.share))}</span>` : "";
    return `<li class="${cls}" title="${esc(tip)}">${bar}${value}<span class="x-label">${esc(label)}</span></li>`;
  }).join("");
  return `<div class="bars" aria-hidden="true"><span class="grid-line g100"><em>100</em></span><span class="grid-line g50"><em>50</em></span><span class="grid-line g0"><em>0</em></span>
    <ul style="--n:${n}">${cols}</ul></div>`;
}

const rangeToggle = (range) => `<div class="seg" role="group" aria-label="${te("stats.range.label")}">${
  RANGES.map((r) => `<button type="button" data-stats-range="${r}" aria-pressed="${r === range}">${te(`stats.range.${r}`)}</button>`).join("")}</div>`;

/** The chart card: title, subtitle and the 7 d / 30 d toggle (DS-CMP-STATS-2), then the bars or the one-sentence empty state. */
function chartHtml(data, id, range) {
  const s = data.summary;
  const head = `<div class="st-card-head"><div><h3>${te("stats.chart.title")}</h3><p class="sub">${te("stats.chart.subtitle")}</p></div>${rangeToggle(range)}</div>`;
  if (s.coverage.kind === "not_enough") return `<section class="st-card">${head}<p class="sub st-empty">${te("stats.chart.empty")}</p></section>`;
  const withData = s.daily.filter((d) => d.share != null);
  const peak = Math.max(...withData.map((d) => d.share));
  const today = s.daily.find((d) => d.is_today)?.share;
  const alt = t("stats.chart.alt", { days: s.daily.length, peak: pct(peak), today: today == null ? t("stats.chart.noData") : pct(today) });
  const rows = s.daily.map((d) => `<tr><td>${esc(dayName(d.day))}</td><td>${d.share == null ? te("stats.chart.noData") : pct(d.share)}</td></tr>`).join("");
  return `<section class="st-card">${head}<div role="img" aria-label="${esc(alt)}" aria-describedby="${id}-table">${barsHtml(s.daily)}</div>
    <table class="sr-only" id="${id}-table"><caption>${esc(alt)}</caption>
      <thead><tr><th scope="col">${te("stats.table.day")}</th><th scope="col">${te("stats.table.peak")}</th></tr></thead><tbody>${rows}</tbody></table></section>`;
}

// ---------- by account (DS-CHT-7) ----------

function accountsHtml(data, view) {
  const s = data.summary;
  if (!s.accounts.length) return "";
  // `indexes` maps a row of the answer to its account index in the configuration (a scoped answer has one row).
  const reportOf = (a) => view?.reports.find((r) => r.index === data.indexes[a.account_index]);
  const known = s.accounts.map(reportOf).filter(Boolean);
  const repeated = repeatedProviders(known), monos = monograms(known);
  const rows = s.accounts.map((a) => {
    const r = reportOf(a);
    const minis = a.days.map((d, i) => {
      const last = i === a.days.length - 1;
      if (d.share == null) return `<i class="mini-bar gap${last ? " last" : ""}"></i>`;
      return `<i class="mini-bar${last ? (d.share >= 1 ? " last full" : " last") : ""}" style="height:${Math.max(6, d.share * 100).toFixed(0)}%"></i>`;
    }).join("");
    const avg = a.average_peak == null ? t("stats.accounts.noData") : t("stats.accounts.avg", { pct: pct(a.average_peak) });
    const hits = a.hits ? t("stats.accounts.hits", { count: a.hits }) : t("stats.accounts.noHits");
    // A provider with several accounts titles its rows with the account (DS-CMP-ROW-1), the others with the provider.
    const label = r ? brand(r.provider).label : "";
    const [title, sub] = !r ? [a.name, ""] : repeated.has(r.provider) ? [a.name, label] : [label, a.name];
    return `<li class="st-account"><span class="st-who">${r ? tileHtml(r.provider, 28, monos.get(reportKey(r)) ?? "") : ""}<span class="st-names"><span class="st-name" title="${esc(r ? `${label} · ${a.name}` : a.name)}">${esc(title)}</span>${sub ? `<span class="st-name sub">${esc(sub)}</span>` : ""}</span></span>
      <span class="mini-bars" aria-hidden="true">${minis}</span><span class="st-sum${a.hits ? " bad" : ""}"><b>${esc(avg)}</b><span class="sub">${esc(hits)}</span></span>
      <span class="sr-only">${esc(`${a.name}: ${avg} · ${hits}`)}</span></li>`;
  }).join("");
  return `<section class="st-section"><div class="st-sec-head"><h3 class="micro">${te("stats.accounts.title")}</h3><span class="micro">${te("stats.accounts.legend")}</span></div><ul class="st-list">${rows}</ul></section>`;
}

// ---------- recent events (DS-CMP-JRNL-1) ----------

function eventsHtml(data) {
  const rows = data.events.map((ev) => {
    // PRD 0036 RF7.3: a connection's event names its aggregator after the connection.
    const who = [ev.account ?? tl(data.removed_account), ev.parent].filter(Boolean).join(" · ");
    const [glyph, tone] = EVENT_ICON[ev.kind] ?? ["info", "muted"];
    return `<li class="st-event"><span class="st-ev-ico ${tone}" aria-hidden="true">${icon(glyph)}</span>
      <div class="st-event-main"><span class="st-event-text">${esc(tl(ev.summary))}</span><span class="sub">${esc(who)}</span></div>
      <time class="st-time" datetime="${esc(ev.occurred_at)}">${esc(eventWhen(ev.occurred_at))}</time></li>`;
  });
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

/** The view for `data` (one `stats` answer): the three cards, the chart card, a row per account and the journal. */
export function statsBodyHtml(data, view, id, { range = "days7" } = {}) {
  return `${kpisHtml(data)}${chartHtml(data, id, range)}${accountsHtml(data, view)}${eventsHtml(data)}${footnoteHtml(data)}`;
}

/** The Stats view of a window: scope select + 7 d / 30 d toggle (DS-CMP-STATS-2), then `statsBodyHtml`. It loads
 *  from the core on mount, on a scope or range change and whenever `update` sees a newer view; an answer for another
 *  generation is discarded. The host calls `update(view)` on every render and `attach(slot)` after it. */
export function mountStats({ tray = false, onOpenFull = null } = {}) {
  const id = `stats-${++uid}`;
  const el = document.createElement("section");
  el.className = `stats${tray ? " tray" : ""}`;
  el.setAttribute("aria-labelledby", `${id}-title`);
  // The scope select sits in the window's own header: in this view's head, or (tray) in the popover's header via `attachScope`.
  const scopeEl = document.createElement("label");
  scopeEl.className = "st-scope";
  scopeEl.innerHTML = `<span class="sr-only">${te("stats.scope.label")}</span><select data-stats-scope></select>`;
  el.innerHTML = `<div class="st-head"><h2 id="${id}-title"${tray ? ' class="sr-only"' : ""}>${te("stats.title")}</h2></div>
    <p class="st-status sub" role="status" aria-live="polite"></p>
    <div class="st-body"></div>
    ${tray && onOpenFull ? `<button type="button" class="link st-open" data-stats-open>${te("stats.openFull")}</button>` : ""}`;
  if (!tray) el.querySelector(".st-head").append(scopeEl);
  const status = el.querySelector(".st-status");
  const body = el.querySelector(".st-body");
  const select = scopeEl.querySelector("select");
  let range = "days7", scope = "all", data = null, error = null, epoch = 0, stamp = null, generation = null, view = null, at = 0;

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
    body.innerHTML = !data || error ? "" : statsBodyHtml(data, view, id, { range });
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
    /** Tray: hand the scope select to the popover header (rebuilt on every render). */
    attachScope(slot) { if (slot && scopeEl.parentNode !== slot) slot.append(scopeEl); },
    dispose() { epoch++; el.remove(); scopeEl.remove(); },
  };
}

