// Settings › Data & privacy (PRD 0020 RF2.5): the measurement history and the provider status pages.
import { t, te } from "../i18n.js";
import { brand, esc, historyErase, historyExport, historySize, icon, pickExportFile, setHistoryDays, setHistoryRecording, setStatusEnabled } from "../lib.js";
import { errorText, fmtBytes, group, l10nError } from "./common.js";

/** Accounts of the loaded configuration that have a report, as `[index, label]` for the history scope. */
const historyAccounts = (view) => (view?.reports ?? []).map((r) => [r.index, `${brand(r.provider).label} · ${r.account}`]);

const PERIODS = [["1", "settings.history.period.day"], ["7", "settings.history.period.week"], ["30", "settings.history.period.month"], ["", "settings.history.period.all"]];

export function html({ settings: s, view }) {
  return historyHtml(s, view()) + privacyHtml(s, view());
}

function historyHtml(s, view) {
  return group("history", te("settings.history.title"), `
      <p class="hint">${te("settings.history.help")}</p>
      <label class="check"><input id="history-recording" type="checkbox" ${s.history_recording !== false ? "checked" : ""} /> ${te("settings.history.record")}</label>
      <p class="hint">${te("settings.history.recordHelp")}</p>
      <p class="err" id="history-recording-msg" role="alert"></p>
      <p class="hint" id="history-size">${te("common.loading")}</p>
      <label class="f"><span>${te("settings.history.days")}</span>
        <input id="history-days" type="number" inputmode="numeric" min="${s.history_days_min}" max="${s.history_days_max}" step="1" value="${esc(s.history_days)}" />
        <small>${te("settings.history.range", { min: s.history_days_min, max: s.history_days_max })}</small></label>
      <div class="dlg-actions left"><button class="btn primary" data-apply-history>${te("settings.history.save")}</button></div>
      <p class="err" id="history-msg" role="alert"></p>
      <label class="f"><span>${te("settings.history.account")}</span>
        <select id="history-account"><option value="">${te("settings.history.allAccounts")}</option>
          ${historyAccounts(view).map(([index, label]) => `<option value="${index}">${esc(label)}</option>`).join("")}</select></label>
      <label class="f"><span>${te("settings.history.period")}</span>
        <select id="history-period">${PERIODS.map(([days, key]) => `<option value="${days}" ${days === "" ? "selected" : ""}>${te(key)}</option>`).join("")}</select></label>
      <div class="dlg-actions left">
        <button class="btn" data-export="csv">${icon("copy")} ${te("settings.history.exportCsv")}</button>
        <button class="btn" data-export="json">${icon("copy")} ${te("settings.history.exportJson")}</button>
        <button class="btn danger-btn" data-erase>${icon("trash")} ${te("settings.history.erase")}</button>
      </div>
      <div class="confirm" id="history-confirm" role="alertdialog" aria-labelledby="history-confirm-text" hidden>
        <p id="history-confirm-text"></p>
        <div class="dlg-actions left"><button class="btn danger-btn" data-erase-confirm>${te("settings.history.eraseConfirm")}</button>
          <button class="btn" data-erase-cancel>${te("common.cancel")}</button></div>
      </div>
      <p class="err" id="history-data-msg" role="status"></p>`, "data-history");
}

/** PRD 0011 RF4.3: public provider status queries, off until the user turns them on, with what they reveal and the
 *  domains they reach for the configured providers (listed while off too, so the choice is informed). */
function privacyHtml(s, view) {
  const domains = view?.status?.domains ?? [];
  return group("privacy", te("settings.privacy.title"), `
      <label class="check"><input id="status-queries" type="checkbox" ${s.status_queries ? "checked" : ""} /> ${te("settings.status.on")}</label>
      <p class="hint">${te("settings.status.help")}</p>
      <p class="hint" data-status-domains>${domains.length
        ? `${te("settings.status.domains")} ${domains.map((d) => `<code>${esc(d)}</code>`).join(", ")}`
        : te("settings.status.none")}</p>
      <p class="err" id="status-msg" role="alert"></p>`, "data-privacy");
}

export function bind(root, ctx) {
  bindHistory(root, ctx);
  // PRD 0011 RF4.3: saved in desktop.json; on failure the box goes back to the saved value.
  root.querySelector("#status-queries").onchange = (e) => setStatusEnabled(e.target.checked).then(
    () => { root.querySelector("#status-msg").textContent = ""; ctx.changed("status_queries"); },
    (x) => { e.target.checked = !e.target.checked; root.querySelector("#status-msg").textContent = errorText(x); });
}

/** PRD 0006 RF4/RF5.3: recording switch and retention (`desktop.json`), size on disk, export and erasure, all done
 *  by the core; erasing always asks first. */
function bindHistory(root, ctx) {
  const days = root.querySelector("#history-days");
  const msg = root.querySelector("#history-msg");
  const dataMsg = root.querySelector("#history-data-msg");
  const select = root.querySelector("#history-account");
  const account = () => (select.value === "" ? null : Number(select.value));
  const period = () => { const v = root.querySelector("#history-period").value; return v === "" ? null : Number(v); };
  const confirm = root.querySelector("#history-confirm");
  const report = (text, ok) => { dataMsg.classList.toggle("ok", ok); dataMsg.textContent = text; };
  const size = root.querySelector("#history-size");
  const showSize = () => historySize().then(
    (bytes) => { size.textContent = t("settings.history.size", { size: fmtBytes(bytes) }); },
    (e) => { size.textContent = t("settings.history.sizeUnknown", { error: l10nError(e) }); });
  showSize();
  // Off stops recording at once and keeps the data; on failure the box goes back to the saved value.
  root.querySelector("#history-recording").onchange = (e) => setHistoryRecording(e.target.checked).then(
    () => { root.querySelector("#history-recording-msg").textContent = ""; ctx.changed("history_recording"); },
    (x) => { e.target.checked = !e.target.checked; root.querySelector("#history-recording-msg").textContent = errorText(x); });
  root.querySelector("[data-apply-history]").onclick = async () => {
    msg.classList.remove("ok");
    try {
      await setHistoryDays(Number(days.value));
      msg.classList.add("ok");
      msg.textContent = t("settings.history.saved", { days: Number(days.value) });
      showSize(); // the new retention prunes at once
      ctx.changed("history_days");
    } catch (e) { msg.textContent = errorText(e); }
  };
  for (const button of root.querySelectorAll("[data-export]")) button.onclick = async () => {
    const format = button.dataset.export;
    const path = await pickExportFile(format);
    if (typeof path !== "string" || !path) return; // cancelled
    try { report(t("settings.history.exported", { count: await historyExport(account(), period(), format, path), path }), true); }
    catch (e) { report(l10nError(e), false); }
  };
  // The scope is fixed when the confirmation opens, so changing the list afterwards cannot widen it.
  let scope = null;
  root.querySelector("[data-erase]").onclick = () => {
    scope = account();
    confirm.querySelector("#history-confirm-text").textContent = scope == null
      ? t("settings.history.eraseAll")
      : t("settings.history.eraseOne", { account: select.selectedOptions[0]?.textContent ?? "" });
    confirm.hidden = false;
    confirm.querySelector("[data-erase-cancel]").focus();
  };
  confirm.querySelector("[data-erase-cancel]").onclick = () => { confirm.hidden = true; root.querySelector("[data-erase]").focus(); };
  confirm.querySelector("[data-erase-confirm]").onclick = async () => {
    confirm.hidden = true;
    try { report(t("settings.history.erased", { count: await historyErase(scope) }), true); ctx.changed("history"); }
    catch (e) { report(l10nError(e), false); }
    showSize();
  };
}
