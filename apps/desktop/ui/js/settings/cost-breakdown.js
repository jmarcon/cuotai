// PRD 0042 RF6.1: Settings › Advanced › Local cost › Sessions, blocks and projects. A segmented control (DS-CMP-SEG)
// switches Days · Models · Sessions · Blocks · Projects; each tab has the KPI header (total, requests, tokens;
// DS-CMP-KPI, "≈" and "estimate", "—" when unknown) and a table. Sessions and projects are labelled inline (the label is
// the person's own words, stored in the local cost index); a project can be hidden from every total and stays listed;
// every tab says how many hidden projects its figures leave out (the core's `hidden_note`).
// Blocks shows the active block first with its live figures (the core's `LiveBlock`), read again from the index every
// 30 s while the tab is shown (no extra scan); the provider window sits beside it with distinct words, never merged.
// Every figure comes from the core (`cost_breakdown`); nothing is computed here but display formatting. Re-validation
// (M-01 to M-03, M-09, L-01, L-09 to L-11): the KPIs and names wrap in every language, a row shows its name as text and
// its code, times and counts follow the app's language, an end on another day keeps its date, older sessions load on
// request, and one polite region outside the panel announces only a changed live line.
import { t, te, tl } from "../i18n.js";
import { costBreakdown, costSetHidden, costSetLabel, esc, fmtDate, fmtDateTime, fmtLocal, fmtNumber } from "../lib.js";
import { approxText, fmtTokens } from "../cost.js";
import { l10nError } from "./common.js";
import { seg, toast } from "./rows.js";

const TABS = ["day", "model", "session", "block", "project"];
const LIVE_MS = 30_000;
/** The sessions one call returns (the backend's `SESSION_ROWS`). */
const PAGE = 100;
/** The tabs whose rows show a code (DS: the code help shows only there). */
const CODE_TABS = new Set(["session", "project"]);
const NONE = { input: 0, output: 0, cache_read: 0, cache_write: 0, cache_write_1h: 0 };
const sum = (tokens) => tokens.input + tokens.output + tokens.cache_read + tokens.cache_write + (tokens.cache_write_1h ?? 0);
/** L-10: a clock time in the app's language and hour cycle, never the browser's. */
const clock = (iso) => fmtLocal(new Date(iso), { hour: "2-digit", minute: "2-digit" });
const sameDay = (a, b) => new Date(a).toDateString() === new Date(b).toDateString();
/** The time of `iso`, with its date unless it is today. */
const when = (iso) => (sameDay(iso, Date.now()) ? clock(iso) : fmtDateTime(iso));
/** L-01: "17 Nov, 14:02–16:40"; an end on another day (past midnight, a 24-hour block) keeps its date. */
const range = (start, end) => `${fmtDateTime(start)}–${sameDay(start, end) ? clock(end) : fmtDateTime(end)}`;
/** L-11: a count grouped in the app's language ("1,234", "1.234"). */
const count = (n) => fmtNumber(n, 0);
const minutes = (m) => (m < 60 ? t("settings.costBy.minutes", { m }) : t("settings.costBy.hours", { h: Math.floor(m / 60), m: m % 60 }));

/** DS-CMP-KPI: one stat card; an unknown figure shows "—" over the hatch (DS-CMP-HON-1), never 0. Figure and captions
 *  wrap (M-01, M-03); a figure longer than six characters steps down a size (DS-CMP-KPI-1). */
const kpi = (label, figure, captions, unknown = false) => `<div class="lc-kpi${unknown ? " unknown" : ""}"><span class="lc-kpi-label">${esc(label)}</span>
  <b class="lc-kpi-figure${figure.length > 6 ? " long" : ""}">${esc(figure)}</b>${captions.map((c) => `<span class="lc-kpi-caption">${esc(c)}</span>`).join("")}</div>`;

/** M-01: the total's figure is only the money ("≈$4.10", "< $0.01") or "—"; what has no price goes to the caption
 *  ("+ unknown cost of 1.2M tokens"), and "—" says why (no usage, cost unknown: DS-CMP-KPI-2). */
function totalKpi(e) {
  const label = t("settings.costBy.kpi.total");
  if (!sum(e.tokens)) return kpi(label, "—", [t("cost.noUsage")], true);
  if (e.total == null) return kpi(label, "—", [t("cost.unknown")], true);
  const unknown = sum(e.unknown_tokens);
  return kpi(label, approxText({ tokens: e.tokens, total: e.total, unknown_tokens: NONE }),
    [t("settings.costBy.kpi.estimate"), ...(unknown ? [t("settings.costBy.kpi.partial", { amount: fmtTokens(unknown) })] : [])]);
}

/** The KPI header of a tab from a core `Estimate`; `requests` replaces the requests' caption (R-02: the Blocks tab
 *  counts the blocks that end in the period). */
function kpisHtml(e, requests = t("settings.costBy.kpi.requestsCaption")) {
  return `<div class="lc-kpis" data-cost-kpis>${totalKpi(e)}
    ${kpi(t("settings.costBy.kpi.requests"), count(e.requests), [requests])}
    ${kpi(t("settings.costBy.kpi.tokens"), fmtTokens(sum(e.tokens)), [t("settings.costBy.kpi.tokensCaption")])}</div>`;
}

/** PRD 0042 M-05: "1 hidden project is not counted in these figures." (the core's text), on every tab. */
const hiddenHtml = (data) => (data.hidden_note ? `<p class="hint" data-cost-hidden-note>${esc(tl(data.hidden_note))}</p>` : "");

const table = (cols, rows) => `<div class="cost-price-table lc-by-table"><table><thead><tr>${cols.map((c) => `<th scope="col">${c}</th>`).join("")}</tr></thead>
  <tbody>${rows.join("")}</tbody></table></div>`;
const empty = () => `<p class="hint" data-cost-by-empty>${te("settings.costBy.empty")}</p>`;

/** An inline label editor (RF2.2/RF4.2): saved on Enter or when leaving the field; empty removes the label. M-02: a
 *  short placeholder ("Add a label", or a project's folder name), the row's name in the accessible name. */
const labelField = (target, digest, label, name, placeholder = t("settings.costBy.addLabel")) => `<input type="text" class="lc-label-edit" maxlength="48" spellcheck="false" autocomplete="off"
  value="${esc(label ?? "")}" placeholder="${esc(placeholder)}" aria-label="${esc(t("settings.costBy.label", { name }))}" data-cost-label="${esc(target)}" data-digest="${esc(digest)}" />`;

/** M-02: the row's name as text, wrapping, then its code ("code 1a2b3c4d", unless the name already shows it) and
 *  `extra` on the muted line. */
const nameHtml = (name, short, extra = "") => {
  const code = short && !name.includes(short) ? te("settings.costBy.code", { short }) : "";
  const sub = [code, extra].filter(Boolean).join(" · ");
  return `<span class="lc-name">${esc(name)}</span>${sub ? `<span class="sub">${sub}</span>` : ""}`;
};

function daysHtml(summary) {
  const rows = summary.sources.filter((s) => s.usage).flatMap((s) => s.usage.days.filter((d) => sum(d.tokens)).map((d) => ({ ...d, label: s.label })))
    .sort((a, b) => b.day.localeCompare(a.day))
    .map((d) => `<tr><th scope="row">${esc(fmtDate(d.day) ?? d.day)}</th><td>${esc(d.label)}</td><td>${esc(fmtTokens(sum(d.tokens)))}</td><td>${esc(count(d.requests))}</td><td>${esc(approxText(d))}</td></tr>`);
  return kpisHtml(summary.total) + hiddenHtml(summary) + (rows.length ? table([te("cost.col.day"), te("settings.costBy.col.source"), te("cost.col.tokens"), te("cost.col.requests"), te("cost.col.cost")], rows) : empty());
}

function modelsHtml(summary) {
  const rows = summary.total.models.map((m) => `<tr><th scope="row"><code>${esc(m.model)}</code></th><td>${esc(fmtTokens(sum(m.tokens)))}</td><td>${esc(count(m.requests))}</td>
    <td>${esc(m.cost == null ? t("cost.unknown") : approxText({ tokens: m.tokens, unknown_tokens: m.tokens, cost: m.cost }))}</td></tr>`);
  return kpisHtml(summary.total) + hiddenHtml(summary) + (rows.length ? table([te("cost.col.model"), te("cost.col.tokens"), te("cost.col.requests"), te("cost.col.cost")], rows) : empty());
}

function sessionsHtml(page) {
  // M-06 (the core's `project_name`, PRD 0042 re-validation): a Project column once the core names it.
  const project = page.sessions.some((s) => s.project_name);
  const rows = page.sessions.map((s) => {
    const name = tl(s.name);
    return `<tr data-session="${esc(s.session)}"><th scope="row">${nameHtml(name, s.short, esc(range(s.first, s.last)))}
      ${labelField("session", s.session, s.label, name)}</th>${project ? `<td>${esc(s.project_name ? tl(s.project_name) : "—")}</td>` : ""}
    <td>${esc(SOURCE_LABELS[s.source] ?? s.source)}</td><td>${esc(minutes(s.duration_minutes))}</td><td>${esc(fmtTokens(sum(s.estimate.tokens)))}</td><td>${esc(approxText(s.estimate))}</td></tr>`;
  });
  // L-09: the next page of older sessions on request (the backend's `offset`), never a silent cut at 100.
  const left = page.count - page.sessions.length;
  const more = left > 0 ? `<div class="dlg-actions left"><button type="button" class="btn sm" data-cost-older>${te("settings.costBy.olderSessions", { count: Math.min(PAGE, left) })}</button></div>` : "";
  const cols = [te("settings.costBy.col.session"), ...(project ? [te("settings.costBy.col.project")] : []), te("settings.costBy.col.source"), te("settings.costBy.col.duration"), te("cost.col.tokens"), te("cost.col.cost")];
  return kpisHtml(page.total) + hiddenHtml(page) + (rows.length ? table(cols, rows) + more : empty());
}

const SOURCE_LABELS = { claude_code: "Claude Code", codex_cli: "Codex CLI", gemini_cli: "Gemini CLI" };

/** RF3.3/RF6.1: the active block of each source with its live figures, the provider window beside it. */
function liveHtml(live) {
  return live.map((l) => {
    const b = l.block;
    const provider = b.provider_window
      ? `<p class="hint" data-cost-provider-window>${te(b.provider_window.used_pct == null ? "settings.costBy.providerReset" : "settings.costBy.providerWindow",
        { pct: Math.round(b.provider_window.used_pct ?? 0), at: when(b.provider_window.resets_at) })}</p>` : "";
    // M-09: not a live region; the one announcer outside the panel says only what changed.
    return `<section class="lc-live" data-cost-live="${esc(b.source)}">
      <h4>${te("settings.costBy.liveTitle", { source: SOURCE_LABELS[b.source] ?? b.source })}</h4>
      <p>${te("settings.costBy.liveWindow", { start: when(b.start), end: sameDay(b.start, b.end) ? clock(b.end) : fmtDateTime(b.end), elapsed: minutes(l.elapsed_minutes), left: minutes(l.left_minutes) })}</p>
      <p><b>${esc(approxText(b.estimate))}</b> · ${te("cost.tokens", { amount: fmtTokens(sum(b.estimate.tokens)) })}</p>
      ${l.line || l.rate_missing ? `<p data-cost-rate>${esc(tl(l.line ?? l.rate_missing))}</p>` : ""}
      ${l.projection ? `<p data-cost-projection>${esc(tl(l.projection))}</p>` : ""}${provider}
      <p class="hint">${te("settings.costBy.localEstimate")}</p></section>`;
  }).join("");
}

function blocksHtml(page) {
  const rows = page.blocks.map((b) => `<tr class="${b.active ? "lc-active" : ""}"><th scope="row">${esc(range(b.start, b.end))}</th><td>${esc(SOURCE_LABELS[b.source] ?? b.source)}</td>
    <td>${te(b.active ? "settings.costBy.active" : "settings.costBy.closed")}</td><td>${esc(count(b.sessions))}</td><td>${esc(fmtTokens(sum(b.estimate.tokens)))}</td><td>${esc(approxText(b.estimate))}</td></tr>`);
  return kpisHtml(page.total, t("settings.costBy.blocksCaption")) + hiddenHtml(page) + liveHtml(page.live ?? []) + (rows.length
    ? table([te("settings.costBy.col.block"), te("settings.costBy.col.source"), te("settings.costBy.col.state"), te("settings.costBy.col.sessions"), te("cost.col.tokens"), te("cost.col.cost")], rows)
    : empty());
}

function projectsHtml(page) {
  // S-01b: the folder names come apart from the page, by fingerprint (only this Settings tab receives them).
  const hints = page.hints ?? {};
  const rows = page.projects.map((p) => {
    // PRD 0020 re-validation R-06: plain text here, escaped once where it is written.
    const name = tl(p.name);
    const hide = p.project ? `<button type="button" class="btn sm" aria-pressed="${p.hidden}" data-cost-hide="${esc(p.project)}">${te(p.hidden ? "settings.costBy.show" : "settings.costBy.hide")}</button>` : "";
    // M-02: the name as text; the field's placeholder is the folder name when the hint holds one (K3's `hints`).
    const folder = hints[p.project] ?? null;
    const cell = p.project ? `${nameHtml(name, p.short)}${labelField("project", p.project, p.label, name, folder ?? undefined)}` : nameHtml(name);
    return `<tr class="${p.hidden ? "lc-hidden" : ""}"><th scope="row">${cell}${p.hidden ? ` <span class="badge">${te("settings.costBy.hidden")}</span>` : ""}</th>
      <td>${esc(count(p.sessions))}</td><td>${esc(fmtTokens(sum(p.estimate.tokens)))}</td><td>${esc(approxText(p.estimate))}</td><td>${hide}</td></tr>`;
  });
  return kpisHtml(page.total) + hiddenHtml(page) + (rows.length
    ? table([te("settings.costBy.col.project"), te("settings.costBy.col.sessions"), te("cost.col.tokens"), te("cost.col.cost"), `<span class="sr-only">${te("settings.costBy.hide")}</span>`], rows)
    : empty());
}

const RENDER = { day: daysHtml, model: modelsHtml, session: sessionsHtml, block: blocksHtml, project: projectsHtml };
// The tab shown survives the page's repaints (the scan's state, a saved setting).
let current = "day";

/** One tab's panel from the core's answer (exported for the UI smoke). */
export const breakdownPanelHtml = (by, data) => RENDER[by](data);

/** M-09: the live lines of a Blocks answer, as the announcer would say them. */
export const liveLines = (data) => (data?.live ?? []).map((l) => (l.line ? tl(l.line) : "")).filter(Boolean).join(" ");
/** M-09: what the announcer says after a Blocks refresh: the new lines only when they differ from `last`; nothing on the
 *  first render (`last` null) or when they did not change. */
export const liveAnnouncement = (last, lines) => (last != null && lines && lines !== last ? lines : null);

/** The sub-page body: the tabs, the code help (Sessions and Projects), the panel the binding fills, named by the
 *  selected tab, and the one polite announcer outside it (M-09). */
export function breakdownHtml(by = current) {
  return `<p class="hint">${te("settings.costBy.help")}</p>
    ${seg("cost-by", TABS.map((id) => [id, te(`settings.costBy.tab.${id}`), `id="cost-by-tab-${id}"`]), by, `aria-label="${te("settings.costBy.tabs")}"`)}
    <p class="hint lc-code-help" data-cost-code-help${CODE_TABS.has(by) ? "" : " hidden"}>${te("settings.costBy.codeHelp")}</p>
    <div class="lc-by" data-cost-by role="region" aria-labelledby="cost-by-tab-${by}"><p class="hint">${te("common.loading")}</p></div>
    <p class="sr-only" aria-live="polite" data-cost-live-announce></p>`;
}

/** Binds the tabs inside `box`; returns what stops the 30 s refresh (the page calls it before a repaint). */
export function bindBreakdown(box, ctx) {
  const group = box.querySelector("#cost-by");
  const panel = box.querySelector("[data-cost-by]");
  const announcer = box.querySelector("[data-cost-live-announce]");
  const help = box.querySelector("[data-cost-code-help]");
  if (!group || !panel) return () => {};
  let by = current;
  let timer = null;
  // M-09: the live lines last shown on the Blocks tab; null until its first render after a tab switch.
  let announced = null;
  // L-09: the Sessions tab's answer with every page loaded so far.
  let sessions = null;
  const load = async () => {
    clearTimeout(timer);
    const shown = by;
    try {
      const data = await costBreakdown(shown);
      if (shown !== by || !panel.isConnected) return;
      const focused = document.activeElement?.closest?.("[data-cost-by]") ? document.activeElement.dataset.digest : null;
      panel.innerHTML = breakdownPanelHtml(shown, data);
      if (focused) panel.querySelector(`[data-digest="${CSS.escape(focused)}"]`)?.focus();
      if (shown === "session") sessions = data;
      if (shown === "block") {
        const lines = liveLines(data);
        const say = liveAnnouncement(announced, lines);
        if (say && announcer) announcer.textContent = say;
        announced = lines;
      }
    } catch (e) {
      panel.innerHTML = `<p class="hint warn-txt" role="alert">${esc(l10nError(e))}</p>`;
    }
    if (shown === "block" && panel.isConnected) timer = setTimeout(load, LIVE_MS);
  };
  // `initRows()` gives the group a value and fires `change` (DS-CMP-SEG-3, arrow keys included).
  group.addEventListener("change", () => {
    by = group.querySelector('[role="radio"][aria-checked="true"]')?.dataset.value ?? by;
    current = by;
    panel.setAttribute("aria-labelledby", `cost-by-tab-${by}`);
    help?.toggleAttribute("hidden", !CODE_TABS.has(by));
    announced = null;
    if (announcer) announcer.textContent = "";
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
  const older = async (button) => {
    if (by !== "session" || !sessions) return;
    button.disabled = true;
    try {
      const from = sessions.sessions.length;
      const next = await costBreakdown("session", from);
      if (by !== "session" || !panel.isConnected) return;
      // A session that started meanwhile shifts the pages: never list one twice.
      const known = new Set(sessions.sessions.map((s) => s.session));
      sessions = { ...next, sessions: [...sessions.sessions, ...next.sessions.filter((s) => !known.has(s.session))] };
      panel.innerHTML = breakdownPanelHtml("session", sessions);
      (panel.querySelectorAll("[data-cost-label]")[from] ?? panel.querySelector("[data-cost-older]"))?.focus();
    } catch (err) { button.disabled = false; toast(l10nError(err)); }
  };
  panel.addEventListener("click", async (e) => {
    const more = e.target.closest("[data-cost-older]");
    if (more) { older(more); return; }
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
