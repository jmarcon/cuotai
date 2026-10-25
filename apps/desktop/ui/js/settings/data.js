// Settings › Data & privacy (PRD 0020 RF2.5): the measurement history and the provider status pages.
import { t, te } from "../i18n.js";
import { STALE, brand, esc, historyErase, historyExport, historySize, icon, pickExportFile, setHistoryDays, setHistoryRecording, setStatusEnabled } from "../lib.js";
import { errorText, fmtBytes, group, l10nError } from "./common.js";
import { buttons, commitField, disc, num, row, rowError, select, sw, toast } from "./rows.js";

/** Accounts of the loaded configuration that have a report, as `[index, label]` for the history scope. */
const historyAccounts = (view) => (view?.reports ?? []).map((r) => [r.index, `${brand(r.provider).label} · ${r.account}`]);

const PERIODS = [["1", "settings.history.period.day"], ["7", "settings.history.period.week"], ["30", "settings.history.period.month"], ["", "settings.history.period.all"]];

export function html({ settings: s, view }) {
  return historyHtml(s) + privacyHtml(s, view()) + historyDataHtml(view());
}

/** The account list is indexed by the configuration generation it was drawn from; when the accounts change
 *  (moved, added or removed from another window while this one is open) the page is drawn again. */
export const viewChanged = (next, prev) => (next?.generation ?? null) !== (prev?.generation ?? null);

function historyHtml(s) {
  return group("history", te("settings.history.title"),
    row({ key: "recording", label: te("settings.history.record"), desc: `<span id="history-size">${te("common.loading")}</span>`,
      info: `${te("settings.history.help")}<br />${te("settings.history.recordHelp")}`, msg: "history-recording-msg",
      control: (a) => sw("history-recording", s.history_recording !== false, a) })
    + row({ key: "days", label: te("settings.history.days"), desc: te("settings.history.range", { min: s.history_days_min, max: s.history_days_max }), msg: "history-msg",
      control: (a) => num("history-days", { value: s.history_days, min: s.history_days_min, max: s.history_days_max, apply: "data-apply-history" }, a) }), "data-history");
}

/** PRD 0011 RF4.3: public provider status queries, off until the user turns them on, with what they reveal and the
 *  domains they reach for the configured providers (listed while off too, so the choice is informed). */
function privacyHtml(s, view) {
  const domains = view?.status?.domains ?? [];
  return group("privacy", te("settings.privacy.title"),
    row({ key: "status", label: te("settings.status.on"), info: te("settings.status.help"), msg: "status-msg",
      control: (a) => sw("status-queries", s.status_queries, a) })
    + disc("status-domains", te("settings.status.domainsTitle"), `<p class="hint" data-status-domains>${domains.length
      ? `${te("settings.status.domains")} ${domains.map((d) => `<code>${esc(d)}</code>`).join(", ")}`
      : te("settings.status.none")}</p>`), "data-privacy");
}

/** RF3.4/RF5.2: export and erasure, collapsed in their own group at the end of the page; erasing keeps its question. */
function historyDataHtml(view) {
  const accounts = [["", te("settings.history.allAccounts")], ...historyAccounts(view).map(([index, label]) => [index, esc(label)])];
  return group("history-data", "", disc("history-data", te("settings.history.data"),
    row({ key: "scope", label: te("settings.history.account"), control: (a) => select("history-account", accounts, "", a) })
    + row({ key: "period", label: te("settings.history.period"), control: (a) => select("history-period", PERIODS.map(([d, k]) => [d, te(k)]), "", a) })
    + row({ key: "export", label: te("settings.history.export"), msg: "history-data-msg", msgRole: "status",
      control: () => buttons(`<button class="btn sm" data-export="csv">${icon("copy")} CSV</button>`, `<button class="btn sm" data-export="json">${icon("copy")} JSON</button>`) })
    + row({ key: "erase", label: te("settings.history.eraseLabel"), danger: true, msg: "history-erase-msg",
      control: () => buttons(`<button class="btn sm danger-btn" data-erase>${icon("trash")} ${te("settings.history.erase")}</button>`),
      body: `<div class="confirm" id="history-confirm" role="alertdialog" aria-labelledby="history-confirm-text" hidden>
        <p id="history-confirm-text"></p>
        <div class="dlg-actions left"><button class="btn danger-btn" data-erase-confirm>${te("settings.history.eraseConfirm")}</button>
          <button class="btn" data-erase-cancel>${te("common.cancel")}</button></div>
      </div>` })));
}

export function bind(root, ctx) {
  bindHistory(root, ctx);
  // PRD 0011 RF4.3: saved in desktop.json; on failure the switch goes back to the saved value.
  root.querySelector("#status-queries").onchange = (e) => setStatusEnabled(e.target.checked).then(
    () => { rowError(e.target, ""); toast(); ctx.changed("status_queries"); },
    (x) => { e.target.checked = !e.target.checked; rowError(e.target, errorText(x)); e.target.focus(); });
}

/** PRD 0006 RF4/RF5.3: recording switch and retention (`desktop.json`), size on disk, export and erasure, all done
 *  by the core; erasing always asks first. */
function bindHistory(root, ctx) {
  const days = root.querySelector("#history-days");
  const select = root.querySelector("#history-account");
  const exportMsg = root.querySelector("#history-data-msg");
  const account = () => (select.value === "" ? null : Number(select.value));
  // The indices in the list belong to this generation; the shell refuses one of another (`stale`).
  const generation = ctx.view()?.generation ?? null;
  const failed = (e, line) => {
    if (e === STALE) { ctx.rerender().then(() => document.querySelector("#history-data-msg")?.replaceChildren(t("settings.history.stale"))); return; }
    line.textContent = l10nError(e);
  };
  const period = () => { const v = root.querySelector("#history-period").value; return v === "" ? null : Number(v); };
  const confirm = root.querySelector("#history-confirm");
  const size = root.querySelector("#history-size");
  const showSize = () => historySize().then(
    (bytes) => { size.textContent = t("settings.history.size", { size: fmtBytes(bytes) }); },
    (e) => { size.textContent = t("settings.history.sizeUnknown", { error: l10nError(e) }); });
  showSize();
  // Off stops recording at once and keeps the data; on failure the switch goes back to the saved value.
  root.querySelector("#history-recording").onchange = (e) => setHistoryRecording(e.target.checked).then(
    () => { rowError(e.target, ""); toast(); ctx.changed("history_recording"); },
    (x) => { e.target.checked = !e.target.checked; rowError(e.target, errorText(x)); e.target.focus(); });
  // RF4.2: the retention saves on Enter, on leaving the field or with the row's Apply; the new value prunes at once.
  commitField(days, (value) => setHistoryDays(Number(value)).then(() => ctx.changed("history_days"), (e) => { throw errorText(e); }),
    { onSaved: (value) => { toast(t("settings.history.saved", { days: Number(value) })); showSize(); } });
  for (const button of root.querySelectorAll("[data-export]")) button.onclick = async () => {
    const format = button.dataset.export;
    const path = await pickExportFile(format);
    if (typeof path !== "string" || !path) return; // cancelled
    try { exportMsg.textContent = ""; toast(t("settings.history.exported", { count: await historyExport(account(), period(), format, path, generation), path })); }
    catch (e) { failed(e, exportMsg); }
  };
  // The scope is fixed when the confirmation opens, so changing the list afterwards cannot widen it.
  let scope = null;
  const eraseMsg = root.querySelector("#history-erase-msg");
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
    try { eraseMsg.textContent = ""; toast(t("settings.history.erased", { count: await historyErase(scope, generation) })); ctx.changed("history"); }
    catch (e) { failed(e, eraseMsg); }
    showSize();
  };
}
