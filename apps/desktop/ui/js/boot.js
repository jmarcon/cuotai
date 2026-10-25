// Shared initialization for all windows: user content → preferences → language → theme.
import { initI18n } from "./i18n.js";
import { loadUserContent } from "./usercontent.js";
import { applyTheme, syncPreferences } from "./lib.js";

export async function boot() {
  await loadUserContent();
  // PRD 0020 RF7.2: desktop.json is the single source of the theme, mode, language, animations, account order and
  // grouping; one read (after the one-time localStorage migration) before the language and theme apply. The
  // <head> script already painted from the mirror, so a failure here keeps the mirrored values.
  await syncPreferences({ boot: true });
  await initI18n();
  applyTheme();
}
