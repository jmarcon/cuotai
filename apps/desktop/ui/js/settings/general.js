// Settings › General (PRD 0020 RF2.5): appearance (theme, light/dark mode, animations) and language, as rows (RF3).
import { currentLocale, machineTranslated, t, te } from "../i18n.js";
import { userContent } from "../usercontent.js";
import { autostartRepair, autostartSet, autostartStale, autostartStatus, esc, langMenuHtml, MOTIONS, openExternal, setMotion, setPreference, themeSwitchHtml, TRANSLATION_GUIDE_URL } from "../lib.js";
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
      + (machineTranslated() ? `<p class="hint lang-mt" data-lang-mt>${te("settings.lang.machine")} <a href="${TRANSLATION_GUIDE_URL}" data-lang-guide>${te("settings.lang.machine.link")}</a></p>` : "")
      + packActiveHtml())
    // Audit #37: the system owns this state; the switch is drawn off and disabled until the system has answered.
    + group("startup", te("settings.startup"), row({ key: "autostart", label: te("settings.autostart.label"), desc: te("settings.autostart.desc"),
      control: (a) => sw("autostart", false, a, "disabled") })
      // Mockup 50-settings-general: macOS only (other systems have no Dock); saved in desktop.json, applied by the shell.
      + (isMac() ? row({ key: "dock", label: te("settings.dock.label"), desc: te("settings.dock.desc"),
        control: (a) => sw("dock", s.show_in_dock !== false, a) }) : ""));
}

/** PRD 0035 re-validation S-01: with a user language pack active, the file it comes from and what it cannot change
 *  (a protected text, so a pack cannot hide or reword it). */
function packActiveHtml() {
  // Only a pack the shell loaded from the user folder (never the development pseudo-locale).
  const file = userContent.locales.find((l) => l.code === currentLocale())?.name;
  if (!file) return "";
  // The file name goes in as a placeholder mark and comes back escaped inside <bdi>, so a right-to-left name cannot
  // reorder the sentence.
  const text = te("userContent.packActive", { file: "\u0000" }).replace("\u0000", `<bdi>${esc(file)}</bdi>`);
  return `<p class="hint lang-mt" data-protected-texts="pack-active" data-pack-active>${text}</p>`;
}

/** Theme, mode and language go through the shell's shared switch (`bindThemeSwitch`, with cross-fade); they are
 *  saved in desktop.json by the shell, which tells the other windows (RF7). Animations save on change
 *  (RF4.1); a refused value goes back to the saved one with the reason in its row (RF4.3). */
/** PRD 0054 RF4.3: the line under "Open at login" when its entry is stale, with its Repair button. */
export const staleHtml = () =>
  `<div class="set-row-body" data-autostart-stale><p class="hint">${te("settings.autostart.stale")} <button type="button" class="btn outline" data-autostart-repair>${te("settings.autostart.repair")}</button></p></div>`;

/** PRD 0054 RF4.1/RF4.2: run `action`; on a refusal, what the system now reports wins over what was asked. */
export async function settleAutostart(action, status) {
  try { return { on: await action(), error: null }; }
  catch (error) {
    let on = null;
    try { on = await status(); } catch { /* unknown: keep the switch as drawn */ }
    return { on, error };
  }
}

export function bind(root, ctx) {
  // "Start at login": read from the system, written through the shell; what the system reports wins.
  const login = root.querySelector("#autostart");
  // PRD 0054 RF4.3: while the entry starts a copy of Cuotai that moved or is gone, the row offers Repair.
  const showStale = (on) => {
    const row = login.closest(".set-row");
    row?.querySelector("[data-autostart-stale]")?.remove();
    if (on) row?.querySelector(".set-row-err")?.insertAdjacentHTML("beforebegin", staleHtml());
  };
  const checkStale = (on) => {
    if (!on) return showStale(false);
    autostartStale().then(showStale, () => showStale(false));
  };
  autostartStatus().then(
    (on) => { login.checked = on; login.disabled = false; checkStale(on); },
    (x) => rowError(login, errorText(x)));
  // PRD 0054 RF4.1/RF4.2: after a refusal the switch shows what the system then reports (the previous value when
  // the system does not answer), and the reason stays in the row.
  const settle = (action, previous) => settleAutostart(action, autostartStatus).then(({ on, error }) => {
    if (on !== null) { login.checked = on; checkStale(on); } else login.checked = previous;
    rowError(login, error ? errorText(error) : "");
    if (!error) toast();
  }).finally(() => { login.disabled = false; });
  login.addEventListener("change", () => {
    login.disabled = true;
    settle(() => autostartSet(login.checked), !login.checked);
  });
  login.closest(".set-row")?.addEventListener("click", (e) => {
    const repair = e.target.closest?.("[data-autostart-repair]");
    if (!repair) return;
    repair.disabled = true;
    login.disabled = true;
    settle(autostartRepair, login.checked).finally(() => { repair.disabled = false; });
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
