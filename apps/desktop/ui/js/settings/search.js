// PRD 0020 RF6: search over the rows of every page. The index is read from the markup the pages themselves draw (a
// row is `row()` markup with its label, description and keywords), so a row cannot exist without being searchable
// (RF6.3). A page whose rows depend on the shell's state exports `searchHtml(ctx)` for the real rows.
import { t, te, currentLocale } from "../i18n.js";
import { esc } from "../lib.js";
import { PAGES } from "./registry.js";

const ENTITIES = { "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&#39;": "'", "&#x27;": "'" };
const plain = (html) => html.replace(/<[^>]*>/g, " ").replace(/&(?:amp|lt|gt|quot|#39|#x27);/g, (m) => ENTITIES[m]).replace(/\s+/g, " ").trim();
/** Lower case without accents, so "configuracao" finds "configuração". */
export const fold = (text) => text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** The text between `start` and `end` after the first `start` in `html` ("" when absent). */
const between = (html, start, end) => {
  const at = html.indexOf(start);
  if (at < 0) return "";
  const from = at + start.length;
  const to = html.indexOf(end, from);
  return to < 0 ? "" : html.slice(from, to);
};

/** Every row of `html` as `{ key, label, desc, kw }` (plain text). A row with no label of its own shows its `data-find`. */
export function rowsOf(html) {
  const rows = [];
  const seen = new Set();
  for (const m of html.matchAll(/<div class="set-row[^"]*" data-row="([^"]+)"([^>]*)>/g)) {
    const [, key, attrs] = m;
    if (seen.has(key)) continue;
    seen.add(key);
    const attr = (name) => plain(between(attrs, `${name}="`, '"'));
    const label = plain(between(html, `id="${key}-label">`, "</span>")) || attr("data-find");
    if (!label) continue;
    rows.push({ key, label, desc: plain(between(html, `id="${key}-desc">`, "</span>")), kw: attr("data-kw") });
  }
  return rows;
}

let cache = null;
/** The index of all pages for the active language, built once and kept until the language changes or `forget()`. */
export async function buildIndex(ctx) {
  const lang = currentLocale();
  if (cache?.lang === lang) return cache.rows;
  const rows = [];
  for (const page of PAGES) {
    let html = "";
    try {
      const mod = await page.load();
      html = mod.searchHtml ? await mod.searchHtml(ctx) : mod.html(ctx);
    } catch { continue; }
    for (const r of rowsOf(html)) rows.push({ ...r, page: page.id, pageLabel: t(page.label), text: fold(`${r.label} ${r.desc} ${r.kw}`), head: fold(r.label) });
  }
  cache = { lang, rows };
  return rows;
}
export const forget = () => { cache = null; };

const wordsOf = (text) => text.split(/[^\p{L}\p{N}]+/u).filter(Boolean);

/** How well `row` matches: 0 the label is the query, 1 the label starts with it, 2 every word is a whole word of the
 *  label, 3 every word is inside the label, 4 every word is a whole word of the row's text, 5 anywhere. */
export function rankOf(row, words) {
  const query = words.join(" ");
  if (row.head === query) return 0;
  if (row.head.startsWith(query)) return 1;
  const head = wordsOf(row.head);
  if (words.every((w) => head.includes(w))) return 2;
  if (words.every((w) => row.head.includes(w))) return 3;
  const text = wordsOf(row.text);
  return words.every((w) => text.includes(w)) ? 4 : 5;
}

/** RF6.1: rows where every word of `query` occurs in the label, description or keywords, best match first (see
 *  `rankOf`; PRD 0020 re-validation V-06), then by page order. `resultsHtml` groups by page in this order, so a
 *  page's group comes where its best row ranks. */
export function search(rows, query) {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const order = new Map(PAGES.map((p, i) => [p.id, i]));
  const hits = rows
    .filter((r) => words.every((w) => r.text.includes(w)))
    .map((r, i) => ({ ...r, rank: rankOf(r, words), at: i }));
  const best = new Map();
  for (const r of hits) best.set(r.page, Math.min(best.get(r.page) ?? 9, r.rank));
  return hits.sort((a, b) => best.get(a.page) - best.get(b.page) || order.get(a.page) - order.get(b.page) || a.rank - b.rank || a.at - b.at)
    .map(({ at, ...r }) => r);
}

/** RF6.2: the results replace the page area, grouped by page; each shows the row's label (and description) and, by its
 *  group, its page. RF6.3: none found says so and suggests clearing the field. */
export function resultsHtml(results) {
  const head = `<h1 class="set-title" id="set-page-title" tabindex="-1">${te("settings.search.title")}</h1>`;
  if (!results.length) {
    return `${head}<div class="set-empty" role="status"><p class="set-empty-title">${te("settings.search.none")}</p>
      <p class="hint">${te("settings.search.noneHint")}</p>
      <button type="button" class="btn" data-search-clear>${te("settings.search.clear")}</button></div>`;
  }
  const pages = [...new Set(results.map((r) => r.page))];
  return `${head}<p class="hint" role="status">${te("settings.search.count", { count: results.length })}</p>`
    + pages.map((id) => {
      const list = results.filter((r) => r.page === id);
      return `<section class="set-sec set-results" aria-label="${esc(list[0].pageLabel)}"><h2>${esc(list[0].pageLabel)}</h2>${list.map((r) =>
        `<button type="button" class="set-result" data-result="${esc(r.page)}" data-row-key="${esc(r.key)}">
          <span class="set-result-label">${esc(r.label)}</span>${r.desc ? `<span class="set-result-desc">${esc(r.desc)}</span>` : ""}</button>`).join("")}</section>`;
    }).join("");
}

/** RF6.2: after the page is drawn, open what hides `key` (a disclosure or the sub-page of a row) and highlight it for
 *  about 1.5 seconds. Resolves to the row, or null when the page does not show it. */
export function reveal(body, key, ms = 1500) {
  const row = body.querySelector(`[data-row="${CSS.escape(key)}"]`);
  if (!row) return null;
  for (let el = row.parentElement; el && el !== body; el = el.parentElement) if (el.matches?.("details")) el.open = true;
  const sub = row.closest("[data-subpage]");
  if (sub) body.querySelector(`[data-open-sub="${CSS.escape(sub.dataset.subpage)}"]`)?.click();
  row.scrollIntoView({ block: "center" });
  // PRD 0020 re-validation V-05: keyboard focus lands on the found row (its control, else its label), not on <body>.
  const control = row.querySelector(".set-row-ctl :is(input, select, textarea, button, [role=radio][tabindex='0']):not(:disabled)");
  const label = row.querySelector(".set-row-label");
  if (control) control.focus({ preventScroll: true });
  else if (label) { label.tabIndex = -1; label.focus({ preventScroll: true }); }
  row.classList.add("found");
  setTimeout(() => row.classList.remove("found"), ms);
  return row;
}
