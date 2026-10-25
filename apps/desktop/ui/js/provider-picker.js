// PRD 0027 RF3: the Add-account catalog picker. Search, ranking, grouping by sign-in method, hints and the
// recommended mode all come from the core (`search_providers`); this module only renders them (DS-CMP-SRCH,
// DS-CMP-CAT, DS-CMP-LIST-6). It never shows an integration-status label (owner rule).
import { esc, icon, logo, searchProviders } from "./lib.js";
import { currentLocale, t, te, tl } from "./i18n.js";

let seq = 0;

/** The picker's markup (`labelledBy`: the id of its visible label); `mountPicker` wires it. */
export const pickerHtml = (labelledBy) => `<div class="picker" id="picker">
    <div class="detect-ask" id="detect-ask" hidden>
      <p>${te("dialog.detect.ask")}</p>
      <div class="dlg-actions left"><button type="button" class="btn sm" data-detect="1">${te("dialog.detect.check")}</button>
        <button type="button" class="btn sm" data-detect="0">${te("dialog.detect.notNow")}</button></div>
    </div>
    <div class="filter picker-search"><span class="filter-ico">${icon("search")}</span>
      <input type="search" id="picker-q" role="combobox" aria-expanded="true" aria-controls="picker-list" aria-autocomplete="list"
        autocomplete="off" spellcheck="false" placeholder="${te("dialog.picker.search")}" aria-label="${te("dialog.picker.search")}" />
      <button type="button" class="icon" id="picker-clear" aria-label="${te("dialog.picker.clear")}" hidden>${icon("x")}</button></div>
    <p class="sr-only" id="picker-count" role="status" aria-live="polite"></p>
    <div class="picker-list" id="picker-list" role="listbox" aria-labelledby="${labelledBy}"></div>
    <div class="picker-empty" id="picker-empty" hidden><p class="hint" id="picker-empty-text"></p>
      <button type="button" class="btn outline sm" id="picker-request">${te("dialog.picker.request")}</button></div>
    <p class="picker-foot" id="picker-foot"></p>
  </div>`;

/** The grouped catalog shows this many providers; the rest are found by searching (mockup 31). */
export const FEATURED = 9;

/** Chips of one entry: "Billed per query" (DS-CMP-BADGE-2, warning) and "Found on this device" (DS-CMP-CAT-2, info). */
export const entryChips = (entry, found) =>
  (entry.metered ? `<span class="mini warn">${icon("warn")}${te("dialog.picker.metered")}</span>` : "")
  + (found ? `<span class="mini info">${icon("check")}${te("dialog.picker.found")}</span>` : "");

/**
 * Wires the picker inside `root`.
 * - `onChoose(entry, detected | null)` runs on click or Enter.
 * - `detected()` returns the current detected sources (core `Detected`), `onDismiss(kind)` forgets one.
 * - `onRequest()` opens the new-provider issue template.
 * Returns `{ refresh, focus }`.
 */
export function mountPicker(root, { onChoose, detected = () => [], onDismiss = () => {}, onRequest = () => {}, onExtensions = () => {} }) {
  const input = root.querySelector("#picker-q");
  const clear = root.querySelector("#picker-clear");
  const list = root.querySelector("#picker-list");
  const empty = root.querySelector("#picker-empty");
  const count = root.querySelector("#picker-count");
  let entries = [];
  let total = 0;
  const foot = root.querySelector("#picker-foot");
  let active = 0;
  const foundFor = (provider) => detected().find((d) => d.provider === provider) ?? null;

  const option = (entry, i) => {
    const found = foundFor(entry.provider);
    const method = tl(entry.method_label);
    const hint = tl(entry.hint);
    return `<div role="option" class="pick" id="pick-${i}" data-i="${i}" aria-selected="${i === active}">
      ${logo(entry.provider, 32)}
      <span class="pick-text"><span class="pick-name"><b>${esc(entry.label)}</b><span class="pick-chips">${entryChips(entry, found)}${found
          ? `<button type="button" class="link-btn" data-dismiss="${esc(found.kind)}">${te("dialog.detect.dismiss")}</button>` : ""}</span></span>
        <small class="pick-method">${esc(method)}${hint !== method ? ` · <span class="pick-hint">${esc(hint)}</span>` : ""}</small></span>
      <span class="btn outline sm pick-connect" aria-hidden="true">${te("dialog.picker.connect")}</span>
    </div>`;
  };

  function paint() {
    const query = input.value;
    const grouped = query.trim() === "";
    clear.hidden = grouped;
    // DS-CMP-CAT-1: search results are always a list; the grouped catalog is a grid when wide enough.
    list.classList.toggle("is-grouped", grouped);
    empty.hidden = entries.length > 0;
    root.querySelector("#picker-empty-text").textContent = t("dialog.picker.empty", { query });
    count.textContent = entries.length ? t("dialog.picker.count", { count: entries.length }) : t("dialog.picker.empty", { query });
    if (!grouped) {
      list.innerHTML = entries.map(option).join("");
    } else {
      // DS-CMP-LIST-6: one group per sign-in method, in the core's order.
      const groups = [];
      entries.forEach((entry, i) => {
        const last = groups.at(-1);
        if (last?.method === entry.method) last.items.push([entry, i]);
        else groups.push({ method: entry.method, label: tl(entry.method_label), items: [[entry, i]] });
      });
      list.innerHTML = groups.map((g, gi) => `<div role="group" class="pick-group" aria-labelledby="pick-g-${gi}">
          <div class="pick-group-label" id="pick-g-${gi}" role="presentation">${esc(g.label)}</div>
          <div class="pick-items">${g.items.map(([entry, i]) => option(entry, i)).join("")}</div></div>`).join("");
    }
    // Mockup 31/31b: the footer counts what the grouped view hides and always offers an extension.
    const hidden = grouped ? total - entries.length : 0;
    const link = `<a href="#" class="text-link" data-extensions>${te("dialog.picker.addExtension")}</a>`;
    foot.innerHTML = hidden > 0
      ? `${esc(t("dialog.picker.more", { count: hidden }))} ${link}`
      : (entries.length ? `${te("dialog.picker.notListed")} ${link}` : "");
    syncActive(false);
  }

  function syncActive(scroll = true) {
    for (const el of list.querySelectorAll(".pick")) el.setAttribute("aria-selected", String(Number(el.dataset.i) === active));
    const el = list.querySelector(`#pick-${active}`);
    if (el) {
      input.setAttribute("aria-activedescendant", el.id);
      if (scroll) el.scrollIntoView({ block: "nearest" });
    } else input.removeAttribute("aria-activedescendant");
  }

  async function refresh() {
    const mine = ++seq;
    const result = await searchProviders(input.value, currentLocale());
    if (mine !== seq) return; // a newer keystroke won
    total = result.length;
    entries = input.value.trim() ? result : result.slice(0, FEATURED);
    // Mockup 31: the empty search names how many providers it covers.
    if (!input.value.trim()) input.placeholder = t("dialog.picker.searchCount", { count: result.length });
    active = 0;
    paint();
  }

  const choose = (i) => { const entry = entries[i]; if (entry) onChoose(entry, foundFor(entry.provider)); };

  input.addEventListener("input", refresh);
  clear.addEventListener("click", () => { input.value = ""; refresh(); input.focus(); });
  root.querySelector("#picker-request").addEventListener("click", onRequest);
  foot.addEventListener("click", (e) => { if (e.target.closest("[data-extensions]")) { e.preventDefault(); onExtensions(); } });
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!entries.length) return;
      active = (active + (e.key === "ArrowDown" ? 1 : -1) + entries.length) % entries.length;
      syncActive();
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(active);
    } else if (e.key === "Escape" && input.value) {
      // RF3.2: Esc clears first; the next Esc closes the dialog.
      e.preventDefault();
      e.stopPropagation();
      input.value = "";
      refresh();
    }
  });
  list.addEventListener("click", (e) => {
    const dismiss = e.target.closest("[data-dismiss]");
    if (dismiss) { e.stopPropagation(); onDismiss(dismiss.dataset.dismiss); return; }
    const el = e.target.closest(".pick");
    if (el) choose(Number(el.dataset.i));
  });
  // RF3.2: typing anywhere in the picker focuses the search.
  root.addEventListener("keydown", (e) => {
    if (e.target === input || e.ctrlKey || e.metaKey || e.altKey || e.key.length !== 1) return;
    if (e.target.closest("button, input, select, textarea")) return;
    input.focus();
  });
  return { refresh: () => paint(), reload: refresh, focus: () => input.focus() };
}
