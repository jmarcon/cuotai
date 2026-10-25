// PRD 0020 RF1/RF2: the Settings window shell (settings.html). It draws the sidebar from the page registry, loads only
// the module of the page shown, remembers the last page and the Advanced group's open state, lands on the page and
// anchor the shell asked for, and tells the other windows what changed. It has no page of its own (RF2.4).
import { boot } from "../boot.js";
import { t, te } from "../i18n.js";
import { bindThemeSwitch, emitSettingsChanged, getSettings, getView, icon, initOrderSync, initTheme, isDesktop, setAdvancedOpen, updaterStatus } from "../lib.js";
import { initUpdates, updateState } from "../updater.js";
import { canLeave, initRows, resetPage } from "./rows.js";
import { ADVANCED, nextEntry, pageById, pageOfAnchor, resolvePage, sidebarHtml } from "./registry.js";
import { buildIndex, forget, resultsHtml, reveal, search } from "./search.js";

const T = window.__TAURI__;
document.documentElement.dataset.platform = /Mac/i.test(navigator.platform || navigator.userAgent || "") ? "mac" : "other";
await boot();

// Per-viewer conveniences (RF2.2, RF2.5): the last page and whether Advanced is open.
const PAGE_KEY = "cuotai.settings.page";
const ADVANCED_KEY = "cuotai.settings.advanced";
const get = (k) => { try { return localStorage.getItem(k); } catch { return null; } };
const put = (k, v) => { try { localStorage.setItem(k, v); } catch {} };

const root = document.getElementById("root");
let current = resolvePage(get(PAGE_KEY)).id;
// PRD 0025 RF5.3: desktop.json holds the Advanced group's state; a value remembered by an older version moves there.
let advancedOpen = await getSettings().then((s) => !!s.advanced_open, () => false);
if (!advancedOpen && get(ADVANCED_KEY) === "open") { advancedOpen = true; setAdvancedOpen(true).catch(() => {}); }
const rememberAdvanced = () => { put(ADVANCED_KEY, advancedOpen ? "open" : "closed"); setAdvancedOpen(advancedOpen).catch(() => {}); };
let view = null;
let leaving = [];
let paintToken = 0;
let shown = null; // the module of the page on screen

/** What a page module sees: the shell's state read for this paint and the ways back into the shell. */
const ctx = {
  settings: null,
  update: null,
  view: () => view,
  setView: (next) => { view = next; },
  changed: (key) => emitSettingsChanged(key),
  rerender: () => paintPage(),
  onLeave: (fn) => leaving.push(fn),
};

function paintFrame() {
  root.innerHTML = `<div class="set-window">
    <aside class="set-side"><div class="set-titlebar" data-tauri-drag-region></div>
      <div class="set-search" role="search">${icon("search")}<input type="search" id="set-search" data-search autocomplete="off" spellcheck="false"
        placeholder="${te("settings.search.placeholder")}" aria-label="${te("settings.search.label")}" aria-controls="set-body" /></div>
      <div data-nav></div></aside>
    <div class="set-main"><div class="set-titlebar" data-tauri-drag-region></div>
      <main class="set-content" id="set-body" data-page-body aria-labelledby="set-page-title"></main></div>
  </div>`;
  initRows(root.querySelector("[data-page-body]"));
  paintNav();
}

function paintNav() {
  const nav = root.querySelector("[data-nav]");
  const focused = document.activeElement?.closest?.("[data-nav]") ? focusKey(document.activeElement) : null;
  nav.innerHTML = sidebarHtml(current, advancedOpen);
  if (focused) nav.querySelector(focused)?.focus();
}
const focusKey = (el) => (el.dataset.page ? `[data-page="${el.dataset.page}"]` : el.matches("[data-advanced-toggle]") ? "[data-advanced-toggle]" : el.matches("[data-page-select]") ? "[data-page-select]" : null);

/** Paints the current page from fresh shell state; a later paint wins over one still loading. */
async function paintPage({ anchor = null, focusTitle = false, find = null } = {}) {
  const token = ++paintToken;
  for (const fn of leaving.splice(0)) { try { fn(); } catch {} }
  const page = resolvePage(current);
  const [mod, settings, update] = await Promise.all([page.load(), getSettings(), updateState() ?? updaterStatus().catch(() => null)]);  // the state this window follows (initUpdates)
  if (token !== paintToken) return;
  ctx.settings = settings;
  ctx.update = update;
  shown = mod;
  const body = root.querySelector("[data-page-body]");
  body.innerHTML = `<h1 class="set-title" id="set-page-title" tabindex="-1">${te(page.label)}</h1>${mod.html(ctx)}`;
  const drawn = mod.bind(body, ctx);
  const scroller = body.parentElement;
  if (focusTitle || anchor) scroller.scrollTop = 0;
  // RF8.1: after a page change, focus moves to its title.
  if (focusTitle) body.querySelector("#set-page-title").focus({ preventScroll: true });
  // RF9.3: a page change cross-fades (instant when motion is reduced or off, through the motion tokens).
  if (focusTitle) { body.classList.remove("page-in"); void body.offsetWidth; body.classList.add("page-in"); }
  // RF6.2: landing from a search result: the page's own state is drawn first, then the row is opened and highlighted.
  if (find) {
    await Promise.resolve(drawn).catch(() => {});
    if (token === paintToken) reveal(body, find);
    return;
  }
  if (!anchor) return;
  // Land on the anchor now and again once the groups that read their state have drawn it, unless the user
  // scrolled meanwhile.
  const land = () => anchorIn(body, anchor)?.scrollIntoView({ block: "start" });
  land();
  const landed = scroller.scrollTop;
  await Promise.resolve(drawn).catch(() => {});
  if (token === paintToken && scroller.scrollTop === landed) land();
}

/** The group an anchor names: its section id, or a group's own data attribute (`updates` → `[data-updates]`). */
function anchorIn(body, anchor) {
  try {
    const a = CSS.escape(anchor);
    return body.querySelector(`#set-${a}, [data-section="${a}"], [data-${a}]`);
  } catch { return null; }
}

/** RF1.3/RF2.2: show `id` (an anchor alone finds its page); the choice is remembered for the next opening. */
async function go(id, { anchor = null, focusTitle = true, find = null } = {}) {
  const page = (id && pageById(id)) || (anchor && pageOfAnchor(anchor)) || pageById(current);
  // RF4.4: an unsaved draft (the hook rules) asks before its page or sub-page is left.
  if (!(await canLeave())) return;
  resetPage();
  clearSearch();
  current = page.id;
  put(PAGE_KEY, current);
  // A link or a restored page inside Advanced opens the group, so the sidebar shows where the user is.
  if (page.group === ADVANCED && !advancedOpen) { advancedOpen = true; rememberAdvanced(); }
  paintNav();
  return paintPage({ anchor, focusTitle, find });
}

const syncTitle = () => {
  document.title = t("settings.title");
  if (isDesktop) T.core.invoke("set_settings_title", { title: document.title }).catch(() => {});
};

root.addEventListener("change", (e) => { if (e.target.matches("[data-page-select]")) go(e.target.value); });
root.addEventListener("click", (e) => {
  const entry = e.target.closest("[data-page]");
  if (entry) return go(entry.dataset.page);
  if (e.target.closest("[data-advanced-toggle]")) {
    advancedOpen = !advancedOpen;
    rememberAdvanced();
    paintNav();
  }
});
// RF8.1: arrow keys, Home and End move between the sidebar entries; Enter or Space opens one (native buttons).
root.addEventListener("keydown", (e) => {
  if (!e.target.closest?.("[data-nav]")) return;
  const entries = [...root.querySelectorAll("[data-nav] .set-nav-item")];
  const next = nextEntry(entries, e.target.closest(".set-nav-item"), e.key);
  if (!next) return;
  e.preventDefault();
  next.focus();
});

// ---------- search (RF6) ----------
let searching = false;
let searchToken = 0;
const field = () => root.querySelector("[data-search]");
/** The context the page modules draw their rows with, so the index sees the rows the pages draw. */
const indexCtx = async () => ({ ...ctx, settings: await getSettings(), update: updateState() ?? (await updaterStatus().catch(() => null)) });
function clearSearch() {
  searching = false;
  searchToken++;
  if (field()) field().value = "";
}
/** Draws the results for the field's text in place of the page; an empty field returns to the page. */
async function runSearch() {
  const query = field().value.trim();
  const token = ++searchToken;
  if (!query) {
    if (searching) { searching = false; await paintPage(); }
    return;
  }
  // RF4.4: unsaved rules ask before the page is replaced; staying keeps the page and clears the field.
  if (!searching) {
    if (!(await canLeave())) { field().value = ""; return; }
    resetPage();
    for (const fn of leaving.splice(0)) { try { fn(); } catch {} }
  }
  const rows = await buildIndex(await indexCtx());
  if (token !== searchToken) return;
  searching = true;
  paintToken++;
  root.querySelector("[data-page-body]").innerHTML = resultsHtml(search(rows, query));
}
root.addEventListener("input", (e) => { if (e.target.matches("[data-search]")) runSearch(); });
root.addEventListener("click", (e) => {
  const hit = e.target.closest("[data-result]");
  if (hit) return go(hit.dataset.result, { find: hit.dataset.rowKey, focusTitle: false });
  if (e.target.closest("[data-search-clear]")) { clearSearch(); paintPage(); field().focus(); }
});
// RF6.1: `/` and Ctrl/Cmd+F focus the search; Esc clears it, then leaves it. RF8.3: Ctrl/Cmd+W closes the window.
const typing = (el) => el?.matches?.("input, textarea, select, [contenteditable]");
const editing = (el) => el?.matches?.("textarea, select, [contenteditable], input:not([type=checkbox], [type=radio], [type=button])");
addEventListener("keydown", (e) => {
  const mod = /Mac/i.test(navigator.platform || "") ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  if ((e.key === "/" && !typing(e.target) && !mod && !e.altKey) || (mod && !e.altKey && e.key.toLowerCase() === "f")) {
    e.preventDefault();
    field()?.focus();
    field()?.select();
  } else if (mod && !e.altKey && e.key.toLowerCase() === "w") {
    e.preventDefault();
    if (isDesktop) T.core.invoke("close_settings_window").catch(() => {});
  } else if (e.key === "Escape" && e.target === field() && !document.querySelector("dialog[open]")) {
    e.preventDefault();
    if (field().value) { clearSearch(); paintPage(); } else field().blur();
  }
});

/** The request the window was opened with: from the shell, or from the query string in the browser demo. */
async function takeTarget() {
  if (isDesktop) return T.core.invoke("settings_target").catch(() => null);
  const q = new URLSearchParams(location.search);
  return { page: q.get("page"), anchor: q.get("anchor") };
}
const applyTarget = (target) => (target?.page || target?.anchor
  ? go(target.page, { anchor: target.anchor, focusTitle: !target.anchor })
  : null);

// Theme, mode and language (with cross-fade) repaint the whole window; the other windows follow the shell event.
const repaintAll = () => { syncTitle(); paintNav(); forget(); return searching ? runSearch() : paintPage(); };
bindThemeSwitch(root, repaintAll);
initTheme(repaintAll);
initOrderSync(() => paintPage());

if (isDesktop) {
  T.event.listen("settings-navigate", () => takeTarget().then(applyTarget));
  // A page drawn from the view (the history's account list) is drawn again when its data changed under it.
  T.event.listen("snapshot", (e) => {
    const prev = view;
    view = e.payload;
    if (shown?.viewChanged?.(view, prev)) paintPage();
  });
  T.event.listen("user-content-changed", () => location.reload());
}
// PRD 0018 RF3.1: the Updates group (About page) acts on the updater state this window follows.
await initUpdates();
try { view = await getView(); } catch { view = null; }
syncTitle();
paintFrame();
const first = await takeTarget();
if (first?.page || first?.anchor) await applyTarget(first);
else await go(current, { focusTitle: false });
// RF8.3: Esc closes a popover first (rows.js), then the sub-page, and the window last; registered after the rows'
// own handler so it only sees an Esc nothing took. A field being edited keeps its Esc.
addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || e.defaultPrevented || editing(e.target) || document.querySelector("dialog[open]")) return;
  if (isDesktop) T.core.invoke("close_settings_window").catch(() => {});
});
