// Settings › About (PRD 0020 RF2.5): version, license, open-source libraries and Updates (PRD 0018).
import { t, te } from "../i18n.js";
import { esc } from "../lib.js";
import { openLibraries } from "../about-dialog.js";
import { openWhatsNew } from "../whats-new-dialog.js";
import { bindUpdates, onUpdateState, updatesSectionHtml } from "../updater.js";
import { group } from "./common.js";

export function html({ settings: s, update }) {
  return group("about", te("settings.about"), `
      <p class="hint">Cuotai ${esc(s.version)} · ${te("about.license", { license: "MIT" })} · ${te("about.updateState", { state: t("update.state." + (update?.phase ?? "idle"), { version: update?.version ?? "" }) })}</p>
      <div class="dlg-actions left"><button class="btn" data-libraries>${te("about.libraries")}</button><button class="btn" data-whats-new>${te("about.whatsNew")}</button></div>`)
    // PRD 0018 RF3.1 / PRD 0022 RF17.4: the updater owns its whole group, heading included (anchor `[data-updates]`).
    + updatesSectionHtml(update);
}

/** What the page shows of the updater; another change (a download's progress) does not redraw it. */
const shownState = (s) => JSON.stringify([s?.phase, s?.version, s?.enabled, s?.automatic, s?.consent_pending, s?.disabled_reason, s?.last_check, s?.error]);

export function bind(root, ctx) {
  root.querySelector("[data-libraries]").onclick = () => openLibraries();
  root.querySelector("[data-whats-new]").onclick = () => openWhatsNew();
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
