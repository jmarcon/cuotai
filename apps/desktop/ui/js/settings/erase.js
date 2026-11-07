// Settings › Data & privacy › Erase all Cuotai data (PRD 0052 RF5.2/RF5.3). The page shows the core's ordered
// inventory (what goes, what is kept), an off-by-default box for the accounts file and `hooks.toml`, and asks for the
// core's confirmation word. Rust re-checks the word, asks once more in a native dialog, stops every writer and erases;
// a complete erase quits the app. This module only draws and forwards the typed word.
import { te } from "../i18n.js";
import { eraseAllData, eraseAllPlan, esc, icon, quitApp } from "../lib.js";
import { l10nError } from "./common.js";
import { buttons, check, row } from "./rows.js";

/** The last row of the page's collapsed "Export and erase" group, after the history's own export and erasure. */
export function eraseHtml() {
  return row({ key: "erase-all", label: te("settings.erase.label"), desc: te("settings.erase.desc"), info: te("settings.erase.help"),
    danger: true, msg: "erase-all-msg",
    control: () => buttons(`<button class="btn sm danger-btn" data-erase-all>${icon("trash")} ${te("settings.erase.open")}</button>`),
    body: `<div class="confirm" id="erase-all-confirm" role="alertdialog" aria-labelledby="erase-all-title" aria-describedby="erase-all-text" hidden></div>` });
}

/** One inventory line: where it is (a home-abbreviated path or the vault service) and its stable id. */
const itemHtml = (item) => `<li><code class="set-path">${esc(item.location)}</code> <small>${esc(item.id)}</small></li>`;

/** The confirmation panel over `plan` (`{ word, items }` from the shell). With `includeConfig` the items the core
 *  keeps by default (the accounts file and `hooks.toml`) move to the erased list. */
export function erasePanelHtml(plan, includeConfig = false) {
  const items = plan?.items ?? [];
  const goes = (item) => item.erase || includeConfig;
  const erased = items.filter(goes);
  const kept = items.filter((item) => !goes(item));
  return `<p id="erase-all-title"><strong>${te("settings.erase.listErased")}</strong></p>
    <ul class="hook-runs" data-erase-list>${erased.map(itemHtml).join("")}</ul>
    ${kept.length ? `<p><strong>${te("settings.erase.listKept")}</strong></p><ul class="hook-runs" data-erase-kept>${kept.map(itemHtml).join("")}</ul>` : ""}
    <label class="check">${check("data-erase-config", includeConfig)}<span>${te("settings.erase.includeConfig")}</span></label>
    <p id="erase-all-text" class="hint warn-txt">${icon("warn")} ${te("settings.erase.irreversible")}</p>
    <label class="f"><span>${te("settings.erase.word", { word: plan?.word ?? "" })}</span>
      <input type="text" id="erase-all-word" data-erase-word autocomplete="off" spellcheck="false" autocapitalize="off" /></label>
    <div class="dlg-actions left"><button class="btn danger-btn" data-erase-all-confirm disabled>${te("settings.erase.confirm")}</button>
      <button class="btn" data-erase-all-cancel>${te("common.cancel")}</button></div>`;
}

/** The confirm button stays off until the typed text is exactly the word (spaces around it ignored, as in Rust). */
export const wordMatches = (typed, word) => typeof typed === "string" && !!word && typed.trim() === word;

/** A partial erase: the ids that failed (never a path), with Try again and Quit; the background work stays stopped. */
export function partialHtml(result) {
  return `<p class="hint warn-txt" role="alert" data-erase-partial>${icon("warn")} ${te("settings.erase.partial", { ids: (result?.failed ?? []).join(", ") })}</p>
    <div class="dlg-actions left"><button class="btn danger-btn" data-erase-retry>${te("settings.erase.retry")}</button>
      <button class="btn" data-erase-quit>${te("settings.erase.quit")}</button></div>`;
}

export function bindErase(root) {
  const open = root.querySelector("[data-erase-all]");
  const box = root.querySelector("#erase-all-confirm");
  const msg = root.querySelector("#erase-all-msg");
  if (!open || !box) return;
  let plan = null;
  let includeConfig = false;
  const draw = () => {
    box.innerHTML = erasePanelHtml(plan, includeConfig);
    const word = box.querySelector("[data-erase-word]");
    const confirm = box.querySelector("[data-erase-all-confirm]");
    word.oninput = () => { confirm.disabled = !wordMatches(word.value, plan.word); };
    box.querySelector("[data-erase-config]").onchange = (e) => { includeConfig = e.target.checked; const typed = word.value; draw(); const again = box.querySelector("[data-erase-word]"); again.value = typed; again.oninput(); box.querySelector("[data-erase-config]").focus(); };
    box.querySelector("[data-erase-all-cancel]").onclick = () => { box.hidden = true; box.innerHTML = ""; open.focus(); };
    const run = async () => {
      msg.textContent = "";
      confirm.disabled = true;
      try {
        const result = await eraseAllData(includeConfig, word.value);
        if (result.complete) { box.innerHTML = `<p class="hint" role="status" data-erase-done>${te("settings.erase.done")}</p>`; return; }
        box.innerHTML = partialHtml(result);
        box.querySelector("[data-erase-retry]").onclick = () => { draw(); };
        box.querySelector("[data-erase-quit]").onclick = () => quitApp();
      } catch (e) {
        msg.textContent = l10nError(e);
        confirm.disabled = !wordMatches(word.value, plan.word);
      }
    };
    confirm.onclick = run;
  };
  open.onclick = async () => {
    msg.textContent = "";
    try { plan = await eraseAllPlan(); }
    catch (e) { msg.textContent = l10nError(e); return; }
    includeConfig = false;
    box.hidden = false;
    draw();
    box.querySelector("[data-erase-word]").focus();
  };
}

