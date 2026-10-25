// Settings › Advanced › Files (PRD 0020 RF2.5): the accounts file and the themes-and-languages folder, as path rows
// (RF3.2) whose full path sits in a remembered disclosure (RF5.2).
import { te } from "../i18n.js";
import { THEMES } from "../themes.js";
import { LOCALES, skippedMessage, userContent } from "../usercontent.js";
import { brand, configLabel, esc, icon, openUserDir, otherConfigNotice, pickFolder, pickTomlFile, setConfigFile, setUserDir, vaultAudit, vaultPrune } from "../lib.js";
import { configErrorText, errorText, group } from "./common.js";
import { buttons, disc, pathHtml, row } from "./rows.js";

// The last failure of each group, shown once by the next paint.
const flash = { config: "", folder: "", credentials: "" };
const take = (key) => { const text = flash[key]; flash[key] = ""; return text; };

export function html({ settings: s, view }) {
  const v = view();
  const nThemes = THEMES.filter((x) => x.user).length;
  const nLocales = LOCALES.filter((x) => x.user).length;
  const warn = (text) => `<p class="hint warn-txt">${text}</p>`;
  const configRow = row({ key: "config-file", label: te("settings.config.title"), info: te("settings.config.help"), msg: "config-msg",
    control: () => buttons(`<button class="btn sm" data-pick-config ${s.config_env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.config.choose")}</button>`,
      `<button class="btn sm" data-reset-config ${s.config_env_locked || !s.config_file ? "disabled" : ""}>${te("settings.config.reset")}</button>`),
    body: `${otherConfigNotice(v) ? warn(esc(otherConfigNotice(v))) : ""}${s.config_env_locked ? warn(te("settings.config.env")) : ""}
      ${disc("config-path", te("settings.config.path"), pathHtml(configLabel(v)))}` });
  const folderRow = row({ key: "user-dir", attrs: 'id="set-folder" data-section="folder"', label: te("settings.folder.title"), info: te("settings.folder.help"), msg: "folder-msg",
    desc: `${s.custom_dir ? te("settings.folder.custom") : te("settings.folder.default")} · ${te("settings.folder.loaded", { themes: nThemes, locales: nLocales })}`,
    control: () => buttons(`<button class="btn sm" data-pick ${s.env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.folder.choose")}</button>`,
      `<button class="btn sm" data-reset ${s.env_locked || !s.custom_dir ? "disabled" : ""}>${te("settings.folder.reset")}</button>`,
      `<button class="btn sm" data-open-dir>${te("settings.folder.open")}</button>`),
    body: `${s.env_locked ? warn(te("settings.folder.env")) : ""}${userContent.errors.map((e) => warn(esc(skippedMessage(e)))).join("")}
      ${disc("folder-path", te("settings.config.path"), pathHtml(s.user_dir))}` });
  return group("config", "", configRow + folderRow)
    // PRD 0029 RF6.2: what Cuotai stored in the system vault, filled in by `bind` (never a value).
    + group("credentials", te("settings.vault.title"), `
      <p class="hint">${te("settings.vault.help")}</p>
      <div data-vault aria-live="polite"><p class="hint">${te("settings.vault.loading")}</p></div>
      <p class="err" role="alert">${esc(take("credentials"))}</p>`);
}


/** One indexed entry: what uses it (or that nothing does) and since when. */
function vaultRow(e) {
  const owner = e.orphan
    ? te("settings.vault.orphan")
    : `${esc(e.account ?? "")} · ${esc(brand(e.provider).label)}`;
  const kind = e.kind ? te(`settings.vault.kind.${e.kind}`) : te("settings.vault.kind.unused");
  const date = new Date(e.created_at).toLocaleDateString(document.documentElement.lang || undefined);
  return `<li><strong>${kind}</strong> — ${owner}${e.present ? "" : ` · ${te("settings.vault.gone")}`} <small>${esc(te("settings.vault.created", { date }))}</small></li>`;
}

export function vaultHtml(entries) {
  if (!entries.length) return `<p class="hint">${te("settings.vault.empty")}</p>`;
  const orphans = entries.filter((e) => e.orphan).length;
  return `<ul class="hook-runs">${entries.map(vaultRow).join("")}</ul>
    ${orphans ? `<div class="dlg-actions left"><button class="btn danger-btn" data-vault-prune>${icon("trash")} ${te("settings.vault.prune", { count: orphans })}</button></div>
    <div class="confirm" data-vault-confirm role="alertdialog" aria-labelledby="vault-confirm-text" hidden>
      <p id="vault-confirm-text">${te("settings.vault.confirm")}</p>
      <div class="dlg-actions left"><button class="btn danger-btn" data-vault-prune-confirm>${te("settings.vault.pruneConfirm")}</button>
        <button class="btn" data-vault-prune-cancel>${te("common.cancel")}</button></div>
    </div>` : ""}`;
}

export function bind(root, ctx) {
  const s = ctx.settings;
  // A failure is shown in its row after the repaint (RF4.3).
  for (const [key, id] of [["config", "#config-msg"], ["folder", "#folder-msg"]]) root.querySelector(id).textContent = take(key);
  const fail = (key, text) => { flash[key] = text; ctx.rerender(); };
  root.querySelector("[data-open-dir]").onclick = () => openUserDir().catch((e) => fail("folder", errorText(e)));
  const apply = async (path) => {
    // Every window reloads on `user-content-changed` to read the new folder, this one included.
    try { await setUserDir(path); location.reload(); }
    catch (e) { fail("folder", errorText(e)); }
  };
  root.querySelector("[data-pick]").onclick = async () => {
    const picked = await pickFolder(s.user_dir);
    if (typeof picked === "string" && picked) apply(picked);
  };
  root.querySelector("[data-reset]").onclick = () => apply(null);
  const applyConfig = async (path) => {
    try { ctx.setView(await setConfigFile(path)); ctx.changed("config_file"); ctx.rerender(); }
    catch (e) { fail("config", configErrorText(e)); }
  };
  root.querySelector("[data-pick-config]").onclick = async () => {
    const picked = await pickTomlFile(s.config_file ?? ctx.view()?.config_path);
    if (typeof picked === "string" && picked) applyConfig(picked);
  };
  root.querySelector("[data-reset-config]").onclick = () => applyConfig(null);
  const vault = root.querySelector("[data-vault]");
  const showVault = () => vaultAudit().then((entries) => {
    vault.innerHTML = vaultHtml(entries);
    const prune = vault.querySelector("[data-vault-prune]");
    if (!prune) return;
    const box = vault.querySelector("[data-vault-confirm]");
    prune.onclick = () => { box.hidden = false; box.querySelector("[data-vault-prune-cancel]").focus(); };
    box.querySelector("[data-vault-prune-cancel]").onclick = () => { box.hidden = true; prune.focus(); };
    box.querySelector("[data-vault-prune-confirm]").onclick = async () => {
      try { await vaultPrune(); await showVault(); }
      catch (e) { fail("credentials", errorText(e)); }
    };
  }).catch((e) => { vault.innerHTML = `<p class="hint warn-txt">${esc(errorText(e))}</p>`; });
  showVault();
}
