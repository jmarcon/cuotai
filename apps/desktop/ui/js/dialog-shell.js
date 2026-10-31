// DS-CMP-DLG (mockups 31 and 07c): the one dialog shell of the first-run, What's new and Report a problem dialogs:
// a header (title and a × close), a body that scrolls on its own, and a footer separated by a hairline whose
// actions sit on the right. Callers pass finished HTML for the body and the footer.
import { te } from "./i18n.js";
import { esc, icon } from "./lib.js";

/** `id`: the title element's id (the dialog's `aria-labelledby`); `title` is plain text; `body`/`foot` are HTML. */
export const shellHtml = ({ id, title, body, foot = "" }) => `<div class="dlg-shell-head">
    <h2 id="${esc(id)}">${esc(title)}</h2>
    <button type="button" class="icon" data-shell-close title="${te("common.close")}" aria-label="${te("common.close")}">${icon("x")}</button>
  </div>
  <div class="dlg-shell-body">${body}</div>
  ${foot ? `<div class="dlg-shell-foot">${foot}</div>` : ""}`;

/** Makes the × close the dialog; call again after the content is replaced. */
export const bindShell = (dlg) => { for (const b of dlg.querySelectorAll("[data-shell-close]")) b.addEventListener("click", () => dlg.close()); };

/** Replacing the content of an open dialog destroys the control that had focus and leaves it on `body`: call this
 *  after the replacement, and focus lands where `showModal` would put it, on the first control. */
export const focusFirst = (dlg) => dlg.querySelector("button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex='-1'])")?.focus();
