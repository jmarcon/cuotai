import { boot } from "./boot.js";
import { te, t, currentLocale } from "./i18n.js";
import { detailHtml, mountModels } from "./detail.js";
import { syncTrayMenu } from "./tray-menu.js";
import {
  bindThemeSwitch, brand, createStore, esc, fmtAmount, fmtReset, icon, initTheme, isDesktop, level,
  logo, noteText, openMainWindow, disabledAccounts, onDisabledChange, quitApp, remainingPct, reportKey, statusLabel, langMenuHtml, sortReports, sortMenuHtml, initOrderSync, openSettingsWindow,
  textualMetric, worstRemaining, ALERTS, alertOf, windowsOf, isMetricHidden, onHiddenChange,
} from "./lib.js";

await boot();

const root = document.getElementById("root");
const disabled = disabledAccounts;
onDisabledChange(() => render());
const store = createStore(() => render());
let selected = null;
let models = null; // seção de modelos da conta aberta no popover; sobrevive aos redesenhos

function row(r) {
  const worst = worstRemaining(r);
  const lv = r.status !== "ready" ? "bad" : level(worst);
  const pct = r.status !== "ready" ? statusLabel(r.status) : worst != null ? `${Math.round(worst)}%` : "—";
  return `<button class="row" data-open="${esc(reportKey(r))}">
    ${logo(r.provider, 32)}
    <div class="row-main">
      <div class="row-top"><b>${esc(brand(r.provider).label)}</b><span class="pct ${lv}">${esc(pct)}</span></div>
      <div class="row-sub">${esc(r.account)}</div>
      <div class="bar thin ${lv}"><span style="width:${worst ?? 0}%"></span></div>
    </div>${icon("chev", "chev")}</button>`;
}

// Vista do popover: donuts (mais contas na mesma tela) ou lista. Preferência salva.
const VIEW_KEY = "cuotai.trayView";
let trayView = (() => { try { return localStorage.getItem(VIEW_KEY) === "list" ? "list" : "donut"; } catch { return "donut"; } })();
addEventListener("storage", (e) => { if (e.key === VIEW_KEY) { trayView = e.newValue === "list" ? "list" : "donut"; render(); } });

const R = 26, CIRC = 2 * Math.PI * R;
/** Anel de porcentagem disponível; sem medida, o anel fica vazio. `center` é o conteúdo (número ou ícone). */
function donutHtml(pct, tone, center) {
  const dash = pct == null ? 0 : (Math.max(0, Math.min(100, pct)) / 100) * CIRC;
  return `<div class="donut ${tone}"><svg viewBox="0 0 64 64" aria-hidden="true">
    <circle class="trk" cx="32" cy="32" r="${R}"/>
    ${dash >= 1 ? `<circle class="arc" cx="32" cy="32" r="${R}" stroke-dasharray="${dash.toFixed(1)} ${CIRC.toFixed(1)}"/>` : ""}</svg>
    <span class="center">${center}</span></div>`;
}

function tile(r) {
  const alert = alertOf(r);
  const worst = r.status === "ready" ? worstRemaining(r) : null;
  const lv = level(worst);
  const w = windowsOf(r);
  const tip = [`${brand(r.provider).label} (${r.account})`,
    alert ? alert.label() : worst != null ? te2("metric.level." + lv, { pct: Math.round(worst) }) : "",
    ...(w.h5 ? [`${t("card.window.h5")}: ${Math.round(w.h5.pct)}%`] : []),
    ...(w.week ? [`${t("card.window.week")}: ${Math.round(w.week.pct)}%`] : [])].filter(Boolean);
  const center = alert ? icon(alert.icon) : worst != null ? `${Math.round(worst)}%` : "—";
  const tone = alert ? alert.tone : lv;
  return `<button class="tile" data-open="${esc(reportKey(r))}" title="${esc(tip.join("\n"))}">
    ${donutHtml(alert ? 100 : worst, tone, center)}
    <span class="tile-name">${logo(r.provider, 14)}<b>${esc(brand(r.provider).label)}</b></span>
    <span class="tile-acct">${esc(r.account)}</span></button>`;
}
const te2 = (key, vars) => t(key, vars);

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
  if (!groups.length)
    return `<div class="alerts">${chip("good", "check", null, [t("tray.allOkTitle", { count: reports.length }), last].filter(Boolean), t("tray.ok"))}</div>`;
  return `<div class="alerts" role="group" aria-label="${te("tray.attention")}">${groups.map(({ c, items }) => {
    const lines = [t("tray.catTitle", { label: c.label(), count: items.length }),
      ...items.slice(0, 8).map((r) => `• ${brand(r.provider).label} (${r.account})${r.notes[0] && c.id !== "exhausted" ? ` — ${short(noteText(r, 0))}` : ""}`)];
    if (items.length > 8) lines.push(t("tray.more", { count: items.length - 8 }));
    if (last) lines.push(last);
    return chip(c.tone, c.icon, items.length, lines, c.label());
  }).join("")}</div>`;
}

function render() {
  const { view, error } = store;
  const loading = !!view && view.busy.length > 0;
  const reports = sortReports(view?.reports ?? []).filter((r) => !disabled.has(reportKey(r)));
  const cur = reports.find((r) => reportKey(r) === selected) ?? null;
  if (models && (!cur || models.key !== selected)) { models.dispose(); models = null; }
  if (cur) {
    models ??= mountModels({ key: selected, index: cur.index, generation: view.generation });
    models.update(cur.index, view.generation);
  }
  const focus = models?.focused();
  const body = cur ? `${detailHtml(cur)}<div data-models-slot></div>`
    : trayView === "donut" ? `<div class="tiles">${reports.map(tile).join("")}</div>` : reports.map(row).join("");
  syncTrayMenu(view);
  root.innerHTML = `<div class="pop">
    <header class="pop-head">
      ${cur ? `<button class="icon back" data-back title="${te("common.back")}">${icon("back")}</button>` : ""}
      <div class="pop-title"><h1>${cur ? esc(brand(cur.provider).label) : `<button class="brand-link" data-open-main title="${te("common.open")}" aria-label="${te("common.open")}"><img class="brand" src="img/logo.svg" alt="" width="18" height="18" />Cuotai</button>`}</h1>
        ${cur ? `<span class="sub">${esc(cur.account)}</span>` : ""}
      </div>
      ${cur ? "" : `<button class="icon" data-view title="${te(trayView === "donut" ? "tray.view.list" : "tray.view.donut")}" aria-label="${te(trayView === "donut" ? "tray.view.list" : "tray.view.donut")}">${icon(trayView === "donut" ? "list" : "grid")}</button>`}
      ${sortMenuHtml()}${langMenuHtml()}
      <button class="icon" data-refresh-all title="${te("common.refresh")}" ${loading ? "disabled" : ""}>${icon("refresh", loading ? "spin" : "")}</button>
    </header>
    <div class="pop-body">${!view && !error ? `<div class="empty">${te("common.loading")}</div>` : ""}${error && !view?.reports.length ? `<div class="empty">${esc(error)}</div>` : ""}${body}</div>
    <footer class="pop-foot">${alertsHtml(view, reports, error) || '<div class="alerts"></div>'}
      <div class="foot-actions">
        <button class="icon" data-open-settings title="${te("settings.title")}" aria-label="${te("settings.title")}">${icon("gear")}</button>
        <button class="icon" data-open-main title="${te("common.open")}" aria-label="${te("common.open")}">${icon("window")}</button>
        ${isDesktop ? `<button class="icon danger" data-quit title="${te("common.quit")}" aria-label="${te("common.quit")}">${icon("power")}</button>` : ""}
      </div>
    </footer></div>`;
  models?.attach(root, focus);
}

root.addEventListener("click", (e) => {
  const t = e.target;
  const open = t.closest("[data-open]");
  if (open) { selected = open.dataset.open; return render(); }
  if (t.closest("[data-back]")) { selected = null; return render(); }
  if (t.closest("[data-view]")) { trayView = trayView === "donut" ? "list" : "donut"; try { localStorage.setItem(VIEW_KEY, trayView); } catch {} return render(); }
  if (t.closest("[data-refresh-all]")) return store.refresh();
  if (t.closest("[data-open-main]")) return openMainWindow();
  if (t.closest("[data-open-settings]")) return openSettingsWindow();
  if (t.closest("[data-quit]")) return quitApp();
});
onHiddenChange(render);
initOrderSync(render);
bindThemeSwitch(root, render);
initTheme(render);
render();

if (window.__TAURI__) window.__TAURI__.event.listen("user-content-changed", () => location.reload());
