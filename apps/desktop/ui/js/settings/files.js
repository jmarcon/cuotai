// Settings › Advanced › Files (PRD 0020 RF2.5): the accounts file and the themes-and-languages folder.
import { te } from "../i18n.js";
import { THEMES } from "../themes.js";
import { LOCALES, skippedMessage, userContent } from "../usercontent.js";
import { configLabel, esc, icon, openUserDir, otherConfigNotice, pickFolder, pickTomlFile, setConfigFile, setUserDir } from "../lib.js";
import { configErrorText, errorText, group } from "./common.js";

// The last failure of each group, shown once by the next paint.
const flash = { config: "", folder: "" };
const take = (key) => { const text = flash[key]; flash[key] = ""; return text; };

export function html({ settings: s, view }) {
  const v = view();
  const nThemes = THEMES.filter((x) => x.user).length;
  const nLocales = LOCALES.filter((x) => x.user).length;
  return group("config", te("settings.config.title"), `
      <p class="hint">${te("settings.config.help")}</p>
      <div class="path-row"><code title="${esc(configLabel(v))}">${esc(configLabel(v) || "—")}</code></div>
      ${otherConfigNotice(v) ? `<p class="hint warn-txt">${esc(otherConfigNotice(v))}</p>` : ""}
      ${s.config_env_locked ? `<p class="hint warn-txt">${te("settings.config.env")}</p>` : ""}
      <div class="dlg-actions left">
        <button class="btn" data-pick-config ${s.config_env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.config.choose")}</button>
        <button class="btn" data-reset-config ${s.config_env_locked || !s.config_file ? "disabled" : ""}>${te("settings.config.reset")}</button>
      </div>
      <p class="err" role="alert">${esc(take("config"))}</p>`)
    + group("folder", te("settings.folder.title"), `
      <p class="hint">${te("settings.folder.help")}</p>
      <div class="path-row"><code title="${esc(s.user_dir ?? "")}">${esc(s.user_dir ?? "—")}</code></div>
      ${s.env_locked ? `<p class="hint warn-txt">${te("settings.folder.env")}</p>` : ""}
      <p class="hint">${s.custom_dir ? te("settings.folder.custom") : te("settings.folder.default")} · ${te("settings.folder.loaded", { themes: nThemes, locales: nLocales })}</p>
      <div class="dlg-actions left">
        <button class="btn" data-pick ${s.env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.folder.choose")}</button>
        <button class="btn" data-reset ${s.env_locked || !s.custom_dir ? "disabled" : ""}>${te("settings.folder.reset")}</button>
        <button class="btn" data-open-dir>${te("settings.folder.open")}</button>
      </div>
      ${userContent.errors.map((e) => `<p class="hint warn-txt">${esc(skippedMessage(e))}</p>`).join("")}
      <p class="err" role="alert">${esc(take("folder"))}</p>`);
}

export function bind(root, ctx) {
  const s = ctx.settings;
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
}
