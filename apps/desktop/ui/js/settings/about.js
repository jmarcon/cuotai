// Settings › About (PRD 0020 RF2.5): version, license, open-source libraries and Updates (PRD 0018).
import { t, te } from "../i18n.js";
import { esc, firstRunRecord, setWhatsNewAuto } from "../lib.js";
import { openLibraries } from "../about-dialog.js";
import { openWhatsNew } from "../whats-new-dialog.js";
import { openReportProblem } from "../report-dialog.js";
import { bindUpdates, onUpdateState, updatesSectionHtml } from "../updater.js";
import { errorText, group } from "./common.js";
import { buttons, row, rowError, sw, toast } from "./rows.js";

export function html({ settings: s, update }) {
  return group("about", te("settings.about"),
    row({ key: "version", label: `Cuotai ${esc(s.version)}${update?.build ? ` (${esc(update.build)})` : ""}`,
      desc: `${te("about.license", { license: "MIT" })} · ${te("about.updateState", { state: t("update.state." + (update?.phase ?? "idle"), { version: update?.version ?? "" }) })}`,
      control: () => buttons(`<button class="btn sm" data-libraries>${te("about.libraries")}</button><button class="btn sm" data-whats-new>${te("about.whatsNew")}</button><button class="btn sm" data-report-problem>${te("report.action")}</button>`) })
    // PRD 0019 RF7.7: the notes open once by themselves after an update; on by default.
    + row({ key: "whats-new-auto", label: te("settings.whatsnew.auto"), desc: te("settings.whatsnew.auto.desc"),
      control: (a) => sw("whats-new-auto", s.whats_new_auto !== false, a) })
    // PRD 0025 RF6.4 (audit #19): the first-run step shows again on the next launch of the main window; it lives
    // here, not on General, which the mockup keeps to appearance, language and startup.
    + row({ key: "setup-again", label: te("settings.setup.again"), info: te("settings.setup.again.help"),
      control: () => buttons(`<button type="button" class="btn sm" data-setup-again>${te("settings.setup.again.button")}</button>`) }))
    // PRD 0018 RF3.1 / PRD 0022 RF17.4: the updater owns its whole group, heading included (anchor `[data-updates]`).
    + updatesSectionHtml(update);
}

/** What the page shows of the updater; another change (a download's progress) does not redraw it. */
const shownState = (s) => JSON.stringify([s?.phase, s?.version, s?.enabled, s?.automatic, s?.consent_pending, s?.disabled_reason, s?.last_check, s?.error]);

export function bind(root, ctx) {
  root.querySelector("[data-libraries]").onclick = () => openLibraries();
  root.querySelector("[data-whats-new]").onclick = () => openWhatsNew();
  root.querySelector("[data-report-problem]").onclick = () => openReportProblem(null, 0);
  const auto = root.querySelector("#whats-new-auto");
  auto.onchange = () => setWhatsNewAuto(auto.checked).then(
    () => { rowError(auto, ""); toast(); ctx.changed("whats_new_auto"); },
    (x) => { auto.checked = !auto.checked; rowError(auto, errorText(x)); });
  const again = root.querySelector("[data-setup-again]");
  again.onclick = () => firstRunRecord(true).then(
    () => { rowError(again, ""); toast(t("settings.setup.again.done")); ctx.changed("onboarding"); },
    (x) => rowError(again, errorText(x)));
  // PRD 0018 RF3.1: updater controls use the state fetched for this page; a change made elsewhere (the main
  // window's banner, the consent question, the daily check) redraws the page while it is shown.
  bindUpdates(root.querySelector("[data-update-body]"));
  const drawn = shownState(ctx.update);
  let stop = null;
  stop = onUpdateState((next) => {
    if (shownState(next) === drawn) return;
    stop?.();
    ctx.rerender();
  });
  ctx.onLeave?.(() => stop?.());
}
