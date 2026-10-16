// Internacionalização: um arquivo JSON por idioma em locales/<código>.json (ou na pasta do usuário).
// Formato: { "_meta": { "name": "Español" }, "chave.simples": "texto {var}", "chave.plural": { "one": "…", "other": "…" } }
// Chaves ausentes caem para o idioma de reserva (en-US) e, por fim, para a própria chave.
//
// RF8 — resolutor `tl()`: textos entregues pelo core (campos, avisos, erros
// de edição, notas de relatório) viajam como `{ id, params, fallback }` onde
// `fallback` é o texto em português. O desktop resolve `id` no catálogo
// corrente e usa `fallback` quando a chave está ausente. Parâmetros podem ser
// strings ou um `Localized` aninhado, resolvido recursivamente para que
// rótulos de status/provedor embarcados nas notas também sejam traduzidos.
import { esc } from "./util.js";

/** Idiomas que acompanham o app. Idiomas do usuário são acrescentados em tempo de execução. */
export const LOCALES = [
  { code: "pt-BR", name: "Português" },
  { code: "en-US", name: "English" },
  { code: "es", name: "Español" },
];
export const FALLBACK = "en-US";
const KEY = "cuotai.locale";

const catalogs = new Map(); // código -> objeto de mensagens
const userCatalogs = new Map(); // idiomas do usuário já validados (têm prioridade sobre o builtin de mesmo código)
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
  catalogs.set(code, messages);
  const i = LOCALES.findIndex((l) => l.code === code);
  if (i >= 0) LOCALES[i] = { code, name, flag, user: true };
  else LOCALES.push({ code, name, flag, user: true });
}

async function load(code) {
  if (catalogs.has(code)) return catalogs.get(code);
  try {
    const res = await fetch(`locales/${code}.json`);
    const data = res.ok ? await res.json() : {};
    catalogs.set(code, data);
    return data;
  } catch {
    catalogs.set(code, {});
    return {};
  }
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
  await load(FALLBACK);
  current = pickLocale(store.get());
  await load(current);
  document.documentElement.lang = current;
}

export async function setLocale(code) {
  if (!hasLocale(code)) return;
  current = code;
  store.set(code);
  await load(code);
  document.documentElement.lang = code;
}

function lookup(key) {
  return catalogs.get(current)?.[key] ?? catalogs.get(FALLBACK)?.[key];
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
 * Resolve an RF8 core text (`{ id, params, fallback }`) against the active
 * catalog with `en-US` and `fallback` as the cascade. When the value is a
 * plain string (pre-RF8 callers) it is returned unchanged. `vars` is merged
 * with `note.params` so callers can override individual placeholders.
 */
export function tl(value, vars = {}, escape = false) {
  if (value == null) return "";
  if (!isL10n(value)) {
    return escape ? esc(String(value)) : String(value);
  }
  const merged = { ...(value.params ?? {}), ...vars };
  // Textos do core já chegam em português (`fallback`): no pt-BR uma chave ausente usa esse texto, nunca o inglês de reserva.
  const own = catalogs.get(current)?.[value.id];
  const template = own ?? (current.toLowerCase().startsWith("pt") ? undefined : catalogs.get(FALLBACK)?.[value.id]);
  const text = template ?? value.fallback;
  return substitute(typeof text === "string" ? text : value.fallback, merged, escape);
}

/** HTML-safe variant of `tl`. */
export function tle(value, vars = {}) {
  return tl(value, vars, true);
}
