// PRD 0025 RF6: the one-step first run in the main window (DS-CMP-DLG-1/2). After the first account (or on a launch
// with accounts and no record) it offers the recommended usage alerts with one **Turn on** (which asks the system for
// permission once) or **Not now**, names the Advanced features it leaves off, and asks the update question of PRD
// 0018 RF2.6 as a separate question with its own buttons. Closing records nothing: the step returns next launch.
// What **Turn on** writes is decided by the shell and the core (`first_run_turn_on`, `AlertPrefs::recommended`).
import { t, te } from "./i18n.js";
import { errText, esc, firstRunRecord, firstRunTurnOn, getSettings, icon, updaterAnswerAutomatic } from "./lib.js";
import { showDialog } from "./motion.js";

let shownThisSession = false;

/** The step is due: accounts exist and this profile has not answered this version of it. */
export const due = (settings, view) => (view?.account_count ?? 0) > 0 && (settings?.onboarding ?? 0) < (settings?.onboarding_version ?? 1);

/** RF6.5: a copy managed by a package manager (PRD 0028 RF8.1) gets the manager's own update command instead of the
 *  question; nothing is asked and no check is ever made by the app. */
const managedHtml = (managed) => `<section class="first-run-update" aria-labelledby="first-run-update-title">
    <h3 id="first-run-update-title">${te("update.title")}</h3>
    <p class="hint">${te(`update.off.${managed.reason}`)}</p><code class="update-cmd">${esc(managed.command)}</code></section>`;

export const stepHtml = ({ updatePending, managed = null }) => `<div class="dlg-body">
  <header class="dlg-head">
    <h2 id="first-run-title">${te("firstrun.title")}</h2>
    <button class="icon" data-first-close title="${te("common.close")}" aria-label="${te("common.close")}">${icon("x")}</button>
  </header>
  <p>${te("firstrun.body")}</p>
  <ul class="first-run-list"><li>${icon("bell")}<span>${te("firstrun.item.alerts")}</span></li></ul>
  <p class="hint">${te("firstrun.advanced")}</p>
  <p class="err" data-first-error role="alert"></p>
  <div class="dlg-actions">
    <button class="btn" data-first-later>${te("firstrun.later")}</button>
    <button class="btn primary" data-first-on>${te("firstrun.on")}</button>
  </div>
  ${updatePending ? `<section class="first-run-update" aria-labelledby="first-run-update-title">
    <h3 id="first-run-update-title">${te("update.consent.title")}</h3>
    <p class="hint">${te("update.consent.body")}</p>
    <p class="err" data-first-update-error role="alert"></p>
    <div class="dlg-actions">
      <button class="btn" data-first-update-no>${te("update.consent.no")}</button>
      <button class="btn" data-first-update-yes>${te("update.consent.yes")}</button>
    </div></section>` : managed?.command ? managedHtml(managed) : ""}
</div>`;

/** Shows the step when due; resolves `true` when it was shown (the caller then skips the separate update dialog). */
export async function maybeShowFirstRun(view, update) {
  if (shownThisSession || document.querySelector("dialog[open]")) return false;
  const settings = await getSettings().catch(() => null);
  if (!due(settings, view)) return false;
  shownThisSession = true;
  const dlg = document.createElement("dialog");
  dlg.className = "dlg first-run";
  dlg.setAttribute("aria-labelledby", "first-run-title");
  dlg.innerHTML = stepHtml({ updatePending: !!update?.consent_pending, managed: update?.upgrade_command ? { reason: update.disabled_reason, command: update.upgrade_command } : null });
  document.body.append(dlg);
  const error = dlg.querySelector("[data-first-error]");
  const on = dlg.querySelector("[data-first-on]");
  const later = dlg.querySelector("[data-first-later]");
  let alertsDone = false;
  let updateDone = !update?.consent_pending;
  const finish = () => { if (alertsDone && updateDone) dlg.close(); };
  const answered = (buttons, text) => {
    for (const b of buttons) b.disabled = true;
    const p = document.createElement("p");
    p.className = "hint";
    p.setAttribute("role", "status");
    p.textContent = text;
    buttons[0].closest(".dlg-actions").replaceWith(p);
  };
  on.onclick = async () => {
    on.disabled = later.disabled = true;
    try {
      const permission = await firstRunTurnOn();
      alertsDone = true;
      // PRD 0003: a refusal leaves the alerts off and says why; the step is still answered.
      answered([later, on], permission === "granted" ? t("firstrun.done") : t("settings.notify.denied"));
      if (permission === "granted") finish();
    } catch (x) {
      on.disabled = later.disabled = false;
      error.textContent = errText(x);
    }
  };
  later.onclick = async () => {
    try {
      await firstRunRecord(false);
      alertsDone = true;
      answered([later, on], t("firstrun.later.done"));
      finish();
    } catch (x) { error.textContent = errText(x); }
  };
  const updateAnswer = (yes) => async () => {
    const box = dlg.querySelector(".first-run-update");
    try {
      await updaterAnswerAutomatic(yes);
      updateDone = true;
      answered([...box.querySelectorAll("button")], t(yes ? "firstrun.update.on" : "firstrun.update.off"));
      finish();
    } catch (x) { box.querySelector("[data-first-update-error]").textContent = errText(x); }
  };
  dlg.querySelector("[data-first-update-yes]")?.addEventListener("click", updateAnswer(true));
  dlg.querySelector("[data-first-update-no]")?.addEventListener("click", updateAnswer(false));
  dlg.querySelector("[data-first-close]").onclick = () => dlg.close();
  dlg.addEventListener("close", () => dlg.remove(), { once: true });
  showDialog(dlg);
  later.focus();
  return true;
}
