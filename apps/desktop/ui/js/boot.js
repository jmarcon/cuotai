// Shared initialization for both windows: user content → language → theme (and motion from desktop.json).
import { initI18n } from "./i18n.js";
import { loadUserContent } from "./usercontent.js";
import { applyTheme, syncSettings } from "./lib.js";

export async function boot() {
  syncSettings(); // non-blocking: <head> and localStorage already applied the mirrors
  await loadUserContent();
  await initI18n();
  applyTheme();
}
