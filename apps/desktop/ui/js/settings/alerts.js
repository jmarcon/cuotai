// Settings › Alerts (PRD 0020 RF2.5): quota-reset notices and usage alerts, with the test notice.
// PRD 0003 RF3.1: reset notices of this device (desktop.json), off by default; the backend schedules them.
// PRD 0008 RF3.1: usage alerts in the same group (off by default), sharing its permission guidance and test.
import { t, te } from "../i18n.js";
import { esc, icon, sendTestNotification, setAlertOptions, setAlertsEnabled, setNotifyEnabled, setNotifyOptions } from "../lib.js";
import { errorText, group } from "./common.js";

export function html({ settings: s }) {
  const n = s.notify;
  const a = s.alerts;
  const leads = s.notify_leads.map((m) => `<option value="${m}" ${n.lead_minutes === m ? "selected" : ""}>${m ? te("settings.notify.leadMin", { min: m }) : te("settings.notify.leadNow")}</option>`).join("");
  const whens = s.notify_whens.map((w) => `<option value="${w}" ${n.when === w ? "selected" : ""}>${te(`settings.notify.when.${w}`)}</option>`).join("");
  return group("notify", te("settings.notify.title"), `
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
      <p class="err" id="notify-msg" role="status"></p>`, "data-notify");
}

/** RF3.3 (PRD 0003): turning either switch on asks the system permission first; anything but `granted` keeps that
 *  switch off and shows the guidance. The test waits for a switch: on desktop the first notice is what makes the
 *  system decide the permission. Failures put the controls back to the saved values. */
export function bind(root, ctx) {
  const s = ctx.settings;
  const on = root.querySelector("#notify-on");
  const alertsOn = root.querySelector("#alerts-on");
  const denied = root.querySelector("#notify-denied");
  const msg = root.querySelector("#notify-msg");
  const test = root.querySelector("[data-notify-test]");
  const report = (text, ok = false) => { msg.classList.toggle("ok", ok); msg.textContent = text; };
  const toggle = (box, enable, key) => async () => {
    const want = box.checked;
    box.disabled = true;
    try {
      const permission = await enable(want);
      box.checked = permission === "granted";
      test.disabled = !(on.checked || alertsOn.checked);
      denied.hidden = !want || permission === "granted";
      report("");
      ctx.changed(key);
    } catch (x) { box.checked = !want; report(errorText(x)); }
    finally { box.disabled = false; }
  };
  on.onchange = toggle(on, setNotifyEnabled, "notify");
  alertsOn.onchange = toggle(alertsOn, setAlertsEnabled, "alerts");
  const lead = root.querySelector("#notify-lead");
  const when = root.querySelector("#notify-when");
  const other = root.querySelector("#notify-other");
  const saved = { lead: lead.value, when: when.value, other: other.checked };
  const options = async () => {
    try {
      await setNotifyOptions(Number(lead.value), when.value, other.checked);
      Object.assign(saved, { lead: lead.value, when: when.value, other: other.checked });
      report("");
      ctx.changed("notify");
    } catch (x) {
      lead.value = saved.lead; when.value = saved.when; other.checked = saved.other;
      report(errorText(x));
    }
  };
  lead.onchange = when.onchange = other.onchange = options;
  bindAlertOptions(root, ctx, s, report);
  test.onclick = () =>
    sendTestNotification(t("notify.test.title"), t("notify.test.body")).then(
      () => report(t("settings.notify.testSent"), true),
      () => report(t("settings.notify.testFailed")));
}

/** PRD 0008 RF3.1: the thresholds as chips (removed by a click, added from the field, 10 to 99), plus the
 *  exhaustion and recovery notices; every change is saved at once and a refused one is put back. */
function bindAlertOptions(root, ctx, s, report) {
  const chips = root.querySelector("#alert-chips");
  const input = root.querySelector("#alert-new");
  const exhaustion = root.querySelector("#alerts-exhaustion");
  const recovery = root.querySelector("#alerts-recovery");
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
      ctx.changed("alerts");
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
  root.querySelector("[data-alert-add]").onclick = add;
  input.onkeydown = (e) => { if (e.key === "Enter") { e.preventDefault(); add(); } };
  exhaustion.onchange = recovery.onchange = () => save(saved.thresholds);
  draw();
}
