// Detalhe de uma conta: mesmo componente no popover e no diálogo da janela principal.
// Critério único: percentual DISPONÍVEL, barra = disponível, quantidade = usado / limite; métricas sem limite viram texto.
import { currentLocale, t, te, tl } from "./i18n.js";
import {
  STALE, accessHtml, ageHtml, bindingChipText, causeText, errText, esc, fmtAmount, fmtReset, getView, icon, incidentDetailHtml, isBindingMetric, isMetricHidden, isRenewedWaiting, isStaleReading, level, loadModels, metricAlertHtml, metricKey, modelState, noteText,
  adviceHtml, fmtMetricExtra, forecastHtml, scopeBlocksHtml, isOverage, overageDetailHtml, paceHtml, paceOf, remainingPct, renewedText, reportKey, restNotes, staleView, statusLabel, syncModels, textualMetric,
} from "./lib.js";
import { expectedLeft, meterHtml } from "./components.js";
import { fold } from "./util.js";

/** Fa gap 8: the same localized limit names as the main-window cards ("5-hour session", "Weekly"). */
const LONG_WINDOW = { window_5h: "h5", weekly: "week" };
const limitLabel = (m) => (LONG_WINDOW[m.window] ? t(`card.window.long.${LONG_WINDOW[m.window]}`) : m.label);

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
    : `<div class="gauge" title="${pace ? te("card.pace.estimate") : ""}">${meterHtml(pct, { state, expected: expectedLeft(m), text: valueText, name: m.label })}</div>`;
  // PRD 0009 RF2.2: the core's binding metric gets the "Binding" tag; every other metric stays listed.
  const binding = isBindingMetric(r, m);
  // DS-CMP-SUB-1 (mockup 03): label + tag, the figure with its unit inline; the meter; the pace stage on the left and
  // "Resets 20:32 · in 2h 11m" (mono, muted) on the right. Counts and the expected share live in the tooltips.
  const paceTip = pace ? [tl(pace.label), t("metric.pace.expected", { pct: Math.round(pace.expected_used_fraction * 100) }), pace.outlook ? tl(pace.outlook) : "", t("card.pace.estimate")].filter(Boolean).join(". ") : "";
  const resetText = reset && !renewed ? (m.resets_label ? tl(m.resets_label) : t("metric.resetsAt", { when: reset })) : "";
  return `<div class="metric ${hid ? "muted" : ""} ${binding ? "binding" : ""} ${m.alert?.active ? "usage-alert" : ""}" title="${esc(fmtAmount(m))}">
    <div class="metric-top">
      <h3>${esc(limitLabel(m))}${binding ? ` <span class="btag" data-binding-chip title="${esc(`${t("card.binding.title")}: ${bindingChipText(r)}`)}">${te("binding.tag")}</span>` : ""}</h3>
      <div class="metric-right">
        ${text == null && pct != null ? `<span class="pct" title="${te(`metric.level.${lv}`, { pct: Math.round(pct) })}">${Math.round(pct)}%<small>${te("card.available")}</small></span>` : ""}
        ${eye ? metricAlertHtml(r, m) : ""}
        ${eye ? `<button class="icon" data-hide="${esc(metricKey(r, m))}" title="${te(hid ? "common.show" : "common.hide")}" aria-label="${te(hid ? "common.show" : "common.hide")}">${icon(hid ? "eye" : "eyeOff")}</button>` : ""}
      </div>
    </div>
    ${gauge}
    ${pace || resetText ? `<div class="metric-meta">${pace ? `<span class="pace-line" data-pace="${esc(pace.stage)}" title="${esc(paceTip)}">${icon({ good: "check", neutral: "clock", warn: "warn", bad: "xcircle" }[pace.tone] ?? "clock", pace.tone)}<span>${esc(tl(pace.label))}</span></span>` : "<span></span>"}${resetText ? `<span class="resets">${esc(resetText)}</span>` : ""}</div>` : ""}
    ${stale ? "" : forecastHtml(m, r)}
    ${m.alert?.missed ? `<div class="resets missed" data-missed>${icon("clock")}<span>${te("alerts.missed", { percent: m.alert.missed })}</span></div>` : ""}
  </div>`;
}

/** Lista de quotas + notas. Sem `eye` (popover), as ocultas não aparecem. PRD 0010 RF3: the age leads; a failure
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
  return `<div class="detail">${incident}${overageDetailHtml(r)}${scopeBlocksHtml(r)}${metrics.map((m) => metricCardHtml(r, m, { eye })).join("")}${reading}${notes.map((n) => `<p class="note">${esc(n)}</p>`).join("")}</div>`;
}

// ---------- modelos (PRD 0002 RF6) ----------
const when = (iso) => {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleString(currentLocale(), { dateStyle: "short", timeStyle: "short" });
};
/** Catálogo exibível: o atual (`ready`) ou o anterior preservado em falha/consulta/interrupção. */
const catalogOf = (s) => (s?.state === "ready" ? s : s?.previous ?? null);
const stateText = (s) => {
  switch (s?.state) {
    case "not_fetched": return t("models.notFetched");
    case "ready": return s.models.length ? t("models.count", { count: s.models.length }) : t("models.empty");
    case "failed": return `${statusLabel(s.status)} · ${tl(s.error)} · ${t("models.attemptAt", { when: when(s.at) })}`;
    case "fetching": return tl(s.note);
    case "interrupted": return `${tl(s.note)} · ${t("models.attemptAt", { when: when(s.at) })}`;
    case "unsupported": return tl(s.reason);
    default: return "";
  }
};

/** Seção pesquisável de modelos da conta `key` (`provider::nome`) no índice `index` da configuração `generation`.
 *  O cache do core é a fonte (nada em localStorage). A primeira consulta só ocorre ao abrir a seção com o estado
 *  `not_fetched`; outra, só por "Sincronizar modelos". Resultado de outra identidade (geração/índice antigos) é
 *  descartado. O host chama `update` a cada view, `focused()` antes de redesenhar e `attach(root, foco)` depois. */
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
  // `epoch` muda a cada troca de identidade: respostas de uma época anterior são ignoradas.
  let state = null, error = null, progress = null, busy = false, gone = false, epoch = 0;

  function render() {
    const cat = gone ? null : catalogOf(state);
    const models = cat?.models ?? [];
    const q = fold(search.value.trim());
    const shown = models.filter((m) => !q || fold(m.id).includes(q) || (m.name != null && fold(m.name).includes(q)));
    el.querySelector(".models-bar").hidden = gone || state?.state === "unsupported";
    search.closest("label").hidden = !models.length;
    sync.disabled = busy;
    status.textContent = gone ? t("models.gone") : progress ?? error ?? stateText(state);
    meta.textContent = cat
      ? [t("models.source", { source: tl(cat.source) }), t("models.fetchedAt", { when: when(cat.fetched_at) }),
        q ? t("models.shown", { shown: shown.length, count: models.length }) : ""].filter(Boolean).join(" · ")
      : "";
    list.innerHTML = q && models.length && !shown.length
      ? `<li class="sub">${te("models.noMatch")}</li>`
      : shown.map((m, i) => `<li tabindex="${i ? -1 : 0}"><code>${esc(m.id)}</code>${m.name && m.name !== m.id ? ` <span class="sub">${esc(m.name)}</span>` : ""}</li>`).join("");
  }

  /** Uma chamada ao backend; `note` marca operação longa (anunciada, botão desativado). */
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
      else error = errText(x); // o catálogo exibido continua
      return null;
    } finally {
      if (e === epoch) {
        if (note) { busy = false; progress = null; }
        render();
      }
    }
  }
  /** Relê o estado sem consultar; uma consulta já em andamento (outra janela/processo) é aguardada, não repetida. */
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
  /** A configuração mudou antes de a UI ver a nova view: localiza a mesma conta (ou nota que sumiu). */
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
    if (e.key === "Enter") e.preventDefault(); // dentro do formulário de conta, Enter não salva
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
