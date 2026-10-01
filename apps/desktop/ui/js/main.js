import {
  bindThemeSwitch, brand, createStore, esc, fmtAmount, fmtCountdown, icon, initTheme, level, logo,
  persistentSet, remainingPct, reportKey, statusLabel, themeSwitchHtml,
} from "./lib.js";

const root = document.getElementById("root");
let showHidden = false;
const disabled = persistentSet("cuotai.disabledAccounts", () => render());
const hidden = persistentSet("cuotai.hiddenMetrics", () => render());
const store = createStore(() => render());

function quotaRow(key, m) {
  const pct = remainingPct(m);
  const lv = level(pct);
  const id = `${key}::${m.label}`;
  const isHidden = hidden.has(id);
  const reset = fmtCountdown(m.resets_at);
  return `<li class="quota ${isHidden ? "muted" : ""}">
    <span class="label"><i class="dot ${lv}"></i>${esc(m.label)}</span>
    <div class="meter">
      <div class="bar ${lv}"><span style="width:${pct ?? 0}%"></span></div>
      <div class="numbers"><span>${esc(fmtAmount(m))}</span>${pct != null ? `<b class="${lv}">${Math.round(pct)}%</b>` : ""}</div>
    </div>
    <span class="reset">${reset ? `in ${reset}` : "<em>N/A</em>"}</span>
    <button class="icon" title="${isHidden ? "Mostrar" : "Ocultar"}" data-hide="${esc(id)}">${icon(isHidden ? "eye" : "eyeOff", "")}</button>
  </li>`;
}

function card(r, index) {
  const key = reportKey(r);
  const on = !disabled.has(key);
  const busy = store.busy.has(index);
  const n = r.metrics.length;
  const visible = r.metrics.filter((m) => showHidden || !hidden.has(`${key}::${m.label}`));
  const body = n
    ? `<div class="quota-count">${n} ${n === 1 ? "quota" : "quotas"}</div><ul class="quotas">${visible.map((m) => quotaRow(key, m)).join("")}</ul>`
    : `<div class="notes">${(r.notes.length ? r.notes : [statusLabel[r.status]]).map((t) => `<p>${esc(t)}</p>`).join("")}</div>`;
  return `<section class="card ${on ? "" : "off"}">
    <div class="card-head">
      ${logo(r.provider)}
      <div class="who"><h2>${esc(brand(r.provider).label)}</h2><p>${esc(r.account)}</p></div>
      ${r.status !== "ready" ? `<span class="pill st-${r.status}">${statusLabel[r.status]}</span>` : ""}
      <div class="actions">
        <button class="icon" title="Atualizar" data-refresh="${index}" ${!on || busy ? "disabled" : ""}>${icon("refresh", busy ? "spin" : "")}</button>
        <button class="icon" title="Editar (em breve)" disabled>${icon("edit")}</button>
        <button class="icon danger" title="Remover (em breve)" disabled>${icon("trash")}</button>
        <button class="switch ${on ? "on" : ""}" role="switch" aria-checked="${on}" title="${on ? "Desativar no painel" : "Ativar no painel"}" data-toggle="${esc(key)}"><span></span></button>
      </div>
    </div>${body}</section>`;
}

function render() {
  const { snapshot, error, loading } = store;
  const reports = snapshot?.reports ?? [];
  root.innerHTML = `<div class="shell">
    <header class="topbar" data-tauri-drag-region>
      <h1 data-tauri-drag-region>Cuotai</h1><div class="spacer" data-tauri-drag-region></div>
      <label class="check"><input type="checkbox" id="show-hidden" ${showHidden ? "checked" : ""}/> Mostrar ocultas</label>
      ${themeSwitchHtml()}
      <button class="btn" data-refresh-all ${loading ? "disabled" : ""}>${icon("refresh", loading ? "spin" : "")} Atualizar tudo</button>
    </header>
    ${error ? `<div class="banner bad">${esc(error)}</div>` : ""}
    ${!snapshot && !error ? `<div class="empty">Carregando…</div>` : ""}
    ${snapshot && !reports.length ? `<div class="empty">Nenhuma conta configurada. Adicione contas em cuotai.toml.</div>` : ""}
    <main class="grid">${reports.map(card).join("")}</main></div>`;
}

root.addEventListener("click", (e) => {
  const t = e.target;
  const one = t.closest("[data-refresh]");
  if (one) return store.refreshOne(Number(one.dataset.refresh));
  if (t.closest("[data-refresh-all]")) return store.refresh();
  const tog = t.closest("[data-toggle]");
  if (tog) { disabled.toggle(tog.dataset.toggle); return render(); }
  const hide = t.closest("[data-hide]");
  if (hide) { hidden.toggle(hide.dataset.hide); return render(); }
});
root.addEventListener("change", (e) => {
  if (e.target.id === "show-hidden") { showHidden = e.target.checked; render(); }
});
bindThemeSwitch(root, render);
initTheme(render);
render();
