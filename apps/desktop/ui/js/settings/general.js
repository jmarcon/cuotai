// Settings › General (PRD 0020 RF2.5): appearance (theme, light/dark mode, animations) and language, as rows (RF3).
import { t, te } from "../i18n.js";
import { autostartSet, autostartStatus, firstRunRecord, langMenuHtml, MOTIONS, setMotion, themeSwitchHtml } from "../lib.js";
import { errorText, group } from "./common.js";
import { buttons, row, rowError, seg, sw, toast } from "./rows.js";

export function html({ settings: s }) {
  // The shared switch draws the mode choice and the theme list; each goes in its own row (mockup 50-settings-general).
  const themeSelect = themeSwitchHtml({ themeSelect: true }).match(/<span class="theme-select">[\s\S]*<\/select><\/span>/)?.[0] ?? "";
  // "Always" stays accepted for a profile that chose it, but is no longer offered (DS-MOT-8, owner: reduce motion).
  const motions = MOTIONS.filter((m) => m !== "always" || s.motion === "always");
  return group("appearance", te("settings.appearance"),
    row({ key: "mode", label: te("settings.mode.label"), control: () => themeSwitchHtml({ labels: true }) })
    + row({ key: "theme", label: te("theme.label"), info: te("settings.theme.desc"), control: () => themeSelect })
    + row({ key: "motion", label: te("settings.motion.label"), info: te("settings.motion.desc"), msg: "motion-msg",
      control: (a) => seg("motion", motions.map((m) => [m, te(`settings.motion.${m}`)]), s.motion, a) }))
    + group("language", te("lang.label"), row({ key: "lang", label: te("lang.label"), info: te("settings.lang.desc"), control: () => langMenuHtml({ field: true }) }))
    // Audit #37: the system owns this state; the switch is drawn off and disabled until the system has answered.
    + group("startup", te("settings.startup"), row({ key: "autostart", label: te("settings.autostart.label"), desc: te("settings.autostart.desc"),
      control: (a) => sw("autostart", false, a, "disabled") }))
    // PRD 0025 RF6.4: the first-run step shows again on the next launch of the main window.
    + group("setup", "", row({ key: "setup-again", label: te("settings.setup.again"), info: te("settings.setup.again.help"),
      control: () => buttons(`<button type="button" class="btn sm" data-setup-again>${te("settings.setup.again.button")}</button>`) }));
}

/** Theme, mode and language go through the shell's shared switch (`bindThemeSwitch`, with cross-fade); they are
 *  saved in desktop.json by the shell, which tells the other windows (RF7). Animations save on change
 *  (RF4.1); a refused value goes back to the saved one with the reason in its row (RF4.3). */
export function bind(root, ctx) {
  const again = root.querySelector("[data-setup-again]");
  again.onclick = () => firstRunRecord(true).then(
    () => { rowError(again, ""); toast(t("settings.setup.again.done")); ctx.changed("onboarding"); },
    (x) => rowError(again, errorText(x)));
  // "Start at login": read from the system, written through the shell; what the system reports wins.
  const login = root.querySelector("#autostart");
  autostartStatus().then(
    (on) => { login.checked = on; login.disabled = false; },
    (x) => rowError(login, errorText(x)));
  login.addEventListener("change", () => {
    login.disabled = true;
    autostartSet(login.checked).then(
      (on) => { login.checked = on; rowError(login, ""); toast(); },
      (x) => { login.checked = !login.checked; rowError(login, errorText(x)); }
    ).finally(() => { login.disabled = false; });
  });
  const motion = root.querySelector("#motion");
  let saved = ctx.settings.motion;
  motion.addEventListener("change", () => setMotion(motion.value).then(
    () => { saved = motion.value; rowError(motion, ""); toast(); ctx.changed("motion"); },
    (x) => { motion.value = saved; rowError(motion, errorText(x)); motion.querySelector('[aria-checked="true"]')?.focus(); }));
}
