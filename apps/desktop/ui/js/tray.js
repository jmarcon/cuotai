import {
  bindThemeSwitch, brand, createStore, esc, fmtAmount, fmtReset, icon, initTheme, isDesktop, level,
  logo, openMainWindow, persistentSet, quitApp, remainingPct, reportKey, statusLabel, themeSwitchHtml,
  usedPct, worstRemaining,
} from "./lib.js";

const root = document.getElementById("root");
const disabled = persistentSet("cuotai.disabledAccounts", () => render());
const store = createStore(() => render());
let selected = null;

function row(r) {
  const worst = worstRemaining(r);
  const lv = r.status !== "ready" ? "bad" : level(worst);
  const pct = r.status !== "ready" ? statusLabel[r.status] : worst != null ? `${Math.round(worst)}%` : "—";
  return `<button class="row" data-open="${esc(reportKey(r))}">
    ${logo(r.provider, 32)}
    <div class="row-main">
      <div class="row-top"><b>${esc(brand(r.provider).label)}</b><span class="pct ${lv}">${esc(pct)}</span></div>
      <div class="row-sub">${esc(r.account)}</div>
      <div class="bar thin ${lv}"><span style="width:${worst ?? 0}%"></span></div>
    </div>${icon("chev", "chev")}</button>`;
}

function metricCard(m) {
  const used = usedPct(m);
  const lv = level(remainingPct(m));
  const reset = fmtReset(m.resets_at);
  return `<div class="metric">
    <div class="metric-top">
      <div><h3>${esc(m.label)}</h3><span class="sub">${esc(fmtAmount(m))}</span></div>
      ${used != null ? `<span class="pct ${lv}">${icon(lv === "good" ? "check" : "warn")}${Math.round(used)}%</span>` : ""}
    </div>
    <div class="bar ${lv}"><span style="width:${used ?? 0}%"></span></div>
    ${reset ? `<div class="resets">Reinicia ${esc(reset)}</div>` : ""}
  </div>`;
}

function detail(r) {
  if (!r.metrics.length)
    return `<div class="detail"><div class="metric"><p class="sub">${esc(r.notes[0] ?? statusLabel[r.status])}</p></div></div>`;
  return `<div class="detail">${r.metrics.map(metricCard).join("")}${r.notes.map((n) => `<p class="note">${esc(n)}</p>`).join("")}</div>`;
}

function render() {
  const { snapshot, error, loading } = store;
  const reports = (snapshot?.reports ?? []).filter((r) => !disabled.has(reportKey(r)));
  const cur = reports.find((r) => reportKey(r) === selected) ?? null;
  const ok = reports.length > 0 && reports.every((r) => r.status === "ready");
  const body = cur ? detail(cur) : reports.map(row).join("");
  root.innerHTML = `<div class="pop">
    <header class="pop-head">
      ${cur ? `<button class="icon back" data-back title="Voltar">${icon("back")}</button>` : ""}
      <div class="pop-title"><h1>${esc(cur ? brand(cur.provider).label : "Cuotai")}</h1>
        ${cur ? `<span class="sub">${esc(cur.account)}</span>`
              : `<span class="status ${error ? "bad" : ok ? "good" : "warn"}"><i></i> ${error ? "Falha ao atualizar" : ok ? "Tudo operacional" : "Atenção"}</span>`}
      </div>
      <button class="icon" data-refresh-all title="Atualizar" ${loading ? "disabled" : ""}>${icon("refresh", loading ? "spin" : "")}</button>
    </header>
    <div class="pop-body">${!snapshot && !error ? `<div class="empty">Carregando…</div>` : ""}${error && !snapshot ? `<div class="empty">${esc(error)}</div>` : ""}${body}</div>
    <footer class="pop-foot">${themeSwitchHtml()}
      <button class="link" data-open-main>${icon("settings")} Abrir</button>
      ${isDesktop ? `<button class="link quit" data-quit>${icon("power")} Sair</button>` : ""}
    </footer></div>`;
}

root.addEventListener("click", (e) => {
  const t = e.target;
  const open = t.closest("[data-open]");
  if (open) { selected = open.dataset.open; return render(); }
  if (t.closest("[data-back]")) { selected = null; return render(); }
  if (t.closest("[data-refresh-all]")) return store.refresh();
  if (t.closest("[data-open-main]")) return openMainWindow();
  if (t.closest("[data-quit]")) return quitApp();
});
bindThemeSwitch(root, render);
initTheme(render);
render();
