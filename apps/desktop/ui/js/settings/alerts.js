// Settings › Alerts (PRD 0020 RF2.5): quota-reset notices and usage alerts, with the test notice.
// PRD 0003 RF3.1: reset notices of this device (desktop.json), off by default; the backend schedules them.
// PRD 0008 RF3.1: usage alerts in the same group (off by default), sharing its permission guidance and test.
// PRD 0024 RF4.1: "Warn before running out", its own switch (off by default) with the lead as a dependent control.
import { t, te } from "../i18n.js";
import { esc, icon, sendTestNotification, setAlertOptions, setAlertsEnabled, setNotifyEnabled, setNotifyOptions, setNotifyPrivate, setProjectedAlert } from "../lib.js";
import { errorText, group } from "./common.js";
import { uiPath } from "../util.js";
import { buttons, disc, row, rowError, select, seg, sw, toast } from "./rows.js";
import { bindQuiet, quietHtml } from "./quiet.js";

export { quietDaysHtml, quietSummary } from "./quiet.js";

/** PRD 0024 RF2.7: the leads the core accepts, in minutes; the switch turns on with 60 (RF4.1). */
const PROJECTED_LEADS = [15, 30, 60];

/** PRD 0022 RF9.5 (DS-CMP-PREV-1, DS-CMP-NOTIF-1/-2): what the test notice shows, as the OS draws it: the
 *  app's mark, the title and one sentence. Static, not a live region; its text is its accessible name. */
const previewHtml = () => `<div class="notif-preview" role="img" aria-label="${te("settings.notify.preview")}: ${te("notify.test.title")}. ${te("notify.test.body")}">
  <span class="notif"><img src="${uiPath("img/logo.svg")}" alt="" width="20" height="20" /><span><b>${te("notify.test.title")}</b><span>${te("notify.test.body")}</span></span></span></div>`;

export function html({ settings: s }) {
  const n = s.notify;
  const a = s.alerts;
  const leads = s.notify_leads.map((m) => [m, m ? te("settings.notify.leadMin", { min: m }) : te("settings.notify.leadNow")]);
  const lead = a.projected_lead_minutes ?? null;
  const projectedLeads = PROJECTED_LEADS.map((m) => [m, te(`settings.alerts.projectedLead${m}`)]);
  const whens = s.notify_whens.map((w) => [w, te(`settings.notify.when.${w}`)]);
  return group("notify", te("settings.notify.title"),
    row({ key: "notify", label: te("settings.notify.on"), info: te("settings.notify.help"),
      control: (x) => sw("notify-on", n.enabled, x),
      body: `<div class="confirm" id="notify-denied" role="alert" hidden><p>${te("settings.notify.denied")}</p><p class="hint">${te("settings.notify.system")}</p></div>` })
    + row({ key: "lead", label: te("settings.notify.lead"), dep: true, control: (x) => select("notify-lead", leads, n.lead_minutes, x) })
    + row({ key: "when", label: te("settings.notify.when"), dep: true, info: te("settings.notify.whenHelp"), control: (x) => seg("notify-when", whens, n.when, x) })
    + row({ key: "other", label: te("settings.notify.other"), dep: true, control: (x) => sw("notify-other", n.include_other, x) })
    + row({ key: "alerts", label: te("settings.alerts.on"), info: te("settings.alerts.help"), attrs: "data-alerts", control: (x) => sw("alerts-on", a.enabled, x) })
    + row({ key: "thresholds", label: te("settings.alerts.thresholds"), dep: true, desc: te("settings.alerts.thresholdsHelp", { min: s.alert_min, max: s.alert_max }),
      body: `<div class="thresholds" role="group" aria-labelledby="thresholds-label"><span id="alert-chips"></span>
          <input id="alert-new" type="number" min="${s.alert_min}" max="${s.alert_max}" step="1" inputmode="numeric" aria-label="${te("settings.alerts.new")}" placeholder="%" />
          <button class="btn sm" type="button" data-alert-add>${te("settings.alerts.add")}</button></div>` })
    + disc("alerts-more", te("settings.alerts.more"),
      row({ key: "exhaustion", label: te("settings.alerts.exhaustion"), dep: true, control: (x) => sw("alerts-exhaustion", a.exhaustion, x) })
      + row({ key: "recovery", label: te("settings.alerts.recovery"), dep: true, control: (x) => sw("alerts-recovery", a.recovery, x) })
      // PRD 0023 RF7.4: paid extra usage starting (and, with exhaustion, its budget running out).
      + row({ key: "overage", label: te("settings.alerts.overage"), dep: true, info: te("settings.alerts.overageHelp"), control: (x) => sw("alerts-overage", a.overage !== false, x) })
      // PRD 0024 RF4.1: its own switch (not a dependent row); here to keep the page within 8 rows (PRD 0020 RF5.1).
      + row({ key: "projected", label: te("settings.alerts.projected"), info: te("settings.alerts.projectedHelp"),
        control: (x) => sw("projected-on", lead != null, x),
        body: `<div class="projected-lead"><span id="projected-lead-label">${te("settings.alerts.projectedLead")}</span>${seg("projected-lead", projectedLeads, lead ?? 60, 'aria-labelledby="projected-lead-label"')}</div>` })
      + row({ key: "private", label: te("settings.notify.private"), info: te("settings.notify.privateHelp"),
        control: (x) => sw("notify-private", n.private, x) }))
    + quietHtml(s.quiet)
    + row({ key: "test", label: te("settings.notify.test"), info: te("settings.notify.accounts"), msg: "notify-msg", msgRole: "status",
      control: () => buttons(`<button class="btn" data-notify-test aria-labelledby="test-label" ${n.enabled || a.enabled || lead != null ? "" : "disabled"}>${icon("bell")}</button>`),
      body: disc("notify-preview", te("settings.notify.preview"), previewHtml()) }), "data-notify");
}

/** RF3.3 (PRD 0003): turning either switch on asks the system permission first; anything but `granted` keeps that
 *  switch off and shows the guidance. The test waits for a switch: on desktop the first notice is what makes the
 *  system decide the permission. Failures put the controls back to the saved values, with the reason in the row
 *  (RF4.3). Dependent rows are disabled while their switch is off (PRD 0020 RF3.3). */
export function bind(root, ctx) {
  const s = ctx.settings;
  const on = root.querySelector("#notify-on");
  const alertsOn = root.querySelector("#alerts-on");
  const denied = root.querySelector("#notify-denied");
  const test = root.querySelector("[data-notify-test]");
  const lead = root.querySelector("#notify-lead");
  const when = root.querySelector("#notify-when");
  const other = root.querySelector("#notify-other");
  const projectedOn = root.querySelector("#projected-on");
  const projectedLead = root.querySelector("#projected-lead");
  const deps = () => {
    for (const el of projectedLead.querySelectorAll("button")) el.disabled = !projectedOn.checked;
    for (const el of [lead, other, ...when.querySelectorAll("button")]) el.disabled = !on.checked;
    for (const el of root.querySelectorAll("#alert-new, [data-alert-add], #alert-chips button, #alerts-exhaustion, #alerts-recovery, #alerts-overage")) el.disabled = !alertsOn.checked;
  };
  const toggle = (box, enable, key) => async () => {
    const want = box.checked;
    box.disabled = true;
    try {
      const permission = await enable(want);
      box.checked = permission === "granted";
      test.disabled = !(on.checked || alertsOn.checked || projectedOn.checked);
      denied.hidden = !want || permission === "granted";
      rowError(box, "");
      if (box.checked === want) toast();
      ctx.changed(key);
    } catch (x) { box.checked = !want; rowError(box, errorText(x)); }
    finally { box.disabled = false; deps(); }
  };
  on.onchange = toggle(on, setNotifyEnabled, "notify");
  alertsOn.onchange = toggle(alertsOn, setAlertsEnabled, "alerts");
  const saved = { lead: lead.value, when: s.notify.when, other: other.checked };
  const options = (el) => async () => {
    try {
      await setNotifyOptions(Number(lead.value), when.value, other.checked);
      Object.assign(saved, { lead: lead.value, when: when.value, other: other.checked });
      rowError(el, "");
      toast();
      ctx.changed("notify");
    } catch (x) {
      lead.value = saved.lead; when.value = saved.when; other.checked = saved.other;
      rowError(el, errorText(x));
    }
  };
  lead.onchange = options(lead);
  when.addEventListener("change", options(when));
  other.onchange = options(other);
  bindProjected(projectedOn, projectedLead, test, ctx, deps);
  bindAlertOptions(root, ctx, s, deps);
  bindPrivate(root, ctx);
  bindQuiet(root, ctx, s.quiet);
  test.onclick = () =>
    sendTestNotification(t("notify.test.title"), t("notify.test.body")).then(
      () => { rowError(test, ""); toast(t("settings.notify.testSent")); },
      () => rowError(test, t("settings.notify.testFailed")));
  deps();
}

/** PRD 0024 RF4.1: the switch asks the system permission like the other notices (anything but `granted` keeps it
 *  off and shows the guidance of the reset notices); the lead is saved at once while the switch is on, and a
 *  refused change is put back with the reason in its row. */
function bindProjected(box, leadControl, test, ctx, deps) {
  const denied = ctx.root?.querySelector?.("#notify-denied") ?? document.querySelector("#notify-denied");
  let saved = Number(leadControl.value) || 60;
  const save = async (want, minutes, el) => {
    el.disabled = true;
    try {
      const permission = await setProjectedAlert(want, minutes);
      box.checked = permission === "granted";
      if (box.checked) saved = minutes;
      leadControl.value = String(saved);
      test.disabled = !(box.checked || document.querySelector("#notify-on")?.checked || document.querySelector("#alerts-on")?.checked);
      if (denied) denied.hidden = !want || permission === "granted";
      rowError(el, "");
      if (box.checked === want) toast();
      ctx.changed("alerts");
    } catch (x) {
      box.checked = !want;
      leadControl.value = String(saved);
      rowError(el, errorText(x));
    } finally { el.disabled = false; deps(); }
  };
  box.onchange = () => save(box.checked, saved, box);
  leadControl.addEventListener("change", () => {
    if (box.checked) save(true, Number(leadControl.value), leadControl);
  });
}

/** PRD 0008 RF3.1: the thresholds as chips (removed by a click, added from the field, 10 to 99), plus the
 *  exhaustion and recovery notices; every change is saved at once and a refused one is put back. */
function bindAlertOptions(root, ctx, s, deps) {
  const chips = root.querySelector("#alert-chips");
  const input = root.querySelector("#alert-new");
  const exhaustion = root.querySelector("#alerts-exhaustion");
  const recovery = root.querySelector("#alerts-recovery");
  const overage = root.querySelector("#alerts-overage");
  const saved = { thresholds: [...s.alerts.thresholds], exhaustion: exhaustion.checked, recovery: recovery.checked, overage: overage.checked };
  const draw = () => {
    chips.innerHTML = saved.thresholds.map((v) => {
      const label = t("settings.alerts.remove", { pct: v });
      return `<button class="chip" type="button" data-alert-remove="${v}" title="${esc(label)}" aria-label="${esc(label)}">${v}% ×</button>`;
    }).join("");
    deps();
  };
  const save = async (thresholds, el) => {
    try {
      await setAlertOptions(thresholds, exhaustion.checked, recovery.checked, overage.checked);
      Object.assign(saved, { thresholds: [...new Set(thresholds)].sort((x, y) => x - y), exhaustion: exhaustion.checked, recovery: recovery.checked, overage: overage.checked });
      rowError(el, "");
      toast();
      ctx.changed("alerts");
      return true;
    } catch (x) {
      exhaustion.checked = saved.exhaustion; recovery.checked = saved.recovery; overage.checked = saved.overage;
      rowError(el, errorText(x));
      return false;
    } finally { draw(); }
  };
  chips.onclick = (e) => {
    const chip = e.target.closest("[data-alert-remove]");
    if (chip) save(saved.thresholds.filter((v) => v !== Number(chip.dataset.alertRemove)), chips);
  };
  const add = async () => {
    const value = Number(input.value);
    if (!Number.isInteger(value) || value < s.alert_min || value > s.alert_max) { rowError(input, t("settings.error.invalid_threshold")); input.focus(); return; }
    if (await save([...saved.thresholds, value], input)) input.value = "";
    else input.focus();
  };
  root.querySelector("[data-alert-add]").onclick = add;
  input.onkeydown = (e) => { if (e.key === "Enter") { e.preventDefault(); add(); } };
  exhaustion.onchange = () => save(saved.thresholds, exhaustion);
  recovery.onchange = () => save(saved.thresholds, recovery);
  overage.onchange = () => save(saved.thresholds, overage);
  draw();
}

/** PRD 0032 RF7.1: "Hide account names in notifications" (on by default); a refused save puts the switch back. */
function bindPrivate(root, ctx) {
  const box = root.querySelector("#notify-private");
  box.onchange = async () => {
    const want = box.checked;
    box.disabled = true;
    try {
      await setNotifyPrivate(want);
      rowError(box, "");
      toast();
      ctx.changed("notify");
    } catch (x) { box.checked = !want; rowError(box, errorText(x)); }
    finally { box.disabled = false; }
  };
}
