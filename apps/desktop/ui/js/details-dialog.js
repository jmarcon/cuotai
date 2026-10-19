// Diálogo de detalhes de uma conta (aberto pela lupa do card): quotas completas, notas, selos e ações.
import { te, tl } from "./i18n.js";
import { brand, esc, hiddenMetrics, icon, logo, reportKey, statusLabel } from "./lib.js";
import { detailHtml, mountModels } from "./detail.js";
import { showDialog } from "./motion.js";

let dlg = null;
let currentKey = null;
let ctx = null; // { reports(), generation(), onEdit(index), onRemove(index, label), onChange() }
let models = null; // seção de modelos montada uma vez por diálogo; sobrevive aos redesenhos

const badges = (list) =>
  list.length ? `<div class="badges">${list.map((b) => `<span class="badge ${esc(b.tone)}">${esc(tl(b.text))}</span>`).join("")}</div>` : "";

function render() {
  const r = ctx.reports().find((x) => reportKey(x) === currentKey);
  if (!r) return dlg.close(); // a conta foi removida ou mudou de identidade
  models ??= mountModels({ key: currentKey, index: r.index, generation: ctx.generation() });
  models.update(r.index, ctx.generation());
  const focus = models.focused();
  const id = r.identity;
  dlg.innerHTML = `<div class="dlg-body">
    <div class="det-head">
      ${logo(r.provider, 44)}
      <div class="who"><h2>${esc(brand(r.provider).label)}</h2><p>${esc(r.account)}</p>${id.email && id.email !== r.account ? `<p class="sub2">${esc(id.email)}</p>` : ""}${badges(id.badges)}</div>
      ${r.status !== "ready" ? `<span class="pill st-${r.status}">${esc(statusLabel(r.status))}</span>` : ""}
      <div class="actions">
        <button class="icon" data-edit title="${te("common.edit")}" aria-label="${te("common.edit")}">${icon("edit")}</button>
        <button class="icon danger" data-remove title="${te("common.remove")}" aria-label="${te("common.remove")}">${icon("trash")}</button>
      </div>
    </div>
    ${id.copy ? `<div class="copy-row"><button class="chip" data-copy="${esc(id.copy.value)}" title="${te("common.copyLabel", { label: tl(id.copy.label) })}">${icon("copy")}<code>${esc(id.copy.value)}</code></button></div>` : ""}
    <div class="det-body">${detailHtml(r, { eye: true })}<div data-models-slot></div></div>
    <div class="dlg-actions"><button class="btn primary" data-close>${te("common.close")}</button></div>
  </div>`;
  models.attach(dlg, focus);
}

/** `from`: o cartão de origem; o diálogo cresce a partir dele (RF3.1). */
export function openDetails(key, context, from = null) {
  ctx = context;
  currentKey = key;
  if (!dlg) {
    dlg = document.createElement("dialog");
    dlg.className = "dlg details";
    document.body.append(dlg);
    dlg.addEventListener("close", () => { models?.dispose(); models = null; dlg = null; });
    dlg.addEventListener("click", (e) => {
      const t = e.target;
      const r = () => ctx.reports().find((x) => reportKey(x) === currentKey);
      if (t.closest("[data-close]")) return dlg.close();
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
      if (t.closest("[data-edit]")) { const rep = r(); dlg.close(); if (rep) ctx.onEdit(rep.index); }
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
