import { boot } from "./boot.js";
import { te, t, tl } from "./i18n.js";
import { openSettings } from "./settings.js";
import { skippedMessage, userContent } from "./usercontent.js";
import { confirmDelete, offerExternalReload, openAccountDialog } from "./account-dialog.js";
import { openDetails, refreshDetails } from "./details-dialog.js";
import { syncTrayMenu } from "./tray-menu.js";
import {
  fmtNext, ALERTS, alertOf, bindThemeSwitch, bindingChipText, blockedText, brand, createStore, esc, fmtAmount, fmtCountdown, getView, icon, initTheme, isBindingMetric, isBlocked, isDesktop, level, logo,
  moveAccount, noteText, onHiddenChange, disabledAccounts, onDisabledChange, rateLimitOf, reportKey, retryPurge, summaryOf, windowsOf, themeSwitchHtml, langMenuHtml, sortReports, sortMenuHtml,
  currentSort, initOrderSync, statusLabel, configChangedOnDisk, reloadConfig, configLabel, errText, otherConfigNotice,
} from "./lib.js";
import { patch } from "./util.js";
import * as motion from "./motion.js";

await boot();

const root = document.getElementById("root");
const disabled = disabledAccounts;
onDisabledChange(() => render());
const store = createStore(() => render());
const busy = (i) => store.view?.busy.includes(i);

/** Uma janela do card (5h ou semanal). Sem a janela, o espaço continua lá, vazio, para o card manter o tamanho. */
function windowSlot(r, name, w) {
  const label = te(`card.window.${name}`);
  if (!w) {
    return `<div class="win none" aria-label="${label}"><div class="win-top"><span class="win-name">${label}</span></div>
      <div class="win-pct dim">—</div><div class="bar"><span style="width:0"></span></div></div>`;
  }
  const lv = level(w.pct);
  const reset = fmtCountdown(w.m.resets_at);
  const binding = isBindingMetric(r, w.m) ? " binding" : "";
  return `<div class="win${binding}" title="${esc(w.m.label)}"><div class="win-top"><span class="win-name">${label}</span>
      <span class="reset">${reset ? te("quota.resetsIn", { time: reset }) : `<em>${te("quota.na")}</em>`}</span></div>
    <div class="win-pct ${lv}" data-pct="${Math.round(w.pct)}">${Math.round(w.pct)}%</div>
    <div class="bar ${lv}"><span style="width:${w.pct}%"></span></div>
    <div class="sum-amount">${esc(fmtAmount(w.m))}</div></div>`;
}

/** Corpo do resumo: janelas de 5h e semanal (a maioria dos provedores); sem nenhuma delas, a quota mais apertada
 *  (ou o texto de uma métrica sem limite, ou o motivo de não haver). */
function summaryHtml(r) {
  const win = windowsOf(r);
  if (win.h5 || win.week) return `<div class="wins">${windowSlot(r, "h5", win.h5)}${windowSlot(r, "week", win.week)}</div>`;
  const s = summaryOf(r);
  if (s.kind === "bar") {
    const lv = level(s.pct);
    const reset = fmtCountdown(s.m.resets_at);
    return `<div class="sum-bar${isBindingMetric(r, s.m) ? " binding" : ""}">
      <b class="big ${lv}" data-pct="${Math.round(s.pct)}">${Math.round(s.pct)}%<small>${te("card.available")}</small></b>
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
  const custom = currentSort() === "custom" && !filterText.trim(); // com filtro, os vizinhos visíveis não são os da ordem real
  const reorderButtons = custom ? [[-1, "↑"], [1, "↓"]].map(([delta, symbol]) => {
    const target = ordered[position + delta];
    const label = `${te("account.reorder")} ${symbol}`;
    return `<button class="icon move" title="${esc(label)}" aria-label="${esc(label)}" data-move-from="${index}" data-move-to="${target?.index ?? index}" ${!target || reorderPending ? "disabled" : ""}>${symbol}</button>`;
  }).join("") : "";
  // Próxima consulta desta conta (contagem regressiva atualizada a cada segundo, sem redesenhar a tela).
  const nextChip = isBusy
    ? `<span class="next"><span class="nt">${te("card.refreshing")}</span></span>`
    : r.next_refresh_at_ms
      ? `<span class="next" data-next-at="${r.next_refresh_at_ms}" title="${te("card.nextRefresh", { time: fmtNext(r.next_refresh_at_ms) })}">${icon("clock")}<span class="nt">${esc(fmtNext(r.next_refresh_at_ms))}</span></span>`
      : "";
  const alert = alertOf(r);
  const rate = rateLimitOf(r);
  const rateChip = rate != null && level(rate) !== "good"
    ? `<span class="mini ${level(rate)}" title="${te("card.rateLimit", { pct: Math.round(rate) })}">${icon("gauge")}${Math.round(rate)}%</span>` : "";
  const alertChip = alert ? `<span class="mini ${alert.tone}" title="${esc(alert.label())}">${icon(alert.icon)}<span class="mini-t">${esc(alert.label())}</span></span>` : "";
  // PRD 0009 RF2.2: the core's binding window next to the state ("Week · 100%"); an emphasis, the other metrics stay.
  const bindingTitle = blockedText(r) ?? t("card.binding.title");
  const bindingChip = r.status === "ready" && r.binding
    ? `<span class="mini binding ${isBlocked(r) ? "bad" : level(100 - r.binding.used_ratio * 100)}" data-binding-chip title="${esc(bindingTitle)}" aria-label="${esc(`${bindingTitle}: ${bindingChipText(r)}`)}">${esc(bindingChipText(r))}</span>` : "";
  return `<section class="card ${on ? "" : "off"} ${alert?.id === "exhausted" ? "exhausted" : ""}" data-card="${esc(key)}">
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
      ${n ? `<span class="count">${te("quota.count", { count: n })}</span>` : ""}${r.model_count != null ? `<span class="count">${r.model_count ? te("card.modelCount", { count: r.model_count }) : te("card.noModels")}</span>` : ""}${rateChip}${alertChip}${bindingChip}${nextChip}
      <span class="grow"></span>${reorderButtons}
    </div>
  </section>`;
}

/** Conta ainda sem relatório ("Aguardando"): esqueleto com shimmer até os dados chegarem (RF2.3). */
const skeleton = (index) => `<section class="card skeleton" data-card="skeleton:${index}" aria-busy="true">
    <div class="card-head"><span class="sk sk-logo"></span><div class="who"><h2>${te("card.waiting")}</h2><span class="sk"></span></div></div>
    <div class="summary"><span class="sk"></span><span class="sk sk-short"></span></div>
  </section>`;

// Filtro por texto (nome do provedor, da conta, e-mail): sem acentos e sem diferenciar maiúsculas.
let filterText = "";
const norm = (s) => String(s ?? "").normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
const matchesFilter = (r) =>
  !filterText.trim() || norm([brand(r.provider).label, r.account, r.identity?.email, r.provider].join(" ")).includes(norm(filterText.trim()));

// Aviso curto após remover uma conta (o que aconteceu com a credencial no cofre).
// `retry` é opcional: quando presente, o aviso ganha um botão "Tentar
// de novo" que re-executa o `purge_credential` no backend usando o
// mesmo handle. O backend re-roda o shared-key guard do core contra a
// configuração atual; o usuário só precisa recarregar quando a
// sobrevivente cair.
let notice = null;
let noticeTimer = null;
function setNotice(tone, text, retry = null) {
  notice = { tone, text, retry };
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => { notice = null; render(); }, 12000);
  render();
}
function onDeleted(result) {
  if (!result) return;
  const retry = result.recovery_handle ? { handle: result.recovery_handle } : null;
  if (result.outcome === "purged") setNotice("ok", t("delete.purged"));
  else if (result.outcome === "kept") setNotice("ok", t("delete.kept"), retry);
  else if (result.outcome === "kept_shared") setNotice("ok", t("delete.keptShared", { other: result.other_account ?? "" }), retry);
  else if (result.outcome === "partial_failure") setNotice("bad", t("delete.partial", { message: result.message ?? "" }), retry);
}

// Botão "Tentar de novo" do aviso RF3. Reusa `onDeleted` para re-render
// o banner com o resultado fresco (o handle pode voltar como `purged`,
// `partial_failure` com nova mensagem, etc.). Chamada a partir do
// handler global de cliques do shell.
async function retryDelete() {
  if (!notice?.retry) return;
  const handle = notice.retry.handle;
  // Desabilita o aviso enquanto a chamada está em voo para não
  // empilhar retentativas. O `setTimeout` é cancelado pelo próximo
  // `setNotice` quando `onDeleted` rodar de novo.
  const previous = notice;
  notice = { tone: previous.tone, text: previous.text, retry: null };
  render();
  try {
    const result = await retryPurge(handle);
    onDeleted(result);
  } catch (x) {
    setNotice("bad", errText(x), previous.retry);
  }
}

const onSaved = (result) => {
  if (result?.cleanup_warning) setNotice("bad", tl(result.cleanup_warning));
};

const detailsContext = {
  reports: () => store.view?.reports ?? [],
  generation: () => store.view?.generation ?? 0,
  onEdit: (index) => openAccountDialog(index, { onSaved }),
  onRemove: (index, label) =>
    confirmDelete(index, label, { usesOauth: !!store.view?.reports.find((r) => r.index === index)?.uses_oauth, onDone: onDeleted }),
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
  const ordered = sortReports(reports).filter(matchesFilter);
  const loading = !!view && view.busy.length > 0;
  // "Aguardando": contas da configuração que ainda não têm relatório (só antes da primeira resposta de cada uma).
  const have = new Set(reports.map((r) => r.index));
  const waiting = view && !filterText.trim() ? [...Array(view.account_count).keys()].filter((i) => !have.has(i)) : [];
  const prev = motion.before(root);
  // Reconciliação por cartão (`data-card`): foco, cursor do filtro, rolagem e barras sobrevivem ao redesenho.
  patch(root, `<div class="shell">
    <header class="topbar" data-tauri-drag-region>
      <img class="brand" src="img/logo.svg" alt="" width="22" height="22" data-tauri-drag-region /><h1 data-tauri-drag-region>Cuotai</h1><div class="spacer" data-tauri-drag-region></div>
      <div class="filter"><span class="filter-ico">${icon("search")}</span>
        <input id="filter" type="text" autocomplete="off" spellcheck="false" value="${esc(filterText)}" placeholder="${te("filter.placeholder")}" aria-label="${te("filter.placeholder")}" />
        ${filterText ? `<button class="icon" data-clear-filter title="${te("filter.clear")}" aria-label="${te("filter.clear")}">${icon("x")}</button>` : ""}</div>
      ${themeSwitchHtml()}${sortMenuHtml()}${langMenuHtml()}
      <button class="icon" data-settings title="${te("settings.title")}" aria-label="${te("settings.title")}">${icon("gear")}</button>
      <button class="icon" data-add title="${te("common.add")}" aria-label="${te("common.add")}">${icon("plus")}</button>
      <button class="icon" data-refresh-all title="${te("common.refreshAll")}" aria-label="${te("common.refreshAll")}" ${loading ? "disabled" : ""}>${icon("refresh")}</button>
    </header>
    ${error ? `<div class="banner bad">${esc(error)}</div>` : ""}
    ${otherConfigNotice(view) ? `<div class="banner" role="status">${esc(otherConfigNotice(view))}</div>` : ""}
    ${notice ? `<div class="banner ${notice.tone}">${esc(notice.text)}${notice.retry ? ` <button class="link" data-retry-delete>${te("delete.retry")}</button>` : ""} <button class="link" data-dismiss-notice>${te("common.dismiss")}</button></div>` : ""}
    ${userContent.errors.map((e) => `<div class="banner">${esc(skippedMessage(e))}</div>`).join("")}
    ${!view && !error ? `<div class="empty">${te("common.loading")}</div>` : ""}${view && !view.loaded && !error && !waiting.length ? `<div class="empty">${te("common.querying")}</div>` : ""}
    ${view && view.loaded && !reports.length ? `<div class="empty">${te("account.empty")}<br/><br/><button class="btn primary" data-add>${te("common.add")}</button><br/><small>${esc(configLabel(view))}</small></div>` : ""}
    ${view && view.loaded && reports.length && !ordered.length ? `<div class="empty">${te("filter.none")}</div>` : ""}
    <main class="grid">${ordered.map((report, position) => card(report, position, ordered)).join("")}${waiting.map(skeleton).join("")}</main></div>`);
  motion.after(root, prev);
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
  if (t.closest("[data-clear-filter]")) { filterText = ""; return render(); }
  if (t.closest("[data-dismiss-notice]")) { notice = null; return render(); }
  if (t.closest("[data-retry-delete]")) return retryDelete();
  const move = t.closest("[data-move-from]");
  if (move) return reorderAccounts(Number(move.dataset.moveFrom), Number(move.dataset.moveTo));
  const one = t.closest("[data-refresh]");
  if (one) { motion.spinOnce(one.querySelector("svg")); return store.refreshOne(Number(one.dataset.refresh)); }
  const all = t.closest("[data-refresh-all]");
  if (all) { motion.spinOnce(all.querySelector("svg")); return store.refresh(); }
  if (t.closest("[data-add]")) return openAccountDialog(null, { onSaved });
  if (t.closest("[data-settings]")) return openSettings({ onChange: render, view: () => store.view });
  const det = t.closest("[data-details]");
  if (det) return openDetails(det.dataset.details, detailsContext, det.closest("[data-card]"));
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
root.addEventListener("input", (e) => {
  if (e.target.id === "filter") { filterText = e.target.value; render(); }
});
root.addEventListener("keydown", (e) => {
  if (e.target.id === "filter" && e.key === "Escape" && filterText) { e.preventDefault(); filterText = ""; render(); }
});
// "/" foca o filtro (como na TUI), exceto ao digitar em outro campo ou com um diálogo aberto.
addEventListener("keydown", (e) => {
  if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey) return;
  const tag = e.target?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || document.querySelector("dialog[open]")) return;
  e.preventDefault();
  document.getElementById("filter")?.focus();
});
// Contagem regressiva da próxima consulta: atualiza só o texto, uma vez por segundo.
setInterval(() => {
  for (const el of document.querySelectorAll("[data-next-at]")) {
    const text = fmtNext(Number(el.dataset.nextAt));
    const nt = el.querySelector(".nt");
    if (nt && nt.textContent !== text) nt.textContent = text;
    el.title = te("card.nextRefresh", { time: text });
    motion.pulseIfDue(el, Number(el.dataset.nextAt));
  }
}, 1000);
initOrderSync(render);
bindThemeSwitch(root, render);
initTheme(render);
render();

// Eventos do shell: abrir a tela de configurações (vindo do popover) e recarregar quando a pasta do usuário mudar.
if (window.__TAURI__) {
  window.__TAURI__.event.listen("open-settings", () => openSettings({ onChange: render, view: () => store.view }));
  window.__TAURI__.event.listen("user-content-changed", () => location.reload());
}

// RF4: cheap external-change detection. When the window regains focus the
// file may have been edited in another tool; the backend compares bytes
// (`config_changed_on_disk`) and, on drift, the user chooses whether to
// reload. Skipped while a dialog is open: the editor runs the same check
// when it opens and offers reload on a save conflict.
document.addEventListener("visibilitychange", async () => {
  if (document.hidden || document.querySelector("dialog[open]")) return;
  try {
    if (!(await configChangedOnDisk())) return;
    if ((await offerExternalReload()) === "reload") await reloadConfig();
  } catch (x) {
    setNotice("bad", errText(x));
  }
});
