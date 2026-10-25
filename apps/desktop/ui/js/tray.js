import { boot } from "./boot.js";
import { te, t, tl, currentLocale } from "./i18n.js";
import { detailHtml, mountModels } from "./detail.js";
import { syncTrayMenu } from "./tray-menu.js";
import { mountStats } from "./stats.js";
import { mountTimeline } from "./timeline.js";
import { easeOut, motionMs } from "./motion.js";
import {
  accountLabel, bindThemeSwitch, brand, createStore, esc, icon, initTheme, isDesktop, level,
  logo, noteText, openMainWindow, openAggregatorDashboard, disabledAccounts, onDisabledChange, quitApp, reportKey, statusLabel, langMenuHtml, sortReports, sortMenuHtml, initOrderSync, openSettingsWindow, bindSettingsShortcut, onSettingsChanged,
  bindingPct, bindingChipText, blockedText, ALERTS, ageHtml, ageText, alertOf, windowsOf, onHiddenChange, bestChipHtml, cardPace, paceHtml, paceText, paceLabelOf, nextNoticeText,
  incidentBadge, incidentHtml, bindStatusLinks, wordmark, overageChipHtml, billingOverage, getSettings, setTrayPrefs,
} from "./lib.js";
import { initUpdates, onUpdateState } from "./updater.js";
import {
  accessibleName, aggregatorsInAll, activeConnection, connectionChips, connectionInfoHtml, connectionsOf, groupsOf, instanceHeadHtml,
  isAggregator, isConnection, onAggregatorModeChange, poolFigure, poolLineHtml, poolPct, statusLine, withConnections,
} from "./aggregator.js";
import { trayUpdateLine } from "./tray-menu.js";

await boot();

const root = document.getElementById("root");
const disabled = disabledAccounts;
onDisabledChange(() => render());
const store = createStore(() => render());
let selected = null;
// PRD 0026: the compact Stats drill-in (DS-CMP-STATS-1) and the reset timeline under the accounts (RF5.1).
let statsOpen = false;
const OPEN_VIEW_KEY = "cuotai.openView";
/** "Open full stats": the main window reads the request from localStorage (`storage` event or at boot). */
function openFullStats() {
  try { localStorage.setItem(OPEN_VIEW_KEY, "stats"); } catch {}
  openMainWindow();
}
// Mounted on first use, when a slot exists to hold them.
let statsMount = null, timelineMount = null;
const statsHost = () => (statsMount ??= mountStats({ compact: true, onOpenFull: openFullStats }));
// Selecting a lane opens that account's details in the popover.
// PRD 0025 RF2.3: the popover's timeline follows the menu-bar selection (every account in Auto).
let timelineOnly = null;
const timelineHost = () => (timelineMount ??= mountTimeline({ compact: true, only: () => timelineOnly, onSelect: (key) => { selected = key; statsOpen = false; render(); } }));
let models = null; // seção de modelos da conta aberta no popover; sobrevive aos redesenhos

function row(r, { via = false } = {}) {
  const pct = bindingPct(r);
  const blocked = blockedText(r);
  const lv = r.status !== "ready" || blocked ? "bad" : level(pct);
  const value = r.status !== "ready" ? statusLabel(r.status) : blocked ?? (pct != null ? `${Math.round(pct)}%` : "—");
  // PRD 0008 RF3.4: an active usage alert highlights the row.
  return `<button class="row${r.usage_alerts?.length ? " usage-alert" : ""}" data-open="${esc(reportKey(r))}">
    ${logo(r.provider, 32)}
    <div class="row-main">
      <div class="row-top"><b>${esc(brand(r.provider).label)}</b><span class="pct ${lv}">${esc(value)}</span></div>
      <div class="row-sub">${esc(accountLabel(r))}${r.binding ? ` · ${esc(bindingChipText(r))}` : ""}</div>
      ${isConnection(r) && connectionChips(r, { via }) ? `<div class="row-chips">${connectionChips(r, { via })}</div>` : ""}
      ${r.usage_alerts?.length ? `<div class="row-alert">${icon("bell")}<span>${esc(ALERTS.find((c) => c.id === "usage").label(r))}</span></div>` : ""}
      <div class="row-age">${ageHtml(r)}</div>
      ${overageChipHtml(r)}
      ${incidentHtml(r, { link: false })}
      ${trayPace(r, (p, label) => paceHtml(p, { label, short: true }))}${bestChipHtml(r)}
      <div class="bar thin ${lv}"><span style="width:${pct ?? 0}%"></span></div>
    </div>${icon("chev", "chev")}</button>`;
}

// Vista do popover: donuts (mais contas na mesma tela) ou lista. PRD 0025 RF4.3: saved in desktop.json
// (`tray.popover_view`), so Settings and the popover agree; the value an older version kept in localStorage moves
// there once.
const VIEW_KEY = "cuotai.trayView";
const legacyView = (() => { try { return localStorage.getItem(VIEW_KEY); } catch { return null; } })();
let trayView = legacyView === "list" ? "list" : "donut";
/** The menu-bar selection the shell sent with the state (`view.tray`), or after a change in Settings. */
let traySel = null;
async function saveTrayView(view) {
  try {
    const s = await getSettings();
    await setTrayPrefs({ style: s.tray?.style ?? "tightest_percent", colored: !!s.tray?.colored, accounts: s.tray?.accounts ?? null,
      popoverView: view === "list" ? "list" : "grid" });
    try { localStorage.removeItem(VIEW_KEY); } catch {}
  } catch {}
}
getSettings().then((s) => {
  const stored = s.tray?.popover_view;
  if (stored) trayView = stored === "list" ? "list" : "donut";
  else if (legacyView) saveTrayView(trayView);
  render();
}, () => {});
const followTray = (tray) => {
  if (!tray) return;
  traySel = tray;
  trayView = tray.popover_view === "list" ? "list" : "donut";
};
if (isDesktop) window.__TAURI__.event.listen("tray-prefs-changed", (e) => { followTray(e.payload); render(); });

/** PRD 0025 RF2.3: the menu-bar accounts first, in the menu bar's order; the others under "Not in menu bar". */
function splitByMenuBar(reports, view) {
  const sel = traySel ?? view?.tray;
  const keys = sel?.selected ?? [];
  const inBar = keys.map((k) => reports.find((r) => r.account_id === k)).filter(Boolean);
  if (!inBar.length || inBar.length === reports.length) return { inBar: reports, rest: [] };
  return { inBar, rest: reports.filter((r) => !inBar.includes(r)) };
}
const notInHtml = () => `<p class="pop-sub" role="presentation">${te("settings.menubar.not_in")}</p>`;

const R = 26, CIRC = 2 * Math.PI * R;
/** Anel de porcentagem disponível; sem medida, o anel fica vazio. `center` é o conteúdo (número ou ícone). */
function donutHtml(pct, tone, center) {
  const dash = pct == null ? 0 : (Math.max(0, Math.min(100, pct)) / 100) * CIRC;
  return `<div class="donut ${tone}"><svg viewBox="0 0 64 64" aria-hidden="true">
    <circle class="trk" cx="32" cy="32" r="${R}"/>
    ${dash >= 1 ? `<circle class="arc" cx="32" cy="32" r="${R}" stroke-dasharray="${dash.toFixed(1)} ${CIRC.toFixed(1)}"/>` : ""}</svg>
    <span class="center">${center}</span></div>`;
}
// RF3.2: o anel anima do último valor exibido (no redesenho e entre fechar e reabrir o popover), nunca do zero.
const dashOf = (tile) => parseFloat(tile.querySelector(".arc")?.getAttribute("stroke-dasharray")) || 0;
const rings = () => new Map([...root.querySelectorAll(".tile[data-open]")].map((el) => [el.dataset.open, dashOf(el)]));
function animateRings(prev) {
  const ms = motionMs("--motion-slow");
  if (!ms || !prev) return;
  for (const el of root.querySelectorAll(".tile[data-open]")) {
    const from = prev.get(el.dataset.open), to = dashOf(el), arc = el.querySelector(".arc");
    // ponytail: anel que vai a vazio (sem `.arc`) troca na hora; animar exigiria manter o arco com traço 0.
    if (arc && from != null && Math.abs(from - to) >= 0.1) {
      arc.animate([{ strokeDasharray: `${from} ${CIRC}` }, { strokeDasharray: `${to} ${CIRC}` }], { duration: ms, easing: easeOut() });
    }
  }
}

function tile(r) {
  const alert = alertOf(r);
  // PRD 0009 RF2.1: the ring is the core's binding metric (a blocked week wins over a free 5 h window).
  const pct = bindingPct(r);
  const lv = level(pct);
  const w = windowsOf(r);
  const incident = incidentBadge(r);
  const tip = [`${brand(r.provider).label} (${r.account})`,
    alert ? alert.label(r) : pct != null ? te2("metric.level." + lv, { pct: Math.round(pct) }) : "",
    blockedText(r) ?? "",
    billingOverage(r) ? tl(billingOverage(r).summary) : "",
    r.binding ? bindingChipText(r) : "",
    ageText(r),
    trayPace(r, (p, label) => paceText(p, { label, short: true })),
    ...(w.h5 ? [`${t("card.window.h5")}: ${Math.round(w.h5.pct)}%`] : []),
    ...(w.week ? [`${t("card.window.week")}: ${Math.round(w.week.pct)}%`] : []),
    incident?.text ?? ""].filter(Boolean);
  const center = alert ? icon(alert.icon) : pct != null ? `${Math.round(pct)}%` : "—";
  const tone = alert ? alert.tone : lv;
  return `<button class="tile${r.usage_alerts?.length ? " usage-alert" : ""}${incident ? ` incident ${incident.tone}` : ""}" data-open="${esc(reportKey(r))}" title="${esc(tip.join("\n"))}" data-binding="${esc(r.binding?.window ?? "")}">
    ${donutHtml(alert && pct == null ? 100 : pct, tone, center)}
    <span class="tile-name">${logo(r.provider, 14)}<b>${esc(brand(r.provider).label)}</b></span>
    <span class="tile-acct">${esc(accountLabel(r))}</span>${ageHtml(r)}</button>`;
}
const te2 = (key, vars) => t(key, vars);

// PRD 0036 RF8.3: the aggregators expanded in place in "All" (accordion, DS-CMP-NAV-2); survives redraws only.
const expanded = new Set();
/** One grouped row per aggregator: tile, label, the pool figure ("1/3" available) and the status line (the exhausted
 *  pool or the tightest connection). Activating it expands it here: pool lines, connection rows and a last ghost
 *  line to the full 9router view. */
function aggregatorRow(r) {
  const key = reportKey(r);
  const open = expanded.has(key);
  const line = statusLine(r);
  const failed = r.status !== "ready";
  const figure = failed ? statusLabel(r.status) : poolFigure(r);
  const tone = failed ? "bad" : line.tone === "bad" ? "bad" : level(poolPct(r));
  return `<div class="agg-row${open ? " open" : ""}" data-agg="${esc(key)}">
    <button class="row" data-agg-toggle="${esc(key)}" aria-expanded="${open}" aria-label="${esc(accessibleName(r))}">
      ${logo(r.provider, 32)}
      <div class="row-main">
        <div class="row-top"><b>${esc(brand(r.provider).label)}</b><span class="pct ${tone}">${esc(figure)}${failed ? "" : `<small> ${te("agg.available")}</small>`}</span></div>
        <div class="row-sub">${esc(r.account)}</div>
        <div class="row-alert ${line.tone}"><span>${esc(line.text)}</span></div>
        <div class="row-age">${ageHtml(r)}</div>
      </div>${icon("chev", "chev")}</button>
    ${open ? `<div class="agg-body">
      ${groupsHtml(r)}
      <button class="link" data-open="${esc(key)}">${icon("list")}<span>${te("agg.openView")}</span></button>
    </div>` : ""}
  </div>`;
}
/** The donut view's aggregator tile: the ring is the share of active connections with room left. */
function aggregatorTile(r) {
  const failed = r.status !== "ready";
  const pct = failed ? null : poolPct(r);
  const line = statusLine(r);
  const tone = failed || line.tone === "bad" ? "bad" : level(pct);
  return `<button class="tile" data-open="${esc(reportKey(r))}" title="${esc([accessibleName(r), line.text, ageText(r)].join("\n"))}" aria-label="${esc(accessibleName(r))}">
    ${donutHtml(pct, tone, failed ? "—" : esc(poolFigure(r)))}
    <span class="tile-name">${logo(r.provider, 14)}<b>${esc(brand(r.provider).label)}</b></span>
    <span class="tile-acct">${esc(r.account)}</span>${ageHtml(r)}</button>`;
}
/** PRD 0036 RF8.1/RF8.2: the dedicated 9router view: the instance line with "Open dashboard", then each upstream's
 *  pool sentence and its connection rows. */
function aggregatorViewHtml(r) {
  return `<div class="agg-view" aria-label="${esc(accessibleName(r))}">
    <div class="agg-head">${instanceHeadHtml(r, 24)}
      ${r.aggregator.dashboard ? `<button class="link" data-agg-dashboard="${r.index}">${icon("window")}<span>${te("agg.openDashboard")}</span></button>` : ""}</div>
    <div class="row-age">${ageHtml(r)}</div>
    ${r.status !== "ready" ? `<p class="cause">${icon("warn")}<span>${esc(noteText(r, 0) || statusLabel(r.status))}</span></p>` : ""}
    ${groupsHtml(r)}
  </div>`;
}
/** Each upstream's pool sentence (or name) followed by its connection rows. */
const groupsHtml = (r) => groupsOf(r).map((g) => `<div class="agg-group">${g.pool ? poolLineHtml(g.pool) : `<p class="agg-pool">${esc(g.name)}</p>`}
  ${g.items.map((c) => row(c)).join("")}</div>`).join("");
/** What "All" draws for each report, by the Settings choice: an aggregator grouped (one row), listed (its active
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
/** PRD 0007 RF3.3: "Week: lasts until the reset" / "Week: runs out at 16:40, 2h 10m before the reset". */
const trayPace = (r, draw) => {
  const m = cardPace(r);
  return m ? draw(m.pace, paceLabelOf(m)) : "";
};

const short = (s, n = 90) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

/** Contador de avisos no rodapé: um chip por categoria (ícone + número de contas), com a lista no tooltip. */
function alertsHtml(view, reports, error) {
  const last = view?.finished_at
    ? t("tray.lastCheck", { time: new Date(view.finished_at).toLocaleTimeString(currentLocale(), { hour: "2-digit", minute: "2-digit" }) })
    : "";
  const chip = (tone, glyph, count, lines, label) =>
    `<span class="alert-chip ${tone}" role="img" tabindex="0" title="${esc(lines.join("\n"))}" aria-label="${esc([label, ...lines].join(". "))}">${icon(glyph)}${count != null ? `<b>${count}</b>` : ""}</span>`;
  if (error) return `<div class="alerts">${chip("bad", "alert", null, [t("tray.error"), error], t("tray.error"))}</div>`;
  if (!view || !reports.length) return ""; // carregando ou sem contas: nada a avisar
  const groups = ALERTS.map((c) => ({ c, items: reports.filter(c.match) })).filter((g) => g.items.length);
  const incidents = incidentsChip(reports, chip, last);
  if (!groups.length && !incidents)
    return `<div class="alerts">${chip("good", "check", null, [t("tray.allOkTitle", { count: reports.length }), last].filter(Boolean), t("tray.ok"))}</div>`;
  return `<div class="alerts" role="group" aria-label="${te("tray.attention")}">${groups.map(({ c, items }) => {
    const lines = [t("tray.catTitle", { label: c.label(), count: items.length }),
      ...items.slice(0, 8).map((r) => `• ${brand(r.provider).label} (${r.account})${r.notes[0] && c.id !== "exhausted" ? ` — ${short(noteText(r, 0))}` : ""}`)];
    if (items.length > 8) lines.push(t("tray.more", { count: items.length - 8 }));
    if (last) lines.push(last);
    return chip(c.tone, c.icon, items.length, lines, c.label());
  }).join("")}${incidents}</div>`;
}

const TONE_RANK = ["muted", "info", "warn", "bad"];
/** PRD 0011 RF4.2: one chip for the providers whose public status the cards flag, with each in the tooltip. */
function incidentsChip(reports, chip, last) {
  const byProvider = new Map();
  for (const r of reports) { const b = incidentBadge(r); if (b && !byProvider.has(r.provider)) byProvider.set(r.provider, b); }
  if (!byProvider.size) return "";
  const badges = [...byProvider.values()];
  const tone = badges.map((b) => b.tone).reduce((a, b) => (TONE_RANK.indexOf(b) > TONE_RANK.indexOf(a) ? b : a));
  const lines = [t("incident.trayTitle"), ...[...byProvider].map(([provider, b]) => `• ${brand(provider).label}: ${short(b.text)}`), last].filter(Boolean);
  return chip(tone, "alert", byProvider.size, lines, t("incident.trayTitle"));
}

/** PRD 0018 RF2.2: a linha de atualização do popover (mesma frase do menu da bandeja). */
const updateNoticeHtml = () => trayUpdateLine();

function render() {
  const { view, error } = store;
  const loading = !!view && view.busy.length > 0;
  const reports = sortReports(view?.reports ?? []).filter((r) => !disabled.has(reportKey(r)));
  // PRD 0036 RF8: a connection or an aggregator can be opened too (their own keys).
  const cur = withConnections(reports).find((r) => reportKey(r) === selected) ?? null;
  if (models && (!cur || models.key !== selected || isConnection(cur) || isAggregator(cur))) { models.dispose(); models = null; }
  if (cur && !isConnection(cur) && !isAggregator(cur)) {
    models ??= mountModels({ key: selected, index: cur.index, generation: view.generation });
    models.update(cur.index, view.generation);
  }
  const focus = models?.focused();
  const statsFocus = statsMount?.focused(), timelineFocus = timelineMount?.focused();
  const showStats = !cur && statsOpen;
  const { inBar, rest } = splitByMenuBar(reports, view);
  const listOf = (list) => {
    const items = allItems(list);
    return trayView === "donut"
      ? `<div class="tiles">${items.map(({ r, group }) => (group ? aggregatorTile(r) : tile(r))).join("")}</div>`
      : items.map(({ r, group, via }) => (group ? aggregatorRow(r) : row(r, { via }))).join("");
  };
  const body = cur
    ? isAggregator(cur) ? aggregatorViewHtml(cur)
      : isConnection(cur) ? `${detailHtml(cur)}${connectionInfoHtml(cur, cur.index)}`
        : `${detailHtml(cur, { action: true })}<div data-models-slot></div>`
    : showStats ? `<div data-stats-slot></div>`
    : `${listOf(inBar)}${rest.length ? notInHtml() + listOf(rest) : ""}${reports.length ? `<div data-timeline-slot></div>` : ""}`;
  syncTrayMenu(view);
  const prev = rings();
  root.innerHTML = `<div class="pop">
    <header class="pop-head">
      ${cur || showStats ? `<button class="icon back" data-back title="${te("common.back")}">${icon("back")}</button>` : ""}
      <div class="pop-title"><h1>${cur ? esc(brand(cur.provider).label) : showStats ? te("stats.title") : `<button class="brand-link wordmark" data-open-main title="${te("common.open")}" aria-label="${te("common.open")}">${wordmark()}</button>`}</h1>
        ${cur ? `<span class="sub">${esc(accountLabel(cur))}</span>` : ""}
      </div>
      ${cur || showStats ? "" : `<button class="icon" data-view title="${te(trayView === "donut" ? "tray.view.list" : "tray.view.donut")}" aria-label="${te(trayView === "donut" ? "tray.view.list" : "tray.view.donut")}">${icon(trayView === "donut" ? "list" : "grid")}</button>`}
      ${sortMenuHtml()}${langMenuHtml()}
      <button class="icon" data-refresh-all title="${te("common.refresh")}" ${loading ? "disabled" : ""}>${icon("refresh", loading ? "spin" : "")}</button>
    </header>
    <div class="pop-body">${!view && !error ? `<div class="empty">${te("common.loading")}</div>` : ""}${error && !view?.reports.length ? `<div class="empty">${esc(error)}</div>` : ""}${body}</div>
    ${updateNoticeHtml() ? `<p class="pop-notice" data-update-notice><button class="link" data-open-main>${icon("refresh")}<span>${esc(updateNoticeHtml())}</span></button></p>` : ""}
    ${nextNoticeText(view) ? `<p class="pop-notice" data-next-notice>${icon("bell")}<span>${esc(nextNoticeText(view))}</span></p>` : ""}
    <footer class="pop-foot">${alertsHtml(view, reports, error) || '<div class="alerts"></div>'}
      <div class="foot-actions">
        <button class="icon" data-open-stats aria-pressed="${showStats}" title="${te("tray.stats")}" aria-label="${te("tray.stats")}">${icon("chart")}</button>
        <button class="icon" data-open-settings title="${te("settings.title")}" aria-label="${te("settings.title")}">${icon("gear")}</button>
        <button class="icon" data-open-main title="${te("common.open")}" aria-label="${te("common.open")}">${icon("window")}</button>
        ${isDesktop ? `<button class="icon danger" data-quit title="${te("common.quit")}" aria-label="${te("common.quit")}">${icon("power")}</button>` : ""}
      </div>
    </footer></div>`;
  models?.attach(root, focus);
  const statsSlot = showStats && view ? root.querySelector("[data-stats-slot]") : null;
  const timelineSlot = !cur && !showStats && reports.length && view ? root.querySelector("[data-timeline-slot]") : null;
  if (statsSlot) {
    statsHost().update(view);
    statsMount.attach(statsSlot, statsFocus);
  } else if (timelineSlot) {
    const { inBar, rest } = splitByMenuBar(reports, view);
    timelineOnly = rest.length ? new Set(inBar.map((r) => r.index)) : null;
    timelineHost().update(view);
    timelineMount.refilter();
    timelineMount.attach(timelineSlot, timelineFocus);
  }
  animateRings(prev);
}

root.addEventListener("click", (e) => {
  const t = e.target;
  const toggle = t.closest("[data-agg-toggle]");
  if (toggle) {
    const key = toggle.dataset.aggToggle;
    if (expanded.has(key)) expanded.delete(key); else expanded.add(key);
    render();
    return root.querySelector(`[data-agg-toggle="${CSS.escape(key)}"]`)?.focus();
  }
  const dashboard = t.closest("[data-agg-dashboard]");
  if (dashboard) return openAggregatorDashboard(Number(dashboard.dataset.aggDashboard)).catch(() => {});
  const open = t.closest("[data-open]");
  if (open) { selected = open.dataset.open; return render(); }
  // PRD 0024 RF7.3: the advice in an account's details points at the best account of its group.
  const show = t.closest("[data-show-account]");
  if (show) {
    const target = store.view?.reports?.find((x) => x.index === Number(show.dataset.showAccount));
    if (target) { selected = reportKey(target); return render(); }
  if (t.closest("[data-back]")) { selected = null; statsOpen = false; return render(); }
  if (t.closest("[data-open-stats]")) { selected = null; statsOpen = !statsOpen; return render(); }
  if (t.closest("[data-view]")) { trayView = trayView === "donut" ? "list" : "donut"; saveTrayView(trayView); return render(); }
  if (t.closest("[data-refresh-all]")) return store.refresh();
  if (t.closest("[data-open-main]")) return openMainWindow();
  if (t.closest("[data-open-settings]")) return openSettingsWindow();
  if (t.closest("[data-quit]")) return quitApp();
});
// PRD 0011 RF4.1: "Open status page" in an account's details.
bindStatusLinks(root);
onHiddenChange(render);
// RF3.2: o shell mostra o popover com `show()` + `set_focus()` e o esconde ao perder o foco. A cada abertura ele
// entra com fade + 6 px vindo do lado do ícone (no topo da tela no macOS, na base no Windows/Linux).
let seen = null; // anéis no último fechar
addEventListener("blur", () => { seen = rings(); });
addEventListener("focus", () => {
  const ms = motionMs("--motion-base"), pop = root.querySelector(".pop");
  if (ms && pop) {
    pop.animate([{ opacity: 0, transform: `translateY(${screenY < screen.height / 2 ? -6 : 6}px)` }, { opacity: 1, transform: "none" }],
      { duration: ms, easing: easeOut() });
  }
  animateRings(seen);
});
initOrderSync(render);
onAggregatorModeChange(render);
bindThemeSwitch(root, render);
initTheme(render);
render();
// PRD 0020 RF1.3/RF7.1: Cmd/Ctrl+, opens Settings; its changes repaint the popover.
bindSettingsShortcut();
onSettingsChanged(() => render());
// PRD 0018 RF2.2: the tray popover shows the same update line as the native
// menu; subscribe only after every render dependency has been initialized.
await initUpdates();
onUpdateState(() => render());

if (window.__TAURI__) window.__TAURI__.event.listen("user-content-changed", () => location.reload());
