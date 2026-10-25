// PRD 0020 RF3/RF4/RF5: the shared row and component set of the Settings pages. A page builds every setting with
// `row()` and one control builder; nothing here knows a setting. The delegated behaviour (segmented choice, info
// popovers, remembered disclosures, sub-pages, the "Saved" toast and fields that commit on Enter or blur) is
// installed once per window by `initRows()`. Styles: `css/settings.css`, theme roles only (docs/design-system.md).
import { t, te } from "../i18n.js";
import { esc, icon } from "../lib.js";

const get = (k) => { try { return localStorage.getItem(k); } catch { return null; } };
const put = (k, v) => { try { localStorage.setItem(k, v); } catch {} };
const DISC_KEY = (key) => `cuotai.settings.disc.${key}`;

/** The aria attributes that tie a control to its row's label and description (RF8.2). */
const aria = (key, desc) => `aria-labelledby="${key}-label"${desc ? ` aria-describedby="${key}-desc"` : ""}`;

/** RF3.1: one setting. Label (and one short description) on the left, the control on the right, an optional
 *  ⓘ popover (RF5.3), an optional full-width `body` under it, and the row's own error line (RF4.3).
 *  `control(aria)` receives the attributes that label it. `dep`: a dependent row (RF3.3), indented under its parent
 *  and disabled with it. `msg`/`msgRole`: the id and role of the error line the page binding writes to. */
export function row({ key, label, desc = "", info = "", control = () => "", body = "", dep = false, danger = false, msg = `${key}-msg`, msgRole = "alert", attrs = "" }) {
  const a = aria(key, desc);
  return `<div class="set-row${dep ? " dep" : ""}${danger ? " danger" : ""}" data-row="${esc(key)}" ${attrs}>
    <div class="set-row-text"><span class="set-row-label" id="${key}-label">${label}</span>${info ? infoHtml(key, info) : ""}
      ${desc ? `<span class="set-row-desc" id="${key}-desc">${desc}</span>` : ""}</div>
    <div class="set-row-ctl">${control(a)}</div>
    ${body ? `<div class="set-row-body">${body}</div>` : ""}
    <p class="set-row-err err" id="${msg}" role="${msgRole}"></p></div>`;
}

/** RF3.2 switch (DS-CMP-SW-1): a native check box exposed as `role="switch"`, so `checked` and `change` keep
 *  working for the bindings. */
export const sw = (id, on, a = "", extra = "") =>
  `<input id="${id}" type="checkbox" role="switch" class="sw" ${on ? "checked" : ""} ${a} ${extra} />`;

/** RF3.2 membership check (DS-CMP-CHK-1), for items of a set such as a hook rule's events. */
export const check = (attrs, on) => `<input type="checkbox" class="chk" ${attrs} ${on ? "checked" : ""} />`;

/** RF3.2 segmented control (DS-CMP-SEG-1/3): 2–5 exclusive choices. The group gets a `value` property and fires
 *  `change`, like a select, once `initRows()` runs. */
export function seg(id, options, value, a = "", extra = "") {
  const any = options.some(([v]) => String(v) === String(value));
  return `<div class="seg" role="radiogroup" id="${id}" ${a} ${extra}>${options.map(([v, label, attrs = ""], i) => {
    const on = String(v) === String(value);
    return `<button type="button" role="radio" data-value="${esc(v)}" aria-checked="${on}" tabindex="${on || (!any && i === 0) ? 0 : -1}" ${attrs}>${label}</button>`;
  }).join("")}</div>`;
}

/** RF3.2 select (DS-CMP-SEL-1). */
export const select = (id, options, value, a = "") =>
  `<select class="set-select" id="${id}" ${a}>${options.map(([v, label]) =>
    `<option value="${esc(v)}" ${String(v) === String(value) ? "selected" : ""}>${label}</option>`).join("")}</select>`;

/** RF3.2 number field with unit, plus the row's own Apply/Revert (RF4.2), shown only while the value is unsaved.
 *  `apply`: extra attributes of the Apply button (the parity checklist looks for them). */
export const num = (id, { value, min, max, step = 1, unit = "", apply = "" }, a = "") =>
  `<span class="set-num"><input id="${id}" type="number" inputmode="numeric" ${min != null ? `min="${esc(min)}"` : ""} ${max != null ? `max="${esc(max)}"` : ""} step="${step}" value="${esc(value)}" ${a} />${unit ? `<span class="set-unit">${unit}</span>` : ""}</span>
    <span class="set-commit" hidden><button type="button" class="btn sm" data-commit-revert>${te("settings.revert")}</button><button type="button" class="btn sm primary" data-commit-apply ${apply}>${te("settings.apply")}</button></span>`;

/** RF3.2 button row control. */
export const buttons = (...html) => `<span class="set-btns">${html.join("")}</span>`;

/** RF3.2 path row: the path truncated in the middle of the row, its actions beside it. */
export const pathHtml = (path) => `<code class="set-path" title="${esc(path ?? "")}">${esc(path || "—")}</code>`;

/** RF5.3: the ⓘ button and its popover; the full text stays in the page, hidden until asked for. */
const infoHtml = (key, text) => `<button type="button" class="set-info" data-info="${key}-info" aria-expanded="false" aria-controls="${key}-info"
    aria-label="${te("settings.info")}" title="${te("settings.info")}">${icon("info")}</button>
  <div class="set-pop" id="${key}-info" role="note" hidden>${text}</div>`;

/** RF5.2: a disclosure whose open state is remembered per key. */
export const disc = (key, summary, body) =>
  `<details class="set-disc" data-disc="${esc(key)}" ${get(DISC_KEY(key)) === "open" ? "open" : ""}><summary>${icon("chev", "set-disc-chev")}<span>${summary}</span></summary><div class="set-disc-body">${body}</div></details>`;

/** RF2.3: a row that opens a sub-page of this page (chevron right, DS-CMP-NAV-2). */
export const subRow = ({ key, label, desc = "", info = "" }) => row({ key, label, desc, info,
  control: (a) => `<button type="button" class="icon set-open-sub" data-open-sub="${esc(key)}" ${a}>${icon("chev")}</button>` });

/** RF2.3: the sub-page itself, hidden until its row opens it; a back button returns (DS-CMP-NAV-1). */
export const subpage = (key, title, body) => `<div class="set-subpage" data-subpage="${esc(key)}" role="region" aria-labelledby="${key}-sub-title">
    <div class="set-subhead"><button type="button" class="icon set-back" data-sub-back aria-label="${te("common.back")}" title="${te("common.back")}">${icon("chev")}</button>
      <h2 id="${key}-sub-title" tabindex="-1">${title}</h2></div>${body}</div>`;

// ---------- feedback (RF4.1, RF4.3) ----------

let toastTimer = null;
/** RF4.1 / DS-CMP-MSG-3: one polite "Saved" toast in the bottom corner, replaced by the next, gone after 4 s
 *  (paused while hovered or focused). */
export function toast(text = t("settings.saved")) {
  let el = document.querySelector(".set-toast");
  if (!el) {
    el = document.createElement("div");
    el.className = "set-toast";
    el.setAttribute("role", "status");
    el.setAttribute("aria-live", "polite");
    el.tabIndex = -1;
    const hold = () => clearTimeout(toastTimer);
    el.addEventListener("mouseenter", hold);
    el.addEventListener("focusin", hold);
    el.addEventListener("mouseleave", () => arm(el));
    el.addEventListener("focusout", () => arm(el));
    document.body.append(el);
  }
  el.textContent = text;
  el.hidden = false;
  arm(el);
}
function arm(el) {
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { el.hidden = true; }, 4000);
}

/** RF4.3: the error line of the row that holds `el` (empty text clears it). */
export function rowError(el, text) {
  const line = el?.closest?.(".set-row")?.querySelector(".set-row-err");
  if (line) line.textContent = text;
}

/** RF4.2: a field that saves on Enter, on leaving it or with its row's Apply; Revert (or Esc) restores the saved
 *  value. `save(value)` resolves when saved and rejects with the text to show; a failure restores the saved
 *  value, shows the text in the row and keeps the focus there. `onSaved` runs after a success. */
export function commitField(input, save, { onSaved = () => toast() } = {}) {
  const r = input.closest(".set-row");
  const bar = r?.querySelector(".set-commit");
  const apply = bar?.querySelector("[data-commit-apply]");
  const revert = bar?.querySelector("[data-commit-revert]");
  let saved = input.value;
  let busy = false;
  const sync = () => { if (bar) bar.hidden = input.value === saved; };
  const commit = async () => {
    if (busy || input.value === saved) return sync();
    busy = true;
    const value = input.value;
    try {
      await save(value);
      saved = value;
      rowError(input, "");
      onSaved(value);
    } catch (text) {
      input.value = saved;
      input.dispatchEvent(new Event("input"));
      rowError(input, typeof text === "string" ? text : t("common.unexpected"));
      if (input.isConnected) input.focus();
    } finally { busy = false; sync(); }
  };
  const back = () => { input.value = saved; input.dispatchEvent(new Event("input")); rowError(input, ""); input.focus(); };
  input.addEventListener("input", sync);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); commit(); }
    else if (e.key === "Escape" && input.value !== saved) { e.preventDefault(); e.stopPropagation(); back(); }
  });
  input.addEventListener("blur", (e) => { if (!bar?.contains(e.relatedTarget)) commit(); });
  if (apply) apply.onclick = commit;
  if (revert) revert.onclick = back;
  return { commit, set: (value) => { input.value = String(value); return commit(); } };
}

// ---------- sub-pages (RF2.3) ----------

let openSub = null;
const guards = new Map();
let content = null;

/** RF4.4: a check run before a sub-page (or its page) is left; resolves `true` to leave. */
export const setGuard = (key, fn) => guards.set(key, fn);

/** Every guard of the shown page agrees to leave it; called by the shell before a page change. */
export async function canLeave() {
  for (const fn of guards.values()) if (!(await fn())) return false;
  return true;
}

/** Forget the page's sub-page and guards (the shell calls it when another page is painted). */
export function resetPage() { openSub = null; guards.clear(); }

/** Hides everything but the open sub-page; runs again when a page repaints a part of itself. */
function applySub() {
  if (!content) return;
  for (const el of content.querySelectorAll(".sub-off")) el.classList.remove("sub-off");
  for (const el of content.querySelectorAll(".set-subpage.on")) el.classList.remove("on");
  if (!openSub) { delete content.dataset.sub; return; }
  const page = content.querySelector(`[data-subpage="${CSS.escape(openSub)}"]`);
  if (!page) { openSub = null; delete content.dataset.sub; return; }
  content.dataset.sub = openSub;
  page.classList.add("on");
  for (let el = page; el && el !== content; el = el.parentElement) {
    for (const sib of el.parentElement.children) if (sib !== el) sib.classList.add("sub-off");
  }
}

async function closeSub() {
  const key = openSub;
  if (!key || !(await (guards.get(key) ?? (() => true))())) return;
  openSub = null;
  applySub();
  content.querySelector(`[data-open-sub="${CSS.escape(key)}"]`)?.focus();
}

// ---------- delegated behaviour, installed once per window ----------

const segButtons = (g) => [...g.querySelectorAll('[role="radio"]')];
function pick(g, button, fire = true) {
  for (const b of segButtons(g)) { const on = b === button; b.setAttribute("aria-checked", String(on)); b.tabIndex = on ? 0 : -1; }
  if (fire) g.dispatchEvent(new Event("change", { bubbles: true }));
}
/** A segmented group reads and writes like a select: `value` is the checked choice. */
function upgradeSeg(g) {
  if (Object.hasOwn(g, "value")) return;
  Object.defineProperty(g, "value", {
    get: () => g.querySelector('[aria-checked="true"]')?.dataset.value ?? "",
    set: (v) => { const b = segButtons(g).find((x) => x.dataset.value === String(v)); pick(g, b, false); if (!b && segButtons(g)[0]) segButtons(g)[0].tabIndex = 0; },
  });
}

function closePops(except = null) {
  for (const b of content?.querySelectorAll('.set-info[aria-expanded="true"]') ?? []) {
    if (b === except) continue;
    b.setAttribute("aria-expanded", "false");
    document.getElementById(b.getAttribute("aria-controls"))?.setAttribute("hidden", "");
  }
}

/** Installs the delegated handlers on the page area; `onEscape` runs when nothing in the page took `Esc`. */
export function initRows(area) {
  content = area;
  const upgrade = () => { for (const g of area.querySelectorAll(".seg")) upgradeSeg(g); };
  new MutationObserver(() => { upgrade(); if (openSub) applySub(); }).observe(area, { childList: true, subtree: true });
  area.addEventListener("click", (e) => {
    const radio = e.target.closest('.seg [role="radio"]');
    if (radio && !radio.disabled && radio.getAttribute("aria-checked") !== "true") { upgrade(); pick(radio.closest(".seg"), radio); }
    const info = e.target.closest(".set-info");
    if (info) {
      const open = info.getAttribute("aria-expanded") !== "true";
      closePops(info);
      info.setAttribute("aria-expanded", String(open));
      document.getElementById(info.getAttribute("aria-controls"))?.toggleAttribute("hidden", !open);
    } else if (!e.target.closest(".set-pop")) closePops();
    const opener = e.target.closest("[data-open-sub]");
    if (opener) {
      openSub = opener.dataset.openSub;
      applySub();
      content.querySelector(`#${CSS.escape(openSub)}-sub-title`)?.focus({ preventScroll: true });
      content.parentElement.scrollTop = 0;
    }
    if (e.target.closest("[data-sub-back]")) closeSub();
  });
  // DS-CMP-SEG-3: arrow keys move the choice.
  area.addEventListener("keydown", (e) => {
    const radio = e.target.closest?.('.seg [role="radio"]');
    if (radio && ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key)) {
      const all = segButtons(radio.closest(".seg")).filter((b) => !b.disabled);
      const step = e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 1;
      const next = all[(all.indexOf(radio) + step + all.length) % all.length];
      e.preventDefault();
      upgrade();
      next.focus();
      pick(radio.closest(".seg"), next);
    }
  });
  // RF5.2: the open state of each disclosure is remembered (`toggle` does not bubble).
  area.addEventListener("toggle", (e) => {
    const d = e.target;
    if (d.matches?.(".set-disc")) put(DISC_KEY(d.dataset.disc), d.open ? "open" : "closed");
  }, true);
  // RF8.3: Esc closes a popover first, then the sub-page.
  addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || e.defaultPrevented || document.querySelector("dialog[open]")) return;
    if (content.querySelector('.set-info[aria-expanded="true"]')) { closePops(); e.preventDefault(); return; }
    if (openSub) { e.preventDefault(); closeSub(); }
  });
  upgrade();
}
