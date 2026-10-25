// PRD 0022 RF10.5 component fixture page (see components.html): every surface is drawn by the production modules
// from the browser demo's own fixtures (example.com identities, no network), so a theme or language change shows here
// exactly as in the app. A surface whose data the demo lacks is left out, as in the app (no surface without data).
import { boot } from "../../ui/js/boot.js";
import { te } from "../../ui/js/i18n.js";
import { esc, forecastHtml, getView, icon, overageChipHtml, reportKey } from "../../ui/js/lib.js";
import { filterTabsHtml, meterHtml, monograms, ringHtml, rowHtml, tileHtml } from "../../ui/js/components.js";
import { detailHtml } from "../../ui/js/detail.js";
import { isAggregator, warningChipsHtml } from "../../ui/js/aggregator.js";

await boot();
const view = await getView();
const reports = view.reports;
const section = (id, title, body) => (body ? `<section data-fx="${id}"><h2>${esc(title)}</h2>${body}</section>` : "");

const meters = `<div class="fx-meters">
  ${meterHtml(82, { expected: 70, name: "fresh good" })}${meterHtml(45, { expected: 60, name: "fresh warn" })}${meterHtml(12, { expected: 30, name: "fresh bad" })}
  ${meterHtml(40, { state: "stale", name: "stale" })}${meterHtml(null, { name: "unknown" })}${meterHtml(0, { state: "exhausted", name: "exhausted" })}
  ${meterHtml(30, { state: "overage", role: "warn", name: "overage" })}${meterHtml(40, { thin: true, expected: 38, name: "thin" })}</div>`;
const rings = `<div class="fx-row">${["pct", "unknown", "exhausted"].map((k) => ringHtml(k, k === "pct" ? 64 : null, 24)).join("")}</div>`;

// DS-CMP-EXTRA-1..2: the overage chip and the extra-usage sub-row of the details.
const extra = reports.find((r) => r.overage);
const extraBody = extra ? `<div class="fx-row">${overageChipHtml(extra)}</div><div class="fx-panel">${detailHtml(extra)}</div>` : "";

// DS-CMP-CALL-3: the forecast callout of every metric the demo projects to run out before its reset.
const forecasts = reports.flatMap((r) => r.metrics.map((m) => forecastHtml(m, r))).filter(Boolean).slice(0, 2).join("");

// DS-CMP-ROW and DS-CMP-MONO (with the MONO-4 override when the page runs with `?mono=XY`).
const monos = monograms(reports);
const rows = reports.filter((r) => !isAggregator(r)).slice(0, 4)
  .map((r) => rowHtml(r, { title: r.account, mono: monos.get(reportKey(r)) ?? "" })).join("");

// DS-CMP-TAB with the aggregator's own tab (PRD 0036 RF8.1); its standing warnings (RF2.3/RF2.4) when the page runs
// with `?aggwarn`; the "Billed per query" chip of PRD 0027 RF3.1 (DS-CMP-BADGE-2).
const tabs = `<div class="fx-panel">${filterTabsHtml(reports, { aggregatorTabs: true, maxTabs: 4 })}</div>`;
const router = reports.find(isAggregator);
const chips = `<div class="fx-row">${router ? warningChipsHtml(router) : ""}
  <span class="mini warn" data-metered-chip>${icon("warn")}<span class="mini-t">${te("dialog.picker.metered")}</span></span>
  ${tileHtml(reports[0].provider, 32, monos.get(reportKey(reports[0])) ?? "")}</div>`;

const root = document.getElementById("app");
root.innerHTML = [
  section("meters", "Meters (DS-CMP-MTR)", meters),
  section("rings", "State rings (DS-CMP-RING-6)", rings),
  section("extra", "Extra usage (DS-CMP-EXTRA)", extraBody),
  section("forecast", "Forecast callout (DS-CMP-CALL-3)", forecasts),
  section("rows", "Rows and monograms (DS-CMP-ROW, DS-CMP-MONO)", `<div class="fx-panel">${rows}</div>`),
  section("tabs", "Filter tabs (DS-CMP-TAB)", tabs),
  section("chips", "Chips (DS-CMP-BADGE)", chips),
].join("");
root.removeAttribute("aria-busy");
