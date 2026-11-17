// User themes and languages. The shell (Rust) reads <config folder>/cuotai/{themes,locales} and hands them over:
// each theme already PARSED by the core (PRD 0050 RF3.1/RF3.2: only its canonical token CSS and registry entry arrive,
// refused files come as errors with the parser's reason), each language pack already PARSED and filtered by the core
// (PRD 0050 RF3.3, PRD 0035 re-validation S-02: `{ name, code, label, flag, digest, messages, ignored }`, never the
// file's text, so nothing here parses a pack; a file the core refused arrives only as an error). Here they are
// registered; invalid items are ignored and listed in `userContent.errors`.
//
// Theme:    themes/<id>.css  — optional header  /*! cuotai-theme {"label":"Name","group":"custom","modes":["dark","light"]} */
// Language: locales/<code>.json — { "_meta": { "name": "Name" }, "key": "text" }
import { LOCALES, isIntlTag, registerLocale, t } from "./i18n.js";
import { isBuiltinTheme, registerTheme, THEMES } from "./themes.js";

const T = window.__TAURI__;
// `locales`: the packs registered, each `{ name, code, label, flag, digest, ignored, messages }` as the core sent it.
export const userContent = { dir: null, errors: [], locales: [] };

// The shape of a pack name; `isIntlTag` then refuses what `Intl` cannot read (`en-US-US`, `en-a1`), PRD 0034
// re-validation S-05.
const LOCALE_RE = /^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$/;
const ID_RE = /^[A-Za-z0-9_.-]{1,128}$/;
const FLAG_RE = /^[A-Za-z]{2}$/;
const PLURAL = new Set(["zero", "one", "two", "few", "many", "other"]);
const isPlainObject = (v) => v !== null && typeof v === "object" && !Array.isArray(v)
  && Object.getPrototypeOf(v) === Object.prototype;

class Skip extends Error {}

// The theme was parsed in the core; only the built-in ids (known here) can still collide.
function addTheme(file) {
  const { id, label, group, modes, text } = file;
  if (isBuiltinTheme(id) || THEMES.some((x) => x.id === id)) throw new Skip("duplicate");
  const style = document.createElement("style");
  style.dataset.userTheme = id;
  style.textContent = text;
  document.head.append(style);
  registerTheme({ id, label, group, modes });
}

// The pack was parsed, checked and filtered in the core (`user_pack::load`). An item still carrying a file's `text`, or
// without a messages object, comes from no shell that filtered it and is refused whole. The other checks repeat the
// core's shape rules as defence in depth; they never parse anything.
function addLocale(item) {
  if (!isPlainObject(item) || Object.hasOwn(item, "text") || !isPlainObject(item.messages)) throw new Skip("unreadable");
  const { name, code, label, flag, digest, messages } = item;
  const ignored = Array.isArray(item.ignored) ? item.ignored.length : 0;
  if (typeof code !== "string" || code.length > 35 || !LOCALE_RE.test(code) || !isIntlTag(code)) throw new Skip("invalidCode");
  if (code.toLowerCase() === "en-us") throw new Skip("reservedCode");
  if (typeof label !== "string" || !label.trim() || [...label].length > 48) throw new Skip("invalidMeta");
  if (flag != null && (typeof flag !== "string" || !FLAG_RE.test(flag))) throw new Skip("invalidMeta");
  const out = {};
  for (const [id, v] of Object.entries(messages)) {
    if (!ID_RE.test(id) || id === "__proto__") throw new Skip("invalidKey");
    const plural = isPlainObject(v) && Object.hasOwn(v, "other")
      && Object.entries(v).every(([k, x]) => PLURAL.has(k) && typeof x === "string");
    if (typeof v !== "string" && !plural) throw new Skip("invalidValue");
    out[id] = typeof v === "string" ? v : { ...v };
  }
  registerLocale(code, label, out, flag ?? undefined, typeof name === "string" ? name : `${code}.json`);
  userContent.locales.push({ name, code, label, flag: flag ?? null, digest, ignored, messages: out });
  return code;
}

export async function loadUserContent() {
  if (!T) return demoPack(); // browser (demo): no user folder
  let raw;
  try { raw = await T.core.invoke("load_user_content"); } catch { return userContent; }
  userContent.dir = raw.dir;
  userContent.locales = [];
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
  // A "protected" notice belongs to one content of one file: the core's digest of it (S-01 migration).
  for (const e of userContent.errors) {
    if (e.reason !== "protected") continue;
    const digest = raw.locales.find((l) => l?.name === e.name)?.digest;
    if (typeof digest === "string") e.key = `${digest}:${e.name}`;
  }
  return userContent;
}

// Browser demo only, with `?pack` in the address: a Dutch pack as the shell would send it (the core's label, a safe
// file name, one dropped protected id), so the UI checks can render the selector marker, the Settings › General line
// and the notice. Nothing is read from disk.
function demoPack() {
  if (!/[?&]pack(=|&|$)/.test(globalThis.location?.search ?? "")) return userContent;
  const item = { name: "nl.json", code: "nl", label: "Nederlands", flag: "nl", digest: "0".repeat(64), ignored: ["serve.banner"],
    messages: { "lang.label": "Taal", "settings.page.general": "Algemeen", "settings.lang.desc": "De taal van het venster en de meldingen.",
      "common.refresh": "Vernieuwen", "common.add": "Account toevoegen" } };
  userContent.locales = [];
  addLocale(item);
  userContent.errors = [{ kind: "locale", name: item.name, reason: "protected", count: 1, key: `${item.digest}:${item.name}` }];
  try { localStorage.setItem("cuotai.userLocales", JSON.stringify([item.code])); } catch {}
  return userContent;
}

// PRD 0035 re-validation S-01: a pack whose protected texts the core dropped is reported in the main window once per
// content of the file (`name` plus the core's `digest`); after Dismiss it returns only when the file changes.
// Settings › Files keeps every notice. Stored as lines, never parsed as JSON.
const SEEN = "cuotai.packNotices";
const seenKeys = () => { try { return (localStorage.getItem(SEEN) ?? "").split("\n").filter(Boolean); } catch { return []; } };
/** The notices the main window shows: every refusal, and each "protected" notice not yet dismissed for this content. */
export const bannerNotices = () => {
  const seen = new Set(seenKeys());
  return userContent.errors.filter((e) => !(e.reason === "protected" && e.key && seen.has(e.key)));
};
/** Dismisses the "protected" notice of one pack content (`key` from `bannerNotices`). */
export function dismissNotice(key) {
  if (typeof key !== "string" || !key) return;
  const keys = [...new Set([...seenKeys(), key])].slice(-64);
  try { localStorage.setItem(SEEN, keys.join("\n")); } catch {}
}

// PRD 0050 RF3.3: a pack that tried to replace protected security texts still loads; the shell removed those texts.
export const skippedMessage = (e) => e.reason === "protected"
  ? t("userContent.protected", { kind: t(`userContent.kind.${e.kind}`), name: e.name, count: e.count ?? 0 })
  : t("userContent.skipped", { kind: t(`userContent.kind.${e.kind}`), name: e.name, reason: t(`userContent.reason.${e.reason}`) });
export { LOCALES };
