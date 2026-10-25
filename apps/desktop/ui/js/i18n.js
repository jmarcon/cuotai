// Internacionalização: um arquivo JSON por idioma em locales/<código>.json (ou na pasta do usuário).
// Formato: { "_meta": { "name": "Español" }, "chave.simples": "texto {var}", "chave.plural": { "one": "…", "other": "…" } }
// Chaves ausentes caem para o idioma de reserva (en-US) e, por fim, para a própria chave.
//
// RF8 — resolutor `tl()`: textos entregues pelo core (campos, avisos, erros
// de edição, notas de relatório, login modes/options/registration) viajam
// como `{ id, params, fallback }` onde `fallback` é o texto em inglês.
// O desktop resolve `id` no catálogo corrente e usa `fallback` quando a
// chave está ausente. Parâmetros podem ser strings ou um `Localized`
// aninhado, resolvido recursivamente para que rótulos de
// status/provedor embarcados nas notas também sejam traduzidos.
//
// O catálogo do core (crates/cuotai-core/locales/{pt-BR,es,it}.json) é carregado
// pelo comando Tauri `core_catalog(locale)` e mesclado sob o catálogo de UI:
// as chaves do core têm prioridade para os ids que o core emite; chaves de
// UI apenas permanecem no JSON local. Pacotes de idioma do usuário podem
// sobrescrever ambos (e acrescentar ids novos do core para um idioma novo).
import { esc } from "./util.js";

/** Idiomas que acompanham o app. Idiomas do usuário são acrescentados em tempo de execução. */
export const LOCALES = [
  { code: "pt-BR", name: "Português" },
  { code: "en-US", name: "English" },
  { code: "es", name: "Español" },
  { code: "it", name: "Italiano" },
];
export const FALLBACK = "en-US";
const KEY = "cuotai.locale";

const uiCatalogs = new Map(); // código -> objeto de mensagens da UI
const userCatalogs = new Map(); // idiomas do usuário já validados (têm prioridade sobre o builtin de mesmo código)
const coreCatalogs = new Map(); // código -> catálogo do core (mescla sob o de UI na resolução)
let current = FALLBACK;
const merged = new Map(); // código -> catálogo mesclado (invalidado a cada carga/registro)

const store = {
  get: () => { try { return localStorage.getItem(KEY); } catch { return null; } },
  set: (v) => { try { localStorage.setItem(KEY, v); } catch {} },
};

export const currentLocale = () => current;
export const hasLocale = (code) => LOCALES.some((l) => l.code === code);

/** Registra um idioma do usuário (já validado). */
export function registerLocale(code, name, messages, flag) {
  userCatalogs.set(code, messages);
  uiCatalogs.set(code, messages);
  merged.clear();
  const i = LOCALES.findIndex((l) => l.code === code);
  if (i >= 0) LOCALES[i] = { code, name, flag, user: true };
  else LOCALES.push({ code, name, flag, user: true });
}

async function loadUi(code) {
  if (uiCatalogs.has(code)) return uiCatalogs.get(code);
  try {
    const res = await fetch(`locales/${code}.json`);
    const data = res.ok ? await res.json() : {};
    uiCatalogs.set(code, data);
    merged.clear();
    return data;
  } catch {
    uiCatalogs.set(code, {});
    merged.clear();
    return {};
  }
}

/** Carrega (e cacheia) o catálogo do core para `code` via Tauri. */
async function loadCore(code) {
  if (coreCatalogs.has(code)) return coreCatalogs.get(code);
  const T = window.__TAURI__;
  if (!T || !T.core?.invoke) {
    // Browser demo served from the repository root (PRD 0022 RF18.2): read the core's catalog file directly.
    // English is the core's built-in fallback and has no file. Absent (any other host): no core texts.
    let data = {};
    try {
      const res = await fetch(`../../../crates/cuotai-core/locales/${code}.json`);
      data = res.ok ? await res.json() : {};
    } catch {}
    coreCatalogs.set(code, data);
    merged.clear();
    return data;
  }
  try {
    const data = await T.core.invoke("core_catalog", { locale: code });
    coreCatalogs.set(code, data ?? {});
    merged.clear();
    return coreCatalogs.get(code);
  } catch {
    coreCatalogs.set(code, {});
    return {};
  }
}

/** Catálogo mesclado: as do usuário (se houver) cobrem as da UI, e as do core são mescladas por baixo para os ids do core. */
function mergedCatalog(code) {
  const hit = merged.get(code);
  if (hit) return hit;
  const ui = uiCatalogs.get(code) ?? {};
  const fallback = uiCatalogs.get(FALLBACK) ?? {};
  const user = userCatalogs.get(code); // se ausente, sem pacote do usuário
  const core = coreCatalogs.get(code) ?? {};
  const out = {};
  // Ordem da mescla (sobreposição vence): en-US UI < core do idioma < UI do idioma < usuário. O inglês vem primeiro:
  // um id do core que só o en-US da UI conhece (o core não tem catálogo en-US) não pode cobrir a tradução do core.
  Object.assign(out, fallback);
  Object.assign(out, core);
  Object.assign(out, ui);
  if (user) Object.assign(out, user);
  merged.set(code, out);
  return out;
}

/** Escolhe o idioma: preferência salva → idioma do sistema (exato ou mesma língua) → en-US. */
export function pickLocale(preferred, systemLangs = navigator.languages ?? [navigator.language]) {
  if (preferred && hasLocale(preferred)) return preferred;
  for (const lang of systemLangs) {
    if (!lang) continue;
    const exact = LOCALES.find((l) => l.code.toLowerCase() === lang.toLowerCase());
    if (exact) return exact.code;
    const base = lang.split("-")[0].toLowerCase();
    const same = LOCALES.find((l) => l.code.split("-")[0].toLowerCase() === base);
    if (same) return same.code;
  }
  return FALLBACK;
}

// ---------- PRD 0022 RF14.3 (DS-MSG-16): the en-XA pseudo-locale, development only ----------
// Generated at load from en-US (the UI catalog; the core's texts come from its English fallbacks in `tl`) and registered only in the browser demo with `?pseudo` in the
// address: never in the desktop app, so release builds cannot offer it. Each string grows (+100 % up to 10
// characters, +50 % up to 20, +40 % beyond), its letters take accents, and it is wrapped in ⟦ ⟧, so hard-coded
// text, concatenation and clipping show. Placeholders such as {count} are kept.
export const PSEUDO = "en-XA";
const ACCENTED = { a: "á", e: "ë", i: "ï", o: "ô", u: "ü", c: "ç", n: "ñ", y: "ý", A: "Å", E: "É", I: "Î", O: "Ö", U: "Ü", C: "Ç", N: "Ñ" };
export function pseudoText(text) {
  if (typeof text !== "string") return text;
  const parts = text.split(/(\{\w+\})/);
  const letters = parts.map((p) => (/^\{\w+\}$/.test(p) ? p : [...p].map((ch) => ACCENTED[ch] ?? ch).join(""))).join("");
  const n = [...text.replace(/\{\w+\}/g, "")].length;
  const grow = Math.ceil(n * (n <= 10 ? 1 : n <= 20 ? 0.5 : 0.4));
  return `⟦${letters}${"·".repeat(grow)}⟧`;
}
const pseudoCatalog = (catalog) => Object.fromEntries(Object.entries(catalog).filter(([k]) => k !== "_meta").map(([k, v]) =>
  [k, v && typeof v === "object" ? Object.fromEntries(Object.entries(v).map(([r, x]) => [r, pseudoText(x)])) : pseudoText(v)]));
const pseudoWanted = () => !globalThis.window?.__TAURI__ && /[?&]pseudo(=|&|$)/.test(globalThis.location?.search ?? "");

export async function initI18n() {
  await loadUi(FALLBACK);
  await loadCore(FALLBACK);
  if (pseudoWanted() && !hasLocale(PSEUDO)) {
    registerLocale(PSEUDO, "Pseudo", { ...pseudoCatalog(coreCatalogs.get(FALLBACK) ?? {}), ...pseudoCatalog(uiCatalogs.get(FALLBACK) ?? {}) });
    store.set(PSEUDO);
  }
  current = pickLocale(store.get());
  await loadUi(current);
  await loadCore(current);
  document.documentElement.lang = current;
  reportLocale(current);
}

export async function setLocale(code) {
  if (!hasLocale(code)) return;
  current = code;
  store.set(code);
  await loadUi(code);
  await loadCore(code);
  document.documentElement.lang = code;
  reportLocale(code);
}

/** PRD 0003 RF2.2: the backend sends reset notices in the language chosen here (saved in desktop.json). */
function reportLocale(code) {
  window.__TAURI__?.core?.invoke("set_locale", { locale: code }).catch(() => {});
  document.dispatchEvent?.(new Event("cuotai:locale")); // the sign-in page follows the language too
}

function lookup(key) {
  // Mescla sob a UI para todas as resoluções de chave em ambas as
  // resoluções — sem isso, `tl()` e `t()` divergem quando o catálogo
  // do core tem uma chave ausente do catálogo de UI.
  const currentCatalog = mergedCatalog(current);
  const fallbackCatalog = mergedCatalog(FALLBACK);
  return currentCatalog[key] ?? fallbackCatalog[key];
}

function resolve(key, vars) {
  let v = lookup(key);
  if (v && typeof v === "object") {
    const rule = new Intl.PluralRules(current).select(Number(vars.count ?? 0));
    v = v[rule] ?? v.other ?? Object.values(v)[0];
  }
  return typeof v === "string" ? v : null;
}

/** Traduz `key`. Plural: passe `{ count }` e use `{ one, other, … }` no catálogo (Intl.PluralRules). */
export function t(key, vars = {}) {
  const v = resolve(key, vars);
  return v == null ? key : v.replace(/\{(\w+)\}/g, (m, n) => (n in vars ? String(vars[n]) : m));
}

/** Como `t`, mas seguro para HTML: escapa o texto traduzido e os valores injetados (sem escape duplo). */
export function te(key, vars = {}) {
  const v = resolve(key, vars);
  if (v == null) return esc(key);
  return esc(v).replace(/\{(\w+)\}/g, (m, n) => (n in vars ? esc(vars[n]) : m));
}

/** True when `value` is a core-delivered RF8 note (`{ id, params, fallback }`). */
function isL10n(value) {
  return value && typeof value === "object" && "id" in value && "fallback" in value;
}

/** Substitute `{name}` placeholders in a translated template. Values may be
 *  strings or nested RF8 notes, which are resolved recursively. */
function substitute(template, vars, escape) {
  return template.replace(/\{(\w+)\}/g, (m, n) => {
    if (!(n in vars)) return m;
    const v = vars[n];
    if (v == null) return "";
    if (isL10n(v)) {
      return escape ? esc(tl(v, vars, true)) : tl(v, vars, true);
    }
    return escape ? esc(String(v)) : String(v);
  });
}

/**
 * Resolve an RF8 core text (`{ id, params, fallback }`) against the merged
 * catalog (UI + core) for the current locale, falling back to `en-US` then
 * to the English `fallback` embedded in the value. Plain strings
 * (pre-RF8 callers) are returned unchanged. `vars` is merged with
 * `note.params` so callers can override individual placeholders.
 *
 * pt-* never falls back to the en-US catalog for a missing key: the
 * `fallback` of the value is used.
 */
export function tl(value, vars = {}, escape = false) {
  if (value == null) return "";
  if (!isL10n(value)) {
    return escape ? esc(String(value)) : String(value);
  }
  const allVars = { ...(value.params ?? {}), ...vars };
  const currentCatalog = mergedCatalog(current);
  const fallbackCatalog = mergedCatalog(FALLBACK);
  const own = currentCatalog[value.id];
  // pt-* não consulta o catálogo en-US: o fallback do próprio valor é usado.
  const template = own ?? (current.toLowerCase().startsWith("pt") ? undefined : fallbackCatalog[value.id]);
  // PRD 0034 RF7: the core's English fallbacks are the source of its en-US texts (there is no en-US core catalog), so the
  // pseudo-locale derives its core texts from them, as it does from the UI catalog.
  const text = template ?? (current === PSEUDO && typeof value.fallback === "string" ? pseudoText(value.fallback) : value.fallback);
  return substitute(typeof text === "string" ? text : value.fallback, allVars, escape);
}

/** HTML-safe variant of `tl`. */
export function tle(value, vars = {}) {
  return tl(value, vars, true);
}
