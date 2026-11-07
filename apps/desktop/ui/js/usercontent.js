// User themes and languages. The shell (Rust) reads <config folder>/cuotai/{themes,locales} and hands them over:
// each theme already PARSED by the core (PRD 0050 RF3.1/RF3.2: only its canonical token CSS and registry entry arrive,
// refused files come as errors with the parser's reason), each language pack with its protected security ids already
// removed (PRD 0050 RF3.3). Here they are registered; invalid items are ignored and listed in `userContent.errors`.
//
// Theme:    themes/<id>.css  — optional header  /*! cuotai-theme {"label":"Name","group":"custom","modes":["dark","light"]} */
// Language: locales/<code>.json — { "_meta": { "name": "Name" }, "key": "text" }
import { LOCALES, registerLocale, t } from "./i18n.js";
import { isBuiltinTheme, registerTheme, THEMES } from "./themes.js";

const T = window.__TAURI__;
export const userContent = { dir: null, errors: [] };

const LOCALE_RE = /^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$/;
const MAX_BYTES = 256 * 1024;

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
  if (!T) return userContent; // browser (demo): no user folder
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

// PRD 0050 RF3.3: a pack that tried to replace protected security texts still loads; the shell removed those texts.
export const skippedMessage = (e) => e.reason === "protected"
  ? t("userContent.protected", { kind: t(`userContent.kind.${e.kind}`), name: e.name, count: e.count ?? 0 })
  : t("userContent.skipped", { kind: t(`userContent.kind.${e.kind}`), name: e.name, reason: t(`userContent.reason.${e.reason}`) });
export { LOCALES };
