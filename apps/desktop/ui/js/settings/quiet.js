// Settings › Alerts › Quiet hours (PRD 0032 RF2.3): the sub-page with the schedule, the digest switch and the
// weekday rows; split from alerts.js to keep each settings module short (PRD 0020 RF3).
import { currentLocale, currentRow, t, te, tl } from "../i18n.js";
import { esc, icon, setQuietHours } from "../lib.js";
import { errorText } from "./common.js";
import { fieldError, row, rowError, subRow, subpage, sw, toast } from "./rows.js";

// PRD 0032 RF2: the weekdays of the quiet-hours schedule with the keys `desktop.json` uses, Monday (ISO 1) first.
const DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const DEFAULT_INTERVAL = { from: "22:00", to: "08:00" };
const MAX_INTERVALS = 3;
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;
const DAY_MINUTES = 24 * 60;
/** The weekday's name in the interface language (2024-01-01 was a Monday). */
const dayName = (key) => new Intl.DateTimeFormat(currentLocale(), { weekday: "long", timeZone: "UTC" }).format(new Date(Date.UTC(2024, 0, 1 + DAYS.indexOf(key))));
/** Re-validation C-05: the weekdays in the order of the language's week, from the core registry's `first_weekday`
 *  (CLDR; ISO 1 = Monday, 7 = Sunday). A language outside the registry (the pseudo-locale, a user pack) takes the
 *  source language's, en-US: Sunday. */
export function weekOrder() {
  const first = Number(currentRow()?.first_weekday);
  const start = Number.isInteger(first) && first >= 1 && first <= 7 ? first - 1 : 6;
  return [...DAYS.slice(start), ...DAYS.slice(0, start)];
}
const minutes = (time) => Number(time.slice(0, 2)) * 60 + Number(time.slice(3));
const hhmm = (m) => `${String(Math.floor(m / 60) % 24).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
/** Minutes from the start day's midnight to the interval's start and end, as the core reads them (an end at or before
 *  the start is on the next day). */
const span = (iv) => {
  const [from, to] = [minutes(iv.from), minutes(iv.to)];
  return [from, to > from ? to : to + DAY_MINUTES];
};
const allDay = (iv) => iv.from === "00:00" && iv.to === "00:00";
/** Re-validation C-04: an `HH:MM` time in the interface's clock (`at.time` in its hour cycle: "10:00 PM", "22:00"). */
const clock = (time) => tl({ id: "at.time", params: { hour: String(Number(time.slice(0, 2))), minute: String(Number(time.slice(3))) }, fallback: time });
/** An interval in the interface's clock, or "All day" for `00:00`–`00:00`. Plain text: callers escape it. */
const rangeText = (iv) => (allDay(iv) ? t("settings.quiet.allDay") : t("settings.quiet.range", { from: clock(iv.from), to: clock(iv.to) }));

/** Re-validation C-06: what "Add interval" proposes: 22:00–08:00 on an empty day, else the first free hour from the
 *  end of the day's last interval that overlaps none of the day's intervals (the core refuses overlaps within one
 *  start day); null when no hour is free, and the button is then disabled. */
export function nextFree(list) {
  if (!list.length) return { ...DEFAULT_INTERVAL };
  const spans = list.filter((iv) => TIME.test(iv.from) && TIME.test(iv.to)).map(span);
  const start = Math.ceil((spans.at(-1)?.[1] ?? 0) / 60) % 24;
  for (let k = 0; k < 24; k++) {
    const from = ((start + k) % 24) * 60;
    const to = from + 60;
    if (spans.every(([a, b]) => !(from < b && a < to))) return { from: hhmm(from), to: hhmm(to) };
  }
  return null;
}

/** Re-validation C-12, the twin of the core's `QuietSchedule::always_on` (which `set_quiet_hours` answers after each
 *  save): every minute of the week inside an interval, an overnight one running into the next day (Sunday's into
 *  Monday's). Used only to draw the page before its first save. */
export function alwaysOn(q) {
  const week = 7 * DAY_MINUTES;
  const spans = [];
  DAYS.forEach((day, d) => {
    for (const iv of q.days?.[day] ?? []) {
      if (!TIME.test(iv.from) || !TIME.test(iv.to)) continue;
      const [a, b] = span(iv).map((m) => m + d * DAY_MINUTES);
      if (b > week) spans.push([a, week], [0, b - week]);
      else spans.push([a, b]);
    }
  });
  spans.sort((x, y) => x[0] - y[0] || x[1] - y[1]);
  let covered = 0;
  for (const [a, b] of spans) {
    if (a > covered) return false;
    covered = Math.max(covered, b);
  }
  return covered >= week;
}

/** One line for the weekly schedule row: off, nothing set, always on, the same intervals every day, or how many days
 *  have some. `always` is the core's answer when known. */
export function quietSummary(q, always = alwaysOn(q)) {
  if (!q.enabled) return te("settings.quiet.summary.off");
  const filled = DAYS.filter((d) => (q.days[d] ?? []).length);
  if (!filled.length) return te("settings.quiet.summary.none");
  if (always) return te("settings.quiet.summary.always");
  const first = JSON.stringify(q.days.mon ?? []);
  if (filled.length === DAYS.length && DAYS.every((d) => JSON.stringify(q.days[d] ?? []) === first)) {
    const list = q.days.mon;
    if (list.length === 1 && !allDay(list[0])) return te("settings.quiet.summary.same", { from: clock(list[0].from), to: clock(list[0].to) });
    return te("settings.quiet.summary.sameMany", { count: list.length });
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

const readoutId = (day, i) => `q-readout-${day}-${i}`;
/** Whether "Add interval" is unavailable on a day: three intervals already, or no free hour left. */
const addBlocked = (list) => list.length >= MAX_INTERVALS || !nextFree(list);

/** The weekday rows, in the language's week order: the sub-page's body, drawn again after every change. Each interval
 *  shows its times in the interface's clock under the native fields (which follow the system's region, C-04); that
 *  line describes both fields. An always-on schedule says what it means on the first row (C-12). */
export function quietDaysHtml(q, always = alwaysOn(q)) {
  const intervals = (day, list) => (list.length
    ? list.map((iv, i) => {
      const name = dayName(day);
      const described = `aria-describedby="${readoutId(day, i)}"`;
      return `<div><div class="q-interval" data-q-day="${day}" data-q-index="${i}">
        <input type="time" data-q-from value="${esc(iv.from)}" aria-label="${te("settings.quiet.from", { day: name, index: i + 1 })}" ${described} />
        <span aria-hidden="true">–</span>
        <input type="time" data-q-to value="${esc(iv.to)}" aria-label="${te("settings.quiet.to", { day: name, index: i + 1 })}" ${described} />
        <button type="button" class="icon" data-q-remove title="${te("settings.quiet.remove", { day: name, index: i + 1 })}" aria-label="${te("settings.quiet.remove", { day: name, index: i + 1 })}">${icon("trash")}</button></div>
        <span class="hint" id="${readoutId(day, i)}" data-q-readout>${esc(rangeText(iv))}</span></div>`;
    }).join("")
    : `<span class="hint">${te("settings.quiet.none")}</span>`);
  return `${row({ key: "quiet-same-row", label: te("settings.quiet.days"), info: te("settings.quiet.overnight"), kw: "quiet",
    desc: q.enabled && always ? te("settings.quiet.summary.always") : "",
    control: (x) => `<button type="button" class="btn sm" data-quiet-same ${x} title="${te("settings.quiet.sameHelp")}">${te("settings.quiet.same")}</button>` })}
    ${weekOrder().map((day) => {
    const list = q.days[day] ?? [];
    return row({ key: `quiet-${day}`, label: dayName(day), kw: "quiet",
      control: (x) => `<button type="button" class="btn sm" data-quiet-add="${day}" ${x} ${addBlocked(list) ? "disabled" : ""}>${icon("plus")} ${te("settings.quiet.add")}</button>`,
      body: intervals(day, list) });
  }).join("")}`;
}

/** The core's refusal from `set_quiet_hours`: a JSON object (`code`, and `day`, `index`, `field`, `with` when it is
 *  one interval's) or a plain code (`unwritable`). */
function refusal(x) {
  if (x && typeof x === "object" && typeof x.code === "string") return x;
  if (typeof x !== "string") return { code: "" };
  try {
    const parsed = JSON.parse(x);
    if (parsed && typeof parsed.code === "string") return parsed;
  } catch { /* a plain code */ }
  return { code: x };
}

/** The reason of a refusal in words: the colliding interval in the interface's clock, an example time in its clock. */
function refusalText(r, q) {
  if (r.code === "invalid_quiet_time") return t("settings.error.invalid_quiet_time", { example: clock(DEFAULT_INTERVAL.from) });
  if (r.code === "invalid_quiet_empty") return t("settings.error.invalid_quiet_empty", { allDay: t("settings.quiet.range", { from: clock("00:00"), to: clock("00:00") }) });
  const other = r.day != null && r.with != null ? q.days[r.day]?.[r.with] : null;
  if ((r.code === "invalid_quiet_duplicate" || r.code === "invalid_quiet_overlap") && other) {
    return t(`settings.error.${r.code}`, { range: rangeText(other), day: dayName(r.day) });
  }
  return errorText(r.code);
}

/** PRD 0032 RF2.3: every change to the quiet hours is saved at once as the whole schedule; a refused one is put
 *  back and the reason shown in its row, tied to the field it is about (C-06). Time fields commit when both are valid
 *  `HH:MM` values; an incomplete or refused one keeps its place while typed and gets the saved time back when the
 *  focus leaves the interval. */
export function bindQuiet(root, ctx, initial) {
  let saved = structuredClone(initial);
  let always = alwaysOn(saved);
  const on = root.querySelector("#quiet-on");
  const digest = root.querySelector("#quiet-digest");
  const days = root.querySelector("[data-quiet-days]");
  const summary = root.querySelector("#quiet-desc");
  const field = (day, index, name) => days.querySelector(`[data-q-day="${day}"][data-q-index="${index}"] [data-q-${name}]`);
  const deps = () => {
    digest.disabled = !on.checked;
    for (const el of days.querySelectorAll("input, button")) el.disabled = !on.checked || (el.matches("[data-quiet-add]") && addBlocked(saved.days[el.dataset.quietAdd] ?? []));
  };
  const paint = (focus) => {
    days.innerHTML = quietDaysHtml(saved, always);
    summary.innerHTML = quietSummary(saved, always);
    on.checked = saved.enabled;
    digest.checked = saved.digest;
    deps();
    if (focus) days.querySelector(focus)?.focus();
  };
  // `repaint: false` for a time field edit: the field keeps its place and its typing position (a time input fires
  // `change` after each completed segment, so drawing it again would cut a typed time short). A refused time edit
  // keeps the typed value too, marked invalid; leaving the interval puts the saved one back.
  const save = async (next, el, focus, repaint = true) => {
    try {
      const answer = await setQuietHours(next);
      saved = next;
      always = typeof answer?.always_on === "boolean" ? answer.always_on : alwaysOn(saved);
      fieldError(el, "");
      toast();
      ctx.changed("quiet");
      if (repaint) paint(focus);
      else {
        summary.innerHTML = quietSummary(saved, always);
        const holder = el.closest("[data-q-day]");
        const iv = holder && saved.days[holder.dataset.qDay]?.[Number(holder.dataset.qIndex)];
        const line = holder && days.querySelector(`#${readoutId(holder.dataset.qDay, holder.dataset.qIndex)}`);
        if (line && iv) line.textContent = rangeText(iv);
        // The always-on note comes or goes with this edit: draw the rows again (the focus stays on the field).
        if (Boolean(days.querySelector("#quiet-same-row-desc")) !== (saved.enabled && always)) {
          const at = holder && `[data-q-day="${holder.dataset.qDay}"][data-q-index="${holder.dataset.qIndex}"] [data-q-${el.matches("[data-q-from]") ? "from" : "to"}]`;
          paint(at);
        }
      }
    } catch (x) {
      const r = refusal(x);
      if (repaint) paint();
      const target = (r.day != null && r.index != null && r.field && field(r.day, r.index, r.field)) || (repaint ? null : el);
      if (target) fieldError(target, refusalText(r, next));
      else rowError(el, refusalText(r, next));
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
      const list = saved.days[day] ?? [];
      const proposal = nextFree(list);
      if (!proposal) return;
      save(edit((q) => { (q.days[day] ??= []).push(proposal); }), add, `[data-q-day="${day}"][data-q-index="${list.length}"] [data-q-from]`);
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
      // The first day shown (the language's first weekday, C-05) is copied, as `settings.quiet.sameHelp` says.
      const first = saved.days[weekOrder()[0]];
      const source = first?.length ? first : [DEFAULT_INTERVAL];
      save(edit((q) => { q.days = Object.fromEntries(DAYS.map((d) => [d, structuredClone(source)])); }), same, "[data-quiet-same]");
    }
  });
  days.addEventListener("change", (e) => {
    const input = e.target.closest("[data-q-from], [data-q-to]");
    if (!input) return;
    const holder = input.closest("[data-q-day]");
    const { qDay: day, qIndex: index } = holder.dataset;
    const from = holder.querySelector("[data-q-from]").value;
    const to = holder.querySelector("[data-q-to]").value;
    // A segment cleared or still being typed leaves the value empty: the field stays (drawing the editor again
    // would drop the focus and the rest of the typed time) and says what is expected; leaving the interval puts the
    // saved time back.
    if (!TIME.test(from) || !TIME.test(to)) {
      fieldError(holder.querySelector(TIME.test(from) ? "[data-q-to]" : "[data-q-from]"), refusalText({ code: "invalid_quiet_time" }, saved));
      return;
    }
    save(edit((q) => { q.days[day][Number(index)] = { from, to }; }), input, null, false);
  });
  days.addEventListener("focusout", (e) => {
    const input = e.target.closest?.("[data-q-from], [data-q-to]");
    if (!input) return;
    const holder = input.closest("[data-q-day]");
    if (!holder.querySelector('[aria-invalid="true"]') && TIME.test(input.value)) return;
    // Moving between the two fields of one interval keeps the typed values, so both can be corrected.
    if (e.relatedTarget?.matches?.("input") && holder.contains(e.relatedTarget)) return;
    const kept = saved.days[holder.dataset.qDay]?.[Number(holder.dataset.qIndex)];
    if (!kept) return;
    holder.querySelector("[data-q-from]").value = kept.from;
    holder.querySelector("[data-q-to]").value = kept.to;
    // The saved time is back, so the note shown while it was incomplete or refused no longer applies.
    fieldError(input, "");
  });
  deps();
}
