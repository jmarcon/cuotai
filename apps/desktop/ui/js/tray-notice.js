// PRD 0054 RF5.1: the notice the main window shows when the tray icon could not be created. Pure, so a script can
// check its markup in every catalog.
import { te } from "./i18n.js";
import { icon } from "./lib.js";

/** No tray icon, so the main window is the only way in: a status notice, not dismissable. The space after the
 *  icon matches the history and config notices. */
export const trayUnavailableHtml = (view, platform) => view?.tray_unavailable
  ? `<div class="banner bad" role="status" data-tray-unavailable>${icon("warn")} <span>${te(/Linux/.test(platform ?? "") ? "tray.unavailableLinux" : "tray.unavailable")}</span></div>` : "";
