// The summary of a main-window card (PRD 0022 mockup 10): the 5 h and weekly limit columns, or the tightest quota, an
// uncapped metric's text, a renewed window waiting for its reading or the reason there is none. Kept apart from
// main.js so the card's summary renders (and is tested) without the window.
import { t, te } from "./i18n.js";
import {
  billingOverage, cardPace, causeText, esc, fmtAmount, fmtShare, icon, isBindingMetric, metricName, paceHtml, paceLineLabel, paceOf, renewedText, renewedWindowsOf,
  resetsText, summaryOf, windowsOf,
} from "./lib.js";
import { expectedLeft, extraColumnHtml, figureHtml, meterHtml } from "./components.js";

/** PRD 0022 RF2: the card's continuous meter; `stale` draws the preserved reading (no pace mark). */
const cardMeter = (m, pct, stale, name = metricName(m)) =>
  meterHtml(pct, { state: stale ? "stale" : m.state === "exhausted" ? "exhausted" : "fresh", expected: expectedLeft(m), name });
/** One limit of a card (mockup 10): the micro label with the "Binding" tag, the figure with its unit below, the
 *  meter, then "Resets 20:32 · in 2h 11m" once, under the bar. Counts ("15 / 1,000") are in the tooltip. */
function limitColumn(r, label, m, pct, stale) {
  const binding = isBindingMetric(r, m);
  const exhausted = m.state === "exhausted";
  const reset = resetsText(m);
  return `<div class="win${binding ? " binding" : ""}" title="${esc(`${metricName(m)} · ${fmtAmount(m)}`)}">
    <div class="win-top"><span class="win-name">${esc(label)}</span>${binding ? `<span class="btag" data-binding-chip>${te("binding.tag")}</span>` : ""}</div>
    ${exhausted ? figureHtml(t("metric.exhausted"), "", { role: "bad", cls: "left wordfig", dim: stale }) : figureHtml(fmtShare(pct), t("card.available"), { cls: "left", dim: stale })}
    ${cardMeter(m, pct, stale, label)}
    <div class="win-reset">${reset ? esc(reset) : `<em>${te("quota.na")}</em>`}</div></div>`;
}
/** One window of the card (5 h or weekly); a missing window keeps its slot with "—" (DS-CMP-CARD-2). A window that
 *  renewed and waits for its next reading says so in its slot, in text (PRD 0033 RF2.1, re-validation V-01). */
function windowSlot(r, name, w, stale = false, renewed = null) {
  const label = t(`card.window.long.${name}`);
  if (!w && renewed) {
    const text = renewedText(renewed);
    return `<div class="win none renewed" role="group" aria-label="${esc(`${label}: ${text}`)}" data-renewed-waiting><div class="win-top"><span class="win-name">${esc(label)}</span></div>
      <p class="win-renewed">${icon("clock")} <span>${esc(text)}</span></p></div>`;
  }
  if (!w) {
    return `<div class="win none" aria-label="${esc(label)}"><div class="win-top"><span class="win-name">${esc(label)}</span></div>
      ${figureHtml("—", "", { cls: "left" })}${meterHtml(null, { state: "unknown", name: label })}</div>`;
  }
  return limitColumn(r, label, w.m, w.pct, stale);
}

/** Summary: the 5 h and weekly windows (most providers); without them, the tightest quota (or the text of an
 *  uncapped metric, a renewed window waiting for its reading, or the reason there is none). Extra usage, when it
 *  is being billed, takes the second column. */
export function summaryHtml(r, stale = false) {
  const win = windowsOf(r);
  // Fa gap 4: paid extra usage is its own limit column (mockup 10), never a repeated chip.
  const extra = stale ? "" : extraColumnHtml(r);
  if (win.h5 || win.week) {
    const renewed = renewedWindowsOf(r);
    return `<div class="wins${extra ? " three" : ""}">${windowSlot(r, "h5", win.h5, stale, renewed.h5)}${windowSlot(r, "week", win.week, stale, renewed.week)}${extra}</div>`;
  }
  const s = summaryOf(r);
  if (s.kind === "bar") return `<div class="wins">${limitColumn(r, metricName(s.m), s.m, s.pct, stale)}${extra}</div>`;
  if (s.kind === "text") {
    return `<div class="sum-text">
      <div class="win-top"><span class="win-name">${esc(s.m.label)}</span></div>
      <div class="balance ${s.m.state === "exhausted" ? "bad" : ""}">${esc(s.text)}</div>${resetsText(s.m) ? `<div class="win-reset">${esc(resetsText(s.m))}</div>` : ""}</div>`;
  }
  if (s.kind === "hidden") return `<p class="sum-note dim">${te("card.allHidden")}</p>`;
  if (s.kind === "renewed") return `<p class="sum-note" data-renewed-waiting>${icon("clock")} <span>${esc(renewedText(s.m))}</span></p>`;
  return `<div class="sum-none">${figureHtml("—", "", { cls: "left" })}${meterHtml(null, { state: "unknown" })}<p class="sum-note">${esc(causeText(r))}</p></div>`;
}

/** Whether the summary draws the extra-usage column (then the foot leaves its chip out). */
export const hasExtraColumn = (r) => !!billingOverage(r) && (!!(windowsOf(r).h5 || windowsOf(r).week) || summaryOf(r).kind === "bar");

/** PRD 0033 RF4.1 (re-validation V-04): the card's pace line when the core's pace metric is not the binding one, which
 *  the status line already paces (and tags "Binding"): the line names its window ("5h: 8 points in reserve"), so an
 *  exhausted binding week beside a paced 5 h window still shows that pace and whose it is. Empty otherwise. */
export function cardPaceLineHtml(r) {
  const m = cardPace(r);
  if (!m || isBindingMetric(r, m)) return "";
  const binding = r.binding ? r.metrics[r.binding.index] : null;
  if (binding && binding.state !== "exhausted" && paceOf(binding)) return "";
  return paceHtml(paceOf(m), { label: paceLineLabel(r, m) });
}
