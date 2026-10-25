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
import { REGISTRY, normalizeLocale } from "./locale-registry.js";

// PRD 0035 RF1.5: the built-in languages come from the core registry (`locale_registry`, or the static copy in the
// browser demo), in its fixed order. A row whose catalogs are not written yet is "translation in progress": it is not
// in LOCALES (never selectable, never detected) but `pendingLocales()` lists it for the language selector; a user pack
// with its code makes it a regular language.
let registry = REGISTRY;
const fromRow = (row) => ({ code: row.code, name: row.endonym, flag: row.flag, script: row.script });
const translatedRows = (rows) => rows.filter((row) => row.status !== "in_progress").map(fromRow);
/** Idiomas selecionáveis: os do registro com catálogos, mais os do usuário (acrescentados em tempo de execução). */
export const LOCALES = translatedRows(registry);
/** Registry languages whose translation is in progress (and no user pack covers), in registry order. */
export const pendingLocales = () => registry.filter((row) => row.status === "in_progress" && !LOCALES.some((l) => l.code === row.code)).map(fromRow);
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
// One Intl.PluralRules per locale: building one on every plural lookup showed in the render profile.
let plural = null;
function pluralRules() {
  if (!plural || plural.locale !== current) plural = { locale: current, rules: new Intl.PluralRules(current) };
  return plural.rules;
}
/** Whether `code` can be chosen: a built-in language with catalogs, or a user pack. */
export const hasLocale = (code) => LOCALES.some((l) => l.code === code);
/** The registry row of the current language (script, plural rule), null for a user pack's own code. */
export const currentRow = () => registry.find((r) => r.code === current) ?? null;

/** Registra um idioma do usuário (já validado). */
export function registerLocale(code, name, messages, flag) {
  userCatalogs.set(code, messages);
  uiCatalogs.set(code, messages);
  merged.clear();
  const i = LOCALES.findIndex((l) => l.code === code);
  const row = registry.find((r) => r.code === code);
  const entry = { ...(row ? fromRow(row) : {}), code, name, flag: flag ?? row?.flag, user: true };
  if (i >= 0) LOCALES[i] = entry;
  else LOCALES.push(entry);
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

/**
 * Escolhe o idioma: preferência salva → cada idioma do sistema (código exato; a linha do registro pela língua, PRD 0035
 * RF1.3, sem nunca dar chinês simplificado a zh-TW; um pacote do usuário da mesma língua) → en-US. A language in
 * progress is never picked. The pre-paint copy in the HTML pages follows the same order.
 */
export function pickLocale(preferred, systemLangs = navigator.languages ?? [navigator.language]) {
  if (preferred && hasLocale(preferred)) return preferred;
  for (const lang of systemLangs) {
    if (!lang) continue;
    const exact = LOCALES.find((l) => l.code.toLowerCase() === lang.toLowerCase());
    if (exact) return exact.code;
    const row = normalizeLocale(lang, registry);
    if (row && hasLocale(row.code)) return row.code;
    const base = lang.split(/[-_]/)[0].toLowerCase();
    const same = LOCALES.find((l) => l.user && l.code.split("-")[0].toLowerCase() === base);
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

/** PRD 0035 RF1.5: take the registry from the backend; the static copy stays where the command is unavailable. */
async function loadRegistry() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return;
  try {
    const rows = await invoke("locale_registry");
    if (!Array.isArray(rows) || !rows.length) return;
    registry = rows;
    const users = LOCALES.filter((l) => l.user);
    LOCALES.splice(0, LOCALES.length, ...translatedRows(rows));
    for (const u of users) {
      const i = LOCALES.findIndex((l) => l.code === u.code);
      if (i >= 0) LOCALES[i] = u;
      else LOCALES.push(u);
    }
  } catch {}
}

export async function initI18n() {
  await loadRegistry();
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

/** A plural object's form for `count` (Intl.PluralRules of the current language), else `other`, else the first. */
function pluralForm(v, count) {
  if (!v || typeof v !== "object") return v;
  const rule = pluralRules().select(Number(count ?? 0));
  return v[rule] ?? v.other ?? Object.values(v)[0];
}

function resolve(key, vars) {
  const v = pluralForm(lookup(key), vars.count);
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
 * to the English `fallback` embedded in the value (PRD 0035 RF1.6: the
 * same chain for every language). Plain strings (pre-RF8 callers) are
 * returned unchanged. `vars` is merged with `note.params` so callers can
 * override individual placeholders. A core plural object (PRD 0035 RF4)
 * picks its form from the `count` parameter.
 */
export function tl(value, vars = {}, escape = false) {
  if (value == null) return "";
  if (!isL10n(value)) {
    return escape ? esc(String(value)) : String(value);
  }
  const allVars = { ...(value.params ?? {}), ...vars };
  const currentCatalog = mergedCatalog(current);
  const fallbackCatalog = mergedCatalog(FALLBACK);
  const template = pluralForm(currentCatalog[value.id] ?? fallbackCatalog[value.id], allVars.count);
  // PRD 0034 RF7: the core's English fallbacks are the source of its en-US texts (there is no en-US core catalog), so the
  // pseudo-locale derives its core texts from them, as it does from the UI catalog.
  const text = template ?? (current === PSEUDO && typeof value.fallback === "string" ? pseudoText(value.fallback) : value.fallback);
  return substitute(typeof text === "string" ? text : value.fallback, allVars, escape);
}

/** HTML-safe variant of `tl`. */
export function tle(value, vars = {}) {
  return tl(value, vars, true);
}
