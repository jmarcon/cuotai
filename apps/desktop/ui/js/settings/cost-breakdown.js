// PRD 0042 RF6.1: Settings › Advanced › Local cost › Sessions, blocks and projects. A segmented control (DS-CMP-SEG)
// switches Days · Models · Sessions · Blocks · Projects; each tab has the KPI header (total, requests, tokens;
// DS-CMP-KPI, "≈" and "estimate", "—" when unknown) and a table. Sessions and projects are labelled inline (the label is
// the person's own words, stored in the local cost index); a project can be hidden from the totals and stays listed.
// Blocks shows the active block first with its live figures (the core's `LiveBlock`), read again from the index every
// 30 s while the tab is shown (no extra scan); the provider window sits beside it with distinct words, never merged.
// Every figure comes from the core (`cost_breakdown`); nothing is computed here but display formatting.
import { t, te, tle } from "../i18n.js";
import { costBreakdown, costSetHidden, costSetLabel, esc, fmtDate, fmtNumber } from "../lib.js";
import { approxText, fmtTokens } from "../cost.js";
import { l10nError } from "./common.js";
import { seg, toast } from "./rows.js";

const TABS = ["day", "model", "session", "block", "project"];
const LIVE_MS = 30_000;
const sum = (tokens) => tokens.input + tokens.output + tokens.cache_read + tokens.cache_write + (tokens.cache_write_1h ?? 0);
const time = (iso) => new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
const stamp = (iso) => `${fmtDate(iso.slice(0, 10)) ?? ""} ${time(iso)}`;
const minutes = (m) => (m < 60 ? t("settings.costBy.minutes", { m }) : t("settings.costBy.hours", { h: Math.floor(m / 60), m: m % 60 }));

/** DS-CMP-KPI: one stat card; an unknown figure shows "—" over the hatch (DS-CMP-HON-1), never 0. */
const kpi = (label, figure, caption, unknown = false) => `<div class="lc-kpi${unknown ? " unknown" : ""}"><span class="lc-kpi-label">${esc(label)}</span>
  <b class="lc-kpi-figure${figure.length > 6 ? " long" : ""}">${esc(figure)}</b><span class="lc-kpi-caption">${esc(caption)}</span></div>`;

/** The KPI header of a tab from a core `Estimate`. */
function kpisHtml(e) {
  const priced = sum(e.tokens) ? e.total != null : true;
  return `<div class="lc-kpis" data-cost-kpis>${kpi(t("settings.costBy.kpi.total"), sum(e.tokens) ? approxText(e) : "—", t("settings.costBy.kpi.estimate"), !priced || !sum(e.tokens))}
    ${kpi(t("settings.costBy.kpi.requests"), fmtNumber(e.requests, 0), t("settings.costBy.kpi.requestsCaption"))}
    ${kpi(t("settings.costBy.kpi.tokens"), fmtTokens(sum(e.tokens)), t("settings.costBy.kpi.tokensCaption"))}</div>`;
}

const table = (cols, rows) => `<div class="cost-price-table lc-by-table"><table><thead><tr>${cols.map((c) => `<th scope="col">${c}</th>`).join("")}</tr></thead>
  <tbody>${rows.join("")}</tbody></table></div>`;
const empty = () => `<p class="hint" data-cost-by-empty>${te("settings.costBy.empty")}</p>`;

/** An inline label editor (RF2.2/RF4.2): saved on Enter or when leaving the field; empty removes the label. */
const labelField = (target, digest, label, placeholder) => `<input type="text" class="lc-label-edit" maxlength="48" spellcheck="false" autocomplete="off"
  value="${esc(label ?? "")}" placeholder="${esc(placeholder)}" aria-label="${esc(t("settings.costBy.label", { name: placeholder }))}" data-cost-label="${esc(target)}" data-digest="${esc(digest)}" />`;

function daysHtml(summary) {
  const rows = summary.sources.filter((s) => s.usage).flatMap((s) => s.usage.days.filter((d) => sum(d.tokens)).map((d) => ({ ...d, label: s.label })))
    .sort((a, b) => b.day.localeCompare(a.day))
    .map((d) => `<tr><th scope="row">${esc(fmtDate(d.day) ?? d.day)}</th><td>${esc(d.label)}</td><td>${esc(fmtTokens(sum(d.tokens)))}</td><td>${d.requests}</td><td>${esc(approxText(d))}</td></tr>`);
  return kpisHtml(summary.total) + (rows.length ? table([te("cost.col.day"), te("settings.costBy.col.source"), te("cost.col.tokens"), te("cost.col.requests"), te("cost.col.cost")], rows) : empty());
}

function modelsHtml(summary) {
  const rows = summary.total.models.map((m) => `<tr><th scope="row"><code>${esc(m.model)}</code></th><td>${esc(fmtTokens(sum(m.tokens)))}</td><td>${m.requests}</td>
    <td>${esc(m.cost == null ? t("cost.unknown") : approxText({ tokens: m.tokens, unknown_tokens: m.tokens, cost: m.cost }))}</td></tr>`);
  return kpisHtml(summary.total) + (rows.length ? table([te("cost.col.model"), te("cost.col.tokens"), te("cost.col.requests"), te("cost.col.cost")], rows) : empty());
}

function sessionsHtml(page) {
  const rows = page.sessions.map((s) => `<tr data-session="${esc(s.session)}"><th scope="row">${labelField("session", s.session, s.label, tle(s.name))}
      <span class="sub">${esc(stamp(s.first))}–${esc(time(s.last))}</span></th><td>${esc(SOURCE_LABELS[s.source] ?? s.source)}</td><td>${esc(minutes(s.duration_minutes))}</td>
    <td>${esc(fmtTokens(sum(s.estimate.tokens)))}</td><td>${esc(approxText(s.estimate))}</td></tr>`);
  const more = page.count > page.sessions.length ? `<p class="hint">${te("settings.costBy.moreSessions", { count: page.count - page.sessions.length })}</p>` : "";
  return kpisHtml(page.total) + (rows.length ? table([te("settings.costBy.col.session"), te("settings.costBy.col.source"), te("settings.costBy.col.duration"), te("cost.col.tokens"), te("cost.col.cost")], rows) + more : empty());
}

const SOURCE_LABELS = { claude_code: "Claude Code", codex_cli: "Codex CLI", gemini_cli: "Gemini CLI" };

/** RF3.3/RF6.1: the active block of each source with its live figures, the provider window beside it. */
function liveHtml(live) {
  return live.map((l) => {
    const b = l.block;
    const provider = b.provider_window
      ? `<p class="hint" data-cost-provider-window>${te(b.provider_window.used_pct == null ? "settings.costBy.providerReset" : "settings.costBy.providerWindow",
        { pct: Math.round(b.provider_window.used_pct ?? 0), at: time(b.provider_window.resets_at) })}</p>` : "";
    return `<section class="lc-live" data-cost-live="${esc(b.source)}" aria-live="polite">
      <h4>${te("settings.costBy.liveTitle", { source: SOURCE_LABELS[b.source] ?? b.source })}</h4>
      <p>${te("settings.costBy.liveWindow", { start: time(b.start), end: time(b.end), elapsed: minutes(l.elapsed_minutes), left: minutes(l.left_minutes) })}</p>
      <p><b>${esc(approxText(b.estimate))}</b> · ${te("cost.tokens", { amount: fmtTokens(sum(b.estimate.tokens)) })}</p>
      <p>${l.line ? esc(tle(l.line)) : te("settings.costBy.noRate")}</p>
      ${l.projection ? `<p data-cost-projection>${esc(tle(l.projection))}</p>` : ""}${provider}
      <p class="hint">${te("settings.costBy.localEstimate")}</p></section>`;
  }).join("");
}

function blocksHtml(page) {
  const rows = page.blocks.map((b) => `<tr class="${b.active ? "lc-active" : ""}"><th scope="row">${esc(stamp(b.start))}–${esc(time(b.end))}</th><td>${esc(SOURCE_LABELS[b.source] ?? b.source)}</td>
    <td>${te(b.active ? "settings.costBy.active" : "settings.costBy.closed")}</td><td>${b.sessions}</td><td>${esc(fmtTokens(sum(b.estimate.tokens)))}</td><td>${esc(approxText(b.estimate))}</td></tr>`);
  return kpisHtml(page.total) + liveHtml(page.live ?? []) + (rows.length
    ? table([te("settings.costBy.col.block"), te("settings.costBy.col.source"), te("settings.costBy.col.state"), te("settings.costBy.col.sessions"), te("cost.col.tokens"), te("cost.col.cost")], rows)
    : empty());
}

function projectsHtml(page) {
  const rows = page.projects.map((p) => {
    const name = tle(p.name);
    const hide = p.project ? `<button type="button" class="btn sm" aria-pressed="${p.hidden}" data-cost-hide="${esc(p.project)}">${te(p.hidden ? "settings.costBy.show" : "settings.costBy.hide")}</button>` : "";
    const cell = p.project ? labelField("project", p.project, p.label, p.folder_hint ?? name) : esc(name);
    return `<tr class="${p.hidden ? "lc-hidden" : ""}"><th scope="row">${cell}${p.hidden ? ` <span class="badge">${te("settings.costBy.hidden")}</span>` : ""}</th>
      <td>${p.sessions}</td><td>${esc(fmtTokens(sum(p.estimate.tokens)))}</td><td>${esc(approxText(p.estimate))}</td><td>${hide}</td></tr>`;
  });
  const hidden = page.hidden ? `<p class="hint">${te("settings.costBy.hiddenNote", { count: page.hidden })}</p>` : "";
  return kpisHtml(page.total) + hidden + (rows.length
    ? table([te("settings.costBy.col.project"), te("settings.costBy.col.sessions"), te("cost.col.tokens"), te("cost.col.cost"), `<span class="sr-only">${te("settings.costBy.hide")}</span>`], rows)
    : empty());
}

const RENDER = { day: daysHtml, model: modelsHtml, session: sessionsHtml, block: blocksHtml, project: projectsHtml };
// The tab shown survives the page's repaints (the scan's state, a saved setting).
let current = "day";

/** One tab's panel from the core's answer (exported for the UI smoke). */
export const breakdownPanelHtml = (by, data) => RENDER[by](data);

/** The sub-page body: the tabs and the panel the binding fills. */
export function breakdownHtml(by = current) {
  return `<p class="hint">${te("settings.costBy.help")}</p>
    ${seg("cost-by", TABS.map((id) => [id, te(`settings.costBy.tab.${id}`)]), by, `aria-label="${te("settings.costBy.tabs")}"`)}
    <div class="lc-by" data-cost-by role="region" aria-live="polite"><p class="hint">${te("common.loading")}</p></div>`;
}

/** Binds the tabs inside `box`; returns what stops the 30 s refresh (the page calls it before a repaint). */
export function bindBreakdown(box, ctx) {
  const group = box.querySelector("#cost-by");
  const panel = box.querySelector("[data-cost-by]");
  if (!group || !panel) return () => {};
  let by = current;
  let timer = null;
  const load = async () => {
    clearTimeout(timer);
    const shown = by;
    try {
      const data = await costBreakdown(shown);
      if (shown !== by || !panel.isConnected) return;
      const focused = document.activeElement?.closest?.("[data-cost-by]") ? document.activeElement.dataset.digest : null;
      panel.innerHTML = breakdownPanelHtml(shown, data);
      if (focused) panel.querySelector(`[data-digest="${CSS.escape(focused)}"]`)?.focus();
    } catch (e) {
      panel.innerHTML = `<p class="hint warn-txt" role="alert">${esc(l10nError(e))}</p>`;
    }
    if (shown === "block" && panel.isConnected) timer = setTimeout(load, LIVE_MS);
  };
  // `initRows()` gives the group a value and fires `change` (DS-CMP-SEG-3, arrow keys included).
  group.addEventListener("change", () => {
    by = group.querySelector('[role="radio"][aria-checked="true"]')?.dataset.value ?? by;
    current = by;
    load();
  });
  const save = async (input) => {
    const value = input.value.trim();
    if (input.dataset.saved === value) return;
    try {
      await costSetLabel(input.dataset.costLabel, input.dataset.digest, value || null);
      input.dataset.saved = value;
      toast(t("settings.saved"));
      ctx.changed?.("cost");
    } catch (e) { toast(l10nError(e)); }
  };
  panel.addEventListener("focusin", (e) => { const input = e.target.closest("[data-cost-label]"); if (input && input.dataset.saved === undefined) input.dataset.saved = input.value.trim(); });
  panel.addEventListener("keydown", (e) => { const input = e.target.closest("[data-cost-label]"); if (input && e.key === "Enter") { e.preventDefault(); save(input); } });
  panel.addEventListener("focusout", (e) => { const input = e.target.closest("[data-cost-label]"); if (input) save(input); });
  panel.addEventListener("click", async (e) => {
    const button = e.target.closest("[data-cost-hide]");
    if (!button) return;
    try {
      await costSetHidden(button.dataset.costHide, button.getAttribute("aria-pressed") !== "true");
      ctx.changed?.("cost");
      load();
    } catch (err) { toast(l10nError(err)); }
  });
  load();
  return () => clearTimeout(timer);
}
