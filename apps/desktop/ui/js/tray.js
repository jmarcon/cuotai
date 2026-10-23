import { boot } from "./boot.js";
import { te, t, currentLocale } from "./i18n.js";
import { detailHtml, mountModels } from "./detail.js";
import { syncTrayMenu } from "./tray-menu.js";
import { easeOut, motionMs } from "./motion.js";
import {
  bindThemeSwitch, brand, createStore, esc, icon, initTheme, isDesktop, level,
  logo, noteText, openMainWindow, disabledAccounts, onDisabledChange, quitApp, reportKey, statusLabel, langMenuHtml, sortReports, sortMenuHtml, initOrderSync, openSettingsWindow,
  bindingPct, bindingChipText, blockedText, ALERTS, ageHtml, ageText, alertOf, windowsOf, onHiddenChange, cardPace, paceHtml, paceText, nextNoticeText,
  incidentBadge, incidentHtml, bindStatusLinks, wordmark,
} from "./lib.js";
import { initUpdates, onUpdateState } from "./updater.js";
import { trayUpdateLine } from "./tray-menu.js";

await boot();

const root = document.getElementById("root");
const disabled = disabledAccounts;
onDisabledChange(() => render());
const store = createStore(() => render());
let selected = null;
let models = null; // seção de modelos da conta aberta no popover; sobrevive aos redesenhos

function row(r) {
  const pct = bindingPct(r);
  const blocked = blockedText(r);
  const lv = r.status !== "ready" || blocked ? "bad" : level(pct);
  const value = r.status !== "ready" ? statusLabel(r.status) : blocked ?? (pct != null ? `${Math.round(pct)}%` : "—");
  // PRD 0008 RF3.4: an active usage alert highlights the row.
  return `<button class="row${r.usage_alerts?.length ? " usage-alert" : ""}" data-open="${esc(reportKey(r))}">
    ${logo(r.provider, 32)}
    <div class="row-main">
      <div class="row-top"><b>${esc(brand(r.provider).label)}</b><span class="pct ${lv}">${esc(value)}</span></div>
      <div class="row-sub">${esc(r.account)}${r.binding ? ` · ${esc(bindingChipText(r))}` : ""}</div>
      ${r.usage_alerts?.length ? `<div class="row-alert">${icon("bell")}<span>${esc(ALERTS.find((c) => c.id === "usage").label(r))}</span></div>` : ""}
      <div class="row-age">${ageHtml(r)}</div>
      ${incidentHtml(r, { link: false })}
      ${trayPace(r, (p, label) => paceHtml(p, { label, short: true }))}
      <div class="bar thin ${lv}"><span style="width:${pct ?? 0}%"></span></div>
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
    <span class="tile-acct">${esc(r.account)}</span>${ageHtml(r)}</button>`;
}
const te2 = (key, vars) => t(key, vars);
/** PRD 0007 RF3.3: "Week: lasts until the reset" / "Week: runs out at 16:40, 2h 10m before the reset". */
const trayPace = (r, draw) => {
  const m = cardPace(r);
  return m ? draw(m.pace, t(`card.binding.${m.window}`)) : "";
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
  const prev = rings();
  root.innerHTML = `<div class="pop">
    <header class="pop-head">
      ${cur ? `<button class="icon back" data-back title="${te("common.back")}">${icon("back")}</button>` : ""}
      <div class="pop-title"><h1>${cur ? esc(brand(cur.provider).label) : `<button class="brand-link wordmark" data-open-main title="${te("common.open")}" aria-label="${te("common.open")}">${wordmark()}</button>`}</h1>
        ${cur ? `<span class="sub">${esc(cur.account)}</span>` : ""}
      </div>
      ${cur ? "" : `<button class="icon" data-view title="${te(trayView === "donut" ? "tray.view.list" : "tray.view.donut")}" aria-label="${te(trayView === "donut" ? "tray.view.list" : "tray.view.donut")}">${icon(trayView === "donut" ? "list" : "grid")}</button>`}
      ${sortMenuHtml()}${langMenuHtml()}
      <button class="icon" data-refresh-all title="${te("common.refresh")}" ${loading ? "disabled" : ""}>${icon("refresh", loading ? "spin" : "")}</button>
    </header>
    <div class="pop-body">${!view && !error ? `<div class="empty">${te("common.loading")}</div>` : ""}${error && !view?.reports.length ? `<div class="empty">${esc(error)}</div>` : ""}${body}</div>
    ${updateNoticeHtml() ? `<p class="pop-notice" data-update-notice><button class="link" data-open-main>${icon("refresh")}<span>${esc(updateNoticeHtml())}</span></button></p>` : ""}
    ${nextNoticeText(view) ? `<p class="pop-notice" data-next-notice>${icon("bell")}<span>${esc(nextNoticeText(view))}</span></p>` : ""}
    <footer class="pop-foot">${alertsHtml(view, reports, error) || '<div class="alerts"></div>'}
      <div class="foot-actions">
        <button class="icon" data-open-settings title="${te("settings.title")}" aria-label="${te("settings.title")}">${icon("gear")}</button>
        <button class="icon" data-open-main title="${te("common.open")}" aria-label="${te("common.open")}">${icon("window")}</button>
        ${isDesktop ? `<button class="icon danger" data-quit title="${te("common.quit")}" aria-label="${te("common.quit")}">${icon("power")}</button>` : ""}
      </div>
    </footer></div>`;
  models?.attach(root, focus);
  animateRings(prev);
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
bindThemeSwitch(root, render);
initTheme(render);
render();
// PRD 0018 RF2.2: the tray popover shows the same update line as the native
// menu; subscribe only after every render dependency has been initialized.
await initUpdates();
onUpdateState(() => render());

if (window.__TAURI__) window.__TAURI__.event.listen("user-content-changed", () => location.reload());
