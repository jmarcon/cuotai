// PRD 0028 RF2.3 (DS-CMP-RPT-1): "Report a problem". The core builds the secret-free issue text; this dialog only
// previews it exactly as it is, offers Copy and Open on GitHub and says that nothing is sent by Cuotai. Open asks the
// shell (`open_problem_report`) to rebuild the same draft and open it: the page never hands a URL to the browser
// (re-validation S-01). A link too long to open prefilled opens the bare form instead ("Open the form", V-09).
import { t, te } from "./i18n.js";
import { errText, esc, icon, openProblemReport, reportProblem } from "./lib.js";
import { showDialog } from "./motion.js";
import { bindShell, focusFirst, shellHtml } from "./dialog-shell.js";

let dlg = null;

/** `index`: the account (null for the application alone, from About); `generation`: the View's, for staleness. */
export async function openReportProblem(index, generation, from = null) {
  if (dlg?.open) return;
  dlg = document.createElement("dialog");
  dlg.className = "dlg dlg-shell report-dlg";
  dlg.setAttribute("aria-labelledby", "report-title");
  document.body.append(dlg);
  dlg.addEventListener("close", () => { dlg.remove(); dlg = null; });
  dlg.innerHTML = shellHtml({ id: "report-title", title: t("report.title"), body: `<p class="hint">${te("common.loading")}</p>` });
  bindShell(dlg);
  showDialog(dlg, from);
  let draft = null, failure = null;
  try { draft = await reportProblem(index, generation); } catch (error) { failure = errText(error); }
  if (!dlg) return;
  const text = draft ? `${draft.title}\n\n${draft.body}` : "";
  dlg.innerHTML = shellHtml({
    id: "report-title",
    title: t("report.title"),
    body: draft ? `<p>${te("report.intro")}</p>
      <pre class="report-preview" tabindex="0" aria-label="${te("report.preview")}">${esc(text)}</pre>
      <p class="hint">${te("report.nothingSent")}</p>${draft.copy_only ? `<p class="hint">${te("report.tooLong")}</p>` : ""}`
      : `<p class="err" role="alert">${esc(failure ?? t("common.unexpected"))}</p>`,
    foot: `<p class="err" id="report-err" role="alert"></p>
      ${draft ? `<button class="btn" data-copy>${icon("copy")} ${te("report.copy")}</button>` : `<button class="btn" data-shell-close>${te("common.close")}</button>`}
      ${draft ? `<button class="btn primary" data-open>${te(draft.copy_only ? "report.openForm" : "report.open")} ${icon("ext")}</button>` : ""}`,
  });
  bindShell(dlg);
  focusFirst(dlg);
  const err = dlg.querySelector("#report-err");
  const copy = dlg.querySelector("[data-copy]");
  if (copy) copy.onclick = async () => {
    try { await navigator.clipboard.writeText(text); copy.lastChild.textContent = ` ${t("report.copied")}`; err.textContent = ""; }
    catch { err.textContent = t("report.copyFailed"); }
  };
  const open = dlg.querySelector("[data-open]");
  if (open) open.onclick = async () => {
    try { await openProblemReport(index, generation); err.textContent = ""; } catch { err.textContent = t("about.openFailed"); }
  };
}
