// PRD 0046 RF2.1/RF3.3 (DS-CMP-BAK-1..3): **Back up…** and **Restore…**. The core decides what a backup holds, the
// restore plan and what is still needed; these dialogs only show it, let the person choose the replacements, and
// call the shell. No secret is ever in a backup, so the next steps say which keys to type and sign-ins to redo.
import { t, te, tl } from "./i18n.js";
import {
  backupPreview, backupWrite, errText, esc, icon, pickBackupFile, pickBackupSave, requestAccountAction, restartApp,
  restoreApply, restorePlan,
} from "./lib.js";
import { showDialog } from "./motion.js";
import { bindShell, focusFirst, shellHtml } from "./dialog-shell.js";

let dlg = null;

const open = (cls, titleId, title, from) => {
  if (dlg?.open) return null;
  dlg = document.createElement("dialog");
  dlg.className = `dlg dlg-shell ${cls}`;
  dlg.setAttribute("aria-labelledby", titleId);
  document.body.append(dlg);
  dlg.addEventListener("close", () => { dlg?.remove(); dlg = null; });
  dlg.innerHTML = shellHtml({ id: titleId, title, body: `<p class="hint">${te("common.loading")}</p>` });
  bindShell(dlg);
  showDialog(dlg, from);
  return dlg;
};

const fill = (box, { id, title, body, foot }) => {
  box.innerHTML = shellHtml({ id, title, body, foot });
  bindShell(box);
  focusFirst(box);
};

/** The files a backup holds and what it leaves out, with why (the core's sentences). */
export const backupBodyHtml = (view, withHistory) => `<p>${te("backup.intro")}</p>
  <h3 class="sub">${te("backup.included")}</h3>
  <ul class="hook-runs">${view.files.map((f) => `<li><code>${esc(f.path)}</code></li>`).join("")}</ul>
  <h3 class="sub">${te("backup.excluded")}</h3>
  <ul class="hook-runs">${view.excluded.map((reason) => `<li>${esc(tl(reason))}</li>`).join("")}</ul>
  <label class="check"><input type="checkbox" data-backup-history ${withHistory ? "checked" : ""} /> ${te("backup.withHistory")}</label>
  ${view.history_large ? `<p class="hint warn-txt" role="status">${te("backup.historyLarge", { size: Math.round(view.history_bytes / 1048576) })}</p>` : ""}
  <p class="err" data-backup-error role="alert"></p>`;

/** RF2.1: preview what goes in, then the native save dialog, then the write. */
export async function openBackupDialog(from = null, { onDone = null } = {}) {
  const box = open("backup-dlg", "backup-title", t("backup.title"), from);
  if (!box) return;
  let withHistory = false;
  const paint = async () => {
    let view;
    try { view = await backupPreview(withHistory); }
    catch (e) {
      if (!dlg) return;
      fill(box, { id: "backup-title", title: t("backup.title"), body: `<p class="err" role="alert">${esc(errText(e))}</p>`,
        foot: `<button class="btn" data-shell-close>${te("common.close")}</button>` });
      return;
    }
    if (!dlg) return;
    fill(box, {
      id: "backup-title",
      title: t("backup.title"),
      body: backupBodyHtml(view, withHistory),
      foot: `<button class="btn" data-shell-close>${te("common.cancel")}</button>
        <button class="btn primary" data-backup-save>${icon("download")} ${te("backup.save")}</button>`,
    });
    box.querySelector("[data-backup-history]").onchange = (e) => { withHistory = e.target.checked; paint(); };
    box.querySelector("[data-backup-save]").onclick = async () => {
      const error = box.querySelector("[data-backup-error]");
      const path = await pickBackupSave(view.default_name);
      if (typeof path !== "string" || !path) return;
      try {
        const written = await backupWrite(path, withHistory);
        fill(box, {
          id: "backup-title",
          title: t("backup.title"),
          body: `<p role="status">${te("backup.done", { path, count: written.files.length })}</p><p class="hint">${te("backup.noSecret")}</p>`,
          foot: `<button class="btn primary" data-shell-close>${te("common.close")}</button>`,
        });
        onDone?.(written);
      } catch (e) { error.textContent = errText(e); }
    };
  };
  await paint();
}

/** The word a plan action reads as. */
const actionWord = (item) => item.action === "merge"
  ? t("restore.action.merge", { rows: item.merge?.rows ?? 0, new: item.merge?.new ?? 0 })
  : t(`restore.action.${item.action}`);

/** One plan item: what happens to it, where it goes; a replacement has its own choice (DS-CMP-BAK-2). */
const itemHtml = (item, index) => {
  const choice = item.action === "replace" || item.action === "skip";
  const what = `<strong>${esc(actionWord(item))}</strong> — <code>${esc(item.target === "history" ? t("restore.target.history") : item.target === "cost_index" ? t("restore.target.costIndex") : item.target)}</code>`;
  const accounts = item.accounts ? `<br><small>${te("restore.accounts", item.accounts)}</small>` : "";
  return choice
    ? `<li><label class="check"><input type="checkbox" data-restore-item="${index}" ${item.action === "replace" ? "checked" : ""} /> ${what}</label>${accounts}</li>`
    : `<li class="${item.action === "keep" ? "dim" : ""}">${what}${accounts}</li>`;
};

/** RF3.3: the plan as a list with a choice per replacement. */
export const planBodyHtml = (plan) => `<p>${te("restore.intro", { date: new Date(plan.created_at).toLocaleString(document.documentElement.lang || undefined), version: plan.cuotai_version })}</p>
  ${plan.items.length ? `<ul class="hook-runs">${plan.items.map(itemHtml).join("")}</ul>` : `<p class="hint">${te("restore.nothing")}</p>`}
  <p class="hint">${te("restore.skipHint")}</p>
  <p class="err" data-restore-error role="alert"></p>`;

/** RF3.3 **Next steps**: what the backup could not carry, each with the action that opens the right dialog. */
export const nextStepsHtml = (plan, done, accountsRestored) => `<p role="status">${te("restore.done", { count: done.written })}</p>
  ${plan.needs.length ? `<h3 class="sub">${te("restore.nextSteps")}</h3>
  <ul class="hook-runs">${plan.needs.map((need, i) => `<li>${esc(tl(need))}${accountsRestored && ["env", "sign_in"].includes(plan.next_steps[i]?.kind)
    ? ` <button class="btn sm" data-restore-step="${plan.next_steps[i].account}">${te(plan.next_steps[i].kind === "sign_in" ? "restore.step.signIn" : "restore.step.addKey")}</button>` : ""}</li>`).join("")}</ul>`
    : `<p class="hint">${te("restore.noSteps")}</p>`}
  ${done.restart ? `<p class="hint">${te("restore.restartHint")}</p>` : ""}`;

/** RF3.3: choose a backup file, preview the plan, restore, then the next steps. */
export async function openRestoreDialog(from = null, { onOpenAccount = null } = {}) {
  const path = await pickBackupFile();
  if (typeof path !== "string" || !path) return;
  const box = open("restore-dlg", "restore-title", t("restore.title"), from);
  if (!box) return;
  let plan;
  try { plan = await restorePlan(path); }
  catch (e) {
    if (!dlg) return;
    fill(box, { id: "restore-title", title: t("restore.title"), body: `<p class="err" role="alert">${esc(errText(e))}</p>`,
      foot: `<button class="btn" data-shell-close>${te("common.close")}</button>` });
    return;
  }
  if (!dlg) return;
  fill(box, {
    id: "restore-title",
    title: t("restore.title"),
    body: planBodyHtml(plan),
    foot: `<button class="btn" data-shell-close>${te("common.cancel")}</button>
      <button class="btn primary" data-restore-go>${te("restore.go")}</button>`,
  });
  box.querySelector("[data-restore-go]").onclick = async () => {
    const accept = [...box.querySelectorAll("[data-restore-item]")].filter((c) => c.checked).map((c) => plan.items[Number(c.dataset.restoreItem)].path);
    let done;
    try { done = await restoreApply(path, accept); }
    catch (e) { box.querySelector("[data-restore-error]").textContent = errText(e); return; }
    // The steps open accounts by their position in the backup's file: only once that file is the one in use.
    const config = plan.items.find((i) => i.path === "config/cuotai.toml");
    const accountsRestored = !!config && (config.action === "add" || accept.includes(config.path));
    fill(box, {
      id: "restore-title",
      title: t("restore.title"),
      body: nextStepsHtml(plan, done, accountsRestored),
      foot: `${done.restart ? `<button class="btn" data-restore-restart>${te("restore.restart")}</button>` : ""}
        <button class="btn primary" data-shell-close>${te("common.close")}</button>`,
    });
    for (const b of box.querySelectorAll("[data-restore-step]")) {
      b.onclick = () => (onOpenAccount ?? ((index) => requestAccountAction("edit", index)))(Number(b.dataset.restoreStep));
    }
    const restart = box.querySelector("[data-restore-restart]");
    if (restart) restart.onclick = () => restartApp().catch(() => {});
  };
}
