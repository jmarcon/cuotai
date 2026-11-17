// PRD 0015 RF4.1: this computer's local usage of the command-line tools (the core's `CostSummary`, from `cost_usage`).
// A "Local usage" block in the same place on every card of a source's tool (today, yesterday and the saved period),
// a card of its own while no account of the tool is configured, and in the details the breakdown by day (a bar chart
// with its equivalent table) and by model. Everything is an estimate at API prices in USD, never a charge, and a
// model without a price reads "cost unknown", never zero. A source that is off shows nothing on the cards and, in
// the details, why and where to turn it on.
import { currentLocale, t, te, tl, tle } from "./i18n.js";
import { esc, fmtDate, fmtMoney, fmtNumber, icon, logo } from "./lib.js";
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
  return fmtMoney(Math.round(amount * display.rate * 100) / 100, display.code);
}

/** RF6.2 with C-07: the core's "≈ R$ 1.149,00 at your rate (set 12 days ago)" of the source's period (plus "update
 *  your rate" from 30 days, "+ unknown" when part has no price); nothing without a rate, usage or a priced amount. */
function displayLine(sc, summary) {
  const text = summary.display_text?.[sc.source];
  return text ? `<p class="lc-fx" data-cost-display>${tle(text)}</p>` : "";
}

/** RF1 with C-05: the plan line of the source (the core's text: always "estimate at API prices"), only on an account
 *  that has a plan; never on an account without one, nor on the card of a source without accounts. */
const planHtml = (sc, summary, accountId) => (accountId ? (summary.plans ?? []) : [])
  .filter((v) => v.source === sc.source && (v.planned_ids ?? []).includes(accountId))
  .map((v) => `<p class="lc-plan" data-cost-plan>${tle(v.text)}</p>`).join("");

/** RF1.4: this account has no plan price yet (whatever the other accounts of the tool have): a link to where it is
 *  typed, nothing more. */
const planAddHtml = (summary, accountId) => (accountId && (summary.planAccounts ?? []).some((a) => a.id === accountId && !a.plan)
  ? `<div class="dlg-actions left"><button class="btn" data-cost-settings data-cost-plan-add>${icon("gear")} ${te("cost.plan.add")}</button></div>` : "");

/** The cost of `e` (an estimate or a day): the priced part, what has no price, or that there was no usage. */
export function costText(e) {
  if (!sum(e.tokens)) return t("cost.noUsage");
  if (e.total === undefined ? e.cost == null : e.total == null) return t("cost.unknown");
  const amount = fmtUsd(e.total === undefined ? e.cost : e.total);
  return sum(e.unknown_tokens) ? t("cost.partial", { cost: amount }) : amount;
}

/** PRD 0022 RF6.1 (DS-CMP-HON-2): `costText` with "≈" before a priced amount (every local cost is an estimate);
 *  "no usage", "cost unknown" and "< $0.01" stay as they are. */
export function approxText(e) {
  const text = costText(e);
  const priced = sum(e.tokens) && (e.total === undefined ? e.cost != null : e.total != null);
  return priced && !text.startsWith("<") ? `≈${text}` : text;
}
const approxUsd = (amount) => (amount > 0 && amount < 0.005 ? fmtUsd(amount) : `≈${fmtUsd(amount)}`);

/** A model name that breaks only after its separators ("-", "/", "."), never inside a word (V-06). */
const modelName = (name) => esc(name).replace(/([-/.])/g, "$1<wbr>");

/** RF4.1: the source the accounts of `provider` show, if the summary has one. */
export const sourceFor = (summary, provider) => summary?.sources.find((s) => s.providers.includes(provider)) ?? null;

const TONES = { partial: "warn", no_logs: "warn", no_access: "bad", unreadable: "bad" };
const noteHtml = (sc) => (sc.note ? `<p class="lc-note ${TONES[sc.state] ?? ""}" data-cost-note>${esc(tl(sc.note))}</p>` : "");

/** C-07: the tile's amount in the second currency: none without usage or a priced amount; "+ unknown" when part of
 *  the usage has no price, like the USD amount above it. */
function convertedTile(e, display) {
  if (!display || !sum(e.tokens) || e.total == null) return "";
  const converted = convertedText(e.total, display);
  return `<span class="lc-fx-amount">≈ ${esc(sum(e.unknown_tokens) ? t("cost.partial", { cost: converted }) : converted)}</span>`;
}

const tile = (label, e, display) => `<div class="lc-tile"><span class="lc-label">${esc(label)}</span>
  <b class="lc-cost${e.total == null && sum(e.tokens) ? " dim" : ""}">${esc(approxText(e))}</b>${convertedTile(e, display)}
  <span class="lc-tokens">${te("cost.tokens", { amount: fmtTokens(sum(e.tokens)) })}</span></div>`;

const tilesHtml = (sc, summary, accountId) => `<div class="lc-tiles">${tile(t("cost.today"), sc.usage.today, summary.display)}${tile(t("cost.yesterday"), sc.usage.yesterday, summary.display)}${tile(tl(summary.span.label), sc.usage.period, summary.display)}</div>${displayLine(sc, summary)}${planHtml(sc, summary, accountId)}`;

/** PRD 0022 (mockups 03, 10): the values of the folded local-cost row: today and the period, with "≈" only before a
 *  priced amount (DS-CMP-HON-2: never before "no usage", "cost unknown" or "< $0.01"); null while the source is off. */
export function costSummary(sc, summary) {
  if (!sc?.usage) return null;
  return { today: approxText(sc.usage.today), period: approxText(sc.usage.period), span: tl(summary.span.label) };
}
/** One muted line for the tray's expanded row: "Local cost · ≈$0.11 today · Last 7 days ≈$1.41 (estimate)". */
export function costLineHtml(sc, summary) {
  const v = costSummary(sc, summary);
  // S-02: an index that cannot be read says so; it never looks like "off".
  if (!v && sc?.state === "unreadable" && sc.note) {
    return `<p class="cost-line bad" data-local-cost="${esc(sc.source)}" data-cost-note>${icon("dollar")}<span>${esc(tl(sc.note))}</span></p>`;
  }
  return v ? `<p class="cost-line" data-local-cost="${esc(sc.source)}">${icon("dollar")}<span>${te("cost.line", v)}</span></p>` : "";
}

/** PRD 0042 M-05: the core's "1 hidden project is not counted in these figures." in a card's or details' muted foot. */
const hiddenNoteHtml = (summary) => (summary?.hidden_note ? `<br/><span data-cost-hidden-note>${esc(tl(summary.hidden_note))}</span>` : "");

/** PRD 0042 RF6.2: "This block ≈ $4.10 · 38K tok/min" (the core's text, with "+ unknown cost of N tokens" for the
 *  part without a price) under the local usage, the projection in the tooltip; nothing while the active block has no
 *  burn rate. The core formats every amount: the desktop adds no money of its own. */
export function liveLineHtml(sc, summary) {
  const live = (summary?.live ?? []).find((l) => l.block.source === sc.source);
  if (!live?.line) return "";
  const tip = [t("cost.live.tip"), live.projection ? tl(live.projection) : ""].filter(Boolean).join(" ");
  return `<p class="lc-live-line" data-cost-live="${esc(sc.source)}" title="${esc(tip)}">${esc(tl(live.line))}</p>`;
}

/** The block inside an account card (`accountId`: the card's account id; none on a source's own card); nothing while
 *  the source is off, only the note while the index cannot be read (S-02). */
export function costCardHtml(sc, summary, accountId = null) {
  if (!sc?.usage) {
    if (sc?.state !== "unreadable") return "";
    return `<div class="local-cost" data-local-cost="${esc(sc.source)}">
    <div class="lc-head">${icon("monitor")}<span class="lc-title">${te("cost.card.title", { source: sc.label })}</span></div>
    ${noteHtml(sc)}</div>`;
  }
  return `<div class="local-cost" data-local-cost="${esc(sc.source)}">
    <div class="lc-head">${icon("monitor")}<span class="lc-title">${te("cost.card.title", { source: sc.label })}</span>
      <button class="icon" data-cost-details="${esc(sc.source)}" title="${te("cost.card.details")}" aria-label="${te("cost.card.details")}">${icon("search")}</button></div>
    ${noteHtml(sc)}${tilesHtml(sc, summary, accountId)}${liveLineHtml(sc, summary)}
    <p class="lc-foot">${te("cost.card.estimate")}${hiddenNoteHtml(summary)}</p></div>`;
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
  const shown = (d) => (priced ? (value(d) ? approxUsd(value(d)) : fmtUsd(0)) : t("cost.tokens", { amount: fmtTokens(value(d)) }));
  const label = t(priced ? "cost.detail.chartCost" : "cost.detail.chartTokens", {
    first: date(days[0].day), last: date(days.at(-1).day),
    peak: priced ? approxUsd(value(peak)) : t("cost.tokens", { amount: fmtTokens(value(peak)) }),
    day: date(peak.day),
  });
  // DS-CHT-3/-5: neutral bars, today in the accent and labelled; a day with use but no price is a hatched bar at its
  // token-proportional height; a day with real zero use is a 1 px baseline tick.
  const now = new Date();
  const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`; // local day, like the core's buckets
  const tokenTop = Math.max(...days.map((d) => sum(d.tokens)), 0);
  const height = (d) => {
    if (priced && d.cost == null && sum(d.tokens)) return tokenTop ? (sum(d.tokens) / tokenTop) * 100 : 0;
    return top ? Math.max((value(d) / top) * 100, value(d) ? 2 : 0) : 0;
  };
  const bars = days.map((d) => {
    const cls = [d.cost == null && sum(d.tokens) ? "unknown" : "", !value(d) && !(d.cost == null && sum(d.tokens)) ? "zero" : "", d.day === today ? "today" : ""].filter(Boolean).join(" ");
    const tip = `${d.day === today ? `${t("cost.today")} · ` : ""}${date(d.day)}: ${d.cost == null && sum(d.tokens) ? t("cost.unknown") : shown(d)}`;
    return `<span class="lc-bar${cls ? ` ${cls}` : ""}" style="height:${height(d).toFixed(1)}%" title="${esc(tip)}"></span>`;
  }).join("");
  const rows = days.map((d) => `<tr><th scope="row">${esc(date(d.day))}</th><td title="${esc(breakdown(d.tokens))}">${esc(fmtTokens(sum(d.tokens)))}</td><td>${esc(fmtNumber(d.requests, 0))}</td><td>${esc(costText(d))}</td></tr>`).join("");
  return `<figure class="lc-chart"><figcaption>${te("cost.detail.byDay", { period: tl(summary.span.label) })}</figcaption>
    <div class="lc-bars" role="img" aria-label="${esc(label)}">${bars}</div>
    <div class="lc-axis sub" aria-hidden="true"><span>${esc(date(days[0].day))}</span><span class="${days.at(-1).day === today ? "today" : ""}">${days.at(-1).day === today ? te("cost.today") : esc(date(days.at(-1).day))}</span></div>
    <details class="lc-table" ${tableOpen ? "open" : ""}><summary>${te("cost.detail.table")}</summary>
      <table><caption class="sr-only">${esc(label)}</caption><thead><tr><th scope="col">${te("cost.col.day")}</th><th scope="col">${te("cost.col.tokens")}</th>
        <th scope="col">${te("cost.col.requests")}</th><th scope="col">${te("cost.col.cost")}</th></tr></thead><tbody>${rows}</tbody></table></details></figure>`;
}

function modelsHtml(sc, summary) {
  const models = sc.usage.period.models;
  if (!models.length) return "";
  const rows = models.map((m) => `<tr><th scope="row"><code>${modelName(m.model)}</code></th><td title="${esc(breakdown(m.tokens))}">${esc(fmtTokens(sum(m.tokens)))}</td><td>${esc(fmtNumber(m.requests, 0))}</td>
    <td class="${m.cost == null ? "dim" : ""}">${esc(m.cost == null ? t("cost.unknown") : fmtUsd(m.cost))}</td>
    <td>${m.price ? te(`cost.origin.${m.price.origin}`) : "—"}</td></tr>`).join("");
  return `<table class="lc-models"><caption>${te("cost.detail.byModel", { period: tl(summary.span.label) })}</caption>
    <thead><tr><th scope="col">${te("cost.col.model")}</th><th scope="col">${te("cost.col.tokens")}</th><th scope="col">${te("cost.col.requests")}</th>
      <th scope="col">${te("cost.col.cost")}</th><th scope="col">${te("cost.col.price")}</th></tr></thead><tbody>${rows}</tbody></table>`;
}

/** C-02: the core's notes that saved plans or the second currency were invalid and are ignored. */
export const settingsNotesHtml = (notes) => (notes ?? []).map((n) => `<p class="lc-note warn" data-cost-settings-note>${esc(tl(n))}</p>`).join("");

/** RF4.1: the details' section of `sc` (`accountId` as in `costCardHtml`): why there are no numbers, today, yesterday
 *  and the period, by day and by model, the prices' date, source and (C-06) age, and the source's models without a
 *  price; while off, why nothing is shown and where to turn it on. */
export function costDetailHtml(sc, summary, accountId = null) {
  const head = `<h3>${icon("monitor")} ${te("cost.detail.title", { source: sc.label })}</h3>`;
  if (!sc.usage) {
    return `<section class="lc-detail" data-cost-detail="${esc(sc.source)}">${head}
      <p class="sub">${te("cost.detail.off", { source: sc.label })}</p>
      <div class="dlg-actions left"><button class="btn" data-cost-settings>${icon("gear")} ${te("cost.detail.openSettings")}</button></div></section>`;
  }
  // C-06: a table's age ("Prices from 319 days ago", from 60 days) sits on the line of that table.
  const tables = sc.usage.period.tables.map((p) => {
    const vars = { date: fmtDate(p.effective) ?? "", origin: t(`cost.origin.${p.origin}`) };
    const line = p.age ? `<span data-cost-prices-age>${te("cost.detail.pricesAge", { ...vars, age: tl(p.age) })}</span>` : te("cost.detail.prices", vars);
    return line + (p.source ? ` <span class="sub">${esc(p.source)}</span>` : "");
  }).join("<br/>");
  return `<section class="lc-detail" data-cost-detail="${esc(sc.source)}">${head}
    <p class="sub">${te("cost.detail.help", { source: sc.label })}</p>
    ${noteHtml(sc)}${settingsNotesHtml(summary.settings_notes)}${tilesHtml(sc, summary, accountId)}
    ${planAddHtml(summary, accountId)}
    ${sc.usage.days.length ? chartHtml(sc, summary) : ""}${modelsHtml(sc, summary)}
    ${summary.pricing_error ? `<p class="lc-note warn">${esc(tl(summary.pricing_error))}</p>` : ""}
    ${sc.usage.unpriced ? `<p class="lc-note warn" data-cost-unpriced>${esc(tl(sc.usage.unpriced))}</p>` : ""}
    <p class="lc-foot">${esc(tl(summary.notice))}${hiddenNoteHtml(summary)}${tables ? `<br/>${tables}` : ""}</p></section>`;
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
