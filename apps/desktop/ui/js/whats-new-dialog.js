// "Sobre › Novidades" (PRD 0019 RF7.7): o histórico do CHANGELOG.md, já analisado pelo core (`whats_new`).
// A UI só desenha: títulos e rótulos vêm como textos do core (`tl`); as entradas ficam em inglês, como o próprio dialog diz.
import { t, te, tl } from "./i18n.js";
import { bindShell, shellHtml } from "./dialog-shell.js";
import { getSettings, whatsNew, whatsNewSeen } from "./lib.js";
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

const shell = (body, foot = `<button class="btn primary" data-shell-close>${te("common.close")}</button>`) =>
  shellHtml({ id: "whats-new-title", title: t("about.whatsNew"), body, foot });

const render = (view) => shell(`<p class="hint">${esc(tl(view.entries_language))}</p>
    <div class="wn-scroll">${view.releases.map(release).join("")}${view.history ? history(view.history) : ""}</div>`);

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
let checked = false;

/** PRD 0019 RF7.7: after a version change the notes open once by themselves in the main window. The shell decides
 *  (`whats_new_due`: the version differs from the last one seen, not a first install, the Settings > About switch
 *  on) and remembers that they were shown. Another dialog (first run, update question) is never covered: it waits
 *  for it and, if one stays open, tries again at the next launch. Once per session. */
export async function maybeShowWhatsNew() {
  if (checked) return false;
  checked = true;
  let settings = null;
  try { settings = await getSettings(); } catch { return false; }
  if (!settings?.whats_new_due || settings.whats_new_auto === false) return false;
  for (let tries = 0; tries < 8; tries++) {
    await pause(1500);
    if (!document.querySelector("dialog[open]")) {
      await whatsNewSeen().catch(() => {});
      await openWhatsNew();
      return true;
    }
  }
  return false;
}

export async function openWhatsNew() {
  if (dlg?.open) return;
  dlg = document.createElement("dialog");
  dlg.className = "dlg dlg-shell whats-new";
  dlg.setAttribute("aria-labelledby", "whats-new-title");
  document.body.append(dlg);
  dlg.addEventListener("close", () => { dlg = null; });
  dlg.innerHTML = shell(`<p class="hint">${te("about.loading")}</p>`, "");
  bindShell(dlg);
  showDialog(dlg);
  let view = null;
  try { view = await whatsNew(); } catch { view = null; }
  if (!dlg) return;
  dlg.innerHTML = view
    ? render(view)
    : shell(`<p class="err" role="alert">${te("common.unexpected")}</p>`);
  bindShell(dlg);
}
