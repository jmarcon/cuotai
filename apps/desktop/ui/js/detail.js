// Detail of an account: same component in the popover and in the main window dialog.
// Single criterion: AVAILABLE percentage, bar = available, amount = used / limit; metrics without a limit become text.
import { t, te, tl } from "./i18n.js";
import {
  STALE, accessHtml, ageHtml, bindingChipText, causeText, errText, esc, fmtAmount, fmtReset, getView, icon, incidentDetailHtml, isBindingMetric, isMetricHidden, isRenewedWaiting, isStaleReading, level, loadModels, metricAlertHtml, metricKey, modelState, noteText,
  adviceHtml, fmtMetricExtra, forecastHtml, scopeBlocksHtml, isOverage, overageDetailHtml, paceHtml, paceOf, remainingPct, renewedText, reportKey, restNotes, staleView, statusLabel, syncModels, textualMetric,
} from "./lib.js";
import { expectedLeft, fmtAt, meterHtml } from "./components.js";
import { breakdownHtml } from "./breakdown.js";
import { fold } from "./util.js";

/** Fa gap 8: the same localized limit names as the main-window cards ("5-hour session", "Weekly"), for the only limit
 *  of its window. Several limits of one window keep what tells them apart: the model or the provider's group name. */
const LONG_WINDOW = { window_5h: "h5", weekly: "week" };
function limitLabel(r, m) {
  const own = tl(m.label_text ?? m.label);
  const long = LONG_WINDOW[m.window];
  if (!long) return own;
  const window = t(`card.window.long.${long}`);
  if ((r.metrics ?? []).filter((x) => x.window === m.window).length < 2) return window;
  return m.scope?.label ? `${m.scope.label} · ${window}` : own;
}

/** `eye`: the quota's hide/show button and, PRD 0008 RF3.2, its usage alert mute (dialog only). */
export function metricCardHtml(r, m, { eye = false, stale = false } = {}) {
  const pct = remainingPct(m);
  const lv = level(pct);
  // PRD 0023: paid extra usage is a textual figure, never a meter that implies "left".
  const extra = isOverage(m);
  const text = extra ? fmtMetricExtra(m) : textualMetric(m);
  const reset = fmtReset(m.resets_at);
  const hid = isMetricHidden(r, m);
  const pace = paceOf(m);
  // PRD 0022 RF2 (DS-CMP-MTR): one continuous bar of the remaining share; the pace corridor sits at the expected
  // remaining share (PRD 0007 RF3.2) and is absent on stale, unknown and exhausted meters.
  const state = stale ? "stale" : m.state === "exhausted" ? "exhausted" : pct == null ? "unknown" : "fresh";
  const valueText = pct == null ? t("meter.noData")
    : pace ? t("meter.valuetext", { pct: Math.round(pct), status: [tl(pace.label), t("metric.pace.expected", { pct: Math.round(pace.expected_used_fraction * 100) })].join(", ") })
      : t("meter.left", { pct: Math.round(pct) });
  const renewed = isRenewedWaiting(m);
  const gauge = renewed
    ? `<p class="sub" data-renewed-waiting>${icon("clock")} ${esc(renewedText(m))}</p>`
    : text != null
    ? `<div class="balance ${m.state === "exhausted" && !extra ? "bad" : ""}">${esc(text)}</div>`
    : `<div class="gauge" title="${pace ? te("card.pace.estimate") : ""}">${meterHtml(pct, { state, expected: expectedLeft(m), text: valueText, name: limitLabel(r, m) })}</div>`;
  // PRD 0009 RF2.2: the core's binding metric gets the "Binding" tag; every other metric stays listed.
  const binding = isBindingMetric(r, m);
  // DS-CMP-SUB-1 (mockup 03): label + tag, the figure with its unit inline; the meter; the pace stage on the left and
  // "Resets 20:32 · in 2h 11m" (mono, muted) on the right. Counts and the expected share live in the tooltips.
  const paceTip = pace ? [tl(pace.label), t("metric.pace.expected", { pct: Math.round(pace.expected_used_fraction * 100) }), pace.outlook ? tl(pace.outlook) : "", t("card.pace.estimate")].filter(Boolean).join(". ") : "";
  const resetText = reset && !renewed ? (m.resets_label ? tl(m.resets_label) : t("metric.resetsAt", { when: reset })) : "";
  // PRD 0041 RF2: a budget quota is the person's number, labelled as such on every surface (never a provider limit).
  const budget = m.budget_badge ? tl(m.budget_badge) : "";
  return `<div class="metric ${hid ? "muted" : ""} ${binding ? "binding" : ""} ${budget ? "budget" : ""} ${m.alert?.active ? "usage-alert" : ""}" title="${esc(fmtAmount(m))}">
    <div class="metric-top">
      <h3>${esc(limitLabel(r, m))}${budget ? ` <span class="btag budget" data-budget-chip>${esc(budget)}</span>` : ""}${binding ? ` <span class="btag" data-binding-chip title="${esc(`${t("card.binding.title")}: ${bindingChipText(r)}`)}">${te("binding.tag")}</span>` : ""}</h3>
      <div class="metric-right">
        ${text == null && pct != null ? `<span class="pct" title="${te(`metric.level.${lv}`, { pct: Math.round(pct) })}">${Math.round(pct)}%<small>${te("card.available")}</small></span>` : ""}
        ${eye ? metricAlertHtml(r, m) : ""}
        ${eye ? `<button class="icon" data-hide="${esc(metricKey(r, m))}" title="${te(hid ? "common.show" : "common.hide")}" aria-label="${te(hid ? "common.show" : "common.hide")}">${icon(hid ? "eye" : "eyeOff")}</button>` : ""}
      </div>
    </div>
    ${gauge}
    ${pace || resetText ? `<div class="metric-meta">${pace ? `<span class="pace-line" data-pace="${esc(pace.stage)}" title="${esc(paceTip)}">${icon({ good: "check", neutral: "clock", warn: "warn", bad: "xcircle" }[pace.tone] ?? "clock", pace.tone)}<span>${esc(tl(pace.label))}</span></span>` : "<span></span>"}${resetText ? `<span class="resets">${esc(resetText)}</span>` : ""}</div>` : ""}
    ${stale || !m.allowance ? "" : `<p class="sub allowance-line" data-allowance title="${te("card.pace.estimate")}">${icon("clock")}<span>${esc(tl(m.allowance))}</span></p>`}
    ${stale ? "" : floorLinesHtml(r, m)}
    ${stale ? "" : forecastHtml(m, r)}
    ${eye && !stale ? budgetActionsHtml(r, m) : ""}
    ${m.alert?.missed ? `<div class="resets missed" data-missed>${icon("clock")}<span>${te("alerts.missed", { percent: m.alert.missed })}</span></div>` : ""}
  </div>`;
}

/** PRD 0041 RF3.2: the floor line under a balance ("Floor $10.00 · $3.20 left"), in the floor's role. */
function floorLinesHtml(r, m) {
  return (r.floors ?? []).filter((f) => f.label === m.label).map((f) =>
    `<p class="sub floor-line ${f.role === "ok" ? "" : f.role}" data-floor>${icon(f.role === "ok" ? "check" : "warn", f.role === "ok" ? "" : f.role)}<span>${esc(tl(f.line))}</span></p>`).join("");
}

/** PRD 0041 RF5.3: "Set a budget…" / "Set a floor…" where the core offers them (details dialog only). */
function budgetActionsHtml(r, m) {
  const i = r.metrics.indexOf(m);
  if (i < 0 || (!m.budget_draft && !m.offers_floor)) return "";
  return `<div class="metric-actions">
    ${m.budget_draft ? `<button type="button" class="btn sm" data-set-budget="${i}">${te("budget.action.set_budget")}</button>` : ""}
    ${m.offers_floor ? `<button type="button" class="btn sm" data-set-floor="${i}">${te("budget.action.set_floor")}</button>` : ""}
  </div>`;
}

/** Quota list + notes. Without `eye` (popover), hidden ones do not appear. PRD 0010 RF3: the age leads; a failure
 *  with a preserved reading lists the old metrics dimmed under the cause of the failure. PRD 0005 RF4: the core's
 *  access level and summary follow the age, and its explanation, when there is one, is the cause. PRD 0011 RF4.1:
 *  the provider's public status follows, while status queries are on. */
export function detailHtml(r, { eye = false, action = eye } = {}) {
  const notes = r.notes.map((_, index) => noteText(r, index));
  // PRD 0022 RF10.1 (DS-CMP-DLG-1): the incident first, the best-account advice, then the limits, then the reading
  // (age and access level).
  const incident = `${incidentDetailHtml(r)}${adviceHtml(r, { action })}`;
  const reading = `<div class="det-reading"><div class="det-age">${ageHtml(r)}</div>${accessHtml(r)}</div>`;
  const rest = restNotes(r).map((n) => `<p class="note">${esc(n)}</p>`).join("");
  if (isStaleReading(r)) {
    const s = staleView(r);
    const metrics = eye ? s.metrics : s.metrics.filter((m) => !isMetricHidden(s, m));
    return `<div class="detail">${incident}<div class="call warning cause">${icon("clock")}<div>${esc(causeText(r))}</div></div>
      <div class="detail stale-reading">${metrics.map((m) => metricCardHtml(s, m, { eye, stale: true })).join("")}</div>${reading}${rest}</div>`;
  }
  if (!r.metrics.length) {
    const tone = r.status === "authentication_error" ? "accent" : r.status === "ready" ? "neutral" : "critical";
    return `<div class="detail">${incident}<div class="call ${tone}">${icon(r.status === "authentication_error" ? "key" : r.status === "ready" ? "info" : "alert")}<div>${esc(causeText(r))}</div></div>${reading}${rest}</div>`;
  }
  const metrics = eye ? r.metrics : r.metrics.filter((m) => !isMetricHidden(r, m));
  // PRD 0043 RF2.2: the limit changes of the last 48 h, as silent notes.
  const changes = (r.limit_notes ?? []).map((n) => `<p class="note limit-note" data-limit-note>${icon("info")}<span>${esc(tl(n))}</span></p>`).join("");
  return `<div class="detail">${incident}${overageDetailHtml(r)}${scopeBlocksHtml(r)}${changes}${metrics.map((m) => metricCardHtml(r, m, { eye })).join("")}${breakdownHtml(r, { action: eye })}${reading}${notes.map((n) => `<p class="note">${esc(n)}</p>`).join("")}</div>`;
}

// ---------- modelos (PRD 0002 RF6) ----------
// DS-MSG-8: the shared time format (time today, weekday within 6 days, otherwise day and month).
const when = (iso) => fmtAt(iso) ?? "";
/** Displayable catalog: the current one (`ready`) or the previous one preserved on failure/query/interruption. */
const catalogOf = (s) => (s?.state === "ready" ? s : s?.previous ?? null);
const stateText = (s) => {
  switch (s?.state) {
    case "not_fetched": return t("models.notFetched");
    case "ready": return s.models.length ? t("models.count", { count: s.models.length }) : t("models.empty");
    case "failed": return t("models.state.failed", { status: statusLabel(s.status), error: tl(s.error), when: when(s.at) });
    case "fetching": return tl(s.note);
    case "interrupted": return t("models.state.interrupted", { note: tl(s.note), when: when(s.at) });
    case "unsupported": return tl(s.reason);
    default: return "";
  }
};
/** DS-COL-3: the role of a status line from the core's classification (`state`, `status`), with its icon; `null` when neutral. */
const stateTone = (s) => {
  if (s?.state === "interrupted") return ["warn", "clock"];
  if (s?.state !== "failed") return null;
  if (s.status === "authentication_error") return ["accent", "key"];
  if (s.status === "rate_limited") return ["warn", "clock"];
  return ["bad", "alert"];
};

/** What the models section shows for core `state` (pure, no DOM): the status line and its DS-COL-3 `tone`
 *  (`[role, icon]` or `null`), the meta line, whether the bar and the search field are hidden, and the models matching
 *  `query` (`noMatch` when a search hides them all). `progress`/`error` are the running-query note and an IPC error. */
export function modelsView(state, { query = "", gone = false, progress = null, error = null } = {}) {
  const cat = gone ? null : catalogOf(state);
  const models = cat?.models ?? [];
  const q = fold(query.trim());
  const shown = models.filter((m) => !q || fold(m.id).includes(q) || (m.name != null && fold(m.name).includes(q)));
  const source = cat && { source: tl(cat.source), when: when(cat.fetched_at) };
  return {
    status: gone ? t("models.gone") : progress ?? error ?? stateText(state),
    tone: gone || progress ? null : error ? ["bad", "alert"] : stateTone(state),
    meta: !cat ? "" : q ? t("models.meta.filtered", { ...source, shown: shown.length, count: models.length }) : t("models.meta", source),
    barHidden: gone || state?.state === "unsupported",
    searchHidden: !models.length,
    shown,
    noMatch: Boolean(q && models.length && !shown.length),
  };
}

/** Searchable models section of account `key` (`provider::name`) at index `index` of configuration `generation`.
 *  The core cache is the source (nothing in localStorage). The first query only happens when opening the section in state
 *  `not_fetched`; another one only through "Sync models". A result from another identity (old generation/index) is
 *  discarded. The host calls `update` on each view, `focused()` before redrawing and `attach(root, focus)` afterwards. */
export function mountModels({ key, index, generation }) {
  const el = document.createElement("details");
  el.className = "models";
  el.innerHTML = `<summary>${te("models.title")}</summary>
    <div class="models-bar">
      <label class="models-search"><span>${te("models.search")}</span><input type="search" autocomplete="off" spellcheck="false" /></label>
      <button type="button" class="models-sync">${icon("refresh")}${te("models.sync")}</button>
    </div>
    <p class="models-status" role="status" aria-live="polite"></p>
    <p class="sub models-meta"></p>
    <ul class="models-list" aria-label="${te("models.title")}"></ul>`;
  const [search, sync, status, meta, list] =
    ["input", ".models-sync", ".models-status", ".models-meta", ".models-list"].map((s) => el.querySelector(s));
  // `epoch` changes with every identity switch: responses from an earlier epoch are ignored.
  let state = null, error = null, progress = null, busy = false, gone = false, epoch = 0;

  function render() {
    const v = modelsView(state, { query: search.value, gone, progress, error });
    el.querySelector(".models-bar").hidden = v.barHidden;
    search.closest("label").hidden = v.searchHidden;
    sync.disabled = busy;
    if (v.tone) status.dataset.tone = v.tone[0];
    else delete status.dataset.tone;
    status.innerHTML = v.tone ? `${icon(v.tone[1])}<span>${esc(v.status)}</span>` : esc(v.status);
    meta.textContent = v.meta;
    list.innerHTML = v.noMatch
      ? `<li class="sub">${te("models.noMatch")}</li>`
      : v.shown.map((m, i) => `<li tabindex="${i ? -1 : 0}"><code>${esc(m.id)}</code>${m.name && m.name !== m.id ? ` <span class="sub">${esc(m.name)}</span>` : ""}</li>`).join("");
  }

  /** A call to the backend; `note` marks a long operation (announced, button disabled). */
  async function call(fn, note) {
    const e = epoch;
    if (note) { busy = true; progress = note; render(); }
    try {
      const s = await fn(index, generation);
      if (e !== epoch) return null;
      state = s;
      error = null;
      return s;
    } catch (x) {
      if (e !== epoch) return null;
      if (x === STALE) resync();
      else error = errText(x); // the displayed catalog stays
      return null;
    } finally {
      if (e === epoch) {
        if (note) { busy = false; progress = null; }
        render();
      }
    }
  }
  /** Re-reads the state without querying; a query already in progress (another window/process) is awaited, not repeated. */
  async function refresh(first) {
    const s = await call(modelState);
    if (s?.state === "fetching") await call(loadModels, tl(s.note));
    else if (first && s?.state === "not_fetched") await call(loadModels, t("models.loading"));
  }
  function update(i, g) {
    if (i === index && g === generation && !gone) return;
    index = i; generation = g; epoch++;
    state = error = progress = null; busy = gone = false;
    if (el.open) refresh(false);
    else render();
  }
  /** The configuration changed before the UI saw the new view: find the same account (or note that it vanished). */
  async function resync() {
    const v = await getView().catch(() => null);
    const r = v?.reports.find((x) => reportKey(x) === key);
    if (r) return update(r.index, v.generation);
    epoch++;
    state = progress = null; busy = false; gone = true;
    render();
  }

  el.addEventListener("toggle", () => { if (el.open && !busy && !gone) refresh(true); });
  sync.addEventListener("click", () => { if (!busy) call(syncModels, t("models.syncing")); });
  search.addEventListener("input", render);
  search.addEventListener("keydown", (e) => {
    if (e.key === "Enter") e.preventDefault(); // inside the account form, Enter does not save
    if (e.key === "ArrowDown") { e.preventDefault(); list.querySelector("li[tabindex]")?.focus(); }
  });
  list.addEventListener("keydown", (e) => {
    const items = [...list.querySelectorAll("li[tabindex]")];
    const i = items.indexOf(document.activeElement);
    if (i < 0) return;
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); return search.focus(); }
    const go = { ArrowDown: i + 1, ArrowUp: i - 1, Home: 0, End: items.length - 1 }[e.key];
    if (go === undefined) return;
    e.preventDefault();
    if (go < 0) return search.closest("label").hidden ? undefined : search.focus();
    items[Math.min(go, items.length - 1)].focus();
  });
  render();
  return {
    key, el, update,
    focused: () => (el.contains(document.activeElement) ? document.activeElement : null),
    attach(root, focus) { root.querySelector("[data-models-slot]")?.append(el); focus?.focus(); },
    dispose() { epoch++; el.remove(); },
  };
}
