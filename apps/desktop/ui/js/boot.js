// Inicialização comum às duas janelas: idioma → conteúdo do usuário → tema (e animações, a partir do desktop.json).
import { initI18n } from "./i18n.js";
import { loadUserContent } from "./usercontent.js";
import { applyTheme, syncSettings } from "./lib.js";

export async function boot() {
  syncSettings(); // non-blocking: <head> and localStorage already applied the mirrors
  await initI18n();
  await loadUserContent(); // registra temas/idiomas do usuário (pode trocar o idioma salvo se ele for do usuário)
  await initI18n();        // reavalia a preferência agora que idiomas do usuário existem
  applyTheme();
}
