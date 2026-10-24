// Settings › About (PRD 0020 RF2.5): version, license, open-source libraries and Updates (PRD 0018).
import { t, te } from "../i18n.js";
import { esc } from "../lib.js";
import { openLibraries } from "../about-dialog.js";
import { bindUpdates, updatesSectionHtml } from "../updater.js";
import { group } from "./common.js";

export function html({ settings: s, update }) {
  return group("about", te("settings.about"), `
      <p class="hint">Cuotai ${esc(s.version)} · ${te("about.license", { license: "MIT" })} · ${te("about.updateState", { state: t("update.state." + (update?.phase ?? "idle"), { version: update?.version ?? "" }) })}</p>
      <div class="dlg-actions left"><button class="btn" data-libraries>${te("about.libraries")}</button></div>`)
    // PRD 0018 RF3.1 / PRD 0022 RF17.4: the updater owns its whole group, heading included (anchor `[data-updates]`).
    + updatesSectionHtml(update);
}

export function bind(root) {
  root.querySelector("[data-libraries]").onclick = () => openLibraries();
  // PRD 0018 RF3.1: updater controls use the state fetched for this page.
  bindUpdates(root.querySelector("[data-update-body]"));
}
