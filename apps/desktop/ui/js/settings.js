// Tela de configurações: aparência, idioma, arquivo de contas e pasta de temas/idiomas do usuário.
import { te, t, tl, tle, LOCALES } from "./i18n.js";
import { THEMES } from "./themes.js";
import { userContent, skippedMessage } from "./usercontent.js";
import { bindThemeSwitch, brand, configLabel, copyServeToken, currentSort, fmtNumber, generateServeToken, getSettings, groupingRecharging, historyErase, historyExport, historySize, icon, langMenuHtml, MOTIONS, openUserDir, otherConfigNotice, pickExportFile, pickFolder, pickTomlFile, revokeServeToken, sendTestNotification, serveStatus, setAlertOptions, setAlertsEnabled, setConfigFile, setGroupRecharging, setHistoryDays, setHistoryRecording, setMotion, setNotifyEnabled, setNotifyOptions, setRefreshSeconds, setServe, setShowPace, setSort, setStatusEnabled, setUserDir, themeSwitchHtml } from "./lib.js";
import { esc } from "./util.js";
import { openLibraries } from "./about-dialog.js";
import { showDialog } from "./motion.js";

let dlg = null;

export async function openSettings(extra = {}) {
  if (dlg?.open) return;
  dlg = document.createElement("dialog");
  dlg.className = "dlg settings";
  document.body.append(dlg);
  dlg.addEventListener("close", () => { dlg = null; });
  let view = extra.view?.();
  const render = async (message = "", configMessage = "") => {
    const s = await getSettings();
    const nThemes = THEMES.filter((x) => x.user).length;
    const nLocales = LOCALES.filter((x) => x.user).length;
    dlg.innerHTML = `<div class="dlg-body">
      <h2>${te("settings.title")}</h2>
      <section class="set-sec"><h3>${te("settings.appearance")}</h3>${themeSwitchHtml({ themeSelect: true })}
        <label class="f"><span>${te("settings.motion.label")}</span>
          <select id="motion">${MOTIONS.map((m) => `<option value="${m}" ${s.motion === m ? "selected" : ""}>${te(`settings.motion.${m}`)}</option>`).join("")}</select>
          <small>${te("settings.motion.help")}</small></label>
        <p class="err" id="motion-msg" role="alert"></p>
        <label class="check"><input id="group-recharging" type="checkbox" ${groupingRecharging() ? "checked" : ""} /> ${te("settings.group.recharging")}</label>
        <p class="hint">${te("settings.group.help")}</p>
        <label class="check"><input id="show-pace" type="checkbox" ${s.show_pace !== false ? "checked" : ""} /> ${te("settings.pace.show")}</label>
        <p class="hint">${te("settings.pace.help")}</p>
        <p class="err" id="pace-msg" role="alert"></p></section>
      <section class="set-sec"><h3>${te("lang.label")}</h3>${langMenuHtml({ inline: true })}</section>
      <section class="set-sec"><h3>${te("settings.order.title")}</h3>
        <label class="check"><input id="order-binding" type="checkbox" ${currentSort() === "binding" ? "checked" : ""} /> ${te("settings.order.binding")}</label>
        <p class="hint">${te("settings.order.help")}</p></section>
      <section class="set-sec"><h3>${te("settings.refresh.title")}</h3>
        <p class="hint">${te("settings.refresh.help")}</p>
        <label class="f"><span>${te("settings.refresh.label")}</span>
          <input id="refresh-seconds" type="number" inputmode="numeric" min="30" step="1" value="${esc(extra.view?.()?.refresh_seconds || 300)}" /><small>${te("settings.refresh.min")}</small></label>
        <div class="dlg-actions left">
          ${[[60, "1 min"], [300, "5 min"], [900, "15 min"], [3600, "1 h"]].map(([sec, label]) => `<button class="btn" data-preset="${sec}">${label}</button>`).join("")}
          <button class="btn primary" data-apply-refresh>${te("settings.refresh.save")}</button>
        </div>
        <p class="err" id="refresh-msg" role="alert"></p>
      </section>
      ${notifySectionHtml(s)}
      <section class="set-sec" data-history><h3>${te("settings.history.title")}</h3>
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
          <select id="history-period">${[["1", "settings.history.period.day"], ["7", "settings.history.period.week"], ["30", "settings.history.period.month"], ["", "settings.history.period.all"]]
            .map(([days, key]) => `<option value="${days}" ${days === "" ? "selected" : ""}>${te(key)}</option>`).join("")}</select></label>
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
        <p class="err" id="history-data-msg" role="status"></p>
      </section>
      ${privacySectionHtml(s, view)}
      <section class="set-sec" data-integration><h3>${te("settings.serve.title")}</h3><p class="hint">${te("common.loading")}</p></section>
      <section class="set-sec"><h3>${te("settings.config.title")}</h3>
        <p class="hint">${te("settings.config.help")}</p>
        <div class="path-row"><code title="${esc(configLabel(view))}">${esc(configLabel(view) || "—")}</code></div>
        ${otherConfigNotice(view) ? `<p class="hint warn-txt">${esc(otherConfigNotice(view))}</p>` : ""}
        ${s.config_env_locked ? `<p class="hint warn-txt">${te("settings.config.env")}</p>` : ""}
        <div class="dlg-actions left">
          <button class="btn" data-pick-config ${s.config_env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.config.choose")}</button>
          <button class="btn" data-reset-config ${s.config_env_locked || !s.config_file ? "disabled" : ""}>${te("settings.config.reset")}</button>
        </div>
        <p class="err" role="alert">${esc(configMessage)}</p>
      </section>
      <section class="set-sec"><h3>${te("settings.folder.title")}</h3>
        <p class="hint">${te("settings.folder.help")}</p>
        <div class="path-row"><code title="${esc(s.user_dir ?? "")}">${esc(s.user_dir ?? "—")}</code></div>
        ${s.env_locked ? `<p class="hint warn-txt">${te("settings.folder.env")}</p>` : ""}
        <p class="hint">${s.custom_dir ? te("settings.folder.custom") : te("settings.folder.default")} · ${te("settings.folder.loaded", { themes: nThemes, locales: nLocales })}</p>
        <div class="dlg-actions left">
          <button class="btn" data-pick ${s.env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.folder.choose")}</button>
          <button class="btn" data-reset ${s.env_locked || !s.custom_dir ? "disabled" : ""}>${te("settings.folder.reset")}</button>
          <button class="btn" data-open-dir>${te("settings.folder.open")}</button>
        </div>
        ${userContent.errors.map((e) => `<p class="hint warn-txt">${esc(skippedMessage(e))}</p>`).join("")}
        <p class="err" role="alert">${esc(message)}</p>
      </section>
      <section class="set-sec"><h3>${te("settings.about")}</h3>
        <p class="hint">Cuotai ${esc(s.version)} · ${te("about.license", { license: "MIT" })}</p>
        <div class="dlg-actions left"><button class="btn" data-libraries>${te("about.libraries")}</button></div></section>
      <div class="dlg-actions"><button class="btn primary" data-close>${te("common.close")}</button></div>
    </div>`;
    dlg.querySelector("[data-close]").onclick = () => dlg.close();
    dlg.querySelector("[data-libraries]").onclick = () => openLibraries();
    dlg.querySelector("#motion").onchange = (e) =>
      setMotion(e.target.value).then(() => { dlg.querySelector("#motion-msg").textContent = ""; }, (x) => { dlg.querySelector("#motion-msg").textContent = errorText(x); });
    // PRD 0009 RF2.3: display-only ordering; the manual order (TOML, shared with the TUI) is never rewritten.
    dlg.querySelector("#order-binding").onchange = (e) => { setSort(e.target.checked ? "binding" : "custom"); extra.onChange?.(); };
    dlg.querySelector("#group-recharging").onchange = (e) => { setGroupRecharging(e.target.checked); extra.onChange?.(); };
    // PRD 0007 RF3.4: saved in desktop.json; on failure the box goes back to the saved value.
    dlg.querySelector("#show-pace").onchange = (e) => setShowPace(e.target.checked).then(
      () => { dlg.querySelector("#pace-msg").textContent = ""; extra.onChange?.(); },
      (x) => { e.target.checked = !e.target.checked; dlg.querySelector("#pace-msg").textContent = errorText(x); });
    const input = dlg.querySelector("#refresh-seconds");
    const msg = dlg.querySelector("#refresh-msg");
    for (const b of dlg.querySelectorAll("[data-preset]")) b.onclick = () => { input.value = b.dataset.preset; msg.textContent = ""; };
    dlg.querySelector("[data-apply-refresh]").onclick = async () => {
      msg.classList.remove("ok");
      try {
        await setRefreshSeconds(Number(input.value));
        msg.classList.add("ok");
        msg.textContent = t("settings.refresh.saved");
      } catch (e) { msg.textContent = l10nError(e); }
    };
    bindHistory(dlg);
    bindNotify(dlg, extra, s);
    // PRD 0011 RF4.3: saved in desktop.json; on failure the box goes back to the saved value.
    dlg.querySelector("#status-queries").onchange = (e) => setStatusEnabled(e.target.checked).then(
      () => { dlg.querySelector("#status-msg").textContent = ""; extra.onChange?.(); },
      (x) => { e.target.checked = !e.target.checked; dlg.querySelector("#status-msg").textContent = errorText(x); });
    bindIntegration(dlg.querySelector("[data-integration]"), extra);
    dlg.querySelector("[data-open-dir]").onclick = () => openUserDir().catch((e) => render(errorText(e)));
    const apply = async (path) => {
      try { await setUserDir(path); location.reload(); } // recarrega para ler a nova pasta
      catch (e) { render(errorText(e)); }
    };
    dlg.querySelector("[data-pick]").onclick = async () => {
      const picked = await pickFolder(s.user_dir);
      if (typeof picked === "string" && picked) apply(picked);
    };
    dlg.querySelector("[data-reset]").onclick = () => apply(null);
    const applyConfig = async (path) => {
      try { view = await setConfigFile(path); extra.onChange?.(); render(); }
      catch (e) { render("", configErrorText(e)); }
    };
    dlg.querySelector("[data-pick-config]").onclick = async () => {
      const picked = await pickTomlFile(s.config_file ?? view?.config_path);
      if (typeof picked === "string" && picked) applyConfig(picked);
    };
    dlg.querySelector("[data-reset-config]").onclick = () => applyConfig(null);
  };
  // Tema e idioma (lista inline) passam pelo seletor compartilhado, com cross-fade; espera o redesenho assíncrono.
  bindThemeSwitch(dlg, () => { extra.onChange?.(); return render(); });
  showDialog(dlg);
  await render();
}

/** Accounts of the loaded configuration that have a report, as `[index, label]` for the history scope. */
const historyAccounts = (view) => (view?.reports ?? []).map((r) => [r.index, `${brand(r.provider).label} · ${r.account}`]);

/** "1.8 MiB" / "12.5 KiB": the size of the history file (binary units, as the 64 MiB ceiling). */
const fmtBytes = (bytes) => (bytes < 1 << 20 ? `${fmtNumber(bytes / 1024, 1)} KiB` : `${fmtNumber(bytes / (1 << 20), 1)} MiB`);

/** PRD 0006 RF4/RF5.3: recording switch and retention (`desktop.json`), size on disk, export and erasure, all done
 *  by the core; erasing always asks first. */
function bindHistory(dlg) {
  const days = dlg.querySelector("#history-days");
  const msg = dlg.querySelector("#history-msg");
  const dataMsg = dlg.querySelector("#history-data-msg");
  const select = dlg.querySelector("#history-account");
  const account = () => (select.value === "" ? null : Number(select.value));
  const period = () => { const v = dlg.querySelector("#history-period").value; return v === "" ? null : Number(v); };
  const confirm = dlg.querySelector("#history-confirm");
  const report = (text, ok) => { dataMsg.classList.toggle("ok", ok); dataMsg.textContent = text; };
  const size = dlg.querySelector("#history-size");
  const showSize = () => historySize().then(
    (bytes) => { size.textContent = t("settings.history.size", { size: fmtBytes(bytes) }); },
    (e) => { size.textContent = t("settings.history.sizeUnknown", { error: l10nError(e) }); });
  showSize();
  // Off stops recording at once and keeps the data; on failure the box goes back to the saved value.
  dlg.querySelector("#history-recording").onchange = (e) => setHistoryRecording(e.target.checked).then(
    () => { dlg.querySelector("#history-recording-msg").textContent = ""; },
    (x) => { e.target.checked = !e.target.checked; dlg.querySelector("#history-recording-msg").textContent = errorText(x); });
  dlg.querySelector("[data-apply-history]").onclick = async () => {
    msg.classList.remove("ok");
    try {
      await setHistoryDays(Number(days.value));
      msg.classList.add("ok");
      msg.textContent = t("settings.history.saved", { days: Number(days.value) });
      showSize(); // the new retention prunes at once
    } catch (e) { msg.textContent = errorText(e); }
  };
  for (const button of dlg.querySelectorAll("[data-export]")) button.onclick = async () => {
    const format = button.dataset.export;
    const path = await pickExportFile(format);
    if (typeof path !== "string" || !path) return; // cancelled
    try { report(t("settings.history.exported", { count: await historyExport(account(), period(), format, path), path }), true); }
    catch (e) { report(l10nError(e), false); }
  };
  // The scope is fixed when the confirmation opens, so changing the list afterwards cannot widen it.
  let scope = null;
  dlg.querySelector("[data-erase]").onclick = () => {
    scope = account();
    confirm.querySelector("#history-confirm-text").textContent = scope == null
      ? t("settings.history.eraseAll")
      : t("settings.history.eraseOne", { account: select.selectedOptions[0]?.textContent ?? "" });
    confirm.hidden = false;
    confirm.querySelector("[data-erase-cancel]").focus();
  };
  confirm.querySelector("[data-erase-cancel]").onclick = () => { confirm.hidden = true; dlg.querySelector("[data-erase]").focus(); };
  confirm.querySelector("[data-erase-confirm]").onclick = async () => {
    confirm.hidden = true;
    try { report(t("settings.history.erased", { count: await historyErase(scope) }), true); }
    catch (e) { report(l10nError(e), false); }
    showSize();
  };
}

/** PRD 0011 RF4.3: public provider status queries, off until the user turns them on, with what they reveal and the
 *  domains they reach for the configured providers (listed while off too, so the choice is informed). */
function privacySectionHtml(s, view) {
  const domains = view?.status?.domains ?? [];
  return `<section class="set-sec" data-privacy><h3>${te("settings.privacy.title")}</h3>
        <label class="check"><input id="status-queries" type="checkbox" ${s.status_queries ? "checked" : ""} /> ${te("settings.status.on")}</label>
        <p class="hint">${te("settings.status.help")}</p>
        <p class="hint" data-status-domains>${domains.length
          ? `${te("settings.status.domains")} ${domains.map((d) => `<code>${esc(d)}</code>`).join(", ")}`
          : te("settings.status.none")}</p>
        <p class="err" id="status-msg" role="alert"></p>
      </section>`;
}

/** PRD 0012 RF5: the local server, off by default. The switch, the state (the effective address, or why it is not
 *  serving, naming another Cuotai server on the port), the port (bound exactly), the token (generated into the system
 *  vault, copied, renewed or revoked; never shown) and "allow browsers" behind the core's warning. Every change
 *  restarts the server and the section repaints from the state the backend answers. */
function integrationHtml(sv) {
  const state = sv.state === "on"
    ? `<p class="hint" data-serve-state><strong>${te("settings.serve.state.on")}</strong> <code>${esc(sv.url)}</code></p>
        <div class="dlg-actions left"><button class="btn" data-serve-copy-url>${icon("copy")} ${te("settings.serve.copyUrl")}</button></div>`
    : sv.state === "error"
      ? `<p class="hint warn-txt" data-serve-state role="alert">${icon("warn")} <strong>${te("settings.serve.state.error")}</strong> ${tle(sv.error)}${
        sv.cuotai_on_port ? ` ${te("settings.serve.cuotaiOnPort", { port: sv.port })}` : ""}</p>`
      : `<p class="hint" data-serve-state><strong>${te("settings.serve.state.off")}</strong></p>`;
  const token = sv.token
    ? `<p class="hint">${te("settings.serve.token.on")}</p>
        <div class="dlg-actions left"><button class="btn" data-serve-token-copy>${icon("copy")} ${te("settings.serve.token.copy")}</button>
          <button class="btn" data-serve-token-new>${te("settings.serve.token.renew")}</button>
          <button class="btn danger-btn" data-serve-token-revoke>${te("settings.serve.token.revoke")}</button></div>
        <p class="hint">${te("settings.serve.token.revokeHelp")}</p>`
    : `<p class="hint">${te("settings.serve.token.off")}</p>
        <div class="dlg-actions left"><button class="btn" data-serve-token-new>${te("settings.serve.token.generate")}</button></div>`;
  return `<h3>${te("settings.serve.title")}</h3>
        <p class="hint">${te("settings.serve.help")}</p>
        <label class="check"><input id="serve-on" type="checkbox" ${sv.enabled ? "checked" : ""} /> ${te("settings.serve.on")}</label>
        ${state}
        <label class="f"><span>${te("settings.serve.port")}</span>
          <input id="serve-port" type="number" inputmode="numeric" min="${sv.port_min}" max="${sv.port_max}" step="1" value="${esc(sv.port)}" />
          <small>${te("settings.serve.portHelp", { port: sv.default_port, min: sv.port_min, max: sv.port_max })}</small></label>
        <div class="dlg-actions left"><button class="btn primary" data-serve-port>${te("settings.serve.portSave")}</button></div>
        <h4>${te("settings.serve.token.title")}</h4>
        ${token}
        <label class="check"><input id="serve-browsers" type="checkbox" ${sv.allow_browsers ? "checked" : ""} /> ${te("settings.serve.browsers")}</label>
        ${sv.allow_browsers ? `<p class="hint warn-txt">${tle(sv.browsers_warning)}</p>` : `<p class="hint">${te("settings.serve.browsersOff")}</p>`}
        <div class="confirm" id="serve-browsers-confirm" role="alertdialog" aria-labelledby="serve-browsers-text" hidden>
          <p id="serve-browsers-text">${tle(sv.browsers_warning)}</p>
          <div class="dlg-actions left"><button class="btn danger-btn" data-serve-browsers-confirm>${te("settings.serve.browsersConfirm")}</button>
            <button class="btn" data-serve-browsers-cancel>${te("common.cancel")}</button></div>
        </div>
        <p class="hint">${te("settings.serve.privacy")}</p>
        <p class="err" id="serve-msg" role="alert"></p>`;
}

/** Paints the section and binds it; a failed change keeps the previous state and shows why. */
async function bindIntegration(sec, extra) {
  let sv;
  try { sv = await serveStatus(); } catch (e) { sec.querySelector(".hint").textContent = l10nError(e); return; }
  const paint = (next, message = "") => {
    sv = next;
    sec.innerHTML = integrationHtml(sv);
    const msg = sec.querySelector("#serve-msg");
    msg.textContent = message;
    const run = (work) => work().then((answer) => { paint(answer); extra.onChange?.(); }, (e) => paint(sv, serveErrorText(e)));
    const port = () => Number(sec.querySelector("#serve-port").value);
    sec.querySelector("#serve-on").onchange = (e) => run(() => setServe(e.target.checked, sv.port, sv.allow_browsers));
    sec.querySelector("[data-serve-port]").onclick = () => run(() => setServe(sv.enabled, port(), sv.allow_browsers));
    sec.querySelector("[data-serve-token-new]").onclick = () => run(generateServeToken);
    const revoke = sec.querySelector("[data-serve-token-revoke]");
    if (revoke) revoke.onclick = () => run(revokeServeToken);
    const copied = (btn) => { const prev = btn.innerHTML; btn.textContent = t("settings.serve.copied"); setTimeout(() => { btn.innerHTML = prev; }, 1500); };
    const copyToken = sec.querySelector("[data-serve-token-copy]");
    if (copyToken) copyToken.onclick = () => copySecret(copyServeToken).then(() => copied(copyToken), (e) => { msg.textContent = serveErrorText(e); });
    const copyUrl = sec.querySelector("[data-serve-copy-url]");
    if (copyUrl) copyUrl.onclick = () => navigator.clipboard?.writeText(sv.url).then(() => copied(copyUrl), () => { msg.textContent = t("settings.serve.copyFailed"); });
    // RF3.3: pages are only let in after the warning was read and confirmed.
    const confirm = sec.querySelector("#serve-browsers-confirm");
    sec.querySelector("#serve-browsers").onchange = (e) => {
      if (!e.target.checked) { run(() => setServe(sv.enabled, sv.port, false)); return; }
      e.target.checked = false;
      confirm.hidden = false;
      sec.querySelector("[data-serve-browsers-confirm]").focus();
    };
    sec.querySelector("[data-serve-browsers-confirm]").onclick = () => run(() => setServe(sv.enabled, sv.port, true));
    sec.querySelector("[data-serve-browsers-cancel]").onclick = () => { confirm.hidden = true; };
  };
  paint(sv);
}

/** Copies a secret fetched on click. WebKit only lets the clipboard follow the click synchronously, so the text goes
 *  in as a pending `ClipboardItem` when supported; otherwise it is written once it arrives. */
function copySecret(fetchText) {
  if (window.ClipboardItem && navigator.clipboard?.write) {
    const blob = fetchText().then((text) => new Blob([text], { type: "text/plain" }));
    return navigator.clipboard.write([new ClipboardItem({ "text/plain": blob })]);
  }
  return fetchText().then((text) => navigator.clipboard.writeText(text));
}

/** Shell codes (`invalid_port`, `unwritable`) or the core's typed vault error. */
const serveErrorText = (e) => {
  const key = `settings.error.${e}`;
  const tr = typeof e === "string" ? t(key) : key;
  return tr === key ? l10nError(e) : tr;
};

/** PRD 0003 RF3.1: reset notices of this device (desktop.json). Off by default; the backend schedules them.
 *  PRD 0008 RF3.1: usage alerts in the same section (off by default), sharing its permission guidance and test. */
function notifySectionHtml(s) {
  const n = s.notify;
  const a = s.alerts;
  const leads = s.notify_leads.map((m) => `<option value="${m}" ${n.lead_minutes === m ? "selected" : ""}>${m ? te("settings.notify.leadMin", { min: m }) : te("settings.notify.leadNow")}</option>`).join("");
  const whens = s.notify_whens.map((w) => `<option value="${w}" ${n.when === w ? "selected" : ""}>${te(`settings.notify.when.${w}`)}</option>`).join("");
  return `<section class="set-sec" data-notify><h3>${te("settings.notify.title")}</h3>
        <p class="hint">${te("settings.notify.help")}</p>
        <label class="check"><input id="notify-on" type="checkbox" ${n.enabled ? "checked" : ""} /> ${te("settings.notify.on")}</label>
        <div class="confirm" id="notify-denied" role="alert" hidden><p>${te("settings.notify.denied")}</p><p class="hint">${te("settings.notify.system")}</p></div>
        <label class="f"><span>${te("settings.notify.lead")}</span><select id="notify-lead">${leads}</select></label>
        <label class="f"><span>${te("settings.notify.when")}</span><select id="notify-when">${whens}</select>
          <small>${te("settings.notify.whenHelp")}</small></label>
        <label class="check"><input id="notify-other" type="checkbox" ${n.include_other ? "checked" : ""} /> ${te("settings.notify.other")}</label>
        <label class="check" data-alerts><input id="alerts-on" type="checkbox" ${a.enabled ? "checked" : ""} /> ${te("settings.alerts.on")}</label>
        <p class="hint">${te("settings.alerts.help")}</p>
        <div class="f"><span id="alert-thresholds-label">${te("settings.alerts.thresholds")}</span>
          <div class="thresholds" role="group" aria-labelledby="alert-thresholds-label"><span id="alert-chips"></span>
            <input id="alert-new" type="number" min="${s.alert_min}" max="${s.alert_max}" step="1" inputmode="numeric" aria-label="${te("settings.alerts.new")}" placeholder="%" />
            <button class="btn" type="button" data-alert-add>${te("settings.alerts.add")}</button></div>
          <small>${te("settings.alerts.thresholdsHelp", { min: s.alert_min, max: s.alert_max })}</small></div>
        <label class="check"><input id="alerts-exhaustion" type="checkbox" ${a.exhaustion ? "checked" : ""} /> ${te("settings.alerts.exhaustion")}</label>
        <label class="check"><input id="alerts-recovery" type="checkbox" ${a.recovery ? "checked" : ""} /> ${te("settings.alerts.recovery")}</label>
        <p class="hint">${te("settings.notify.accounts")}</p>
        <div class="dlg-actions left"><button class="btn" data-notify-test ${n.enabled || a.enabled ? "" : "disabled"}>${icon("bell")} ${te("settings.notify.test")}</button></div>
        <p class="err" id="notify-msg" role="status"></p>
      </section>`;
}

/** RF3.3: turning either switch on asks the system permission first; anything but `granted` keeps that switch off
 *  and shows the guidance. The test waits for a switch: on desktop the first notice is what makes the system decide
 *  the permission. Failures put the controls back to the saved values. */
function bindNotify(dlg, extra, s) {
  const on = dlg.querySelector("#notify-on");
  const alertsOn = dlg.querySelector("#alerts-on");
  const denied = dlg.querySelector("#notify-denied");
  const msg = dlg.querySelector("#notify-msg");
  const test = dlg.querySelector("[data-notify-test]");
  const report = (text, ok = false) => { msg.classList.toggle("ok", ok); msg.textContent = text; };
  const toggle = (box, enable) => async () => {
    const want = box.checked;
    box.disabled = true;
    try {
      const permission = await enable(want);
      box.checked = permission === "granted";
      test.disabled = !(on.checked || alertsOn.checked);
      denied.hidden = !want || permission === "granted";
      report("");
      extra.onChange?.();
    } catch (x) { box.checked = !want; report(errorText(x)); }
    finally { box.disabled = false; }
  };
  on.onchange = toggle(on, setNotifyEnabled);
  alertsOn.onchange = toggle(alertsOn, setAlertsEnabled);
  const lead = dlg.querySelector("#notify-lead");
  const when = dlg.querySelector("#notify-when");
  const other = dlg.querySelector("#notify-other");
  const saved = { lead: lead.value, when: when.value, other: other.checked };
  const options = async () => {
    try {
      await setNotifyOptions(Number(lead.value), when.value, other.checked);
      Object.assign(saved, { lead: lead.value, when: when.value, other: other.checked });
      report("");
      extra.onChange?.();
    } catch (x) {
      lead.value = saved.lead; when.value = saved.when; other.checked = saved.other;
      report(errorText(x));
    }
  };
  lead.onchange = when.onchange = other.onchange = options;
  bindAlertOptions(dlg, extra, s, report);
  test.onclick = () =>
    sendTestNotification(t("notify.test.title"), t("notify.test.body")).then(
      () => report(t("settings.notify.testSent"), true),
      () => report(t("settings.notify.testFailed")));
}

/** PRD 0008 RF3.1: the thresholds as chips (removed by a click, added from the field, 10 to 99), plus the
 *  exhaustion and recovery notices; every change is saved at once and a refused one is put back. */
function bindAlertOptions(dlg, extra, s, report) {
  const chips = dlg.querySelector("#alert-chips");
  const input = dlg.querySelector("#alert-new");
  const exhaustion = dlg.querySelector("#alerts-exhaustion");
  const recovery = dlg.querySelector("#alerts-recovery");
  const saved = { thresholds: [...s.alerts.thresholds], exhaustion: exhaustion.checked, recovery: recovery.checked };
  const draw = () => {
    chips.innerHTML = saved.thresholds.map((v) => {
      const label = t("settings.alerts.remove", { pct: v });
      return `<button class="chip" type="button" data-alert-remove="${v}" title="${esc(label)}" aria-label="${esc(label)}">${v}% ×</button>`;
    }).join("");
  };
  const save = async (thresholds) => {
    try {
      await setAlertOptions(thresholds, exhaustion.checked, recovery.checked);
      Object.assign(saved, { thresholds: [...new Set(thresholds)].sort((x, y) => x - y), exhaustion: exhaustion.checked, recovery: recovery.checked });
      report("");
      extra.onChange?.();
      return true;
    } catch (x) {
      exhaustion.checked = saved.exhaustion; recovery.checked = saved.recovery;
      report(errorText(x));
      return false;
    } finally { draw(); }
  };
  chips.onclick = (e) => {
    const chip = e.target.closest("[data-alert-remove]");
    if (chip) save(saved.thresholds.filter((v) => v !== Number(chip.dataset.alertRemove)));
  };
  const add = async () => {
    const value = Number(input.value);
    if (!Number.isInteger(value) || value < s.alert_min || value > s.alert_max) return report(t("settings.error.invalid_threshold"));
    if (await save([...saved.thresholds, value])) input.value = "";
  };
  dlg.querySelector("[data-alert-add]").onclick = add;
  input.onkeydown = (e) => { if (e.key === "Enter") { e.preventDefault(); add(); } };
  exhaustion.onchange = recovery.onchange = () => save(saved.thresholds);
  draw();
}

const errorText = (e) => {
  const code = typeof e === "string" ? e : "";
  const key = `settings.error.${code}`;
  const tr = t(key);
  return tr === key ? t("common.unexpected") : tr;
};

/** Códigos do shell (`not_file`, `unwritable`, `env_locked`) ou erro tipado do core ao carregar o TOML. */
const configErrorText = (e) => {
  const key = `settings.config.error.${e}`;
  const tr = typeof e === "string" ? t(key) : key;
  return tr === key ? l10nError(e) : tr;
};

/** Erro do shell: JSON `{ id, params, fallback }` (RF8) ou texto simples. */
const l10nError = (e) => {
  if (typeof e === "string") {
    try { const p = JSON.parse(e); if (p && typeof p === "object" && "id" in p) return tl(p); } catch {}
    return e;
  }
  return e?.message ?? t("common.unexpected");
};
