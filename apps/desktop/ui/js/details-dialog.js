// Diálogo de detalhes de uma conta (aberto pela lupa do card): quotas completas, histórico, notas, selos e ações.
import { te, tl } from "./i18n.js";
import { acknowledgeMissed, bellHtml, brand, esc, hiddenMetrics, icon, logo, openSettingsWindow, reportKey, statusLabel } from "./lib.js";
import { detailHtml, mountModels } from "./detail.js";
import { costDetailHtml, sourceFor } from "./cost.js";
import { mountHistory } from "./history.js";
import { showDialog } from "./motion.js";
import { openReportProblem } from "./report-dialog.js";
import { connectionInfoHtml, handleConnectionAction, instanceInfoHtml, isAggregator, isConnection } from "./aggregator.js";

let dlg = null;
let currentKey = null;
let ctx = null; // { reports(), generation(), historyEpoch(), view(), localCost(), onEdit(index), onPasteSession(index), onRemove(index, label), onChange(), onBell(key, on), onMetricAlert(key, label, on), onSettings() }
let models = null; // seção de modelos montada uma vez por diálogo; sobrevive aos redesenhos
let history = null; // PRD 0006 RF5.2: idem, para o histórico

const badges = (list) =>
  list.length ? `<div class="badges">${list.map((b) => `<span class="badge ${esc(b.tone)}">${esc(tl(b.text))}</span>`).join("")}</div>` : "";
/** What makes the history worth reading again: a new reading, an erasure or the recording switch. */
const historyStamp = (r) => `${r.checked_at}|${ctx.historyEpoch()}`;

/** PRD 0015 RF4.1: this computer's local usage of the account's tool, the same section on every account of it. */
function localCostHtml(r) {
  const summary = ctx.localCost?.();
  const sc = sourceFor(summary, r.provider);
  return sc ? costDetailHtml(sc, summary) : "";
}

function render() {
  const r = ctx.reports().find((x) => reportKey(x) === currentKey);
  if (!r) return dlg.close(); // a conta foi removida ou mudou de identidade
  // PRD 0036 RF8.5: a 9router connection is not an account of its own: no edit, removal, models or history here
  // (its index is the aggregator's); recovery is "Open dashboard".
  if (isConnection(r)) return renderConnection(r);
  models ??= mountModels({ key: currentKey, index: r.index, generation: ctx.generation() });
  models.update(r.index, ctx.generation());
  history ??= mountHistory({ key: currentKey, index: r.index, generation: ctx.generation(), stamp: historyStamp(r) });
  history.update(r.index, ctx.generation(), historyStamp(r));
  const focus = models.focused() ?? history.focused();
  const id = r.identity;
  dlg.innerHTML = `<div class="dlg-body">
    <div class="det-head">
      ${logo(r.provider, 44)}
      <div class="who"><h2>${esc(brand(r.provider).label)}</h2><p>${esc(r.account)}</p>${id.email && id.email !== r.account ? `<p class="sub2">${esc(id.email)}</p>` : ""}${badges(id.badges)}</div>
      ${r.status !== "ready" ? `<span class="pill st-${r.status}">${esc(statusLabel(r.status))}</span>` : ""}
      <div class="actions">
        ${bellHtml(ctx.view(), r)}
        <button class="icon" data-edit title="${te("common.edit")}" aria-label="${te("common.edit")}">${icon("edit")}</button>
        <button class="icon danger" data-remove title="${te("common.remove")}" aria-label="${te("common.remove")}">${icon("trash")}</button>
      </div>
    </div>
    ${id.copy ? `<div class="copy-row"><button class="chip" data-copy="${esc(id.copy.value)}" title="${te("common.copyLabel", { label: tl(id.copy.label) })}">${icon("copy")}<code>${esc(id.copy.value)}</code></button></div>` : ""}
    ${r.paste_session ? `<div class="dlg-actions"><button type="button" class="btn" data-paste-session>${icon("edit")}<span>${te("card.pasteSession")}</span></button></div>` : ""}
    <div class="det-body">${detailHtml(r, { eye: true })}${isAggregator(r) ? instanceInfoHtml(r) : ""}${localCostHtml(r)}<div data-history-slot></div><div data-models-slot></div></div>
    <div class="dlg-actions">${r.status !== "ready" ? `<button class="btn" data-report>${te("report.action")}</button>` : ""}<button class="btn primary" data-close>${te("common.close")}</button></div>
  </div>`;
  history.attach(dlg);
  models.attach(dlg, focus);
}

function renderConnection(r) {
  dlg.innerHTML = `<div class="dlg-body">
    <div class="det-head">
      ${logo(r.provider, 44)}
      <div class="who"><h2>${esc(brand(r.provider).label)}</h2><p>${esc(r.account)}</p></div>
      ${r.status !== "ready" ? `<span class="pill st-${r.status}">${esc(statusLabel(r.status))}</span>` : ""}
      <div class="actions">${bellHtml(ctx.view(), r)}</div>
    </div>
    <div class="det-body">${detailHtml(r, { eye: true })}${connectionInfoHtml(r, r.index)}</div>
    <div class="dlg-actions"><button class="btn primary" data-close>${te("common.close")}</button></div>
  </div>`;
}

/** `from`: o cartão de origem; o diálogo cresce a partir dele (RF3.1). */
export function openDetails(key, context, from = null) {
  ctx = context;
  currentKey = key;
  // PRD 0032 RF8.2: the "crossed while closed" marker is shown by this first render and recorded as seen.
  const opened = ctx.reports().find((x) => reportKey(x) === key);
  if (opened?.account_key && opened.metrics.some((m) => m.alert?.missed)) acknowledgeMissed(opened.account_key).catch(() => {});
  if (!dlg) {
    dlg = document.createElement("dialog");
    dlg.className = "dlg details";
    document.body.append(dlg);
    dlg.addEventListener("close", () => { models?.dispose(); models = null; history?.dispose(); history = null; dlg = null; });
    dlg.addEventListener("click", (e) => {
      const t = e.target;
      const r = () => ctx.reports().find((x) => reportKey(x) === currentKey);
      if (t.closest("[data-close]")) return dlg.close();
      // PRD 0028 RF2.3: the failing account's secret-free problem report, previewed before anything opens.
      if (t.closest("[data-report]")) { const rep = r(); if (rep) openReportProblem(rep.index, ctx.generation()); return; }
      // PRD 0015 RF4.1: a source that is off points to Settings → Local cost.
      if (t.closest("[data-cost-settings]")) { dlg.close(); return ctx.onSettings(); }
      // PRD 0024 RF7.3: the only action of the advice focuses the other account's card.
      const show = t.closest("[data-show-account]");
      if (show) { dlg.close(); return ctx.onShowAccount?.(Number(show.dataset.showAccount)); }
      const bell = t.closest("[data-bell]");
      if (bell) return ctx.onBell(bell.dataset.bell, bell.dataset.bellOn !== "true");
      // PRD 0008 RF3.2: mute or unmute one metric's usage alerts.
      const metricAlert = t.closest("[data-metric-alert]");
      if (metricAlert) {
        const { metricAlert: key, metricLabel: label, metricOn: on } = metricAlert.dataset;
        return ctx.onMetricAlert(key, label, on !== "true");
      }
      const hide = t.closest("[data-hide]");
      if (hide) { hiddenMetrics.toggle(hide.dataset.hide); render(); return ctx.onChange(); }
      const cp = t.closest("[data-copy]");
      if (cp) {
        navigator.clipboard?.writeText(cp.dataset.copy).then(() => {
          cp.classList.add("done");
          setTimeout(() => cp.classList.remove("done"), 1200);
        }).catch(() => {});
        return;
      }
      // PRD 0036 RF8.5: the aggregator's own dashboard, and its alert mode (each connection or whole pools).
      const dashboard = t.closest("[data-agg-dashboard]");
      if (dashboard) return ctx.onDashboard?.(Number(dashboard.dataset.aggDashboard));
      const settingsPage = t.closest("[data-settings]");
      if (settingsPage) { dlg.close(); return openSettingsWindow({ page: settingsPage.dataset.settings || null }); }
      // PRD 0036 RF8.5: refresh, hide or "show in both" for this connection; a hidden one leaves the dialog.
      if (handleConnectionAction(t, () => { if (t.closest("[data-conn-flag=hidden]")) dlg.close(); ctx.onConnectionChange?.(); }, (x) => ctx.onError?.(x))) return;
      const each = t.closest("[data-pool-each]");
      if (each) return ctx.onPoolAlerts?.(each.dataset.poolEach, each.checked);
      if (t.closest("[data-edit]")) { const rep = r(); dlg.close(); if (rep) ctx.onEdit(rep.index); }
      // SES-01: straight to the account's paste box.
      else if (t.closest("[data-paste-session]")) { const rep = r(); dlg.close(); if (rep) ctx.onPasteSession(rep.index); }
      else if (t.closest("[data-remove]")) {
        const rep = r(); dlg.close();
        if (rep) ctx.onRemove(rep.index, `${brand(rep.provider).label} — ${rep.account}`);
      }
    });
    showDialog(dlg, from);
  }
  render();
}

/** Redesenha o conteúdo quando os dados mudam com o diálogo aberto. */
export function refreshDetails() {
  if (dlg?.open) render();
}
