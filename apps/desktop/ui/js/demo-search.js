// PRD 0027 re-validation C-07: a JavaScript port of the core's catalog search (`cuotai-core/src/catalog.rs`: `fold`,
// `tier`, `Fields`, `search_in`) and of `account_edit::picker_search`, so the browser demo, which has no core, ranks and
// groups providers exactly as the app does. The catalog comes from `demo-catalog.json`, which a core test generates;
// `scripts/test-provider-picker-ui.mjs` checks this port against the core's own results (`tools/catalog-parity.json`).
// Change both sides together.

/** ASCII base of a precomposed Latin letter (`catalog::base_letter`), or null. */
const BASE = new Map(Object.entries({
  a: "àáâãäåāăą", c: "çćčĉċ", d: "ďđ", e: "èéêëēĕėęě", g: "ĝğġģ", i: "ìíîïĩīĭįı", n: "ñńňņ", o: "òóôõöøōŏő",
  r: "ŕřŗ", s: "śšşŝ", t: "ťţ", u: "ùúûüũūŭůűų", y: "ýÿ", z: "źžż",
}).flatMap(([base, letters]) => [...letters].map((letter) => [letter, base])).concat([["ß", "ss"], ["æ", "ae"], ["œ", "oe"]]));

const ALNUM = /^[\p{Alphabetic}\p{N}]$/u;
const ASCII_ALNUM = /^[A-Za-z0-9]$/;

/** `catalog::fold`: lower-case, accents and full-width forms folded, a space between words and between scripts. */
export function fold(text) {
  let out = "";
  let lastAscii = null;
  const push = (c) => {
    if (ALNUM.test(c)) {
      const ascii = ASCII_ALNUM.test(c);
      if (lastAscii !== null && lastAscii !== ascii) out += " ";
      out += c;
      lastAscii = ascii;
    } else {
      if (!out.endsWith(" ") && out !== "") out += " ";
      lastAscii = null;
    }
  };
  for (let c of String(text ?? "")) {
    const cp = c.codePointAt(0);
    if (cp >= 0xff01 && cp <= 0xff5e) c = String.fromCodePoint(cp - 0xfee0);
    else if (cp === 0x3000) c = " ";
    for (const lower of c.toLowerCase()) {
      const lcp = lower.codePointAt(0);
      if (lcp >= 0x300 && lcp <= 0x36f) continue;
      const base = BASE.get(lower);
      if (base) for (const b of base) push(b);
      else push(lower);
    }
  }
  return out.trimEnd();
}

/** `catalog::tier`: exact 4, prefix 3, word prefix 2, substring 1, absent 0. */
const tier = (field, token) => (field === token ? 4 : field.startsWith(token) ? 3
  : field.split(" ").some((word) => word.startsWith(token)) ? 2 : field.includes(token) ? 1 : 0);

/** Folded forms of a localized text in the query locale and in en-US (`catalog::texts`). */
function texts(text, locale, resolve) {
  const forms = [fold(resolve(text, locale))];
  const en = fold(resolve(text, "en-US"));
  if (!forms.includes(en)) forms.push(en);
  return forms;
}

function fields(entry, locale, resolve) {
  return {
    name: texts(entry.display_name, locale, resolve),
    slug: fold(entry.provider),
    aliases: entry.aliases.map(fold),
    category: texts(entry.category_label, locale, resolve),
    method: texts(entry.method_label, locale, resolve),
    hint: texts(entry.hint, locale, resolve),
  };
}

const maxTier = (list, token) => Math.max(0, ...list.map((f) => tier(f, token)));

function providerScore(f, token) {
  const best = (list, weight, offset) => { const t = maxTier(list, token); return t > 0 ? t * weight - offset : 0; };
  return Math.max(best(f.name, 100, 0), best([f.slug], 100, 10), best(f.aliases, 15, 0), best(f.category, 5, 0));
}

const modeScore = (f, token) => Math.max(maxTier(f.method, token), maxTier(f.hint, token)) * 5;

/**
 * `account_edit::picker_search` over `catalog` (the generated `demo-catalog.json`, already restricted to the offered
 * providers). `resolve(localized, locale)` renders a core text in a language. Returns the catalog entries in order.
 */
export function searchCatalog(catalog, query, locale, resolve) {
  const bounded = Array.from(String(query ?? "")).slice(0, catalog.max_query_chars).join("");
  const entries = catalog.entries;
  const folded = fold(bounded);
  let out;
  if (folded === "") {
    out = [...entries].sort((a, b) => catalog.methods.indexOf(a.method) - catalog.methods.indexOf(b.method));
  } else {
    const tokens = folded.split(" ");
    const hits = [];
    entries.forEach((entry, index) => {
      const f = fields(entry, locale, resolve);
      let total = 0;
      let modeSpecific = false;
      for (const token of tokens) {
        const provider = providerScore(f, token);
        if (provider > 0) { total += provider; continue; }
        const mode = modeScore(f, token);
        if (mode === 0) return;
        total += mode;
        modeSpecific = true;
      }
      hits.push({ index, score: total, modeSpecific });
    });
    const kept = hits.filter((hit) => {
      const provider = entries[hit.index].provider;
      const namesMode = hits.some((h) => entries[h.index].provider === provider && h.modeSpecific);
      return namesMode ? hit.modeSpecific : entries[hit.index].recommended;
    });
    kept.sort((a, b) => b.score - a.score || a.index - b.index);
    out = kept.map((hit) => entries[hit.index]);
  }
  // `picker_search`: the empty query keeps one entry per provider, its recommended mode.
  return String(query ?? "").trim() === "" ? out.filter((entry) => entry.recommended) : out;
}

/** A core text's template in `catalog` (a core locale file), its params substituted; the English fallback otherwise. */
export function resolveWith(catalog, text) {
  const template = catalog?.[text.id];
  const form = typeof template === "object" && template ? template.other : template;
  if (typeof form !== "string") return text.fallback;
  return form.replace(/\{(\w+)\}/g, (m, name) => {
    const value = text.params?.[name];
    if (value == null) return m;
    return typeof value === "object" ? resolveWith(catalog, value) : String(value);
  });
}
