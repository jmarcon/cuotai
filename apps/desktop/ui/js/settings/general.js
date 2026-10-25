// Settings › General (PRD 0020 RF2.5): appearance (theme, light/dark mode, animations) and language, as rows (RF3).
import { te } from "../i18n.js";
import { langMenuHtml, MOTIONS, setMotion, themeSwitchHtml } from "../lib.js";
import { errorText, group } from "./common.js";
import { row, rowError, seg, toast } from "./rows.js";

export function html({ settings: s }) {
  // The shared switch draws the mode choice and the theme list; each goes in its own row.
  const themeSelect = themeSwitchHtml({ themeSelect: true }).match(/<select[\s\S]*<\/select>/)?.[0] ?? "";
  return group("appearance", te("settings.appearance"),
    row({ key: "theme", label: te("theme.label"), control: () => themeSelect })
    + row({ key: "mode", label: te("theme.mode.label"), control: () => themeSwitchHtml() })
    + row({ key: "motion", label: te("settings.motion.label"), info: te("settings.motion.help"), msg: "motion-msg",
      control: (a) => seg("motion", MOTIONS.map((m) => [m, te(`settings.motion.${m}`)]), s.motion, a) }))
    + group("language", te("lang.label"), row({ key: "lang", label: "", body: langMenuHtml({ inline: true }) }));
}

/** Theme, mode and language go through the shell's shared switch (`bindThemeSwitch`, with cross-fade); they are
 *  mirrored in localStorage, so the other windows follow them through `storage` events. Animations save on change
 *  (RF4.1); a refused value goes back to the saved one with the reason in its row (RF4.3). */
export function bind(root, ctx) {
  const motion = root.querySelector("#motion");
  let saved = ctx.settings.motion;
  motion.addEventListener("change", () => setMotion(motion.value).then(
    () => { saved = motion.value; rowError(motion, ""); toast(); ctx.changed("motion"); },
    (x) => { motion.value = saved; rowError(motion, errorText(x)); motion.querySelector('[aria-checked="true"]')?.focus(); }));
}
