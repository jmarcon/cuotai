// Settings > Advanced > Extensions (PRD 0020 RF2.5; PRDs 0017 and 0028 RF7): the extension manager as a page. The list,
// the consent, the secrets and the per-account overrides are the main window dialog's own code (`mountExtensions`),
// so the two entry points cannot drift; the main window keeps its dialog (the puzzle button, the Add-account picker).
import { te } from "../i18n.js";
import { requestAccountAction } from "../lib.js";
import { mountExtensions } from "../extensions-dialog.js";
import { group } from "./common.js";
import { row } from "./rows.js";

export function html() {
  return group("extensions", "", row({ key: "extensions", label: te("settings.page.extensions"), desc: te("extensions.help"), msg: "extensions-msg",
    body: `<div class="extensions-dlg ext-page" data-ext-body><p class="hint">${te("common.loading")}</p></div>` }));
}

/** Resolves once the extensions are listed (the shell then lands on an anchor). */
export function bind(root, ctx) {
  const host = root.querySelector("[data-ext-body]");
  host.replaceChildren();
  // Removing the account of an extension is the main window's job (it owns the confirmation and the vault choice);
  // the extension id goes along so the confirmation still offers to delete its stored secrets (PRD 0028 RF7.4).
  return mountExtensions(host, { intro: false, onChanged: () => ctx.changed("extensions"), onRemove: (index, _name, secretsOf = null) => requestAccountAction("remove", index, secretsOf ? { secretsOf } : {}) });
}
