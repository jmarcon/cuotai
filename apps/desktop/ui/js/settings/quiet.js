// Settings › Alerts › Quiet hours (PRD 0032 RF2.3): the sub-page with the schedule, the digest switch and the
// weekday rows; split from alerts.js to keep each settings module short (PRD 0020 RF3).
import { currentLocale, t, te } from "../i18n.js";
import { esc, icon, setQuietHours } from "../lib.js";
import { errorText } from "./common.js";
import { row, rowError, subRow, subpage, sw, toast } from "./rows.js";

// PRD 0032 RF2: the weekdays of the quiet-hours schedule in the order and with the keys `desktop.json` uses.
const DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const DEFAULT_INTERVAL = { from: "22:00", to: "08:00" };
const MAX_INTERVALS = 3;
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;
/** The weekday's name in the interface language (2024-01-01 was a Monday). */
const dayName = (key) => new Intl.DateTimeFormat(currentLocale(), { weekday: "long", timeZone: "UTC" }).format(new Date(Date.UTC(2024, 0, 1 + DAYS.indexOf(key))));

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
export function quietHtml(q) {
  return subRow({ key: "quiet", label: te("settings.quiet.title"), desc: quietSummary(q) })
    + subpage("quiet", te("settings.quiet.title"),
      `<section class="set-sec">${row({ key: "quiet-on", label: te("settings.quiet.on"), info: te("settings.quiet.help"), kw: "quiet", control: (x) => sw("quiet-on", q.enabled, x) })}
      ${row({ key: "quiet-digest", label: te("settings.quiet.digest"), dep: true, info: te("settings.quiet.digestHelp"), kw: "quiet", control: (x) => sw("quiet-digest", q.digest, x) })}</section>
      <section class="set-sec"><div data-quiet-days>${quietDaysHtml(q)}</div></section>`);
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
  return `${row({ key: "quiet-same-row", label: te("settings.quiet.days"), info: te("settings.quiet.overnight"), kw: "quiet",
    control: () => `<button type="button" class="btn sm" data-quiet-same title="${te("settings.quiet.sameHelp")}">${te("settings.quiet.same")}</button>` })}
    ${DAYS.map((day) => {
    const list = q.days[day] ?? [];
    return row({ key: `quiet-${day}`, label: dayName(day), kw: "quiet",
      control: (x) => `<button type="button" class="btn sm" data-quiet-add="${day}" ${x} ${list.length >= MAX_INTERVALS ? "disabled" : ""}>${icon("plus")} ${te("settings.quiet.add")}</button>`,
      body: intervals(day, list) });
  }).join("")}`;
}

/** PRD 0032 RF2.3: every change to the quiet hours is saved at once as the whole schedule; a refused one is put
 *  back and the reason shown in its row. Time fields commit when both are valid `HH:MM` values; an incomplete
 *  one keeps its place while typed and gets the saved time back when the focus leaves it. */
export function bindQuiet(root, ctx, initial) {
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
  // `repaint: false` for a time field edit: the field keeps its place and its typing position (a time input fires
  // `change` after each completed segment, so drawing it again would cut a typed time short).
  const save = async (next, el, focus, repaint = true) => {
    try {
      await setQuietHours(next);
      saved = next;
      rowError(el, "");
      toast();
      ctx.changed("quiet");
      if (repaint) paint(focus);
      else summary.innerHTML = quietSummary(saved);
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
    // A segment cleared or still being typed leaves the value empty: the field stays (drawing the editor again
    // would drop the focus and the rest of the typed time) and shows the reason; leaving it puts the saved time back.
    if (!TIME.test(from) || !TIME.test(to)) { rowError(field, t("settings.error.invalid_quiet")); return; }
    save(edit((q) => { q.days[day][Number(index)] = { from, to }; }), field, null, false);
  });
  days.addEventListener("focusout", (e) => {
    const field = e.target.closest?.("[data-q-from], [data-q-to]");
    if (!field || TIME.test(field.value)) return;
    const { qDay: day, qIndex: index } = field.closest("[data-q-day]").dataset;
    const kept = saved.days[day]?.[Number(index)];
    if (kept) field.value = field.matches("[data-q-from]") ? kept.from : kept.to;
  });
  deps();
}
