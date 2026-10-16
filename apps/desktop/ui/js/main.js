import { boot } from "./boot.js";
import { te, t } from "./i18n.js";
import { openSettings } from "./settings.js";
import { skippedMessage, userContent } from "./usercontent.js";
import { confirmDelete, openAccountDialog } from "./account-dialog.js";
import { openDetails, refreshDetails } from "./details-dialog.js";
import { syncTrayMenu } from "./tray-menu.js";
import {
  ALERTS, alertOf, bindThemeSwitch, brand, createStore, esc, fmtAmount, fmtCountdown, getView, icon, initTheme, isDesktop, level, logo,
  moveAccount, noteText, onHiddenChange, disabledAccounts, onDisabledChange, rateLimitOf, reportKey, summaryOf, windowsOf, themeSwitchHtml, langMenuHtml, sortReports, sortMenuHtml,
  currentSort, initOrderSync, statusLabel,
} from "./lib.js";

await boot();

const root = document.getElementById("root");
const disabled = disabledAccounts;
onDisabledChange(() => render());
const store = createStore(() => render());
const busy = (i) => store.view?.busy.includes(i);

/** Uma janela do card (5h ou semanal). Sem a janela, o espaço continua lá, vazio, para o card manter o tamanho. */
function windowSlot(name, w) {
  const label = te(`card.window.${name}`);
  if (!w) {
    return `<div class="win none" aria-label="${label}"><div class="win-top"><span class="win-name">${label}</span></div>
      <div class="win-pct dim">—</div><div class="bar"><span style="width:0"></span></div></div>`;
  }
  const lv = level(w.pct);
  const reset = fmtCountdown(w.m.resets_at);
  return `<div class="win" title="${esc(w.m.label)}"><div class="win-top"><span class="win-name">${label}</span>
      <span class="reset">${reset ? te("quota.resetsIn", { time: reset }) : `<em>${te("quota.na")}</em>`}</span></div>
    <div class="win-pct ${lv}">${Math.round(w.pct)}%</div>
    <div class="bar ${lv}"><span style="width:${w.pct}%"></span></div>
    <div class="sum-amount">${esc(fmtAmount(w.m))}</div></div>`;
}

/** Corpo do resumo: janelas de 5h e semanal (a maioria dos provedores); sem nenhuma delas, a quota mais apertada
 *  (ou o texto de uma métrica sem limite, ou o motivo de não haver). */
function summaryHtml(r) {
  const win = windowsOf(r);
  if (win.h5 || win.week) return `<div class="wins">${windowSlot("h5", win.h5)}${windowSlot("week", win.week)}</div>`;
  const s = summaryOf(r);
  if (s.kind === "bar") {
    const lv = level(s.pct);
    const reset = fmtCountdown(s.m.resets_at);
    return `<div class="sum-bar">
      <b class="big ${lv}">${Math.round(s.pct)}%<small>${te("card.available")}</small></b>
      <div class="sum-meter">
        <div class="sum-top"><span class="lt" title="${esc(s.m.label)}">${esc(s.m.label)}</span><span class="reset">${reset ? te("quota.resetsIn", { time: reset }) : `<em>${te("quota.na")}</em>`}</span></div>
        <div class="bar ${lv}"><span style="width:${s.pct}%"></span></div>
        <div class="sum-amount">${esc(fmtAmount(s.m))}</div>
      </div></div>`;
  }
  if (s.kind === "text") {
    const reset = fmtCountdown(s.m.resets_at);
    return `<div class="sum-text">
      <div class="sum-top"><span class="lt" title="${esc(s.m.label)}">${esc(s.m.label)}</span><span class="reset">${reset ? te("quota.resetsIn", { time: reset }) : ""}</span></div>
      <div class="balance ${s.m.state === "exhausted" ? "bad" : ""}">${esc(s.text)}</div></div>`;
  }
  if (s.kind === "hidden") return `<p class="sum-note dim">${te("card.allHidden")}</p>`;
  return `<p class="sum-note">${esc(r.notes.length ? noteText(r, 0) : statusLabel(r.status))}</p>`;
}

function card(r, position, ordered) {
  const index = r.index;
  const key = reportKey(r);
  const on = !disabled.has(key);
  const isBusy = busy(index);
  const n = r.metrics.length;
  const custom = currentSort() === "custom";
  const reorderButtons = custom ? [[-1, "↑"], [1, "↓"]].map(([delta, symbol]) => {
    const target = ordered[position + delta];
    const label = `${te("account.reorder")} ${symbol}`;
    return `<button class="icon move" title="${esc(label)}" aria-label="${esc(label)}" data-move-from="${index}" data-move-to="${target?.index ?? index}" ${!target || reorderPending ? "disabled" : ""}>${symbol}</button>`;
  }).join("") : "";
  const alert = alertOf(r);
  const rate = rateLimitOf(r);
  const rateChip = rate != null && level(rate) !== "good"
    ? `<span class="mini ${level(rate)}" title="${te("card.rateLimit", { pct: Math.round(rate) })}">${icon("gauge")}${Math.round(rate)}%</span>` : "";
  const alertChip = alert ? `<span class="mini ${alert.tone}" title="${esc(alert.label())}">${icon(alert.icon)}<span class="mini-t">${esc(alert.label())}</span></span>` : "";
  return `<section class="card ${on ? "" : "off"}" data-card="${esc(key)}">
    <div class="card-head">
      ${custom ? `<button class="icon grip" draggable="${!reorderPending}" data-grip="${esc(key)}" title="${te("account.reorder")}" aria-label="${te("account.reorder")}">${icon("grip")}</button>` : ""}
      ${logo(r.provider)}
      <div class="who"><h2 title="${esc(brand(r.provider).label)}">${esc(brand(r.provider).label)}</h2><p title="${esc(r.account)}">${esc(r.account)}</p></div>
      <div class="actions">
        <button class="icon" title="${te("common.refresh")}" data-refresh="${index}" ${!on || isBusy ? "disabled" : ""}>${icon("refresh", isBusy ? "spin" : "")}</button>
        <button class="icon" title="${te("card.details")}" aria-label="${te("card.details")}" data-details="${esc(key)}">${icon("search")}</button>
        <button class="switch ${on ? "on" : ""}" role="switch" aria-checked="${on}" title="${te(on ? "account.disable" : "account.enable")}" data-toggle="${esc(key)}"><span></span></button>
      </div>
    </div>
    <div class="summary">${summaryHtml(r)}</div>
    <div class="card-foot">
      ${n ? `<span class="count">${te("quota.count", { count: n })}</span>` : ""}${rateChip}${alertChip}
      <span class="grow"></span>${reorderButtons}
    </div>
  </section>`;
}

const detailsContext = {
  reports: () => store.view?.reports ?? [],
  onEdit: (index) => openAccountDialog(index),
  onRemove: (index, label) => confirmDelete(index, label),
  onChange: () => render(),
};
onHiddenChange(() => { render(); refreshDetails(); });

let dragKey = null;
let renderPending = false;
let reorderPending = false;
function render() {
  refreshDetails();
  syncTrayMenu(store.view);
  if (dragKey) { renderPending = true; return; } // não reconstrói a tela no meio de um arraste
  const { view, error } = store;
  const reports = view?.reports ?? [];
  const ordered = sortReports(reports);
  const loading = !!view && view.busy.length > 0;
  root.innerHTML = `<div class="shell">
    <header class="topbar" data-tauri-drag-region>
      <img class="brand" src="img/logo.svg" alt="" width="22" height="22" data-tauri-drag-region /><h1 data-tauri-drag-region>Cuotai</h1><div class="spacer" data-tauri-drag-region></div>
      ${themeSwitchHtml()}${sortMenuHtml()}${langMenuHtml()}
      <button class="icon" data-settings title="${te("settings.title")}" aria-label="${te("settings.title")}">${icon("gear")}</button>
      <button class="icon" data-add title="${te("common.add")}" aria-label="${te("common.add")}">${icon("plus")}</button>
      <button class="icon" data-refresh-all title="${te("common.refreshAll")}" aria-label="${te("common.refreshAll")}" ${loading ? "disabled" : ""}>${icon("refresh", loading ? "spin" : "")}</button>
    </header>
    ${error ? `<div class="banner bad">${esc(error)}</div>` : ""}
    ${userContent.errors.map((e) => `<div class="banner">${esc(skippedMessage(e))}</div>`).join("")}
    ${!view && !error ? `<div class="empty">${te("common.loading")}</div>` : ""}${view && !view.loaded && !error ? `<div class="empty">${te("common.querying")}</div>` : ""}
    ${view && view.loaded && !reports.length ? `<div class="empty">${te("account.empty")}<br/><br/><button class="btn primary" data-add>${te("common.add")}</button><br/><small>${esc(view.config_path ?? "")}</small></div>` : ""}
    <main class="grid">${ordered.map((report, position) => card(report, position, ordered)).join("")}</main></div>`;
}

root.addEventListener("click", (e) => {
  const t = e.target;
  const cp = t.closest("[data-copy]");
  if (cp) {
    navigator.clipboard?.writeText(cp.dataset.copy).then(() => {
      cp.classList.add("done");
      setTimeout(() => cp.classList.remove("done"), 1200);
    }).catch(() => {});
    return;
  }
  const move = t.closest("[data-move-from]");
  if (move) return reorderAccounts(Number(move.dataset.moveFrom), Number(move.dataset.moveTo));
  const one = t.closest("[data-refresh]");
  if (one) return store.refreshOne(Number(one.dataset.refresh));
  if (t.closest("[data-refresh-all]")) return store.refresh();
  if (t.closest("[data-add]")) return openAccountDialog();
  if (t.closest("[data-settings]")) return openSettings({ configPath: store.view?.config_path, onChange: render });
  const det = t.closest("[data-details]");
  if (det) return openDetails(det.dataset.details, detailsContext);
  const tog = t.closest("[data-toggle]");
  if (tog) { disabled.toggle(tog.dataset.toggle); return render(); }
});
// Reorder through the core so the TOML, reports, deadlines and both
// frontends share one order. No refresh command is issued by this path.
async function reorderAccounts(from, to, focusKey = null) {
  if (reorderPending || from === to || !Number.isInteger(from) || !Number.isInteger(to)) return;
  reorderPending = true;
  render();
  try {
    await moveAccount(from, to);
    if (!isDesktop) {
      store.view = await getView();
      store.error = store.view.error ?? null;
    }
  } catch (error) {
    store.error = String(error);
  } finally {
    reorderPending = false;
    render();
    if (focusKey) root.querySelector(`[data-grip="${CSS.escape(focusKey)}"]`)?.focus();
  }
}

// Mouse: drag the grip, drop on another card, or click the explicit arrows.
const clearDrop = () => root.querySelectorAll(".drop").forEach((el) => el.classList.remove("drop"));
root.addEventListener("dragstart", (e) => {
  const grip = e.target.closest?.("[data-grip]");
  if (!grip) return;
  if (reorderPending) return e.preventDefault();
  dragKey = grip.dataset.grip;
  e.dataTransfer.effectAllowed = "move";
  e.dataTransfer.setData("text/plain", dragKey);
  const cardEl = grip.closest("[data-card]");
  e.dataTransfer.setDragImage(cardEl, 24, 24);
  requestAnimationFrame(() => cardEl.classList.add("dragging"));
});
root.addEventListener("dragover", (e) => {
  const target = e.target.closest?.("[data-card]");
  if (!dragKey || !target) return;
  e.preventDefault();
  clearDrop();
  if (target.dataset.card !== dragKey) target.classList.add("drop");
});
root.addEventListener("drop", (e) => {
  const target = e.target.closest?.("[data-card]");
  if (!dragKey || !target) return;
  e.preventDefault();
  const reports = store.view?.reports ?? [];
  const from = reports.find((r) => reportKey(r) === dragKey)?.index;
  const to = reports.find((r) => reportKey(r) === target.dataset.card)?.index;
  if (from != null && to != null) reorderAccounts(from, to);
});
root.addEventListener("dragend", () => {
  dragKey = null;
  clearDrop();
  if (renderPending) { renderPending = false; }
  render();
});

// Keyboard: the same arrow keys advertised by the grip move by one card.
root.addEventListener("keydown", (e) => {
  const grip = e.target.closest?.("[data-grip]");
  const delta = { ArrowUp: -1, ArrowLeft: -1, ArrowDown: 1, ArrowRight: 1 }[e.key];
  if (!grip || !delta) return;
  e.preventDefault();
  const key = grip.dataset.grip;
  const ordered = sortReports(store.view?.reports ?? []);
  const position = ordered.findIndex((r) => reportKey(r) === key);
  const target = ordered[position + delta];
  if (position >= 0 && target) reorderAccounts(ordered[position].index, target.index, key);
});
initOrderSync(render);
bindThemeSwitch(root, render);
initTheme(render);
render();

// Eventos do shell: abrir a tela de configurações (vindo do popover) e recarregar quando a pasta do usuário mudar.
if (window.__TAURI__) {
  window.__TAURI__.event.listen("open-settings", () => openSettings({ configPath: store.view?.config_path, onChange: render }));
  window.__TAURI__.event.listen("user-content-changed", () => location.reload());
}
