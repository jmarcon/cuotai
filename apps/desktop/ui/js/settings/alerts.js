// Settings › Alerts (PRD 0020 RF2.5): quota-reset notices and usage alerts, with the test notice.
// PRD 0003 RF3.1: reset notices of this device (desktop.json), off by default; the backend schedules them.
// PRD 0008 RF3.1: usage alerts in the same group (off by default), sharing its permission guidance and test.
// PRD 0024 RF4.1: "Warn before running out", its own switch (off by default) with the lead as a dependent control.
import { currentLocale, t, te } from "../i18n.js";
import { esc, icon, sendTestNotification, setAlertOptions, setAlertsEnabled, setNotifyEnabled, setNotifyOptions, setNotifyPrivate, setProjectedAlert, setQuietHours } from "../lib.js";
import { errorText, group } from "./common.js";
import { buttons, disc, row, rowError, select, seg, subRow, subpage, sw, toast } from "./rows.js";

// PRD 0032 RF2: the weekdays of the quiet-hours schedule in the order and with the keys `desktop.json` uses.
const DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const DEFAULT_INTERVAL = { from: "22:00", to: "08:00" };
const MAX_INTERVALS = 3;
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;
/** The weekday's name in the interface language (2024-01-01 was a Monday). */
const dayName = (key) => new Intl.DateTimeFormat(currentLocale(), { weekday: "long", timeZone: "UTC" }).format(new Date(Date.UTC(2024, 0, 1 + DAYS.indexOf(key))));

/** PRD 0024 RF2.7: the leads the core accepts, in minutes; the switch turns on with 60 (RF4.1). */
const PROJECTED_LEADS = [15, 30, 60];

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
      + row({ key: "private", label: te("settings.notify.private"), info: te("settings.notify.privateHelp"),
        control: (x) => sw("notify-private", n.private, x) }))
    + row({ key: "projected", label: te("settings.alerts.projected"), info: te("settings.alerts.projectedHelp"),
      control: (x) => sw("projected-on", lead != null, x),
      body: `<div class="projected-lead"><span id="projected-lead-label">${te("settings.alerts.projectedLead")}</span>${seg("projected-lead", projectedLeads, lead ?? 60, 'aria-labelledby="projected-lead-label"')}</div>` })
    + quietHtml(s.quiet)
    + row({ key: "test", label: te("settings.notify.test"), info: te("settings.notify.accounts"), msg: "notify-msg", msgRole: "status",
      control: () => buttons(`<button class="btn" data-notify-test aria-labelledby="test-label" ${n.enabled || a.enabled || lead != null ? "" : "disabled"}>${icon("bell")}</button>`) }), "data-notify");
}

/** One line for the weekly schedule row: nothing set, the same intervals every day, or how many days have some. */
export function quietSummary(q) {
  if (!q.enabled) return te("settings.quiet.summary.off");
  const filled = DAYS.filter((d) => (q.days[d] ?? []).length);
  if (!filled.length) return te("settings.quiet.summary.none");
  const monday = JSON.stringify(q.days.mon ?? []);
  if (filled.length === DAYS.length && DAYS.every((d) => JSON.stringify(q.days[d] ?? []) === monday)) {
    const list = q.days.mon;
    return list.length === 1 ? te("settings.quiet.summary.same", { from: list[0].from, to: list[0].to }) : te("settings.quiet.summary.sameMany", { count: list.length });
  }
  return te("settings.quiet.summary.days", { count: filled.length });
}

/** PRD 0032 RF2.3: one row on the page (with the schedule in a line) opens the quiet-hours sub-page: the switch,
 *  the digest switch and one row per weekday (up to three intervals each, labelled time fields, "Same every day"). */
function quietHtml(q) {
  const keywords = te("settings.quiet.keywords");
  return subRow({ key: "quiet", label: te("settings.quiet.title"), desc: quietSummary(q), keywords })
    + subpage("quiet", te("settings.quiet.title"),
      row({ key: "quiet-on", label: te("settings.quiet.on"), info: te("settings.quiet.help"), keywords, control: (x) => sw("quiet-on", q.enabled, x) })
      + row({ key: "quiet-digest", label: te("settings.quiet.digest"), dep: true, info: te("settings.quiet.digestHelp"), keywords, control: (x) => sw("quiet-digest", q.digest, x) })
      + `<h5>${te("settings.quiet.days")}</h5><div data-quiet-days>${quietDaysHtml(q)}</div>`);
}

/** The weekday rows: the sub-page's body, drawn again after every change. */
export function quietDaysHtml(q) {
  const intervals = (day, list) => (list.length
    ? list.map((iv, i) => {
      const name = dayName(day);
      return `<div class="q-interval" data-q-day="${day}" data-q-index="${i}">
        <input type="time" data-q-from value="${esc(iv.from)}" aria-label="${te("settings.quiet.from", { day: name, index: i + 1 })}" />
        <span aria-hidden="true">–</span>
        <input type="time" data-q-to value="${esc(iv.to)}" aria-label="${te("settings.quiet.to", { day: name, index: i + 1 })}" />
        <button type="button" class="icon" data-q-remove title="${te("settings.quiet.remove", { day: name, index: i + 1 })}" aria-label="${te("settings.quiet.remove", { day: name, index: i + 1 })}">${icon("trash")}</button></div>`;
    }).join("")
    : `<span class="hint">${te("settings.quiet.none")}</span>`);
  return `<p class="hint">${te("settings.quiet.overnight")}</p>
    <div class="dlg-actions left"><button type="button" class="btn" data-quiet-same title="${te("settings.quiet.sameHelp")}">${te("settings.quiet.same")}</button></div>
    ${DAYS.map((day) => {
    const list = q.days[day] ?? [];
    return row({ key: `quiet-${day}`, label: dayName(day), kw: "quiet",
      control: (x) => `<button type="button" class="btn sm" data-quiet-add="${day}" ${x} ${list.length >= MAX_INTERVALS ? "disabled" : ""}>${icon("plus")} ${te("settings.quiet.add")}</button>`,
      body: intervals(day, list) });
  }).join("")}`;
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

/** PRD 0032 RF2.3: every change to the quiet hours is saved at once as the whole schedule; a refused one is put
 *  back and the reason shown in its row. Time fields commit when both are valid `HH:MM` values. */
function bindQuiet(root, ctx, initial) {
  let saved = structuredClone(initial);
  const on = root.querySelector("#quiet-on");
  const digest = root.querySelector("#quiet-digest");
  const days = root.querySelector("[data-quiet-days]");
  const summary = root.querySelector("#quiet-desc");
  const deps = () => {
    digest.disabled = !on.checked;
    for (const el of days.querySelectorAll("input, button")) el.disabled = !on.checked || (el.matches("[data-quiet-add]") && (saved.days[el.dataset.quietAdd] ?? []).length >= MAX_INTERVALS);
  };
  const paint = (focus) => {
    days.innerHTML = quietDaysHtml(saved);
    summary.innerHTML = quietSummary(saved);
    on.checked = saved.enabled;
    digest.checked = saved.digest;
    deps();
    if (focus) days.querySelector(focus)?.focus();
  };
  const save = async (next, el, focus) => {
    try {
      await setQuietHours(next);
      saved = next;
      rowError(el, "");
      toast();
      ctx.changed("quiet");
      paint(focus);
    } catch (x) {
      paint();
      rowError(el, errorText(x));
    }
  };
  const edit = (change) => {
    const next = structuredClone(saved);
    change(next);
    return next;
  };
  on.onchange = () => save(edit((q) => { q.enabled = on.checked; }), on);
  digest.onchange = () => save(edit((q) => { q.digest = digest.checked; }), digest);
  days.addEventListener("click", (e) => {
    const add = e.target.closest("[data-quiet-add]");
    if (add) {
      const day = add.dataset.quietAdd;
      const last = (saved.days[day] ?? []).length;
      save(edit((q) => { (q.days[day] ??= []).push({ ...DEFAULT_INTERVAL }); }), add, `[data-q-day="${day}"][data-q-index="${last}"] [data-q-from]`);
      return;
    }
    const remove = e.target.closest("[data-q-remove]");
    if (remove) {
      const { qDay: day, qIndex: index } = remove.closest("[data-q-day]").dataset;
      save(edit((q) => { q.days[day].splice(Number(index), 1); if (!q.days[day].length) delete q.days[day]; }), remove, `[data-quiet-add="${day}"]`);
      return;
    }
    const same = e.target.closest("[data-quiet-same]");
    if (same) {
      const source = saved.days.mon?.length ? saved.days.mon : [DEFAULT_INTERVAL];
      save(edit((q) => { q.days = Object.fromEntries(DAYS.map((d) => [d, structuredClone(source)])); }), same, "[data-quiet-same]");
    }
  });
  days.addEventListener("change", (e) => {
    const field = e.target.closest("[data-q-from], [data-q-to]");
    if (!field) return;
    const holder = field.closest("[data-q-day]");
    const { qDay: day, qIndex: index } = holder.dataset;
    const from = holder.querySelector("[data-q-from]").value;
    const to = holder.querySelector("[data-q-to]").value;
    if (!TIME.test(from) || !TIME.test(to)) { paint(); rowError(field, t("settings.error.invalid_quiet")); return; }
    save(edit((q) => { q.days[day][Number(index)] = { from, to }; }), field, `[data-q-day="${day}"][data-q-index="${index}"] ${field.matches("[data-q-from]") ? "[data-q-from]" : "[data-q-to]"}`);
  });
  deps();
}
