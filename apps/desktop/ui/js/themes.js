// Theme registry. Built-ins live here; user themes enter at runtime (js/usercontent.js).
// To add a built-in: css/themes/<id>.css + @import in css/themes/index.css + one line below.
// `modes`: offered appearances (every theme must offer dark and light). `group`: key of `theme.group.*`
// (or free text, shown as is).
export const THEMES = [
  { id: "cuotai", label: "Cuotai", group: "cuotai", modes: ["dark", "light"] },
  { id: "dracula", label: "Dracula", group: "editors", modes: ["dark", "light"] },
  { id: "onedark", label: "One Dark", group: "editors", modes: ["dark", "light"] },
  { id: "github", label: "GitHub", group: "editors", modes: ["dark", "light"] },
  { id: "catppuccin", label: "Catppuccin", group: "editors", modes: ["dark", "light"] },
  { id: "monokai", label: "Monokai", group: "editors", modes: ["dark", "light"] },
  { id: "tokyo", label: "Tokyo Night", group: "editors", modes: ["dark", "light"] },
  { id: "macos", label: "macOS", group: "systems", modes: ["dark", "light"] },
  { id: "windows11", label: "Windows 11", group: "systems", modes: ["dark", "light"] },
  { id: "win98", label: "Windows 98", group: "systems", modes: ["dark", "light"] },
  { id: "winxp", label: "Windows XP", group: "systems", modes: ["dark", "light"] },
  { id: "ubuntu", label: "Ubuntu", group: "systems", modes: ["dark", "light"] },
  { id: "crt", label: "CRT (monitor de tubo)", group: "retro", modes: ["dark", "light"] },
  { id: "synthwave", label: "Old Terminal (synthwave)", group: "retro", modes: ["dark", "light"] },
  { id: "tron", label: "TRON", group: "retro", modes: ["dark", "light"] },
];

/** PRD 0022 RF15.1: the default for new installs; existing profiles keep Dracula (see the pre-paint scripts). */
export const DEFAULT_THEME = "cuotai";
/** Group order in the selector; unknown groups (the user's) come after, in alphabetical order. */
export const GROUP_ORDER = ["cuotai", "editors", "systems", "retro", "custom"];
export const themeById = (id) => THEMES.find((t) => t.id === id) ?? THEMES.find((t) => t.id === DEFAULT_THEME);
export const isBuiltinTheme = (id) => THEMES.some((t) => t.id === id && !t.user);

/** Adds a user theme (already validated). */
export function registerTheme(theme) {
  THEMES.push({ ...theme, user: true });
}

/** Effective appearance: user preference (system|light|dark) limited to what the theme offers. */
export function effectiveMode(theme, pref, systemDark) {
  const wanted = pref === "system" ? (systemDark ? "dark" : "light") : pref;
  return theme.modes.includes(wanted) ? wanted : theme.modes[0];
}
