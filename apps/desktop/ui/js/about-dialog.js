// "Sobre › Bibliotecas de código aberto": a lista vem do core (`about_info`), a mesma da TUI.
import { te, t } from "./i18n.js";
import { aboutInfo, openExternal } from "./lib.js";
import { esc } from "./util.js";
import { showDialog } from "./motion.js";

const ROLES = ["runtime", "build", "test", "asset", "theme", "reference"];
let dlg = null;

const row = (l) => `<li class="lib">
  <div class="lib-top"><b>${esc(l.name)}</b><span class="lib-lic">${esc(l.license)}</span></div>
  <div class="lib-sub">${l.used_by.map((c) => esc(t(`about.component.${c}`))).join(" · ")}</div>
  <button class="link lib-repo" data-repo="${esc(l.repository)}" title="${te("about.open")}">${esc(l.repository)}</button>
</li>`;

export async function openLibraries() {
  if (dlg?.open) return;
  dlg = document.createElement("dialog");
  dlg.className = "dlg libs";
  document.body.append(dlg);
  dlg.addEventListener("close", () => { dlg = null; });
  dlg.innerHTML = `<div class="dlg-body"><h2>${te("about.libraries")}</h2><p class="hint">${te("about.loading")}</p></div>`;
  showDialog(dlg);
  let info;
  try { info = await aboutInfo(); } catch { info = null; }
  if (!dlg) return;
  const sections = info ? ROLES.map((role) => {
    const list = info.libraries.filter((l) => l.role === role).sort((a, b) => a.name.localeCompare(b.name, "en", { sensitivity: "base" }));
    return list.length ? `<section class="lib-sec"><h3>${te(`about.role.${role}`)} <small>(${list.length})</small></h3>
      <p class="hint">${te(`about.role.${role}.note`)}</p><ul class="lib-list">${list.map(row).join("")}</ul></section>` : "";
  }).join("") : `<p class="err" role="alert">${te("common.unexpected")}</p>`;
  const brazil = info ? info.libraries.filter((l) => l.from_brazil).sort((a, b) => a.name.localeCompare(b.name, "en", { sensitivity: "base" })) : [];
  const thanks = brazil.length ? `<section class="lib-sec lib-thanks"><h3>${te("about.brazil")}</h3>
    <p class="hint">${te("about.brazil.note")}</p><ul class="lib-list">${brazil.map(row).join("")}</ul></section>` : "";
  dlg.innerHTML = `<div class="dlg-body">
    <h2>${te("about.libraries")}</h2>
    ${info ? `<p class="hint">${te("about.libraries.help")}</p>` : ""}
    <div class="lib-scroll">${thanks}${sections}</div>
    <p class="err" id="lib-err" role="alert"></p>
    <div class="dlg-actions"><button class="btn primary" data-close>${te("common.close")}</button></div></div>`;
  dlg.querySelector("[data-close]").onclick = () => dlg.close();
  dlg.addEventListener("click", async (e) => {
    const b = e.target.closest("[data-repo]");
    if (!b) return;
    try { await openExternal(b.dataset.repo); } catch { dlg.querySelector("#lib-err").textContent = t("about.openFailed"); }
  });
}
