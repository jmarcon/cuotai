// Tray popover (PRD 0022, docs/design-system.md DS-CMP-SHELL/HEAD/TAB/ROW/FOOT): the header holds only the filter
// tabs, the body lists account rows (or the alternative Rings and Compact views chosen in Settings › Dashboard, one
// preference with the menu bar's `tray.popover_view`), the menu-bar accounts first (PRD 0025 RF2.3), aggregators
// grouped, listed or hidden (PRD 0036 RF8.3) and the reset timeline below (PRD 0026 RF5.1); the footer carries
// Settings, Stats, the age of the last completed refresh, the refresh button and the main window.
import { boot } from "./boot.js";
import { te, t } from "./i18n.js";
import { detailHtml, mountModels } from "./detail.js";
import { syncTrayMenu, trayUpdateLine } from "./tray-menu.js";
import { mountStats } from "./stats.js";
import { mountTimeline } from "./timeline.js";
import { costLineHtml, sourceFor } from "./cost.js";
import {
  accountLabel, bestChipHtml, costUsage, bindStatusLinks, bindThemeSwitch, bindSettingsShortcut, brand, createStore, disabledAccounts, esc, fmtAge, icon,
  initOrderSync, initTheme, initTrayView, level, nextNoticeText, noteText, onDisabledChange, onHiddenChange, onSettingsChanged, onTrayViewChange,
  openAggregatorDashboard, openMainWindow, openSettingsWindow, overageChipHtml, reportKey, setMetricAlert, sortReports, statusLabel, trayViewPref, wordmark,
  errText, isDesktop,
} from "./lib.js";
import {
  accountMeterHtml, accountState, announceChanges, clearChange, figureHtml, figureOf, filterTabsHtml, fitStatusLines, meterHtml, monograms,
  repeatedProviders, ringHtml, rowHtml, rowName, stateGlyph, statusParts, tileHtml, trackChanges,
} from "./components.js";
import {
  accessibleName, aggregatorsInAll, activeConnection, connectionChips, connectionInfoHtml, connectionsOf, groupsOf, instanceHeadHtml,
  handleConnectionAction, isAggregator, isConnection, onAggregatorModeChange, poolFigure, poolLineHtml, poolPct, statusLine, withConnections,
} from "./aggregator.js";
import { initUpdates, onUpdateState } from "./updater.js";
import { initRowLayout } from "./density.js";

await boot();

const root = document.getElementById("root");
const disabled = disabledAccounts;
onDisabledChange(() => render());
const store = createStore(() => render());
let expanded = null; // the one expanded row (DS-CMP-ROW-5), or the opened drill-in (Rings, Compact, 9router, connection)
let filter = "all"; // the selected filter tab: "all" or a provider id (remembered for this window, DS-CMP-TAB-5)
let moreOpen = false;
let filterBeforeOpen = null; // the tab to return to when an account opened from "All" closes (fa gap 6)
let models = null; // models section of the expanded account; survives re-renders
// DS-CMP-TAB-4: provider tabs shown before "More"; measured after each render (tabs never wrap or scroll).
let tabFit = 8;
// PRD 0036 RF8.3: the aggregators expanded in place in "All" (accordion, DS-CMP-NAV-2); survives redraws only.
const aggOpen = new Set();

// PRD 0026: the compact Stats drill-in (DS-CMP-STATS-1) and the reset timeline under the accounts (RF5.1).
let statsOpen = false;
const OPEN_VIEW_KEY = "cuotai.openView";
/** "Open full stats": the main window reads the request from localStorage (`storage` event or at boot). */
function openFullStats() {
  try { localStorage.setItem(OPEN_VIEW_KEY, "stats"); } catch {}
  openMainWindow();
}
let statsMount = null, timelineMount = null;
const statsHost = () => (statsMount ??= mountStats({ tray: true, onOpenFull: openFullStats }));
// PRD 0025 RF2.3: the popover's timeline follows the menu-bar selection (every account in Auto). A lane opens its account.
let timelineOnly = null;
const timelineHost = () => (timelineMount ??= mountTimeline({ compact: true, only: () => timelineOnly, onSelect: (key) => { expanded = key; statsOpen = false; render(); } }));

/** The menu-bar selection the shell sent with the state (`view.tray`), or after a change in Settings. */
let traySel = null;
if (window.__TAURI__) window.__TAURI__.event.listen("tray-prefs-changed", (e) => { if (e.payload) traySel = e.payload; render(); });
/** PRD 0025 RF2.3: the menu-bar accounts first, in the menu bar's order; the others under "Not in menu bar". */
function splitByMenuBar(reports, view) {
  const sel = traySel ?? view?.tray;
  const keys = sel?.selected ?? [];
  const inBar = keys.map((k) => reports.find((r) => r.account_id === k)).filter(Boolean);
  if (!inBar.length || inBar.length === reports.length) return { inBar: reports, rest: [] };
  return { inBar, rest: reports.filter((r) => !inBar.includes(r)) };
}
const notInHtml = () => `<p class="pop-sub" role="presentation">${te("settings.menubar.not_in")}</p>`;

/** The chips a row carries besides its status line: extra usage (PRD 0023), the best account (PRD 0024 RF7) and a
 *  9router connection's own state (PRD 0036). */
// Fa gap 4: an exhausted account on extra usage says so on its status line, so the extra chip stays off the row.
const rowChips = (r, via = false) => `${isConnection(r) ? connectionChips(r, { via }) : ""}${accountState(r).kind === "exhausted" ? "" : overageChipHtml(r)}${bestChipHtml(r)}`;

/** The row of one account, with the multi-account title rule (DS-CMP-ROW-1) and its monogram. */
function row(r, ctx, open = null, { via = false } = {}) {
  const repeated = ctx.repeated.has(r.provider) && !isConnection(r);
  return rowHtml(r, {
    title: repeated ? accountLabel(r) : brand(r.provider).label,
    sub: repeated ? "" : accountLabel(r),
    mono: ctx.monos.get(reportKey(r)) ?? "",
    open,
    chips: rowChips(r, via),
  });
}

/** Rings view (DS-CMP-SHELL-5, RF5.3): one static open C ring per account, the value inside, one status line. A word
 *  never goes inside the ring: an exhausted or failed account shows its state icon (DS-CMP-RING-2). */
function ringTile(r, ctx) {
  const st = accountState(r);
  const f = figureOf(r, st);
  const p = statusParts(r, st);
  const name = rowName(r, st);
  // DS-CMP-RING-2: a figure or a state icon inside the ring, never a word; long figures step down a size.
  const glyph = st.icon ?? (f.word ? (st.kind === "exhausted" ? "xcircle" : "info") : null);
  const center = glyph ? icon(glyph, `sg ${st.icon ? st.role : st.kind === "exhausted" ? "bad" : "muted"}`)
    : `<span class="${[...f.fig].length > 3 ? "long" : ""}${f.role ? ` ${f.role}` : ""}">${esc(f.fig)}</span>`;
  return `<button class="tile" data-open="${esc(reportKey(r))}" aria-label="${esc(name)}" title="${esc(name)}">
    <span class="donut${st.stale || (r.status === "ready" && r.freshness === "aging") ? " stale" : ""}">${ringHtml(st.kind === "status" ? "unknown" : st.kind === "exhausted" ? "pct" : st.kind, st.pct, 64)}<span class="center">${center}</span></span>
    <span class="tile-name">${tileHtml(r.provider, 14)}<b>${esc(ctx.repeated.has(r.provider) ? accountLabel(r) : brand(r.provider).label)}</b></span>
    <span class="tile-st lbl ${p.role}">${tileStateIcon(st, p)}<span>${esc(p.label)}</span></span></button>`;
}
/** Fa gap 10 (mockup 01c): the tile's status leads with a state icon (✓ ⚠ ✕ clock), never a second ring. */
function tileStateIcon(st, p) {
  if (st.icon) return icon(st.icon, `sg ${st.role}`);
  if (st.kind === "exhausted") return icon("xcircle", "sg bad");
  if (st.kind === "stale" || st.kind === "unknown") return icon("clock", `sg ${st.kind === "stale" ? "warn" : "muted"}`);
  if (p.pace) return icon({ good: "check", neutral: "check", warn: "warn", bad: "xcircle" }[p.pace.tone] ?? "clock", `sg ${p.role}`);
  const role = p.role === "fg" ? st.role : p.role;
  return icon(role === "good" ? "check" : role === "bad" ? "xcircle" : role === "warn" ? "warn" : "clock", `sg ${role}`);
}

/** Compact view (RF8.1): one 44 px line per account: tile, name, 72 px bar with the pace tick, state glyph, figure. */
function compactRow(r, ctx) {
  const st = accountState(r);
  const f = figureOf(r, st);
  const name = rowName(r, st);
  return `<button class="crow" data-open="${esc(reportKey(r))}" aria-label="${esc(name)}" title="${esc(name)}">
    ${tileHtml(r.provider, 24, ctx.monos.get(reportKey(r)) ?? "")}
    <span class="nm">${esc(ctx.repeated.has(r.provider) ? accountLabel(r) : brand(r.provider).label)}</span>
    <span class="cg">${stateGlyph(st)}</span><span class="cb">${accountMeterHtml(r, st, { thin: true })}</span>
    <span class="cf${f.dim ? " stale" : ""}"><b class="${f.role}">${esc(f.word && st.kind === "status" ? statusParts(r, st).label : f.fig)}</b>${f.unit ? `<small>${esc(f.unit)}</small>` : ""}</span>${icon("chev", "chev")}</button>`;
}

// ---------- PRD 0036: aggregators (9router) ----------
/** The pool's figure ("1/3" available) over its meter; the status line names the exhausted pool or the tightest one. */
function aggregatorRow(r, view) {
  const key = reportKey(r);
  const open = aggOpen.has(key);
  const line = statusLine(r);
  const failed = r.status !== "ready";
  const pct = failed ? null : poolPct(r);
  const role = failed || line.tone === "bad" ? "bad" : line.tone === "warn" ? "warn" : "muted";
  return `<article class="row agg-row${open ? " open" : ""}" data-agg="${esc(key)}">
    <button class="rh" data-agg-toggle="${esc(key)}" aria-expanded="${open}" aria-label="${esc(accessibleName(r))}" title="${esc(accessibleName(r))}">
      ${tileHtml(r.provider, 32)}
      <span class="rc">
        <span class="r1"><span class="nm">${esc(brand(r.provider).label)}</span><span class="ac">${esc(r.account)}</span>${failed ? figureHtml(statusLabel(r.status), "", { role: "bad", cls: "wordfig" }) : figureHtml(poolFigure(r), t("agg.available"))}${icon(open ? "chevDown" : "chev", "chev")}</span>
        ${failed ? "" : `<span class="rm">${meterHtml(pct, { role: level(pct), text: `${poolFigure(r)} ${t("agg.available")}` })}</span>`}
        <span class="stl" title="${esc(line.text)}"><span class="stl-l"><span class="lbl ${role}">${esc(line.text)}</span></span></span>
      </span>
    </button>
    ${open ? `<div class="rb agg-body">${groupsHtml(r, view)}
      <button class="link" data-open="${esc(key)}">${icon("list")}<span>${te("agg.openView")}</span></button></div>` : ""}
  </article>`;
}
/** The Rings view's aggregator tile: the ring is the share of active connections with room left. */
function aggregatorTile(r) {
  const failed = r.status !== "ready";
  const pct = failed ? null : poolPct(r);
  const line = statusLine(r);
  return `<button class="tile" data-open="${esc(reportKey(r))}" title="${esc(accessibleName(r))}" aria-label="${esc(accessibleName(r))}">
    <span class="donut">${ringHtml(pct == null ? "unknown" : "pct", pct, 64)}<span class="center">${failed ? icon("alert", "sg bad") : `<span>${esc(poolFigure(r))}</span>`}</span></span>
    <span class="tile-name">${tileHtml(r.provider, 14)}<b>${esc(brand(r.provider).label)}</b></span>
    <span class="tile-st lbl ${line.tone === "bad" ? "bad" : "muted"}"><span>${esc(line.text)}</span></span></button>`;
}
/** PRD 0036 RF8.1/RF8.2: the 9router drill-in: the instance line with "Open dashboard", then each upstream's pool
 *  sentence and its connection rows. */
function aggregatorViewHtml(r, view) {
  return `<div class="agg-view" aria-label="${esc(accessibleName(r))}">
    <div class="agg-head">${instanceHeadHtml(r, 24)}
      ${r.aggregator.dashboard ? `<button class="link" data-agg-dashboard="${r.index}">${icon("window")}<span>${te("agg.openDashboard")}</span></button>` : ""}</div>
    ${r.status !== "ready" ? `<div class="call critical">${icon("alert")}<div>${esc(noteText(r, 0) || statusLabel(r.status))}</div></div>` : ""}
    ${groupsHtml(r, view)}
  </div>`;
}
/** Each upstream's pool sentence (or name) followed by its connection rows. */
function groupsHtml(r, view) {
  const ctx = context(view);
  return groupsOf(r).map((g) => `<div class="agg-group">${g.pool ? poolLineHtml(g.pool) : `<p class="agg-pool">${esc(g.name)}</p>`}
    ${g.items.map((c) => row(c, ctx)).join("")}</div>`).join("");
}
/** What the list draws for each report, by the Settings choice: an aggregator grouped (one row), listed (its active
 *  connections among the accounts, marked "via 9router") or hidden. */
function allItems(reports) {
  const mode = aggregatorsInAll();
  return reports.flatMap((r) => {
    if (!isAggregator(r)) return [{ r }];
    if (mode === "hidden") return [];
    if (mode === "listed") return connectionsOf(r).filter(activeConnection).map((c) => ({ r: c, via: true }));
    return [{ r, group: true }];
  });
}

function context(view) {
  const all = (view?.reports ?? []).filter((r) => !disabled.has(reportKey(r)));
  return { repeated: repeatedProviders(all.filter((r) => !isAggregator(r))), monos: monograms(view?.reports ?? []) };
}

/** DS-CMP-FOOT-4: one notice above the footer at a time, highest priority first (error > reset notice > update). */
function noticeHtml(view, error) {
  if (error && view?.reports?.length) return `<p class="pop-notice bad" role="alert">${icon("alert")}<span>${esc(error)}</span></p>`;
  const next = nextNoticeText(view);
  if (next) return `<p class="pop-notice" data-next-notice>${icon("bell")}<span>${esc(next)}</span></p>`;
  const update = trayUpdateLine();
  if (update) return `<p class="pop-notice" data-update-notice><button class="link" data-open-main>${icon("download")}<span>${esc(update)}</span></button></p>`;
  return "";
}

const isMac = () => /^Mac/.test(globalThis.navigator?.platform ?? "");
/** DS-CMP-FOOT-1..3: Settings and Stats on the left; the last completed refresh's age and refresh on the right. The
 *  Settings keycap lives in the tooltip and `aria-keyshortcuts` (PRD 0022 RF7.2 amendment); the main window opens
 *  from the tray menu (mockup 01 has no footer icon for it). */
function footerHtml(view, statsShown) {
  const busy = !!view && view.busy.length > 0;
  const age = view?.finished_at ? fmtAge(view.finished_at) : "";
  const refreshName = busy ? t("tray.refreshing") : t("common.refreshAll");
  return `<footer class="pop-foot">
    <button class="link" data-open-settings aria-keyshortcuts="${isMac() ? "Meta+Comma" : "Control+Comma"}" title="${te("settings.title")} (${isMac() ? "⌘," : "Ctrl+,"})">${icon("gear")}<span>${te("settings.title")}</span></button>
    <button class="link" data-open-stats aria-pressed="${statsShown}" title="${te("tray.stats")}">${icon("chartBar")}<span>${te("tray.stats")}</span></button>
    <span class="grow"></span>
    ${age ? `<span class="foot-age" title="${esc(t("tray.footer.age", { age }))}">${esc(age)}</span>` : ""}
    <button class="icon" data-refresh-all aria-label="${esc(refreshName)}" title="${esc(refreshName)}" ${busy ? 'aria-busy="true" disabled' : ""}>${icon("refresh", busy ? "spin" : "")}</button>
  </footer>`;
}

// PRD 0015 RF4.1: the local cost summary, read like the main window does (every 30 s and when the popover shows).
let localCost = null;
const loadLocalCost = () => costUsage().then((next) => { if (JSON.stringify(next) !== JSON.stringify(localCost)) { localCost = next; render(); } }, () => {});
setInterval(loadLocalCost, 30_000);
document.addEventListener("visibilitychange", () => { if (!document.hidden) loadLocalCost(); });

/** The expanded account's body (mockup 03): the shared details (with the best-account action), the local-cost line,
 *  the models and the actions (Details opens the account in the main window). */
const openBody = (r) => isConnection(r)
  ? `${detailHtml(r)}${connectionInfoHtml(r, r.index)}`
  : `${detailHtml(r, { action: true })}${costLineHtml(sourceFor(localCost, r.provider), localCost)}<div data-models-slot></div>
  <div class="acts"><button class="link" data-open-main>${te("common.details")}</button><button class="link" data-refresh-one="${r.index}">${icon("refresh")}<span>${te("common.refresh")}</span></button></div>`;

function render() {
  const { view, error } = store;
  const all = sortReports(view?.reports ?? []).filter((r) => !disabled.has(reportKey(r)));
  if (filter !== "all" && !all.some((r) => r.provider === filter)) filter = "all";
  const reports = filter === "all" ? all : all.filter((r) => r.provider === filter);
  trackChanges(view);
  const ctx = context(view);
  // PRD 0036 RF8: a connection or an aggregator can be opened too (their own keys).
  const cur = withConnections(all).find((r) => reportKey(r) === expanded) ?? null;
  if (!cur) expanded = null;
  const plain = cur && !isConnection(cur) && !isAggregator(cur);
  if (models && (!plain || models.key !== expanded)) { models.dispose(); models = null; }
  if (plain) {
    models ??= mountModels({ key: expanded, index: cur.index, generation: view.generation });
    models.update(cur.index, view.generation);
  }
  const focus = models?.focused();
  const statsFocus = statsMount?.focused(), timelineFocus = timelineMount?.focused();
  const trayView = trayViewPref();
  const showStats = !cur && statsOpen;
  // Rings, Compact, a 9router instance and a connection open as a drill-in (DS-CMP-SHELL-2); the list expands in place.
  const drill = cur && (trayView !== "list" || isAggregator(cur) || isConnection(cur) || !reports.includes(cur));
  const { inBar, rest } = splitByMenuBar(reports, view);
  const listOf = (list) => {
    const items = allItems(list);
    if (trayView === "rings") return `<div class="tiles">${items.map(({ r, group }) => (group ? aggregatorTile(r) : ringTile(r, ctx))).join("")}</div>`;
    if (trayView === "compact") return items.map(({ r, group }) => (group ? aggregatorRow(r, view) : compactRow(r, ctx))).join("");
    return items.map(({ r, group, via }) => (group ? aggregatorRow(r, view) : row(r, ctx, reportKey(r) === expanded ? openBody(r) : null, { via }))).join("");
  };
  let body;
  if (!view && !error) body = `<div class="skeleton-rows" aria-busy="true">${'<span class="sk-row"></span>'.repeat(4)}</div>`;
  else if (error && !view?.reports?.length) body = `<div class="call critical">${icon("alert")}<div>${esc(error)}</div></div>`;
  else if (view && !all.length) {
    body = `<div class="empty-state">${icon("plus")}<p>${te("tray.empty")}</p><button class="btn primary" data-open-main>${te("common.add")}</button></div>`;
  } else if (drill) body = isAggregator(cur) ? aggregatorViewHtml(cur, view) : row(cur, ctx, openBody(cur));
  else if (showStats) body = `<div data-stats-slot></div>`;
  else body = `${listOf(inBar)}${rest.length ? notInHtml() + listOf(rest) : ""}${reports.length ? `<div data-timeline-slot></div>` : ""}`;
  syncTrayMenu(view);
  // DS-CMP-HEAD-2 and PRD 0022 RF7.3: the overview header holds only the filter tabs (no logotype); a drill-in
  // header has the back control, the title, the logotype and the esc keycap.
  const drillTitle = drill ? (!isAggregator(cur) && ctx.repeated.has(cur.provider) ? accountLabel(cur) : brand(cur.provider).label)
    : showStats ? t("tray.stats") : "";
  const header = drill || showStats
    ? `<header class="pop-head drill"><button class="icon back" data-back aria-label="${te("common.back")}" title="${te("common.back")}">${icon("back")}</button>
        <h1>${esc(drillTitle)}</h1><span class="grow"></span>
        ${showStats ? `<span data-stats-scope-slot></span>` : `<span class="wordmark sm">${wordmark()}</span>`}<kbd aria-hidden="true">esc</kbd></header>`
    : `<header class="pop-head">${all.length ? filterTabsHtml(all, { selected: filter, moreOpen, maxTabs: tabFit }) : `<h1 class="wordmark">${wordmark()}</h1>`}</header>`;
  root.innerHTML = `<div class="pop">${header}
    <div class="pop-body">${body}</div>
    ${noticeHtml(view, error)}${footerHtml(view, showStats)}</div>`;
  const head = root.querySelector(".pop-head"), strip = head?.querySelector(".tabs");
  if (strip && strip.scrollWidth > head.clientWidth - 24 && tabFit > 1) { tabFit--; return render(); }
  models?.attach(root, focus);
  const statsSlot = showStats && view ? root.querySelector("[data-stats-slot]") : null;
  const timelineSlot = !drill && !showStats && reports.length && view ? root.querySelector("[data-timeline-slot]") : null;
  if (statsSlot) {
    statsHost().update(view);
    statsMount.attach(statsSlot, statsFocus);
    statsMount.attachScope(root.querySelector("[data-stats-scope-slot]"));
  } else if (timelineSlot) {
    timelineOnly = rest.length ? new Set(inBar.map((r) => r.index)) : null;
    timelineHost().update(view);
    timelineMount.refilter();
    timelineMount.attach(timelineSlot, timelineFocus);
  }
  fitStatusLines(root);
  announceChanges();
}

function select(next) {
  filter = next;
  filterBeforeOpen = null;
  moreOpen = false;
  render();
  (root.querySelector(`[role="tab"][data-tab="${CSS.escape(next)}"]`) ?? root.querySelector("[data-more]"))?.focus();
}
function back() {
  const key = expanded;
  expanded = null;
  if (filterBeforeOpen) { filter = filterBeforeOpen; filterBeforeOpen = null; }
  statsOpen = false;
  render();
  if (key) root.querySelector(`[data-open="${CSS.escape(key)}"], [data-agg-toggle="${CSS.escape(key)}"]`)?.focus();
}

root.addEventListener("click", (e) => {
  const el = e.target;
  const tab = el.closest("[data-tab]");
  if (tab) return select(tab.dataset.tab);
  if (el.closest("[data-more]")) { moreOpen = !moreOpen; return render(); }
  const toggle = el.closest("[data-agg-toggle]");
  if (toggle) {
    const key = toggle.dataset.aggToggle;
    if (aggOpen.has(key)) aggOpen.delete(key); else aggOpen.add(key);
    render();
    return root.querySelector(`[data-agg-toggle="${CSS.escape(key)}"]`)?.focus();
  }
  const dashboard = el.closest("[data-agg-dashboard]");
  if (dashboard) return openAggregatorDashboard(Number(dashboard.dataset.aggDashboard)).catch(() => {});
  const fail = (x) => { store.error = errText(x); render(); };
  if (handleConnectionAction(el, () => isDesktop || store.reload(), fail)) return;
  const settingsPage = el.closest("[data-settings]");
  if (settingsPage) return openSettingsWindow({ page: settingsPage.dataset.settings || null });
  // Mockup 03 (#16): "Alert me" in the run-out callout turns that metric's usage alert on.
  const alertMe = el.closest("[data-metric-alert]");
  if (alertMe) {
    const { metricAlert: key, metricLabel: label, metricOn: on } = alertMe.dataset;
    return setMetricAlert(key, label, on !== "true").then(() => isDesktop || store.reload(), fail);
  }
  const open = el.closest("[data-open]");
  if (open) {
    const key = open.dataset.open;
    expanded = expanded === key && !open.closest(".agg-body") ? null : key;
    statsOpen = false;
    // Fa gap 6 (mockup 03): opening an account from "All" in the list selects its provider's tab, so the expanded
    // account is shown on its own tab instead of in the middle of the full list; Esc goes back to "All".
    const target = expanded && trayViewPref() === "list" ? withConnections(store.view?.reports ?? []).find((x) => reportKey(x) === expanded) : null;
    if (target && filter === "all" && !isConnection(target) && !isAggregator(target)) { filterBeforeOpen = filter; filter = target.provider; }
    else if (!expanded && filterBeforeOpen) { filter = filterBeforeOpen; filterBeforeOpen = null; }
    clearChange(key); // RF12.3: opening the account clears its change marker
    render();
    return root.querySelector(`[data-open="${CSS.escape(key)}"]`)?.focus();
  }
  // PRD 0024 RF7.3: the advice in an account's details points at the best account of its group.
  const show = el.closest("[data-show-account]");
  if (show) {
    const target = store.view?.reports?.find((x) => x.index === Number(show.dataset.showAccount));
    if (target) { expanded = reportKey(target); return render(); }
  }
  if (el.closest("[data-back]")) return back();
  if (el.closest("[data-open-stats]")) { expanded = null; statsOpen = !statsOpen; return render(); }
  const one = el.closest("[data-refresh-one]");
  if (one) return store.refreshOne(Number(one.dataset.refreshOne));
  if (el.closest("[data-refresh-all]")) return store.refresh();
  if (el.closest("[data-open-main]")) return openMainWindow();
  if (el.closest("[data-open-settings]")) return openSettingsWindow();
});
// DS-A11Y-2/-4: arrow keys move inside the tab list (roving tabindex); Esc closes the menu, then the drill-in or row.
root.addEventListener("keydown", (e) => {
  const tab = e.target.closest?.('[role="tab"], [data-more]');
  if (tab && ["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) {
    const tabs = [...root.querySelectorAll('[role="tab"], [data-more]')];
    const i = tabs.indexOf(tab);
    const next = { ArrowLeft: i - 1, ArrowRight: i + 1, Home: 0, End: tabs.length - 1 }[e.key];
    e.preventDefault();
    tabs[(next + tabs.length) % tabs.length]?.focus();
    return;
  }
  if (e.key !== "Escape") return;
  if (moreOpen) { e.preventDefault(); moreOpen = false; render(); root.querySelector("[data-more]")?.focus(); return; }
  if (expanded || statsOpen) { e.preventDefault(); back(); }
});
// PRD 0011 RF4.1: "Open status page" in an account's details.
bindStatusLinks(root);
onHiddenChange(render);
onTrayViewChange(() => { expanded = null; render(); });
// DS-A11Y-6: when the popover opens, focus lands on the expanded row or the selected tab. The popover itself has
// no entrance motion (PRD 0022 RF12.1 item 10).
addEventListener("focus", () => {
  if (root.contains?.(document.activeElement)) return;
  (root.querySelector(".row.open .rh") ?? root.querySelector('[role="tab"][aria-selected="true"]'))?.focus({ preventScroll: true });
});
initOrderSync(render);
onAggregatorModeChange(render);
bindThemeSwitch(root, render);
initTheme(render);
render();
initTrayView();
// PRD 0025 RF9.5: the row display of the menu bar.
initRowLayout("tray", render).then(render);
loadLocalCost();
// PRD 0020 RF1.3/RF7.1: Cmd/Ctrl+, opens Settings; its changes repaint the popover.
bindSettingsShortcut();
onSettingsChanged(() => render());
// PRD 0018 RF2.2: the tray popover shows the same update line as the native
// menu; subscribe only after every render dependency has been initialized.
await initUpdates();
onUpdateState(() => render());

if (window.__TAURI__) window.__TAURI__.event.listen("user-content-changed", () => location.reload());
