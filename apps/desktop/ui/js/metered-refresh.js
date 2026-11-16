// PRD 0033 re-validation V-06 / L-2: a refresh the user asks for on a metered account (AWS Bedrock: AWS bills each
// Cost Explorer request) is an explicit, billed choice. The per-account refresh asks first (DS-CMP-DLG); the core
// refuses inside its 15-minute floor and answers with a note, shown here as a toast (DS-CMP-MSG-3/4). The background
// schedule and "Refresh all" never come through here: they keep each account's interval.
import { te, tl } from "./i18n.js";
import { accountFullLabel } from "./lib.js";
import { showDialog } from "./motion.js";

/** True when a refresh of `report` the user triggers is a billed query that needs a confirmation (the core's access
 *  descriptor marks the provider as metered). A connection of an aggregator is never metered. */
export const needsBilledConfirm = (report) => !!report?.access?.metered && !report?.connection;

/** The confirmation's content: the title names the action and the account, the body says it is billed. Cancel is
 *  first so it has the initial focus; "Query now" is the primary, last (DS-CMP-BTN-4). */
export function billedConfirmHtml(report) {
  return `<form method="dialog" class="dlg-body" data-billed-confirm>
    <h2 id="billed-title">${te("refresh.billed.title", { account: accountFullLabel(report) })}</h2>
    <p id="billed-body">${te("refresh.billed.body")}</p>
    <div class="dlg-actions"><button type="button" class="btn" data-billed="cancel">${te("common.cancel")}</button><button type="button" class="btn primary" data-billed="query">${te("refresh.billed.query")}</button></div>
  </form>`;
}

/** Asks before a billed refresh; resolves `true` only for "Query now" (Esc, the close button and Cancel are `false`). */
export function askBilledRefresh(report) {
  return new Promise((resolve) => {
    const dlg = document.createElement("dialog");
    dlg.className = "dlg dlg-sm";
    dlg.setAttribute("aria-labelledby", "billed-title");
    dlg.setAttribute("aria-describedby", "billed-body");
    dlg.innerHTML = billedConfirmHtml(report);
    document.body.append(dlg);
    let choice = false;
    for (const btn of dlg.querySelectorAll("[data-billed]")) {
      btn.addEventListener("click", () => { choice = btn.dataset.billed === "query"; dlg.close(); });
    }
    dlg.addEventListener("close", () => resolve(choice), { once: true });
    showDialog(dlg);
  });
}

let toastTimer = null;
/** DS-CMP-MSG-3/4: one polite toast in the bottom corner of the window, replaced by the next, gone after 5 s (paused
 *  while hovered or focused). */
export function showToast(text) {
  let el = document.querySelector(".toast");
  const arm = () => { clearTimeout(toastTimer); toastTimer = setTimeout(() => { el.hidden = true; }, 5000); };
  if (!el) {
    el = document.createElement("div");
    el.className = "toast";
    el.setAttribute("role", "status");
    el.setAttribute("aria-live", "polite");
    el.tabIndex = -1;
    el.addEventListener("mouseenter", () => clearTimeout(toastTimer));
    el.addEventListener("focusin", () => clearTimeout(toastTimer));
    el.addEventListener("mouseleave", arm);
    el.addEventListener("focusout", arm);
    document.body.append(el);
  }
  el.textContent = text;
  el.hidden = false;
  arm();
}

/** The text of the core's answer to an explicit refresh (`note.metered.refused`, its clocks nested core texts), in the
 *  UI language; empty when the refresh ran. */
export const refusalText = (note) => (note ? tl(note) : "");

/** A per-account refresh the user triggered: a metered account asks first (on Cancel nothing is invoked); the core's
 *  refusal becomes a toast. `run` performs the refresh and resolves with the core's note or null. Resolves with that
 *  note, or null when nothing ran. */
export async function manualRefresh(report, run, { confirm = askBilledRefresh, toast = showToast } = {}) {
  if (needsBilledConfirm(report) && !(await confirm(report))) return null;
  const note = (await run()) ?? null;
  const text = refusalText(note);
  if (text) toast(text);
  return note;
}

/** L-2: whether an account draft is metered. The draft descriptor carries the cost advisory exactly when the provider
 *  is metered (`UsageAccess::cost_advisory`); a descriptor that names `metered` wins. */
export const draftIsMetered = (access) => !!(access?.metered ?? access?.cost_advisory);

/** L-2: the hint beside "Test connection" of a metered draft, linked to the button with `aria-describedby`. */
export const billedTestHint = (id) => `<small class="hint billed-hint" id="${id}">${te("dialog.test.billed")}</small>`;
