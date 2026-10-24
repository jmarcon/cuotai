// Settings › General (PRD 0020 RF2.5): appearance (theme, light/dark mode, animations) and language.
import { te } from "../i18n.js";
import { langMenuHtml, MOTIONS, setMotion, themeSwitchHtml } from "../lib.js";
import { errorText, group } from "./common.js";

export function html({ settings: s }) {
  return group("appearance", te("settings.appearance"), `${themeSwitchHtml({ themeSelect: true })}
      <label class="f"><span>${te("settings.motion.label")}</span>
        <select id="motion">${MOTIONS.map((m) => `<option value="${m}" ${s.motion === m ? "selected" : ""}>${te(`settings.motion.${m}`)}</option>`).join("")}</select>
        <small>${te("settings.motion.help")}</small></label>
      <p class="err" id="motion-msg" role="alert"></p>`)
    + group("language", te("lang.label"), langMenuHtml({ inline: true }));
}

/** Theme, mode and language go through the shell's shared switch (`bindThemeSwitch`, with cross-fade); they are
 *  mirrored in localStorage, so the other windows follow them through `storage` events. */
export function bind(root, ctx) {
  root.querySelector("#motion").onchange = (e) => setMotion(e.target.value).then(
    () => { root.querySelector("#motion-msg").textContent = ""; ctx.changed("motion"); },
    (x) => { root.querySelector("#motion-msg").textContent = errorText(x); });
}
