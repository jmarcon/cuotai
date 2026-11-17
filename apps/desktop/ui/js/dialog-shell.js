// DS-CMP-DLG (mockups 31 and 07c): the one dialog shell of the first-run, What's new and Report a problem dialogs:
// a header (title and a × close), a body that scrolls on its own, and a footer separated by a hairline whose
// actions sit on the right. Callers pass finished HTML for the body and the footer.
import { te } from "./i18n.js";
import { esc, icon } from "./lib.js";

/** `id`: the title element's id (the dialog's `aria-labelledby`); `title` is plain text; `body`/`foot` are HTML.
 *  `protectedTexts` names a protected surface (PRD 0035 re-validation S-01: the first-run and backup dialogs): its
 *  parts carry `data-protected-texts` and the × reads the protected `confirm.close`, which a language pack cannot
 *  change. */
export const shellHtml = ({ id, title, body, foot = "", protectedTexts = null }) => {
  const mark = protectedTexts ? ` data-protected-texts="${esc(protectedTexts)}"` : "";
  const close = te(protectedTexts ? "confirm.close" : "common.close");
  return `<div class="dlg-shell-head"${mark}>
    <h2 id="${esc(id)}">${esc(title)}</h2>
    <button type="button" class="icon" data-shell-close title="${close}" aria-label="${close}">${icon("x")}</button>
  </div>
  <div class="dlg-shell-body"${mark}>${body}</div>
  ${foot ? `<div class="dlg-shell-foot"${mark}>${foot}</div>` : ""}`;
};

/** Makes the × close the dialog; call again after the content is replaced. */
export const bindShell = (dlg) => { for (const b of dlg.querySelectorAll("[data-shell-close]")) b.addEventListener("click", () => dlg.close()); };

/** Replacing the content of an open dialog destroys the control that had focus and leaves it on `body`: call this
 *  after the replacement, and focus lands where `showModal` would put it, on the first control. */
export const focusFirst = (dlg) => dlg.querySelector("button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex='-1'])")?.focus();
