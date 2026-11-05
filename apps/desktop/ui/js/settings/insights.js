// Settings › Alerts, "Insights" disclosure (PRD 0043): notify when a limit changes (RF2.3, off by default), the days
// off the rhythm and the digest mark (RF1.3), and the opt-in weekly digest with its day, hour and test (RF3.2, RF4.3).
// Everything lives in the `notify` section of desktop.json; the core decides when the digest is due.
import { t, te } from "../i18n.js";
import { icon, sendTestDigest, setInsightPrefs } from "../lib.js";
import { errorText } from "./common.js";
import { buttons, disc, row, rowError, select, sw, toast } from "./rows.js";

const DAYS = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

export function insightsHtml(n) {
  const daysOff = new Set(n.days_off ?? ["sat", "sun"]);
  const digest = n.digest ?? { enabled: false, weekday: 0, hour: 9 };
  const weekdays = DAYS.map((d, i) => [i, te(`settings.insights.day.${d}`)]);
  const hours = Array.from({ length: 24 }, (_, h) => [h, `${String(h).padStart(2, "0")}:00`]);
  return disc("insights", te("settings.insights.title"),
    row({ key: "limit-changes", label: te("settings.insights.limitChanges"), info: te("settings.insights.limitChangesHelp"),
      control: (x) => sw("limit-changes-on", !!n.limit_changes, x) })
    + row({ key: "days-off", label: te("settings.insights.daysOff"), info: te("settings.insights.daysOffHelp"),
      body: `<div class="days-off" role="group" aria-labelledby="days-off-label">${DAYS.map((d) =>
        `<label class="day-off"><input type="checkbox" class="chk" data-day-off="${d}" ${daysOff.has(d) ? "checked" : ""} /><span>${te(`settings.insights.day.${d}`)}</span></label>`).join("")}</div>` })
    + row({ key: "digest", label: te("settings.insights.digest"), info: te("settings.insights.digestHelp"),
      control: (x) => sw("digest-on", !!digest.enabled, x) })
    + row({ key: "digest-day", label: te("settings.insights.digestDay"), dep: true, control: (x) => select("digest-day", weekdays, digest.weekday, x) })
    + row({ key: "digest-hour", label: te("settings.insights.digestHour"), dep: true, control: (x) => select("digest-hour", hours, digest.hour, x) })
    + row({ key: "digest-test", label: te("settings.insights.digestTest"), msgRole: "status",
      control: () => buttons(`<button class="btn" type="button" data-digest-test aria-labelledby="digest-test-label">${icon("bell")}</button>`) }));
}

export function bindInsights(root, ctx) {
  const limit = root.querySelector("#limit-changes-on");
  const digestOn = root.querySelector("#digest-on");
  const day = root.querySelector("#digest-day");
  const hour = root.querySelector("#digest-hour");
  const test = root.querySelector("[data-digest-test]");
  const days = () => [...root.querySelectorAll("[data-day-off]")].filter((b) => b.checked).map((b) => b.dataset.dayOff);
  const values = () => ({ limitChanges: limit.checked, daysOff: days(), digestEnabled: digestOn.checked, digestWeekday: Number(day.value), digestHour: Number(hour.value) });
  let saved = values();
  const deps = () => { day.disabled = !digestOn.checked; hour.disabled = !digestOn.checked; };
  const restore = () => {
    limit.checked = saved.limitChanges;
    digestOn.checked = saved.digestEnabled;
    day.value = String(saved.digestWeekday);
    hour.value = String(saved.digestHour);
    for (const b of root.querySelectorAll("[data-day-off]")) b.checked = saved.daysOff.includes(b.dataset.dayOff);
  };
  const save = (el) => async () => {
    const want = values();
    try {
      await setInsightPrefs(want);
      saved = want;
      rowError(el, "");
      toast();
      ctx.changed("notify");
    } catch (x) { restore(); rowError(el, errorText(x)); }
    finally { deps(); }
  };
  limit.onchange = save(limit);
  digestOn.onchange = save(digestOn);
  day.onchange = save(day);
  hour.onchange = save(hour);
  for (const b of root.querySelectorAll("[data-day-off]")) b.onchange = save(b);
  test.onclick = () => sendTestDigest().then(
    () => { rowError(test, ""); toast(t("settings.notify.testSent")); },
    (x) => rowError(test, x === "notify_off" ? t("settings.insights.digestTestOff") : x === "no_history" ? t("settings.insights.digestTestEmpty") : t("settings.notify.testFailed")));
  deps();
}

