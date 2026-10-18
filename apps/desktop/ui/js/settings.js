// Tela de configurações: aparência, idioma, arquivo de contas e pasta de temas/idiomas do usuário.
import { te, t, tl, LOCALES } from "./i18n.js";
import { THEMES } from "./themes.js";
import { userContent, skippedMessage } from "./usercontent.js";
import { bindThemeSwitch, configLabel, getSettings, icon, langMenuHtml, openUserDir, otherConfigNotice, pickFolder, pickTomlFile, setConfigFile, setRefreshSeconds, setUserDir, themeSwitchHtml } from "./lib.js";
import { esc } from "./util.js";
import { openLibraries } from "./about-dialog.js";

let dlg = null;

export async function openSettings(extra = {}) {
  if (dlg?.open) return;
  dlg = document.createElement("dialog");
  dlg.className = "dlg settings";
  document.body.append(dlg);
  dlg.addEventListener("close", () => { dlg.remove(); dlg = null; });
  let view = extra.view?.();
  const render = async (message = "", configMessage = "") => {
    const s = await getSettings();
    const nThemes = THEMES.filter((x) => x.user).length;
    const nLocales = LOCALES.filter((x) => x.user).length;
    dlg.innerHTML = `<div class="dlg-body">
      <h2>${te("settings.title")}</h2>
      <section class="set-sec"><h3>${te("settings.appearance")}</h3>${themeSwitchHtml({ themeSelect: true })}</section>
      <section class="set-sec"><h3>${te("lang.label")}</h3>${langMenuHtml({ inline: true })}</section>
      <section class="set-sec"><h3>${te("settings.refresh.title")}</h3>
        <p class="hint">${te("settings.refresh.help")}</p>
        <label class="f"><span>${te("settings.refresh.label")}</span>
          <input id="refresh-seconds" type="number" inputmode="numeric" min="30" step="1" value="${esc(extra.view?.()?.refresh_seconds || 300)}" /><small>${te("settings.refresh.min")}</small></label>
        <div class="dlg-actions left">
          ${[[60, "1 min"], [300, "5 min"], [900, "15 min"], [3600, "1 h"]].map(([sec, label]) => `<button class="btn" data-preset="${sec}">${label}</button>`).join("")}
          <button class="btn primary" data-apply-refresh>${te("settings.refresh.save")}</button>
        </div>
        <p class="err" id="refresh-msg" role="alert"></p>
      </section>
      <section class="set-sec"><h3>${te("settings.config.title")}</h3>
        <p class="hint">${te("settings.config.help")}</p>
        <div class="path-row"><code title="${esc(configLabel(view))}">${esc(configLabel(view) || "—")}</code></div>
        ${otherConfigNotice(view) ? `<p class="hint warn-txt">${esc(otherConfigNotice(view))}</p>` : ""}
        ${s.config_env_locked ? `<p class="hint warn-txt">${te("settings.config.env")}</p>` : ""}
        <div class="dlg-actions left">
          <button class="btn" data-pick-config ${s.config_env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.config.choose")}</button>
          <button class="btn" data-reset-config ${s.config_env_locked || !s.config_file ? "disabled" : ""}>${te("settings.config.reset")}</button>
        </div>
        <p class="err" role="alert">${esc(configMessage)}</p>
      </section>
      <section class="set-sec"><h3>${te("settings.folder.title")}</h3>
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
        <p class="err" role="alert">${esc(message)}</p>
      </section>
      <section class="set-sec"><h3>${te("settings.about")}</h3>
        <p class="hint">Cuotai ${esc(s.version)} · ${te("about.license", { license: "MIT" })}</p>
        <div class="dlg-actions left"><button class="btn" data-libraries>${te("about.libraries")}</button></div></section>
      <div class="dlg-actions"><button class="btn primary" data-close>${te("common.close")}</button></div>
    </div>`;
    dlg.querySelector("[data-close]").onclick = () => dlg.close();
    dlg.querySelector("[data-libraries]").onclick = () => openLibraries();
    const input = dlg.querySelector("#refresh-seconds");
    const msg = dlg.querySelector("#refresh-msg");
    for (const b of dlg.querySelectorAll("[data-preset]")) b.onclick = () => { input.value = b.dataset.preset; msg.textContent = ""; };
    dlg.querySelector("[data-apply-refresh]").onclick = async () => {
      msg.classList.remove("ok");
      try {
        await setRefreshSeconds(Number(input.value));
        msg.classList.add("ok");
        msg.textContent = t("settings.refresh.saved");
      } catch (e) { msg.textContent = l10nError(e); }
    };
    dlg.querySelector("[data-open-dir]").onclick = () => openUserDir().catch((e) => render(errorText(e)));
    const apply = async (path) => {
      try { await setUserDir(path); location.reload(); } // recarrega para ler a nova pasta
      catch (e) { render(errorText(e)); }
    };
    dlg.querySelector("[data-pick]").onclick = async () => {
      const picked = await pickFolder(s.user_dir);
      if (typeof picked === "string" && picked) apply(picked);
    };
    dlg.querySelector("[data-reset]").onclick = () => apply(null);
    const applyConfig = async (path) => {
      try { view = await setConfigFile(path); extra.onChange?.(); render(); }
      catch (e) { render("", configErrorText(e)); }
    };
    dlg.querySelector("[data-pick-config]").onclick = async () => {
      const picked = await pickTomlFile(s.config_file ?? view?.config_path);
      if (typeof picked === "string" && picked) applyConfig(picked);
    };
    dlg.querySelector("[data-reset-config]").onclick = () => applyConfig(null);
  };
  bindThemeSwitch(dlg, () => { extra.onChange?.(); render(); });
  dlg.addEventListener("click", async (e) => {
    const item = e.target.closest("[data-locale]");
    if (!item) return;
    const { setLocale } = await import("./i18n.js");
    await setLocale(item.dataset.locale);
    extra.onChange?.(); render();
  });
  dlg.showModal();
  await render();
}

const errorText = (e) => {
  const code = typeof e === "string" ? e : "";
  const key = `settings.error.${code}`;
  const tr = t(key);
  return tr === key ? t("common.unexpected") : tr;
};

/** Códigos do shell (`not_file`, `unwritable`, `env_locked`) ou erro tipado do core ao carregar o TOML. */
const configErrorText = (e) => {
  const key = `settings.config.error.${e}`;
  const tr = typeof e === "string" ? t(key) : key;
  return tr === key ? l10nError(e) : tr;
};

/** Erro do shell: JSON `{ id, params, fallback }` (RF8) ou texto simples. */
const l10nError = (e) => {
  if (typeof e === "string") {
    try { const p = JSON.parse(e); if (p && typeof p === "object" && "id" in p) return tl(p); } catch {}
    return e;
  }
  return e?.message ?? t("common.unexpected");
};
