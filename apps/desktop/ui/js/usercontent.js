// Temas e idiomas do usuário. O shell (Rust) lê <pasta de config>/cuotai/{themes,locales} e entrega os textos;
// aqui eles são VALIDADOS e registrados. Itens inválidos são ignorados e listados em `userContent.errors`.
//
// Tema:   themes/<id>.css  — cabeçalho opcional  /*! cuotai-theme {"label":"Nome","group":"custom","modes":["dark","light"]} */
// Idioma: locales/<código>.json — { "_meta": { "name": "Nome" }, "chave": "texto" }
import { LOCALES, registerLocale, t } from "./i18n.js";
import { isBuiltinTheme, registerTheme, THEMES } from "./themes.js";

const T = window.__TAURI__;
export const userContent = { dir: null, errors: [] };

const ID_RE = /^[a-z0-9][a-z0-9-]{0,31}$/;
const LOCALE_RE = /^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$/;
const MAX_BYTES = 256 * 1024;

class Skip extends Error {}

/** Regras: sem @import/@font-face/URLs externas, sem @-regras, e todo seletor dentro de :root[data-theme="<id>"]. */
export function validateThemeCss(id, css) {
  const body = css.replace(/\/\*[\s\S]*?\*\//g, "");
  if (/@import|@font-face|@charset|@namespace/i.test(body) ||
      /url\(\s*['"]?\s*(https?:|\/\/|file:|javascript:)/i.test(body) ||
      /expression\(|javascript:|behavior\s*:|-moz-binding/i.test(body)) throw new Skip("externalUrl");
  if (/@[a-z-]+/i.test(body.replace(/"[^"]*"|'[^']*'/g, ""))) throw new Skip("atRule");
  const prefix = `:root[data-theme="${id}"]`;
  for (const m of body.matchAll(/([^{}]+)\{[^{}]*\}/g)) {
    for (const sel of m[1].split(",")) if (!sel.trim().startsWith(prefix)) throw new Skip("scope");
  }
}

function themeMeta(id, css) {
  const m = css.match(/\/\*!\s*cuotai-theme\s*(\{[\s\S]*?\})\s*\*\//);
  let meta = {};
  if (m) { try { meta = JSON.parse(m[1]); } catch { throw new Skip("invalidMeta"); } }
  const inferred = ["dark", "light"].filter((mode) => css.includes(`[data-mode="${mode}"]`));
  const modes = Array.isArray(meta.modes) ? meta.modes.filter((x) => x === "dark" || x === "light") : inferred;
  if (!modes.length) throw new Skip("invalidMeta");
  return { id, label: String(meta.label || id).slice(0, 48), group: String(meta.group || "custom").slice(0, 32), modes };
}

function addTheme(file) {
  const id = file.name.replace(/\.css$/i, "");
  if (!ID_RE.test(id)) throw new Skip("invalidId");
  if (isBuiltinTheme(id) || THEMES.some((x) => x.id === id)) throw new Skip("duplicate");
  if (file.text.length > MAX_BYTES) throw new Skip("tooLarge");
  validateThemeCss(id, file.text);
  const meta = themeMeta(id, file.text);
  const style = document.createElement("style");
  style.dataset.userTheme = id;
  style.textContent = file.text;
  document.head.append(style);
  registerTheme(meta);
}

function addLocale(file) {
  const code = file.name.replace(/\.json$/i, "");
  if (!LOCALE_RE.test(code)) throw new Skip("invalidId");
  if (file.text.length > MAX_BYTES) throw new Skip("tooLarge");
  let data;
  try { data = JSON.parse(file.text); } catch { throw new Skip("invalidJson"); }
  if (!data || typeof data !== "object" || Array.isArray(data)) throw new Skip("invalidJson");
  const messages = {};
  for (const [k, v] of Object.entries(data)) {
    if (k === "_meta") continue;
    const plural = v && typeof v === "object" && !Array.isArray(v) && Object.values(v).every((x) => typeof x === "string");
    if (typeof v === "string" || plural) messages[k] = v;
    else throw new Skip("invalidMeta");
  }
  const name = String(data._meta?.name ?? "").slice(0, 48);
  if (!name) throw new Skip("invalidMeta");
  registerLocale(code, name, messages, data._meta?.flag);
  return code;
}

export async function loadUserContent() {
  if (!T) return userContent; // navegador (demo): sem pasta de usuário
  let raw;
  try { raw = await T.core.invoke("load_user_content"); } catch { return userContent; }
  userContent.dir = raw.dir;
  const errors = [...raw.errors];
  const userLocales = [];
  for (const [kind, files, add] of [["theme", raw.themes, addTheme], ["locale", raw.locales, addLocale]]) {
    for (const f of files) {
      try {
        const id = add(f);
        if (kind === "locale") userLocales.push(id);
      } catch (e) {
        errors.push({ kind, name: f.name, reason: e instanceof Skip ? e.message : "unreadable" });
      }
    }
  }
  try { localStorage.setItem("cuotai.userLocales", JSON.stringify(userLocales)); } catch {}
  userContent.errors = errors.map((e) => (typeof e === "string" ? { kind: "theme", name: e, reason: "unreadable" } : e));
  return userContent;
}

export const skippedMessage = (e) =>
  t("userContent.skipped", { kind: t(`userContent.kind.${e.kind}`), name: e.name, reason: t(`userContent.reason.${e.reason}`) });
export { LOCALES };
