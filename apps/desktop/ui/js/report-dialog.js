// PRD 0028 RF2.3 (DS-CMP-RPT-1): "Report a problem". The core builds the secret-free issue text; this dialog only
// previews it exactly as it is, offers Copy and Open on GitHub (through the allow-listed `open_external`) and says
// that nothing is sent by Cuotai. A link too long to open leaves only Copy.
import { t, te } from "./i18n.js";
import { errText, esc, openExternal, reportProblem } from "./lib.js";
import { showDialog } from "./motion.js";

let dlg = null;

/** `index`: the account (null for the application alone, from About); `generation`: the View's, for staleness. */
export async function openReportProblem(index, generation, from = null) {
  if (dlg?.open) return;
  dlg = document.createElement("dialog");
  dlg.className = "dlg report-dlg";
  document.body.append(dlg);
  dlg.addEventListener("close", () => { dlg.remove(); dlg = null; });
  dlg.innerHTML = `<div class="dlg-body"><h2>${te("report.title")}</h2><p class="hint">${te("common.loading")}</p></div>`;
  showDialog(dlg, from);
  let draft = null, failure = null;
  try { draft = await reportProblem(index, generation); } catch (error) { failure = errText(error); }
  if (!dlg) return;
  const text = draft ? `${draft.title}\n\n${draft.body}` : "";
  dlg.innerHTML = `<div class="dlg-body">
    <h2>${te("report.title")}</h2>
    ${draft ? `<pre class="report-preview" tabindex="0" aria-label="${te("report.preview")}">${esc(text)}</pre>
      <p class="hint">${te("report.nothingSent")}</p>${draft.copy_only ? `<p class="hint">${te("report.tooLong")}</p>` : ""}`
      : `<p class="err" role="alert">${esc(failure ?? t("common.unexpected"))}</p>`}
    <p class="err" id="report-err" role="alert"></p>
    <div class="dlg-actions">
      <button class="btn" data-close>${te("common.close")}</button>
      ${draft ? `<button class="btn" data-copy>${te("report.copy")}</button>` : ""}
      ${draft && !draft.copy_only ? `<button class="btn primary" data-open>${te("report.open")}</button>` : ""}
    </div></div>`;
  const err = dlg.querySelector("#report-err");
  dlg.querySelector("[data-close]").onclick = () => dlg.close();
  const copy = dlg.querySelector("[data-copy]");
  if (copy) copy.onclick = async () => {
    try { await navigator.clipboard.writeText(text); copy.textContent = t("report.copied"); err.textContent = ""; }
    catch { err.textContent = t("report.copyFailed"); }
  };
  const open = dlg.querySelector("[data-open]");
  if (open) open.onclick = async () => {
    try { await openExternal(draft.url); err.textContent = ""; } catch { err.textContent = t("about.openFailed"); }
  };
}
