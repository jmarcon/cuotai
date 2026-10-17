// Internacionalização: um arquivo JSON por idioma em locales/<código>.json (ou na pasta do usuário).
// Formato: { "_meta": { "name": "Español" }, "chave.simples": "texto {var}", "chave.plural": { "one": "…", "other": "…" } }
// Chaves ausentes caem para o idioma de reserva (en-US) e, por fim, para a própria chave.
//
// RF8 — resolutor `tl()`: textos entregues pelo core (campos, avisos, erros
// de edição, notas de relatório, login modes/options/registration) viajam
// como `{ id, params, fallback }` onde `fallback` é o texto em português.
// O desktop resolve `id` no catálogo corrente e usa `fallback` quando a
// chave está ausente. Parâmetros podem ser strings ou um `Localized`
// aninhado, resolvido recursivamente para que rótulos de
// status/provedor embarcados nas notas também sejam traduzidos.
//
// O catálogo do core (crates/cuotai-core/locales/{en-US,es}.json) é carregado
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
];
export const FALLBACK = "en-US";
const KEY = "cuotai.locale";

const uiCatalogs = new Map(); // código -> objeto de mensagens da UI
const userCatalogs = new Map(); // idiomas do usuário já validados (têm prioridade sobre o builtin de mesmo código)
const coreCatalogs = new Map(); // código -> catálogo do core (mescla sob o de UI na resolução)
let current = FALLBACK;

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
    return data;
  } catch {
    uiCatalogs.set(code, {});
    return {};
  }
}

/** Carrega (e cacheia) o catálogo do core para `code` via Tauri. */
async function loadCore(code) {
  if (coreCatalogs.has(code)) return coreCatalogs.get(code);
  const T = window.__TAURI__;
  if (!T || !T.core?.invoke) {
    coreCatalogs.set(code, {});
    return {};
  }
  try {
    const data = await T.core.invoke("core_catalog", { locale: code });
    coreCatalogs.set(code, data ?? {});
    return coreCatalogs.get(code);
  } catch {
    coreCatalogs.set(code, {});
    return {};
  }
}

/** Catálogo mesclado: as do usuário (se houver) cobrem as da UI, e as do core são mescladas por baixo para os ids do core. */
function mergedCatalog(code) {
  const ui = uiCatalogs.get(code) ?? {};
  const fallback = uiCatalogs.get(FALLBACK) ?? {};
  const user = userCatalogs.get(code); // se ausente, sem pacote do usuário
  const core = coreCatalogs.get(code) ?? {};
  const out = {};
  // Ordem da mescla (sobreposição vence): core < en-US UI < atual UI < usuário.
  Object.assign(out, core);
  Object.assign(out, fallback);
  Object.assign(out, ui);
  if (user) Object.assign(out, user);
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

export async function initI18n() {
  await loadUi(FALLBACK);
  await loadCore(FALLBACK);
  current = pickLocale(store.get());
  await loadUi(current);
  await loadCore(current);
  document.documentElement.lang = current;
}

export async function setLocale(code) {
  if (!hasLocale(code)) return;
  current = code;
  store.set(code);
  await loadUi(code);
  await loadCore(code);
  document.documentElement.lang = code;
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
 * to the Portuguese `fallback` embedded in the value. Plain strings
 * (pre-RF8 callers) are returned unchanged. `vars` is merged with
 * `note.params` so callers can override individual placeholders.
 *
 * pt-* never falls back to the en-US catalog for a missing key: the
 * Portuguese `fallback` is authoritative.
 */
export function tl(value, vars = {}, escape = false) {
  if (value == null) return "";
  if (!isL10n(value)) {
    return escape ? esc(String(value)) : String(value);
  }
  const merged = { ...(value.params ?? {}), ...vars };
  const currentCatalog = mergedCatalog(current);
  const fallbackCatalog = mergedCatalog(FALLBACK);
  const own = currentCatalog[value.id];
  // pt-* não consulta fallback en-US: o fallback do próprio valor (português) é autoritativo.
  const template = own ?? (current.toLowerCase().startsWith("pt") ? undefined : fallbackCatalog[value.id]);
  const text = template ?? value.fallback;
  return substitute(typeof text === "string" ? text : value.fallback, merged, escape);
}

/** HTML-safe variant of `tl`. */
export function tle(value, vars = {}) {
  return tl(value, vars, true);
}
