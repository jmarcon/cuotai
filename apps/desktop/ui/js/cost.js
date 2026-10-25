// PRD 0015 RF4.1: this computer's local usage of the command-line tools (the core's `CostSummary`, from `cost_usage`).
// A "Local usage" block in the same place on every card of a source's tool (today, yesterday and the saved period),
// a card of its own while no account of the tool is configured, and in the details the breakdown by day (a bar chart
// with its equivalent table) and by model. Everything is an estimate at API prices in USD, never a charge, and a
// model without a price reads "cost unknown", never zero. A source that is off shows nothing on the cards and, in
// the details, why and where to turn it on.
import { currentLocale, t, te, tl, tle } from "./i18n.js";
import { esc, fmtDate, icon, logo } from "./lib.js";
import { showDialog } from "./motion.js";

const sum = (tokens) => tokens.input + tokens.output + tokens.cache_read + tokens.cache_write + (tokens.cache_write_1h ?? 0);
/** PRD 0030 RF5: the token kinds of `tokens`, for a tooltip; 5-minute and 1-hour cache writes apart. */
const breakdown = (tokens) => t("cost.tokens.breakdown", { input: fmtTokens(tokens.input), output: fmtTokens(tokens.output), cache_read: fmtTokens(tokens.cache_read), cache_write: fmtTokens(tokens.cache_write), cache_write_1h: fmtTokens(tokens.cache_write_1h ?? 0) });
const usd = (amount) => new Intl.NumberFormat(currentLocale(), { style: "currency", currency: "USD" }).format(amount);

/** "$3.40"; a positive amount below a cent reads "< $0.01", never free. */
export const fmtUsd = (amount) => (amount > 0 && amount < 0.005 ? `< ${usd(0.01)}` : usd(amount));
/** "1.2M" tokens, in the current language. */
export const fmtTokens = (count) => new Intl.NumberFormat(currentLocale(), { notation: "compact", maximumFractionDigits: 1 }).format(count);

/** PRD 0030 RF6: `amount` (USD) in the display currency the person typed, at their rate; `null` without one. Display
 *  only: totals stay in USD and nothing converted is stored or exported. */
export function convertedText(amount, display) {
  if (!display || amount == null) return null;
  const value = Math.round(amount * display.rate * 100) / 100;
  try { return new Intl.NumberFormat(currentLocale(), { style: "currency", currency: display.code }).format(value); } catch { return `${display.code} ${value.toFixed(2)}`; }
}

/** RF6.2: "≈ R$ 1.149,00 at your rate (set 12 days ago)", plus "update your rate" from 30 days (the core's text). */
function displayLine(amount, display) {
  const shown = convertedText(amount, display);
  if (shown == null) return "";
  const stale = display.stale;
  return `<p class="lc-fx" data-cost-display>${tle({ id: stale ? "cost.display.line_stale" : "cost.display.line", params: { amount: shown, days: display.age_days },
    fallback: `≈ ${shown} no seu câmbio (definido há ${display.age_days} dias)${stale ? "; atualize o câmbio" : ""}` })}</p>`;
}

/** RF1: the plan lines of a source's accounts (the core's text: always "estimate at API prices"); nothing without a plan. */
const planHtml = (sc, summary) => (summary.plans ?? []).filter((v) => v.source === sc.source)
  .map((v) => `<p class="lc-plan" data-cost-plan>${tle(v.text)}</p>`).join("");

/** RF1.4: no plan price yet for an account of this tool: a link to where it is typed, nothing more. */
const planAddHtml = (sc, summary) => ((summary.planAccounts ?? []).some((a) => a.source === sc.source) && !(summary.planAccounts ?? []).some((a) => a.source === sc.source && a.plan)
  ? `<div class="dlg-actions left"><button class="btn" data-cost-settings data-cost-plan-add>${icon("gear")} ${te("cost.plan.add")}</button></div>` : "");

/** The cost of `e` (an estimate or a day): the priced part, what has no price, or that there was no usage. */
export function costText(e) {
  if (!sum(e.tokens)) return t("cost.noUsage");
  if (e.total === undefined ? e.cost == null : e.total == null) return t("cost.unknown");
  const amount = fmtUsd(e.total === undefined ? e.cost : e.total);
  return sum(e.unknown_tokens) ? t("cost.partial", { cost: amount }) : amount;
}

/** RF4.1: the source the accounts of `provider` show, if the summary has one. */
export const sourceFor = (summary, provider) => summary?.sources.find((s) => s.providers.includes(provider)) ?? null;

const TONES = { partial: "warn", no_logs: "warn", no_access: "bad" };
const noteHtml = (sc) => (sc.note ? `<p class="lc-note ${TONES[sc.state] ?? ""}" data-cost-note>${esc(tl(sc.note))}</p>` : "");

const tile = (label, e, display) => `<div class="lc-tile"><span class="lc-label">${esc(label)}</span>
  <b class="lc-cost${e.total == null && sum(e.tokens) ? " dim" : ""}">${esc(costText(e))}</b>${display && e.total != null ? `<span class="lc-fx-amount">≈ ${esc(convertedText(e.total, display))}</span>` : ""}
  <span class="lc-tokens">${te("cost.tokens", { amount: fmtTokens(sum(e.tokens)) })}</span></div>`;

const tilesHtml = (sc, summary) => `<div class="lc-tiles">${tile(t("cost.today"), sc.usage.today, summary.display)}${tile(t("cost.yesterday"), sc.usage.yesterday, summary.display)}${tile(tl(summary.span.label), sc.usage.period, summary.display)}</div>${displayLine(sc.usage.period.total, summary.display)}${planHtml(sc, summary)}`;

/** The block inside an account card; nothing while the source is off. */
export function costCardHtml(sc, summary) {
  if (!sc?.usage) return "";
  return `<div class="local-cost" data-local-cost="${esc(sc.source)}">
    <div class="lc-head">${icon("monitor")}<span class="lc-title">${te("cost.card.title", { source: sc.label })}</span>
      <button class="icon" data-cost-details="${esc(sc.source)}" title="${te("cost.card.details")}" aria-label="${te("cost.card.details")}">${icon("search")}</button></div>
    ${noteHtml(sc)}${tilesHtml(sc, summary)}
    <p class="lc-foot">${te("cost.card.estimate")}</p></div>`;
}

/** A card of its own for a source that is on while no account of its tool is configured. */
export function costSoloCardHtml(sc, summary) {
  return `<section class="card local-cost-card" data-card="cost:${esc(sc.source)}">
    <div class="card-head">${logo(sc.providers[0])}
      <div class="who"><h2>${te("cost.solo.title")}</h2><p>${esc(sc.label)}</p></div></div>
    <div class="summary">${costCardHtml(sc, summary)}</div></section>`;
}

// The day table stays open across the redraws of the dialog that holds it.
let tableOpen = false;
document.addEventListener("toggle", (e) => { if (e.target.matches?.(".lc-table")) tableOpen = e.target.open; }, true);

/** The bar chart of the days: the priced cost per day, or the tokens when nothing is priced. Assistive technology
 *  reads it as one image with its label (the days and the peak) and then the equivalent table. */
function chartHtml(sc, summary) {
  const days = sc.usage.days;
  const date = (day) => fmtDate(day) ?? "";
  const priced = days.some((d) => d.cost > 0);
  const value = (d) => (priced ? d.cost ?? 0 : sum(d.tokens));
  const top = Math.max(...days.map(value), 0);
  const peak = days.reduce((best, d) => (value(d) > value(best) ? d : best), days[0]);
  const shown = (d) => (priced ? fmtUsd(value(d)) : t("cost.tokens", { amount: fmtTokens(value(d)) }));
  const label = t(priced ? "cost.detail.chartCost" : "cost.detail.chartTokens", {
    first: date(days[0].day), last: date(days.at(-1).day),
    peak: priced ? fmtUsd(value(peak)) : t("cost.tokens", { amount: fmtTokens(value(peak)) }),
    day: date(peak.day),
  });
  const bars = days.map((d) => `<span class="lc-bar${d.cost == null && sum(d.tokens) ? " unknown" : ""}" style="height:${top ? Math.max((value(d) / top) * 100, value(d) ? 2 : 0).toFixed(1) : 0}%" title="${esc(`${date(d.day)}: ${shown(d)}`)}"></span>`).join("");
  const rows = days.map((d) => `<tr><th scope="row">${esc(date(d.day))}</th><td title="${esc(breakdown(d.tokens))}">${esc(fmtTokens(sum(d.tokens)))}</td><td>${esc(String(d.requests))}</td><td>${esc(costText(d))}</td></tr>`).join("");
  return `<figure class="lc-chart"><figcaption>${te("cost.detail.byDay", { period: tl(summary.span.label) })}</figcaption>
    <div class="lc-bars" role="img" aria-label="${esc(label)}">${bars}</div>
    <div class="lc-axis sub" aria-hidden="true"><span>${esc(date(days[0].day))}</span><span>${esc(date(days.at(-1).day))}</span></div>
    <details class="lc-table" ${tableOpen ? "open" : ""}><summary>${te("cost.detail.table")}</summary>
      <table><caption class="sr-only">${esc(label)}</caption><thead><tr><th scope="col">${te("cost.col.day")}</th><th scope="col">${te("cost.col.tokens")}</th>
        <th scope="col">${te("cost.col.requests")}</th><th scope="col">${te("cost.col.cost")}</th></tr></thead><tbody>${rows}</tbody></table></details></figure>`;
}

function modelsHtml(sc, summary) {
  const models = sc.usage.period.models;
  if (!models.length) return "";
  const rows = models.map((m) => `<tr><th scope="row"><code>${esc(m.model)}</code></th><td title="${esc(breakdown(m.tokens))}">${esc(fmtTokens(sum(m.tokens)))}</td><td>${esc(String(m.requests))}</td>
    <td class="${m.cost == null ? "dim" : ""}">${esc(m.cost == null ? t("cost.unknown") : fmtUsd(m.cost))}</td>
    <td>${m.price ? te(`cost.origin.${m.price.origin}`) : "—"}</td></tr>`).join("");
  return `<table class="lc-models"><caption>${te("cost.detail.byModel", { period: tl(summary.span.label) })}</caption>
    <thead><tr><th scope="col">${te("cost.col.model")}</th><th scope="col">${te("cost.col.tokens")}</th><th scope="col">${te("cost.col.requests")}</th>
      <th scope="col">${te("cost.col.cost")}</th><th scope="col">${te("cost.col.price")}</th></tr></thead><tbody>${rows}</tbody></table>`;
}

/** RF4.1: the details' section of `sc`: why there are no numbers, today, yesterday and the period, by day and by
 *  model, the prices' date and source; while off, why nothing is shown and where to turn it on. */
export function costDetailHtml(sc, summary) {
  const head = `<h3>${icon("monitor")} ${te("cost.detail.title", { source: sc.label })}</h3>`;
  if (!sc.usage) {
    return `<section class="lc-detail" data-cost-detail="${esc(sc.source)}">${head}
      <p class="sub">${te("cost.detail.off", { source: sc.label })}</p>
      <div class="dlg-actions left"><button class="btn" data-cost-settings>${icon("gear")} ${te("cost.detail.openSettings")}</button></div></section>`;
  }
  const tables = sc.usage.period.tables.map((p) => te("cost.detail.prices", { date: fmtDate(p.effective) ?? "", origin: t(`cost.origin.${p.origin}`) }) + (p.source ? ` <span class="sub">${esc(p.source)}</span>` : "")).join("<br/>");
  return `<section class="lc-detail" data-cost-detail="${esc(sc.source)}">${head}
    <p class="sub">${te("cost.detail.help", { source: sc.label })}</p>
    ${noteHtml(sc)}${tilesHtml(sc, summary)}
    ${planAddHtml(sc, summary)}
    ${sc.usage.days.length ? chartHtml(sc, summary) : ""}${modelsHtml(sc, summary)}
    ${summary.pricing_error ? `<p class="lc-note warn">${esc(tl(summary.pricing_error))}</p>` : ""}
    ${summary.unpriced ? `<p class="lc-note warn" data-cost-unpriced>${esc(tl(summary.unpriced))}</p>` : ""}
    <p class="lc-foot">${esc(tl(summary.notice))}${summary.prices_age ? `<br/><span data-cost-prices-age>${esc(tl(summary.prices_age))}</span>` : ""}${tables ? `<br/>${tables}` : ""}</p></section>`;
}

let dlg = null;
/** The details of a source without an account of its tool (from its own card). `ctx`: `{ summary(), onSettings() }`. */
export function openCostDetails(source, ctx, from = null) {
  const paint = () => {
    const sc = ctx.summary()?.sources.find((s) => s.source === source);
    if (!sc) return dlg.close();
    dlg.innerHTML = `<div class="dlg-body">${costDetailHtml(sc, ctx.summary())}
      <div class="dlg-actions"><button class="btn primary" data-close>${te("common.close")}</button></div></div>`;
  };
  if (!dlg) {
    dlg = document.createElement("dialog");
    dlg.className = "dlg details";
    dlg.setAttribute("aria-label", t("cost.solo.title"));
    document.body.append(dlg);
    dlg.addEventListener("close", () => { dlg.remove(); dlg = null; });
    dlg.addEventListener("click", (e) => {
      if (e.target.closest("[data-close]")) return dlg.close();
      if (e.target.closest("[data-cost-settings]")) { dlg.close(); ctx.onSettings(); }
    });
    dlg.refresh = paint;
    paint();
    showDialog(dlg, from);
    return;
  }
  paint();
}

/** Repaints the open source details when the summary changes. */
export function refreshCostDetails() {
  if (dlg?.open) dlg.refresh();
}
