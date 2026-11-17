// Internationalization: one JSON file per language in locales/<code>.json (or in the user folder).
// Format: { "_meta": { "name": "Español" }, "simple.key": "text {var}", "plural.key": { "one": "…", "other": "…" } }
// Missing keys fall back to the fallback language (en-US) and, finally, to the key itself.
//
// RF8 — `tl()` resolver: texts delivered by the core (fields, warnings, edit
// errors, report notes, login modes/options/registration) travel
// as `{ id, params, fallback }` where `fallback` is the English text.
// The desktop resolves `id` in the current catalog and uses `fallback` when the
// key is missing. Parameters can be strings or a nested
// `Localized`, resolved recursively so that status/provider
// labels embedded in the notes are translated too.
//
// The core catalog (crates/cuotai-core/locales/{pt-BR,es,it}.json) is loaded
// by the Tauri command `core_catalog(locale)` and merged under the UI catalog:
// the core keys take priority for the ids the core emits; UI
// keys only remain in the local JSON. User language packs can
// override both (and add new core ids for a new language).
import { esc, uiPath } from "./util.js";
import { REGISTRY, normalizeLocale } from "./locale-registry.js";
import { desktopWording } from "./desktop-wording.js";

// PRD 0035 RF1.5: the built-in languages come from the core registry (`locale_registry`, or the static copy in the
// browser demo), in its fixed order. A row whose catalogs are not written yet is "translation in progress": it is not
// in LOCALES (never selectable, never detected) but `pendingLocales()` lists it for the language selector; a user pack
// with its code makes it a regular language.
let registry = REGISTRY;
const fromRow = (row) => ({ code: row.code, name: row.endonym, flag: row.flag, script: row.script });
const translatedRows = (rows) => rows.filter((row) => row.status !== "in_progress").map(fromRow);
/** Selectable languages: those in the registry with catalogs, plus the user's (added at runtime). */
export const LOCALES = translatedRows(registry);
/** Registry languages whose translation is in progress (and no user pack covers), in registry order. */
export const pendingLocales = () => registry.filter((row) => row.status === "in_progress" && !LOCALES.some((l) => l.code === row.code)).map(fromRow);
export const FALLBACK = "en-US";
const KEY = "cuotai.locale";

const uiCatalogs = new Map(); // code -> UI message object
const userCatalogs = new Map(); // user languages already validated (they take priority over the built-in of the same code)
const coreCatalogs = new Map(); // code -> core catalog (merges under the UI one at resolution)
let current = FALLBACK;
const merged = new Map(); // code -> merged catalog (invalidated on each load/registration)

const store = {
  get: () => { try { return localStorage.getItem(KEY); } catch { return null; } },
  set: (v) => { try { localStorage.setItem(KEY, v); } catch {} },
};

export const currentLocale = () => current;
/** PRD 0035 re-validation R-01: the shipped language a code reads under: the code's own registry row, else the row it
 *  normalizes to (`fr-CA` → fr, `pt-PT` → pt-BR, never `zh-TW` → zh-CN), when that row has catalogs; else null. A
 *  user pack overrides that shipped catalog key by key and never replaces it. */
function shippedBase(code) {
  if (code === FALLBACK) return FALLBACK;
  const row = registry.find((r) => r.code === code) ?? normalizeLocale(code, registry);
  return row && row.status !== "in_progress" ? row.code : null;
}
/** PRD 0035 RF11.5: the active language's shipped UI catalog says it was machine-translated and not yet reviewed by a
 *  native speaker (`_meta.reviewed: false`); a user pack never carries `_meta`, so the shipped one is read. */
export const machineTranslated = () => uiCatalogs.get(shippedBase(current))?._meta?.reviewed === false;

// PRD 0035 re-validation S-01: test hook. While a trace runs, every id `t`/`te`/`tl` resolve is recorded, after
// `desktopWording`, so the id recorded is the one rendered. `scripts/test-pack-protection-ui.mjs` walks the protected
// dialogs with it.
let traced = null;
/** Runs `fn` (sync or async) and resolves to the ids it rendered. */
export async function traceTexts(fn) {
  const previous = traced;
  const ids = new Set();
  traced = ids;
  try { await fn(); } finally { traced = previous; }
  return ids;
}
// One Intl.PluralRules per locale: building one on every plural lookup showed in the render profile.
let plural = null;
function pluralRules() {
  if (!plural || plural.locale !== current) plural = { locale: current, rules: new Intl.PluralRules(intlTag(current)) };
  return plural.rules;
}
/** PRD 0034 re-validation S-05: whether `code` is a language tag `Intl` reads (`en-US-US` matches a loose shape but
 *  makes every `Intl` constructor throw). */
export function isIntlTag(code) {
  if (typeof code !== "string" || code === "") return false;
  try { return Intl.getCanonicalLocales(code).length === 1 && Boolean(new Intl.PluralRules(code)); } catch { return false; }
}
/** `code` when `Intl` reads it, else en-US: a bad tag formats in English instead of breaking every render. */
const intlTags = new Map();
export function intlTag(code) {
  if (code === undefined) return code;
  if (!intlTags.has(code)) intlTags.set(code, isIntlTag(code) ? code : FALLBACK);
  return intlTags.get(code);
}
/** Whether `code` can be chosen: a built-in language with catalogs, or a user pack. */
export const hasLocale = (code) => LOCALES.some((l) => l.code === code);
/** The registry row of the current language (script, plural rule), null for a user pack's own code. */
export const currentRow = () => registry.find((r) => r.code === current) ?? null;

/** Registers a user language (already parsed and filtered by the core). It overlays the shipped catalog of its
 *  language (R-01): the UI catalogs stay the shipped ones. */
export function registerLocale(code, name, messages, flag, file = null) {
  // S-03: en-US is the fallback every language reads; a pack never takes its place (the core refuses it first).
  if (code === FALLBACK) return;
  userCatalogs.set(code, messages);
  merged.clear();
  const i = LOCALES.findIndex((l) => l.code === code);
  const row = registry.find((r) => r.code === code);
  const entry = { ...(row ? fromRow(row) : {}), code, name, flag: flag ?? row?.flag, user: true, file };
  if (i >= 0) LOCALES[i] = entry;
  else LOCALES.push(entry);
}

/** Loads (and caches) the shipped UI catalog `code` reads under ({} for a language Cuotai does not ship). */
async function loadUi(language) {
  const code = shippedBase(language);
  if (!code) return {};
  if (uiCatalogs.has(code)) return uiCatalogs.get(code);
  try {
    const res = await fetch(uiPath(`locales/${code}.json`));
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

/** Loads (and caches) the core catalog of the shipped language `code` reads under, via Tauri. */
async function loadCore(language) {
  const code = shippedBase(language) ?? FALLBACK;
  if (coreCatalogs.has(code)) return coreCatalogs.get(code);
  const T = window.__TAURI__;
  if (!T || !T.core?.invoke) {
    // Browser demo served from the repository root (PRD 0022 RF18.2): read the core's catalog file directly.
    // English is the core's built-in fallback and has no file. Absent (any other host): no core texts.
    let data = {};
    try {
      const res = await fetch(uiPath(`../../../crates/cuotai-core/locales/${code}.json`));
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

/** Merged catalog: the user's (if any) cover the shipped UI's of its language, and the core's are merged underneath
 *  for the core ids. */
function mergedCatalog(code) {
  const hit = merged.get(code);
  if (hit) return hit;
  const base = shippedBase(code);
  const ui = (base && uiCatalogs.get(base)) ?? {};
  const fallback = uiCatalogs.get(FALLBACK) ?? {};
  // The fallback layer is always the shipped en-US: never a pack (S-03).
  const user = code === FALLBACK ? null : userCatalogs.get(code); // if absent, no user pack
  const core = (base && coreCatalogs.get(base)) ?? {};
  const out = {};
  // Merge order (overlay wins): shipped en-US UI < core(base) < shipped UI(base) < user pack. English comes first:
  // a core id that only the UI en-US knows (the core has no en-US catalog) must not cover the core translation. The
  // pack has no protected id left (the core filtered it), so a protected text reads the shipped translation of the
  // pack's language, else English (PRD 0035 re-validation S-01, R-01).
  Object.assign(out, fallback);
  Object.assign(out, core);
  Object.assign(out, ui);
  if (user) Object.assign(out, user);
  merged.set(code, out);
  return out;
}

/**
 * Chooses the language: saved preference → each system language (exact code; the registry row by language, PRD 0035
 * RF1.3, never giving Simplified Chinese to zh-TW; a user pack of the same language) → en-US. A language in
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
  // Merges under the UI for all key resolutions in both
  // resolutions — without this, `tl()` and `t()` diverge when the core catalog
  // has a key missing from the UI catalog.
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
  traced?.add(key);
  const v = pluralForm(lookup(key), vars.count);
  return typeof v === "string" ? v : null;
}

/** Translates `key`. Plural: pass `{ count }` and use `{ one, other, … }` in the catalog (Intl.PluralRules). */
export function t(key, vars = {}) {
  const v = resolve(key, vars);
  return v == null ? key : v.replace(/\{(\w+)\}/g, (m, n) => (n in vars ? String(vars[n]) : m));
}

/** Like `t`, but HTML-safe: escapes the translated text and the injected values (no double escaping). */
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
 *  strings or nested RF8 notes, which are resolved recursively with their own
 *  parameters only (an outer parameter never overrides a nested one), escaped
 *  once by that inner call when `escape` is set. */
function substitute(template, vars, escape) {
  return template.replace(/\{(\w+)\}/g, (m, n) => {
    if (!(n in vars)) return m;
    const v = vars[n];
    if (v == null) return "";
    if (isL10n(v)) {
      // PRD 0001 re-validation (L5): one escape, the nested note's own parameters.
      return tl(v, {}, escape);
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
  // PRD 0023 re-validation (V-02): a core amount (`amount.money`, `amount.number`, `amount.units`) carries machine
  // params and is formatted here in the UI locale, so it matches every other figure of the screen.
  const amount = amountText(value) ?? clockText(value);
  if (amount != null) return escape ? esc(amount) : amount;
  // PRD 0047 G1: a core text naming a terminal variable or `params.*` reads in the desktop's own words.
  value = desktopWording(value);
  traced?.add(value.id);
  const allVars = { ...(value.params ?? {}), ...vars };
  const currentCatalog = mergedCatalog(current);
  const fallbackCatalog = mergedCatalog(FALLBACK);
  const template = pluralForm(currentCatalog[value.id] ?? fallbackCatalog[value.id], allVars.count);
  // PRD 0034 RF7: the core's English fallbacks are the source of its en-US texts (there is no en-US core catalog), so the
  // pseudo-locale derives its core texts from them, as it does from the UI catalog.
  const text = template ?? (current === PSEUDO && typeof value.fallback === "string" ? pseudoText(value.fallback) : value.fallback);
  const resolved = typeof text === "string" ? text : String(value.fallback ?? "");
  // PRD 0022 re-validation (S-01): the HTML-safe path escapes the whole resolved text, not only the params. The
  // template comes from a catalog (a user pack included), and the core fallback is already interpolated, so its
  // params are inside it. Escaping first and substituting escaped params after keeps a single escape.
  const shown = escape ? esc(resolved) : resolved;
  // PRD 0034 re-validation (S-02): params are substituted in one pass. A fallback the core already interpolated is
  // shown as it is: substituting it again would expand a `{name}` that a param value carries.
  if (template == null && interpolated(value)) return shown;
  return substitute(shown, allVars, escape);
}

/** True when the note's fallback already holds a param value (a nested note: its English text) that carries a
 *  `{name}` placeholder, so the core interpolated it rather than a fixture writing it as a raw template. Only such a
 *  value can make a second pass differ from the first, so any other fallback is substituted once, as a template. */
function interpolated(note) {
  if (typeof note.fallback !== "string") return false;
  return Object.values(note.params ?? {}).some((v) => {
    const text = isL10n(v) ? englishText(v) : v;
    return typeof text === "string" && /\{\w+\}/.test(text) && note.fallback.includes(text);
  });
}

/** A note's English text as the core interpolates it: its fallback, substituted once when it is a raw template. */
function englishText(note) {
  if (typeof note.fallback !== "string") return "";
  if (interpolated(note)) return note.fallback;
  const params = note.params ?? {};
  return note.fallback.replace(/\{(\w+)\}/g, (m, n) => {
    if (!(n in params)) return m;
    const v = params[n];
    return v == null ? "" : isL10n(v) ? englishText(v) : String(v);
  });
}

/** Intl locale of the UI: the pseudo-locale formats like en-US. */
const intlLocale = () => (current === PSEUDO ? FALLBACK : intlTag(current));
/** The text of a core `amount.*` note in the UI locale; null for any other note or a malformed value (the caller
 *  then shows the core's en-US fallback). */
function amountText(value) {
  if (typeof value.id !== "string" || !value.id.startsWith("amount.")) return null;
  const p = value.params ?? {};
  const n = typeof p.value === "string" && p.value.trim() !== "" ? Number(p.value) : NaN;
  if (!Number.isFinite(n)) return null;
  try {
    if (value.id === "amount.money" && /^[A-Za-z]{3}$/.test(p.currency ?? "")) {
      return new Intl.NumberFormat(intlLocale(), { style: "currency", currency: p.currency.toUpperCase() }).format(n);
    }
    const number = new Intl.NumberFormat(intlLocale(), { maximumFractionDigits: 2 }).format(n);
    if (value.id === "amount.number") return number;
    // PRD 0034 re-validation V-10: a no-break space, so the number never wraps away from its unit.
    // PRD 0034 re-validation V-05: a unit the core sends as a text of its own (`metric.unit.*`) reads in the UI
    // language; a plain string is already a display text (a currency code, a provider's own unit).
    if (value.id === "amount.units" && isL10n(p.unit)) return `${number}\u00a0${tl(p.unit)}`;
    if (value.id === "amount.units" && typeof p.unit === "string") return `${number}\u00a0${p.unit}`;
  } catch { /* an unknown code: the fallback */ }
  return null;
}

/** One `Intl.DateTimeFormat` per locale and shape: a Stats view renders a hundred clock texts, and building a formatter
 *  is the costly part (PRD 0026 re-validation: the tray drill-in's open time). */
const dateFormats = new Map();
function dateFormat(shape) {
  const key = `${intlLocale()}|${JSON.stringify(shape)}`;
  let fmt = dateFormats.get(key);
  if (!fmt) { fmt = new Intl.DateTimeFormat(intlLocale(), shape); dateFormats.set(key, fmt); }
  return fmt;
}
const hourCycles = new Map();
const hour12 = () => {
  const locale = intlLocale();
  if (!hourCycles.has(locale)) hourCycles.set(locale, /h1[12]/.test(dateFormat({ hour: "numeric" }).resolvedOptions().hourCycle ?? ""));
  return hourCycles.get(locale);
};

/** PRD 0024 re-validation (V-01): the text of a core clock (`at.time` `{hour, minute}`, `at.weekday` `{weekday,
 *  hour?, minute?}` with an ISO weekday, `at.date` `{day, month, hour?, minute?}`, `at.hour` `{hour}`) in the UI locale and its hour
 *  cycle, never an ISO date (DS-MSG-8). Without `hour` it is the chip form: the short weekday or the date alone.
 *  Null for any other note or a malformed param (the caller then shows the core's en-US fallback). */
function clockText(value) {
  if (!["at.time", "at.weekday", "at.date", "at.hour"].includes(value.id)) return null;
  const p = value.params ?? {};
  const int = (key, lo, hi) => {
    const n = typeof p[key] === "string" && /^\d{1,2}$/.test(p[key]) ? Number(p[key]) : NaN;
    return n >= lo && n <= hi ? n : null;
  };
  // PRD 0026 re-validation (V-11): `at.hour` `{hour}` is an hour alone (a chart axis): "6 PM" on a 12-hour clock, the
  // two-digit hour ("18") on a 24-hour one, as the core renders it.
  if (value.id === "at.hour") {
    const hour = int("hour", 0, 23);
    if (hour == null) return null;
    try {
      return hour12() ? holdMark(dateFormat({ hour: "numeric" }).format(new Date(2024, 0, 1, hour))) : String(hour).padStart(2, "0");
    } catch {
      return null;
    }
  }
  const timed = p.hour != null || p.minute != null;
  const [hour, minute] = [int("hour", 0, 23), int("minute", 0, 59)];
  if ((timed || value.id === "at.time") && (hour == null || minute == null)) return null;
  const time = timed ? { hour: "numeric", minute: "2-digit" } : {};
  let date;
  let shape;
  if (value.id === "at.time") {
    date = new Date(2024, 0, 1, hour, minute);
    shape = time;
  } else if (value.id === "at.weekday") {
    const weekday = int("weekday", 1, 7);
    if (weekday == null) return null;
    // 2024-01-01 is a Monday, so day `weekday` of January 2024 has that ISO weekday.
    date = new Date(2024, 0, weekday, hour ?? 0, minute ?? 0);
    shape = { weekday: timed ? "long" : "short", ...time };
  } else {
    const [day, month] = [int("day", 1, 31), int("month", 1, 12)];
    if (day == null || month == null) return null;
    // A leap year, so 29 February is a date.
    date = new Date(2024, month - 1, day, hour ?? 0, minute ?? 0);
    shape = { day: "2-digit", month: "2-digit", ...time };
  }
  try {
    return holdMark(dateFormat(shape).format(date));
  } catch {
    return null;
  }
}

/** PRD 0034 re-validation V-10: Chrome's `Intl` writes a plain space before the day-period mark ("2:03 PM"); the core
 *  writes a no-break space, so the time never wraps away from its mark. */
const holdMark = (text) => text.replace(/(\d) (?=\p{L})/gu, "$1\u00a0");

/** HTML-safe variant of `tl`: the template (or the core fallback) and every param are escaped exactly once. */
export function tle(value, vars = {}) {
  return tl(value, vars, true);
}
