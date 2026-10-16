// Detalhe de uma conta: mesmo componente no popover e no diálogo da janela principal.
// Critério único: percentual DISPONÍVEL, barra = disponível, quantidade = usado / limite; métricas sem limite viram texto.
import { te } from "./i18n.js";
import { esc, fmtAmount, fmtReset, icon, isMetricHidden, level, metricKey, noteText, remainingPct, statusLabel, textualMetric } from "./lib.js";

/** `eye`: mostra o botão de ocultar/exibir a quota (só no diálogo). */
export function metricCardHtml(r, m, { eye = false } = {}) {
  const pct = remainingPct(m);
  const lv = level(pct);
  const text = textualMetric(m);
  const reset = fmtReset(m.resets_at);
  const hid = isMetricHidden(r, m);
  const gauge = text != null
    ? `<div class="balance ${m.state === "exhausted" ? "bad" : ""}">${esc(text)}</div>`
    : `<div class="bar ${lv}"><span style="width:${pct ?? 0}%"></span></div>`;
  return `<div class="metric ${hid ? "muted" : ""}">
    <div class="metric-top">
      <div><h3>${esc(m.label)}</h3><span class="sub">${esc(fmtAmount(m))}</span></div>
      <div class="metric-right">
        ${text == null && pct != null ? `<span class="pct ${lv}" title="${te(`metric.level.${lv}`, { pct: Math.round(pct) })}">${icon(lv === "good" ? "check" : "warn")}${Math.round(pct)}%<small>${te("card.available")}</small></span>` : ""}
        ${eye ? `<button class="icon" data-hide="${esc(metricKey(r, m))}" title="${te(hid ? "common.show" : "common.hide")}" aria-label="${te(hid ? "common.show" : "common.hide")}">${icon(hid ? "eye" : "eyeOff")}</button>` : ""}
      </div>
    </div>
    ${gauge}
    ${reset ? `<div class="resets">${te("metric.resetsAt", { when: reset })}</div>` : ""}
  </div>`;
}

/** Lista de quotas + notas. Sem `eye` (popover), as ocultas não aparecem. */
export function detailHtml(r, { eye = false } = {}) {
  const notes = r.notes.map((_, index) => noteText(r, index));
  if (!r.metrics.length)
    return `<div class="detail"><div class="metric"><p class="sub">${esc(notes[0] ?? statusLabel(r.status))}</p></div>${notes.slice(1).map((n) => `<p class="note">${esc(n)}</p>`).join("")}</div>`;
  const metrics = eye ? r.metrics : r.metrics.filter((m) => !isMetricHidden(r, m));
  return `<div class="detail">${metrics.map((m) => metricCardHtml(r, m, { eye })).join("")}${notes.map((n) => `<p class="note">${esc(n)}</p>`).join("")}</div>`;
}
