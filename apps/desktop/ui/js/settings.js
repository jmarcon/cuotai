// Tela de configurações: aparência, idioma, arquivo de contas e pasta de temas/idiomas do usuário.
import { te, t, tl, tle, currentLocale, LOCALES } from "./i18n.js";
import { THEMES } from "./themes.js";
import { userContent, skippedMessage } from "./usercontent.js";
import { bindThemeSwitch, brand, configLabel, confirmHooks, copyServeToken, costDisable, costEnable, costErase, costPrices, costSetOptions, costSetPaths, costStatus, currentSort, deleteWidgetData, fmtNumber, fmtReset, generateServeToken, getSettings, groupingRecharging, historyErase, historyExport, historySize, hooksStatus, icon, langMenuHtml, MOTIONS, onHookRun, openUserDir, otherConfigNotice, pickExportFile, pickFolder, pickTomlFile, revokeServeToken, saveHooks, sendTestNotification, serveStatus, setAlertOptions, setAlertsEnabled, setConfigFile, setGroupRecharging, setHistoryDays, setHistoryRecording, setHooksEnabled, setMotion, setNotifyEnabled, setNotifyOptions, setRefreshSeconds, setServe, setShowPace, setSort, setStatusEnabled, setUserDir, setWidgets, testHook, themeSwitchHtml, widgetsStatus } from "./lib.js";
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
      <section class="set-sec" data-cost><h3>${te("settings.cost.title")}</h3>
        <p class="hint">${te("settings.cost.help")}</p>
        <div data-cost-body><p class="hint">${te("common.loading")}</p></div></section>
      <section class="set-sec" data-integration><h3>${te("settings.serve.title")}</h3>
        <div data-serve><p class="hint">${te("common.loading")}</p></div>
        <div data-hooks><p class="hint">${te("common.loading")}</p></div>
        <div data-widgets><p class="hint">${te("common.loading")}</p></div></section>
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
    bindIntegration(dlg.querySelector("[data-serve]"), extra);
    bindHooks(dlg.querySelector("[data-hooks]"), extra);
    bindWidgets(dlg.querySelector("[data-widgets]"), extra);
    bindCost(dlg.querySelector("[data-cost-body]"), extra);
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

/** PRD 0015 RF1/RF2/RF4.1: local cost from the logs of command-line tools, off by default. Per source a switch that
 *  first shows the core's disclosure (exactly the directories read, what for, and that only numbers are extracted)
 *  and turns on with its digest; turning off confirms that everything derived is erased; the directories it reads
 *  (a different list turns a source that is on off, so its new disclosure is reviewed). Then the period, the index
 *  retention, the prices in effect (the embedded table under the user's pricing.json), the index size and its
 *  erasure; the state the core's background scan last reported (paused while saving energy). */
function costHtml(cv, prices) {
  const state = { starting: "settings.cost.state.starting", scanning: "settings.cost.state.scanning", paused: "settings.cost.state.paused" }[cv.state];
  const sources = cv.sources.map((s) => `<div class="cost-source" data-cost-source="${esc(s.source)}">
        <label class="check"><input type="checkbox" data-cost-toggle="${esc(s.source)}" ${s.enabled ? "checked" : ""} /> ${te("settings.cost.read", { source: s.label })}</label>
        <p class="hint" data-cost-state>${s.enabled ? te("settings.cost.since", { at: fmtReset(s.consented_at) ?? "—" }) : te("settings.cost.off")}</p>
        <p class="hint">${te("settings.cost.dirs", { pattern: s.disclosure.pattern })} ${s.disclosure.directories.map((d) => `<code>${esc(d)}</code>`).join(", ")}${s.disclosure.custom ? ` (${te("settings.cost.pathsChosen")})` : ""}</p>
        ${s.enabled && s.summary ? `<p class="hint" data-cost-summary>${tle(s.summary)}</p>` : ""}
        ${s.enabled ? `<p class="hint" data-cost-indexed>${te("settings.cost.indexed", { files: s.indexed.files, invalid: s.indexed.invalid_lines })}</p>` : ""}
        <details class="cost-paths" data-cost-paths-box="${esc(s.source)}" ${pathsOpen.has(s.source) ? "open" : ""}><summary>${te("settings.cost.pathsEdit")}</summary>
          <label class="f"><span>${te("settings.cost.paths", { source: s.label })}</span>
            <textarea rows="3" spellcheck="false" autocomplete="off" data-cost-paths="${esc(s.source)}" placeholder="~/…">${esc(s.paths.join("\n"))}</textarea>
            <small>${te("settings.cost.pathsHelp")}</small></label>
          <div class="dlg-actions left"><button class="btn" data-cost-paths-save="${esc(s.source)}">${te("settings.cost.pathsSave")}</button>
            <button class="btn" data-cost-paths-reset="${esc(s.source)}" ${s.paths.length ? "" : "disabled"}>${te("settings.cost.pathsReset")}</button></div>
        </details>
      </div>`).join("");
  return `${sources}
        ${state ? `<p class="hint${cv.state === "paused" ? " warn-txt" : ""}" data-cost-worker role="status">${te(state)}</p>` : ""}
        ${cv.error ? `<p class="hint warn-txt" role="alert">${tle(cv.error)}</p>` : ""}
        <label class="f"><span>${te("settings.cost.period")}</span>
          <select id="cost-period">${cv.periods.map((p) => `<option value="${esc(p.id)}" ${p.id === cv.period ? "selected" : ""}>${tle(p.label)}</option>`).join("")}</select></label>
        <label class="f"><span>${te("settings.cost.retention")}</span>
          <input id="cost-retention" type="number" inputmode="numeric" min="${cv.retention_min}" max="${cv.retention_max}" step="1" value="${esc(cv.retention_days)}" />
          <small>${te("settings.cost.retentionRange", { min: cv.retention_min, max: cv.retention_max })}</small></label>
        <div class="dlg-actions left"><button class="btn primary" data-cost-save>${te("settings.cost.save")}</button></div>
        <div class="cost-prices" data-cost-prices>${prices ? pricesHtml(prices) : `<p class="hint">${te("common.loading")}</p>`}</div>
        <p class="hint" data-cost-size>${te("settings.cost.size", { size: fmtBytes(cv.size) })}</p>
        <div class="dlg-actions left"><button class="btn danger-btn" data-cost-erase>${icon("trash")} ${te("settings.cost.erase")}</button></div>
        <div class="confirm" id="cost-erase-confirm" role="alertdialog" aria-labelledby="cost-erase-text" hidden>
          <p id="cost-erase-text">${te("settings.cost.eraseText")}</p>
          <div class="dlg-actions left"><button class="btn danger-btn" data-cost-erase-confirm>${te("settings.cost.eraseConfirm")}</button>
            <button class="btn" data-cost-erase-cancel>${te("common.cancel")}</button></div>
        </div>
        <p class="err" id="cost-msg" role="alert"></p>`;
}

/** RF3/RF4.1: the prices the estimate uses, with each table's date and source and the state of pricing.json. */
function pricesHtml(p) {
  const user = p.models.filter((m) => m.origin === "user").length;
  const tables = p.tables.map((tb) => `<p class="hint">${tb.origin === "embedded"
    ? te("settings.cost.pricesEmbedded", { date: tb.effective, source: tb.source ?? "—" })
    : te("settings.cost.pricesUser", { date: tb.effective, source: tb.source ?? "—", count: user })}</p>`).join("");
  const file = p.pricing_file;
  const state = file.error ? `<p class="hint warn-txt" role="alert">${tle(file.error)}</p>`
    : file.used ? "" : `<p class="hint">${te("settings.cost.pricesMissing")}</p>`;
  const rate = (r) => fmtNumber(r, 6);
  const rows = p.models.map((m) => `<tr><th scope="row"><code>${esc(m.model)}</code></th><td>${rate(m.rates.input)}</td><td>${rate(m.rates.output)}</td>
      <td>${rate(m.rates.cache_read)}</td><td>${rate(m.rates.cache_write)}</td><td>${te(`cost.origin.${m.origin}`)}</td></tr>`).join("");
  const cols = ["model", "input", "output", "cacheRead", "cacheWrite", "origin"].map((c) => `<th scope="col">${te(`settings.cost.col.${c}`)}</th>`).join("");
  return `<h4>${te("settings.cost.prices")}</h4>
    <p class="hint">${te("settings.cost.pricesHelp")}</p>${tables}${state}
    <p class="hint">${te("settings.cost.pricesFile")} <code>${esc(file.path)}</code></p>
    <details class="cost-price-table" ${pricesOpen ? "open" : ""}><summary>${te("settings.cost.pricesShow", { count: p.models.length })}</summary>
      <table><thead><tr>${cols}</tr></thead><tbody>${rows}</tbody></table></details>
    <p class="hint">${tle(p.notice)} ${te("settings.cost.unknownModel")}</p>`;
}

// What stays open across the section's repaints.
const pathsOpen = new Set();
let pricesOpen = false;

/** Paints the section from the backend's state and binds it; while a scan is under way it reads the state again.
 *  `keep`: a repaint the user did not ask for (the scan's state, the prices arriving) keeps the directories being
 *  typed and the focus. */
async function bindCost(box, extra) {
  let cv, prices = null;
  try { cv = await costStatus(); } catch (e) { box.querySelector(".hint").textContent = l10nError(e); return; }
  let timer = null;
  const paint = (next, message = "", ok = false, keep = false) => {
    cv = next;
    clearTimeout(timer);
    const drafts = keep ? [...box.querySelectorAll("[data-cost-paths]")].map((el) => [el.dataset.costPaths, el.value]) : [];
    const focused = keep && document.activeElement?.dataset?.costPaths;
    const caret = focused ? document.activeElement.selectionStart : 0;
    box.innerHTML = costHtml(cv, prices);
    for (const [source, value] of drafts) box.querySelector(`[data-cost-paths="${source}"]`).value = value;
    if (focused) {
      const el = box.querySelector(`[data-cost-paths="${focused}"]`);
      el.focus();
      el.setSelectionRange(caret, caret);
    }
    const msg = box.querySelector("#cost-msg");
    msg.classList.toggle("ok", ok);
    msg.textContent = message;
    // A change also repaints the cards' local usage (`onChange`); a refused one keeps what was typed.
    const run = (work, done = () => "") => work().then((answer) => { paint(answer, done(), true); extra.onChange?.(); }, (e) => paint(cv, l10nError(e), false, true));
    for (const input of box.querySelectorAll("[data-cost-toggle]")) input.onchange = async () => {
      const source = cv.sources.find((s) => s.source === input.dataset.costToggle);
      input.checked = source.enabled; // only the confirmed answer changes it
      if (!source.enabled) {
        if (await confirmDisclosure(source.disclosure)) run(() => costEnable(source.source, source.disclosure.digest));
      } else if (await confirmCostOff(source)) run(() => costDisable(source.source));
    };
    for (const el of box.querySelectorAll("[data-cost-paths-box]")) {
      el.ontoggle = () => (el.open ? pathsOpen.add(el.dataset.costPathsBox) : pathsOpen.delete(el.dataset.costPathsBox));
    }
    // RF4.1: a different list needs a new consent, so a source that is on turns off (erasing what was derived) first.
    const savePaths = async (source, paths) => {
      const chosen = paths.map((p) => p.trim()).filter(Boolean);
      const changed = JSON.stringify([...new Set(chosen)]) !== JSON.stringify(source.paths);
      if (changed && source.enabled && !(await confirmPathsOff(source))) return;
      run(() => costSetPaths(source.source, chosen), () => (changed ? t("settings.cost.pathsSaved", { source: source.label }) : ""));
    };
    for (const button of box.querySelectorAll("[data-cost-paths-save]")) button.onclick = () => {
      const source = cv.sources.find((s) => s.source === button.dataset.costPathsSave);
      savePaths(source, box.querySelector(`[data-cost-paths="${source.source}"]`).value.split("\n"));
    };
    for (const button of box.querySelectorAll("[data-cost-paths-reset]")) button.onclick = () => {
      savePaths(cv.sources.find((s) => s.source === button.dataset.costPathsReset), []);
    };
    const table = box.querySelector(".cost-price-table");
    if (table) table.ontoggle = () => { pricesOpen = table.open; };
    box.querySelector("[data-cost-save]").onclick = () => run(
      () => costSetOptions(box.querySelector("#cost-period").value, Number(box.querySelector("#cost-retention").value)),
      () => t("settings.cost.saved"));
    const confirm = box.querySelector("#cost-erase-confirm");
    box.querySelector("[data-cost-erase]").onclick = () => { confirm.hidden = false; confirm.querySelector("[data-cost-erase-cancel]").focus(); };
    confirm.querySelector("[data-cost-erase-cancel]").onclick = () => { confirm.hidden = true; box.querySelector("[data-cost-erase]").focus(); };
    confirm.querySelector("[data-cost-erase-confirm]").onclick = async () => {
      try {
        const count = await costErase();
        paint(await costStatus(), t("settings.cost.erased", { count }), true);
        extra.onChange?.();
      } catch (e) { paint(cv, l10nError(e)); }
    };
    // The background scan answers after the change: read its state again until it settles.
    const settled = ["paused", "failed"].includes(cv.state);
    const busy = ["starting", "scanning"].includes(cv.state) || (!settled && cv.sources.some((s) => s.enabled && !s.summary));
    if (busy) timer = setTimeout(() => { if (box.isConnected) costStatus().then((answer) => paint(answer, msg.textContent, msg.classList.contains("ok"), true), () => {}); }, 1000);
  };
  paint(cv);
  // The prices change only with pricing.json; read once per opening.
  costPrices().then((answer) => {
    prices = answer;
    const msg = box.querySelector("#cost-msg");
    if (msg) paint(cv, msg.textContent, msg.classList.contains("ok"), true);
  }, (e) => { const slot = box.querySelector("[data-cost-prices]"); if (slot) slot.innerHTML = `<p class="hint warn-txt">${esc(l10nError(e))}</p>`; });
}

/** RF1.1: the core's disclosure of one source before it is turned on; resolves to the user's answer. */
function confirmDisclosure(d) {
  return askDialog("cost-confirm", te("settings.cost.confirm.title", { source: d.label }), `<p>${tle(d.purpose)}</p>
      <p>${te("settings.cost.confirm.dirs", { pattern: d.pattern })}</p>
      <ul class="cost-dirs" data-cost-dirs>${d.directories.map((dir) => `<li><code>${esc(dir)}</code></li>`).join("")}</ul>
      <p>${tle(d.extracted)}</p>
      <p class="hint">${tle(d.never)}</p>
      <p class="hint">${tle(d.off)}</p>`, te("settings.cost.confirm.yes"));
}

/** RF2.4: turning a source off erases everything derived from it; resolves to the user's answer. */
function confirmCostOff(s) {
  return askDialog("cost-off", te("settings.cost.disable.title", { source: s.label }),
    `<p>${te("settings.cost.disable.text", { source: s.label })}</p>`, te("settings.cost.disable.yes"));
}

/** RF4.1: choosing other directories for a source that is on turns it off and erases what was derived from it. */
function confirmPathsOff(s) {
  return askDialog("cost-off", te("settings.cost.pathsOffTitle", { source: s.label }),
    `<p>${te("settings.cost.pathsOffText", { source: s.label })}</p>`, te("settings.cost.pathsOffYes"));
}

/** A modal question with Cancel focused first; resolves `true` only for the confirming button. */
function askDialog(id, title, body, yes) {
  return new Promise((resolve) => {
    const d = document.createElement("dialog");
    d.className = `dlg ${id}`;
    d.setAttribute("role", "alertdialog");
    d.setAttribute("aria-labelledby", `${id}-title`);
    d.innerHTML = `<div class="dlg-body"><h2 id="${id}-title">${title}</h2>${body}
      <div class="dlg-actions"><button class="btn" data-ask-no>${te("common.cancel")}</button>
        <button class="btn danger-btn" data-ask-yes>${yes}</button></div></div>`;
    document.body.append(d);
    let answer = false;
    d.addEventListener("close", () => { d.remove(); resolve(answer); }, { once: true });
    d.querySelector("[data-ask-yes]").onclick = () => { answer = true; d.close(); };
    d.querySelector("[data-ask-no]").onclick = () => d.close();
    showDialog(d);
    d.querySelector("[data-ask-no]").focus();
  });
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
  return `<p class="hint">${te("settings.serve.help")}</p>
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

/** PRD 0014 RF4.1/RF4.2/RF2.3: the widget snapshot of this device, off by default. The switch, whether identity
 *  (e-mail) travels with it, how many accounts are published, the file location (and whether it is the app group a
 *  WidgetKit extension can read), and "delete widget data". The redraw of a widget depends on the operating system,
 *  so the data on it may lag; on macOS the extension also needs a signed app with the app group entitlement. */
function widgetsHtml(wv, message = "") {
  const p = wv.prefs ?? { enabled: false, include_identity: false, max_accounts: 6 };
  const where = wv.app_group
    ? te("settings.widgets.path.appGroup")
    : wv.macos ? te("settings.widgets.path.userDataMac") : te("settings.widgets.path.userData");
  return `<h4>${te("settings.widgets.title")}</h4>
    <p class="hint">${te("settings.widgets.help")}</p>
    <label class="check"><input id="widgets-on" type="checkbox" ${p.enabled ? "checked" : ""} /> ${te("settings.widgets.on")}</label>
    <p class="hint">${te(p.enabled ? "settings.widgets.onState" : "settings.widgets.off")}</p>
    <label class="check"><input id="widgets-identity" type="checkbox" ${p.include_identity ? "checked" : ""} /> ${te("settings.widgets.identity")}</label>
    <p class="hint">${te("settings.widgets.identityHelp")}</p>
    <label class="f"><span>${te("settings.widgets.maxAccounts")}</span>
      <input id="widgets-max" type="number" inputmode="numeric" min="${esc(wv.accounts_min)}" max="${esc(wv.accounts_max)}" step="1" value="${esc(p.max_accounts)}" />
      <small>${te("settings.widgets.maxAccountsHelp", { min: wv.accounts_min, max: wv.accounts_max })}</small></label>
    <p class="hint">${te("settings.widgets.path.label")}</p>
    <div class="path-row"><code title="${esc(wv.path ?? "")}">${esc(wv.path || "—")}</code></div>
    <p class="hint">${where}</p>
    <p class="hint">${te("settings.widgets.redraw")}</p>
    ${wv.macos && !wv.app_group ? `<p class="hint warn-txt">${icon("warn")} ${te("settings.widgets.macos")}</p>` : ""}
    <div class="dlg-actions left"><button class="btn danger-btn" data-widgets-delete>${icon("trash")} ${te("settings.widgets.delete")}</button></div>
    <p class="err" id="widgets-msg" role="alert">${esc(message)}</p>`;
}

/** Paints the section and binds it. Each change is saved at once and answers the new state; a refused change keeps
 *  the previous values and says why. */
async function bindWidgets(sec, extra) {
  let wv;
  try { wv = await widgetsStatus(); } catch (e) { sec.querySelector(".hint").textContent = l10nError(e); return; }
  const paint = (next, message = "") => {
    wv = next;
    sec.innerHTML = widgetsHtml(wv, message);
    const msg = sec.querySelector("#widgets-msg");
    const save = () => setWidgets(
      sec.querySelector("#widgets-on").checked,
      sec.querySelector("#widgets-identity").checked,
      Number(sec.querySelector("#widgets-max").value));
    const run = (work) => work().then((answer) => { paint(answer); extra.onChange?.(); }, (e) => { msg.textContent = serveErrorText(e); });
    sec.querySelector("#widgets-on").onchange = () => run(save);
    sec.querySelector("#widgets-identity").onchange = () => run(save);
    sec.querySelector("#widgets-max").onchange = () => run(save);
    sec.querySelector("[data-widgets-delete]").onclick = () => run(async () => {
      await deleteWidgetData();
      return widgetsStatus();
    }).then(() => {
      sec.querySelector("#widgets-msg").textContent = t("settings.widgets.deleted");
    });
  };
  paint(wv);
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

/** Shell codes (`invalid_port`, `unwritable`, `hooks_off`, …) or the core's typed error. */
const serveErrorText = (e) => {
  const key = `settings.error.${e}`;
  const tr = typeof e === "string" ? t(key) : key;
  return tr === key ? l10nError(e) : tr;
};

/** PRD 0013 RF4: hooks, off by default. The switch, the state (running, waiting for the confirmation, or why not), an
 *  editor of `hooks.toml`'s rules (name, events and the command as one field per argument; filters and time limits
 *  written by hand are kept and shown), a test per running rule that fires its sample event, and the latest runs.
 *  Turning hooks on, or saving rules while they are on, first shows the exact commands to confirm (RF4.2). */
function hooksHtml(hv, drafts, dirty) {
  const review = `<button class="btn" data-hooks-review>${te("settings.hooks.review")}</button>`;
  const state = {
    on: `<p class="hint" data-hooks-state><strong>${te("settings.hooks.state.on", { count: hv.running.length })}</strong></p>`,
    off: `<p class="hint" data-hooks-state><strong>${te("settings.hooks.state.off")}</strong></p>`,
    unconfirmed: `<p class="hint warn-txt" data-hooks-state role="alert">${icon("warn")} <strong>${te("settings.hooks.state.unconfirmed")}</strong></p>
      <div class="dlg-actions left">${review}</div>`,
    invalid: `<p class="hint warn-txt" data-hooks-state role="alert">${icon("warn")} <strong>${te("settings.hooks.state.invalid")}</strong> ${tle(hv.error)}</p>`,
    missing: `<p class="hint" data-hooks-state>${te("settings.hooks.state.missing")}</p>`,
    no_config: `<p class="hint" data-hooks-state>${te("settings.hooks.state.noConfig")}</p>`,
  }[hv.state] ?? "";
  const editor = hv.state === "no_config" ? "" : hv.rules == null
    ? `<p class="hint warn-txt">${te("settings.hooks.unparsable")}</p>`
    : `${drafts.map((d, i) => ruleHtml(hv, d, i, dirty)).join("")}
      <div class="dlg-actions left"><button class="btn" data-hook-add>${icon("plus")} ${te("settings.hooks.add")}</button>
        <button class="btn primary" data-hooks-save ${dirty ? "" : "disabled"}>${te("settings.hooks.save")}</button>
        <button class="btn" data-hooks-discard ${dirty ? "" : "disabled"}>${te("settings.hooks.discard")}</button></div>`;
  const runs = hv.runs.length
    ? `<ul class="hook-runs" data-hook-runs>${hv.runs.map((r) => `<li class="${r.outcome === "exited" && r.exit_code === 0 ? "" : "warn-txt"}">
        <time datetime="${esc(r.at)}">${esc(new Date(r.at).toLocaleString(currentLocale()))}</time> ${tle(r.summary)}</li>`).join("")}</ul>`
    : `<p class="hint" data-hook-runs>${te("settings.hooks.runs.none")}</p>`;
  return `<h4>${te("settings.hooks.title")}</h4>
        <p class="hint">${te("settings.hooks.help")}</p>
        <label class="check"><input id="hooks-on" type="checkbox" ${hv.enabled ? "checked" : ""} ${hv.state === "no_config" ? "disabled" : ""} /> ${te("settings.hooks.on")}</label>
        ${state}
        ${hv.path ? `<div class="path-row"><code title="${esc(hv.path)}">${esc(hv.path)}</code></div>` : ""}
        ${hv.warnings.map((w) => `<p class="hint warn-txt">${tle(w)}</p>`).join("")}
        ${editor}
        <p class="err" id="hooks-msg" role="status"></p>
        <h5>${te("settings.hooks.runs.title")}</h5>
        ${runs}`;
}

/** One rule of the editor: name, events (payload names with a label) and the command, one field per argument. */
function ruleHtml(hv, d, i, dirty) {
  const kept = [d.provider != null && `provider = ${JSON.stringify(d.provider)}`, d.account != null && `account = ${JSON.stringify(d.account)}`,
    d.min_threshold != null && `min_threshold = ${d.min_threshold}`, d.timeout_seconds != null && `timeout_seconds = ${d.timeout_seconds}`].filter(Boolean);
  const testable = !dirty && hv.state === "on" && hv.running.includes(d.name);
  return `<fieldset class="hook-rule" data-rule="${i}">
    <legend>${te("settings.hooks.rule", { index: i + 1 })}</legend>
    <label class="f"><span>${te("settings.hooks.name")}</span><input data-name value="${esc(d.name)}" maxlength="64" autocomplete="off" spellcheck="false" /></label>
    <div class="f"><span>${te("settings.hooks.events")}</span>
      <div class="hook-events">${hv.events.map((name) => `<label class="check"><input type="checkbox" data-event="${name}" ${d.events.includes(name) ? "checked" : ""} />
        <span><code>${name}</code> <small>${te(`settings.hooks.event.${name}`)}</small></span></label>`).join("")}</div></div>
    <div class="f"><span>${te("settings.hooks.command")}</span>
      <ol class="hook-args">${d.command.map((arg, j) => `<li><input data-arg="${j}" value="${esc(arg)}" autocomplete="off" spellcheck="false"
          aria-label="${te(j === 0 ? "settings.hooks.program" : "settings.hooks.argument", { index: j })}" placeholder="${te(j === 0 ? "settings.hooks.programHint" : "settings.hooks.argumentHint")}" />
        <button class="icon" data-arg-remove="${j}" title="${te("settings.hooks.argumentRemove")}" aria-label="${te("settings.hooks.argumentRemove")}">${icon("trash")}</button></li>`).join("")}</ol>
      <small>${te("settings.hooks.commandHelp")}</small></div>
    <div class="dlg-actions left"><button class="btn" data-arg-add>${icon("plus")} ${te("settings.hooks.argumentAdd")}</button></div>
    ${kept.length ? `<p class="hint">${te("settings.hooks.kept", { filters: kept.join(", ") })}</p>` : ""}
    <div class="dlg-actions left"><button class="btn" data-hook-test ${testable ? "" : "disabled"} title="${te(testable ? "settings.hooks.testHelp" : "settings.hooks.testOff")}">${te("settings.hooks.test")}</button>
      <button class="btn danger-btn" data-hook-remove>${icon("trash")} ${te("settings.hooks.remove")}</button></div>
  </fieldset>`;
}

/** Paints the hooks and binds them. Edits stay in the page until saved (the core validates and writes the file over
 *  the revision it was read at); a failure keeps them and shows why. The log repaints after every run. */
async function bindHooks(box, extra) {
  let hv;
  try { hv = await hooksStatus(); } catch (e) { box.querySelector(".hint").textContent = l10nError(e); return; }
  let drafts = structuredClone(hv.rules ?? []);
  let dirty = false;
  const paint = (next, message = "") => {
    if (next) { hv = next; if (!dirty) drafts = structuredClone(hv.rules ?? []); }
    box.innerHTML = hooksHtml(hv, drafts, dirty);
    box.querySelector("#hooks-msg").textContent = message;
    bind();
  };
  // The state is read again (a refused confirmation or save may mean the file changed); unsaved edits stay.
  const fail = (e) => {
    const message = serveErrorText(e);
    hooksStatus().then((next) => paint(next, message), () => paint(null, message));
  };
  const edited = () => {
    if (dirty) return;
    dirty = true;
    box.querySelector("[data-hooks-save]").disabled = false;
    box.querySelector("[data-hooks-discard]").disabled = false;
    for (const b of box.querySelectorAll("[data-hook-test]")) { b.disabled = true; b.title = t("settings.hooks.testOff"); }
  };
  /** RF4.2: confirm the rules listed in `view`, then (when asked) turn hooks on. */
  const confirmThen = async (view, enable) => {
    if (view.digest && !(await confirmCommands(view.listing))) return paint(null);
    if (view.digest) view = await confirmHooks(view.digest);
    paint(enable ? await setHooksEnabled(true) : view);
    extra.onChange?.();
  };
  const bind = () => {
    box.querySelector("#hooks-on").onchange = (e) => {
      const on = e.target.checked;
      (on ? confirmThen(hv, true) : setHooksEnabled(false).then((next) => { paint(next); extra.onChange?.(); })).catch(fail);
    };
    const review = box.querySelector("[data-hooks-review]");
    if (review) review.onclick = () => confirmThen(hv, false).catch(fail);
    for (const fs of box.querySelectorAll("[data-rule]")) {
      const d = drafts[Number(fs.dataset.rule)];
      fs.querySelector("[data-name]").oninput = (e) => { d.name = e.target.value; edited(); };
      for (const c of fs.querySelectorAll("[data-event]")) c.onchange = () => {
        d.events = c.checked ? [...d.events.filter((x) => x !== c.dataset.event), c.dataset.event] : d.events.filter((x) => x !== c.dataset.event);
        edited();
      };
      for (const input of fs.querySelectorAll("[data-arg]")) input.oninput = () => { d.command[Number(input.dataset.arg)] = input.value; edited(); };
      const restructure = (change, focus) => () => {
        change();
        dirty = true;
        paint(null);
        box.querySelector(focus())?.focus();
      };
      for (const b of fs.querySelectorAll("[data-arg-remove]")) {
        b.onclick = restructure(() => d.command.splice(Number(b.dataset.argRemove), 1), () => `[data-rule="${fs.dataset.rule}"] [data-arg-add]`);
      }
      fs.querySelector("[data-arg-add]").onclick = restructure(() => d.command.push(""), () => `[data-rule="${fs.dataset.rule}"] [data-arg="${d.command.length - 1}"]`);
      fs.querySelector("[data-hook-remove]").onclick = restructure(() => drafts.splice(Number(fs.dataset.rule), 1), () => "[data-hook-add]");
      fs.querySelector("[data-hook-test]").onclick = () => testHook(d.name).then((next) => paint(next, t("settings.hooks.testSent", { name: d.name })), fail);
    }
    const add = box.querySelector("[data-hook-add]");
    if (!add) return;
    add.onclick = () => {
      drafts.push({ name: "", events: [], command: [""] });
      dirty = true;
      paint(null);
      box.querySelector(`[data-rule="${drafts.length - 1}"] [data-name]`)?.focus();
    };
    box.querySelector("[data-hooks-discard]").onclick = () => { dirty = false; hooksStatus().then((next) => paint(next), fail); };
    box.querySelector("[data-hooks-save]").onclick = async () => {
      try {
        const next = await saveHooks(hv.revision, drafts);
        dirty = false;
        paint(next, t("settings.hooks.saved"));
        extra.onChange?.();
        // Saved while on: the new commands run only once confirmed.
        if (next.enabled && next.digest) await confirmThen(next, false);
      } catch (e) { fail(e); }
    };
  };
  paint(hv);
  const unlisten = onHookRun(() => {
    if (!box.isConnected) { unlisten.then((stop) => stop()); return; }
    hooksStatus().then((next) => paint(next), () => {});
  });
  box.closest("dialog")?.addEventListener("close", () => unlisten.then((stop) => stop()), { once: true });
}

/** RF4.2: the confirmation as a dialog with the core's exact listing (the file, then each rule's name, events,
 *  filters, time limit and command as a JSON array, so quotes and separators show). Resolves to the user's answer;
 *  closing it any other way declines. */
function confirmCommands(listing) {
  return new Promise((resolve) => {
    const d = document.createElement("dialog");
    d.className = "dlg hooks-confirm";
    d.setAttribute("role", "alertdialog");
    d.setAttribute("aria-labelledby", "hooks-confirm-title");
    const line = (l) => (l.id === "hooks.confirm.header" ? `<p>${tle(l)}</p>`
      : l.id === "hooks.confirm.rule" ? `<h3>${tle(l)}</h3>`
        : l.id === "hooks.confirm.command" ? `<p><code class="hook-command">${tle(l)}</code></p>`
          : `<p class="hint">${tle(l)}</p>`);
    d.innerHTML = `<div class="dlg-body"><h2 id="hooks-confirm-title">${te("settings.hooks.confirm.title")}</h2>
      <p class="hint warn-txt">${te("settings.hooks.confirm.help")}</p>
      <div class="hook-listing" data-hooks-listing>${listing.map(line).join("")}</div>
      <div class="dlg-actions"><button class="btn" data-hooks-decline>${te("common.cancel")}</button>
        <button class="btn danger-btn" data-hooks-accept>${te("settings.hooks.confirm.yes")}</button></div></div>`;
    document.body.append(d);
    let answer = false;
    d.addEventListener("close", () => resolve(answer), { once: true });
    d.querySelector("[data-hooks-accept]").onclick = () => { answer = true; d.close(); };
    d.querySelector("[data-hooks-decline]").onclick = () => d.close();
    showDialog(d);
    d.querySelector("[data-hooks-decline]").focus();
  });
}

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
