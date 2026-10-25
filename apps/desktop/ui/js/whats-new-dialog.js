// "Sobre › Novidades" (PRD 0019 RF7.7): o histórico do CHANGELOG.md, já analisado pelo core (`whats_new`).
// A UI só desenha: títulos e rótulos vêm como textos do core (`tl`); as entradas ficam em inglês, como o próprio dialog diz.
import { te, tl } from "./i18n.js";
import { whatsNew } from "./lib.js";
import { esc } from "./util.js";
import { showDialog } from "./motion.js";

let dlg = null;

const entry = (text) => `<li class="wn-entry">${esc(text)}</li>`;

const category = (block) => `<div class="wn-cat">
  <h4>${esc(tl(block.label))}</h4>
  <ul class="wn-list">${block.entries.map(entry).join("")}</ul></div>`;

const notes = (section) => `${section.paragraphs.map((p) => `<p class="hint">${esc(p)}</p>`).join("")}${section.categories.map(category).join("")}`;

const release = (r) => `<section class="wn-sec">
  <h3>${esc(r.version)} <small>${esc(r.date)}${r.current ? ` · ${te("about.whatsNew.current")}` : ""}</small></h3>
  ${notes(r)}</section>`;

const history = (h) => `<section class="wn-sec wn-history">
  <h3>${esc(h.title)}</h3>${notes(h)}</section>`;

const close = `<div class="dlg-actions"><button class="btn primary" data-close>${te("common.close")}</button></div>`;

function render(view) {
  return `<div class="dlg-body">
    <h2>${esc(tl(view.title))}</h2>
    <p class="hint">${esc(tl(view.entries_language))}</p>
    <div class="wn-scroll">${view.releases.map(release).join("")}${view.history ? history(view.history) : ""}</div>
    ${close}</div>`;
}

export async function openWhatsNew() {
  if (dlg?.open) return;
  dlg = document.createElement("dialog");
  dlg.className = "dlg whats-new";
  document.body.append(dlg);
  dlg.addEventListener("close", () => { dlg = null; });
  dlg.innerHTML = `<div class="dlg-body"><h2>${te("about.whatsNew")}</h2><p class="hint">${te("about.loading")}</p></div>`;
  showDialog(dlg);
  let view = null;
  try { view = await whatsNew(); } catch { view = null; }
  if (!dlg) return;
  dlg.innerHTML = view
    ? render(view)
    : `<div class="dlg-body"><h2>${te("about.whatsNew")}</h2><p class="err" role="alert">${te("common.unexpected")}</p>${close}</div>`;
  dlg.querySelector("[data-close]").onclick = () => dlg.close();
}
