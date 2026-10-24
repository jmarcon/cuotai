// PRD 0020 RF1/RF2: the Settings window shell (settings.html). It draws the sidebar from the page registry, loads only
// the module of the page shown, remembers the last page and the Advanced group's open state, lands on the page and
// anchor the shell asked for, and tells the other windows what changed. It has no page of its own (RF2.4).
import { boot } from "../boot.js";
import { t, te } from "../i18n.js";
import { bindThemeSwitch, emitSettingsChanged, getSettings, getView, initOrderSync, initTheme, isDesktop, updaterStatus } from "../lib.js";
import { initUpdates } from "../updater.js";
import { ADVANCED, nextEntry, pageById, pageOfAnchor, resolvePage, sidebarHtml } from "./registry.js";

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
let advancedOpen = get(ADVANCED_KEY) === "open";
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
    <aside class="set-side"><div class="set-titlebar" data-tauri-drag-region></div><div data-nav></div></aside>
    <div class="set-main"><div class="set-titlebar" data-tauri-drag-region></div>
      <main class="set-content" data-page-body aria-labelledby="set-page-title"></main></div>
  </div>`;
  paintNav();
}

function paintNav() {
  const nav = root.querySelector("[data-nav]");
  const focused = document.activeElement?.closest?.("[data-nav]") ? focusKey(document.activeElement) : null;
  nav.innerHTML = sidebarHtml(current, advancedOpen);
  if (focused) nav.querySelector(focused)?.focus();
}
const focusKey = (el) => (el.dataset.page ? `[data-page="${el.dataset.page}"]` : el.matches("[data-advanced-toggle]") ? "[data-advanced-toggle]" : null);

/** Paints the current page from fresh shell state; a later paint wins over one still loading. */
async function paintPage({ anchor = null, focusTitle = false } = {}) {
  const token = ++paintToken;
  for (const fn of leaving.splice(0)) { try { fn(); } catch {} }
  const page = resolvePage(current);
  const [mod, settings, update] = await Promise.all([page.load(), getSettings(), updaterStatus().catch(() => null)]);
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
function go(id, { anchor = null, focusTitle = true } = {}) {
  const page = (id && pageById(id)) || (anchor && pageOfAnchor(anchor)) || pageById(current);
  current = page.id;
  put(PAGE_KEY, current);
  // A link or a restored page inside Advanced opens the group, so the sidebar shows where the user is.
  if (page.group === ADVANCED && !advancedOpen) { advancedOpen = true; put(ADVANCED_KEY, "open"); }
  paintNav();
  return paintPage({ anchor, focusTitle });
}

const syncTitle = () => {
  document.title = t("settings.title");
  if (isDesktop) T.core.invoke("set_settings_title", { title: document.title }).catch(() => {});
};

root.addEventListener("click", (e) => {
  const entry = e.target.closest("[data-page]");
  if (entry) return go(entry.dataset.page);
  if (e.target.closest("[data-advanced-toggle]")) {
    advancedOpen = !advancedOpen;
    put(ADVANCED_KEY, advancedOpen ? "open" : "closed");
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

/** The request the window was opened with: from the shell, or from the query string in the browser demo. */
async function takeTarget() {
  if (isDesktop) return T.core.invoke("settings_target").catch(() => null);
  const q = new URLSearchParams(location.search);
  return { page: q.get("page"), anchor: q.get("anchor") };
}
const applyTarget = (target) => (target?.page || target?.anchor
  ? go(target.page, { anchor: target.anchor, focusTitle: !target.anchor })
  : null);

// Theme, mode and language (with cross-fade) repaint the whole window; the other windows follow by `storage` events.
const repaintAll = () => { syncTitle(); paintNav(); return paintPage(); };
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
