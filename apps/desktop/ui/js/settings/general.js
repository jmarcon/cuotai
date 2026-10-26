// Settings › General (PRD 0020 RF2.5): appearance (theme, light/dark mode, animations) and language, as rows (RF3).
import { machineTranslated, t, te } from "../i18n.js";
import { autostartSet, autostartStatus, langMenuHtml, MOTIONS, openExternal, setMotion, setPreference, themeSwitchHtml, TRANSLATION_GUIDE_URL } from "../lib.js";
import { errorText, group } from "./common.js";
import { row, rowError, seg, sw, toast } from "./rows.js";

/** The Dock exists only on macOS; the Settings window marks the platform on its root. */
const isMac = () => (globalThis.document?.documentElement?.dataset?.platform ?? "") === "mac";

export function html({ settings: s }) {
  // The shared switch draws the mode choice and the theme list; each goes in its own row (mockup 50-settings-general).
  const themeSelect = themeSwitchHtml({ themeSelect: true, selectOnly: true });
  // "Always" stays accepted for a profile that chose it, but is no longer offered (DS-MOT-8, owner: reduce motion).
  const motions = MOTIONS.filter((m) => m !== "always" || s.motion === "always");
  return group("appearance", te("settings.appearance"),
    row({ key: "mode", label: te("settings.mode.label"), control: () => themeSwitchHtml({ labels: true }) })
    + row({ key: "theme", label: te("theme.label"), desc: te("settings.theme.desc"), control: () => themeSelect })
    + row({ key: "motion", label: te("settings.motion.label"), desc: te("settings.motion.desc"), msg: "motion-msg",
      control: (a) => seg("motion", motions.map((m) => [m, te(`settings.motion.${m}`)]), s.motion, a) }))
    + group("language", te("lang.label"), row({ key: "lang", label: te("lang.label"), desc: te("settings.lang.desc"), control: () => langMenuHtml({ field: true }) })
      // PRD 0035 RF11.5: a machine-translated language says so and asks for review (not a row: no search keywords).
      + (machineTranslated() ? `<p class="hint lang-mt" data-lang-mt>${te("settings.lang.machine")} <a href="${TRANSLATION_GUIDE_URL}" data-lang-guide>${te("settings.lang.machine.link")}</a></p>` : ""))
    // Audit #37: the system owns this state; the switch is drawn off and disabled until the system has answered.
    + group("startup", te("settings.startup"), row({ key: "autostart", label: te("settings.autostart.label"), desc: te("settings.autostart.desc"),
      control: (a) => sw("autostart", false, a, "disabled") })
      // Mockup 50-settings-general: macOS only (other systems have no Dock); saved in desktop.json, applied by the shell.
      + (isMac() ? row({ key: "dock", label: te("settings.dock.label"), desc: te("settings.dock.desc"),
        control: (a) => sw("dock", s.show_in_dock !== false, a) }) : ""));
}

/** Theme, mode and language go through the shell's shared switch (`bindThemeSwitch`, with cross-fade); they are
 *  saved in desktop.json by the shell, which tells the other windows (RF7). Animations save on change
 *  (RF4.1); a refused value goes back to the saved one with the reason in its row (RF4.3). */
export function bind(root, ctx) {
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
  // "Show in Dock": the shell switches the activation policy as it saves (regular or menu-bar only).
  const dock = root.querySelector("#dock");
  dock?.addEventListener("change", () => setPreference("show_in_dock", dock.checked).then(
    () => { rowError(dock, ""); toast(); ctx.changed("show_in_dock"); },
    (x) => { dock.checked = !dock.checked; rowError(dock, errorText(x)); }));
  root.querySelector("[data-lang-guide]")?.addEventListener("click", (e) => {
    e.preventDefault();
    openExternal(TRANSLATION_GUIDE_URL).catch(() => {});
  });
  const motion = root.querySelector("#motion");
  let saved = ctx.settings.motion;
  motion.addEventListener("change", () => setMotion(motion.value).then(
    () => { saved = motion.value; rowError(motion, ""); toast(); ctx.changed("motion"); },
    (x) => { motion.value = saved; rowError(motion, errorText(x)); motion.querySelector('[aria-checked="true"]')?.focus(); }));
}
