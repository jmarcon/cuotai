// PRD 0045 RF6.1: the details' Breakdown section and the card's "Top: …" line. The backend sends the rows in the
// core's spend order, the core's percentage order (`by_percent`), the card line and the truncation note; this module
// only lays them out (DS-CMP-SUB rows: label, meter when the row has a cap, its amount otherwise, reset chip).
import { t, te, tl } from "./i18n.js";
import { esc, fmtAmount, fmtReset, fmtShare, remainingPct, textualMetric } from "./lib.js";
import { expectedLeft, meterHtml } from "./components.js";

/** The order the person chose in this window ("spend" or "percent"); kept across redraws. */
let sortBy = "spend";
export const breakdownSort = () => sortBy;

/** The rows of `b` in the chosen order, as indices into `b.rows`. */
export function orderOf(b, by = sortBy) {
  return by === "percent" && Array.isArray(b?.by_percent) && b.by_percent.length === b.rows.length
    ? b.by_percent
    : b.rows.map((_, i) => i);
}

/** One DS-CMP-SUB row. A row with a cap draws the meter of its remaining share; any other row its amount. */
function rowHtml(row, so, po, i) {
  const m = row.figure;
  const label = tl(row.label);
  const pct = m ? remainingPct(m) : null;
  const text = m ? textualMetric(m) : null;
  const amount = m ? fmtAmount(m) : "—";
  const state = m?.state === "exhausted" ? "exhausted" : pct == null ? "unknown" : "fresh";
  const figure = m && text == null && pct != null
    ? `<span class="pct">${fmtShare(pct)}<small>${te("card.available")}</small></span>`
    : `<span class="brk-amount">${esc(text ?? amount)}</span>`;
  const gauge = m && text == null && pct != null
    ? `<div class="gauge">${meterHtml(pct, { state, expected: expectedLeft(m), text: t("meter.left", { pct: Math.round(pct) }), name: label })}</div>`
    : "";
  const reset = m && fmtReset(m.resets_at) ? (m.resets_label ? tl(m.resets_label) : t("metric.resetsAt", { when: fmtReset(m.resets_at) })) : "";
  const tip = row.metrics.map((x) => `${tl(x.label_text ?? x.label)}: ${fmtAmount(x)}`).join("\n");
  const notes = row.notes.map((n) => `<span class="brk-note">${esc(tl(n))}</span>`).join("");
  // PRD 0045 RF5.2: a budget on this row, in the account editor (details dialog only).
  const budget = row.budget_draft ? `<div class="metric-actions"><button type="button" class="btn sm" data-set-row-budget="${i}">${te("budget.action.set_budget")}</button></div>` : "";
  return `<div class="metric brk-row${row.others ? " others" : ""}" data-breakdown-row="${esc(row.id)}" data-so="${so}" data-po="${po}" title="${esc(tip)}">
    <div class="metric-top"><h3>${esc(label)}</h3><div class="metric-right">${figure}</div></div>
    ${gauge}
    ${reset || notes ? `<div class="metric-meta">${notes || "<span></span>"}${reset ? `<span class="resets">${esc(reset)}</span>` : ""}</div>` : ""}
    ${budget}
  </div>`;
}

/** The details' Breakdown section of report `r`; "" without rows. `action` (the details dialog) adds the
 *  "Alert on breakdown rows" switch (PRD 0045 RF5.2) and the rows' "Set a budget…". */
export function breakdownHtml(r, { action = false } = {}) {
  const b = r?.breakdown;
  if (!b?.rows?.length) return "";
  const percent = orderOf(b, "percent");
  const rows = orderOf(b).map((i) => rowHtml(action ? b.rows[i] : { ...b.rows[i], budget_draft: null }, i, percent.indexOf(i), i)).join("");
  const alerts = action
    ? `<label class="check"><input type="checkbox" data-breakdown-alerts="${esc(r.account_key)}" ${b.alert_rows ? "checked" : ""} /> ${te("breakdown.alerts")}</label>
    <p class="hint">${te("breakdown.alertsHelp")}</p>`
    : "";
  const sort = ["spend", "percent"].map((by) =>
    `<button type="button" data-breakdown-sort="${by}" aria-pressed="${by === sortBy}">${te(`breakdown.sort.${by}`)}</button>`).join("");
  return `<section class="breakdown" data-breakdown aria-label="${te("breakdown.title")}">
    <div class="brk-head"><h3>${te("breakdown.title")}</h3><div class="seg" role="group" aria-label="${te("breakdown.sort.label")}">${sort}</div></div>
    <div class="brk-rows">${rows}</div>
    ${b.truncated ? `<p class="sub brk-truncated">${esc(tl(b.truncated))}</p>` : ""}
    ${alerts}
  </section>`;
}

/** Re-sorts the rows of `section` in place for `by` (each row carries its position in both orders), so the DOM
 *  order stays the reading order. */
function resort(section, by) {
  const list = section.querySelector(".brk-rows");
  if (!list) return;
  const key = by === "percent" ? "po" : "so";
  const nodes = [...list.children].sort((a, b) => Number(a.dataset[key]) - Number(b.dataset[key]));
  for (const node of nodes) list.append(node);
  for (const button of section.querySelectorAll("[data-breakdown-sort]")) button.setAttribute("aria-pressed", String(button.dataset.breakdownSort === by));
}

/** One listener per window for the sort toggle of any Breakdown section. */
globalThis.document?.addEventListener?.("click", (e) => {
  const button = e.target?.closest?.("[data-breakdown-sort]");
  const section = button?.closest("[data-breakdown]");
  if (!section) return;
  sortBy = button.dataset.breakdownSort === "percent" ? "percent" : "spend";
  resort(section, sortBy);
});
