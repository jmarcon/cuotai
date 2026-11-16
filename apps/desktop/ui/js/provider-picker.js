// PRD 0027 RF3: the Add-account catalog picker. Search, ranking, grouping by sign-in method, hints and the
// recommended mode all come from the core (`search_providers`); this module only renders them (DS-CMP-SRCH,
// DS-CMP-CAT, DS-CMP-LIST-6). It never shows an integration-status label (owner rule).
import { esc, icon, logo, pickerHiddenNotice, searchProviders } from "./lib.js";
import { currentLocale, t, te, tl } from "./i18n.js";

let seq = 0;
/** The right-hand note of a group header (mockup 31). */
const GROUP_HINTS = { browser_sign_in: "dialog.picker.hint.browser", api_key: "dialog.picker.hint.apiKey" };

/** The picker's markup (`labelledBy`: the id of its visible label); `mountPicker` wires it. */
export const pickerHtml = (labelledBy) => `<div class="picker" id="picker">
    <div class="filter picker-search"><span class="filter-ico">${icon("search")}</span>
      <input type="search" id="picker-q" role="combobox" aria-expanded="true" aria-controls="picker-list" aria-autocomplete="list"
        autocomplete="off" spellcheck="false" maxlength="256" placeholder="${te("dialog.picker.search")}" aria-label="${te("dialog.picker.search")}" />
      <button type="button" class="icon" id="picker-clear" aria-label="${te("dialog.picker.clear")}" hidden>${icon("x")}</button></div>
    <div class="detect-ask" id="detect-ask" hidden>
      <p>${te("dialog.detect.ask")}</p>
      <div class="dlg-actions left"><button type="button" class="btn sm" data-detect="1">${te("dialog.detect.check")}</button>
        <button type="button" class="btn sm" data-detect="0">${te("dialog.detect.notNow")}</button></div>
    </div>
    <p class="sr-only" id="picker-count" role="status" aria-live="polite"></p>
    <div class="picker-list" id="picker-list" role="listbox" tabindex="-1" aria-labelledby="${labelledBy}"></div>
    <div class="picker-empty" id="picker-empty" hidden><p class="hint" id="picker-empty-text"></p>
      <button type="button" class="btn outline sm" id="picker-request">${te("dialog.picker.request")}</button></div>
    <p class="picker-foot" id="picker-foot"></p>
  </div>`;

/** Chips of one entry: "Billed per query" (DS-CMP-BADGE-2, warning) and "Found on this device" (DS-CMP-CAT-2, info). */
export const entryChips = (entry, found) =>
  (entry.metered ? `<span class="mini warn">${icon("warn")}${te("dialog.picker.metered")}</span>` : "")
  + (found ? `<span class="mini info"${found.hint ? ` title="${esc(tl(found.hint))}"` : ""}>${icon("check")}${te("dialog.picker.found")}</span>` : "");

/**
 * One result row (`role="option"`). `grouped`: the empty-query catalog, whose group header already names the method,
 * so the subtitle is the hint only; a search result reads "method · hint". A found source adds its hint (C-12). The row
 * holds no control: focus stays in the search field (`aria-activedescendant`), so "Don't suggest this again" lives next
 * to the consent line of the chosen form instead (PRD 0027 re-validation C-01).
 */
export function pickOption(entry, i, { active = -1, found = null, grouped = false } = {}) {
  const method = tl(entry.method_label);
  const hint = tl(entry.hint);
  const parts = grouped ? [hint] : [method, ...(hint !== method ? [hint] : [])];
  if (found?.hint) parts.push(tl(found.hint));
  const subtitle = parts.map((p, k) => (k === 0 && !grouped ? esc(p) : `<span class="pick-hint">${esc(p)}</span>`)).join(" · ");
  return `<div role="option" class="pick" id="pick-${i}" data-i="${i}" data-provider="${esc(entry.provider)}" aria-selected="${i === active}">
      ${logo(entry.provider, 32)}
      <span class="pick-text"><span class="pick-name"><b>${esc(entry.label)}</b><span class="pick-chips">${entryChips(entry, found)}</span></span>
        <small class="pick-method">${subtitle}</small></span>
      <span class="btn outline sm pick-connect" aria-hidden="true">${te("dialog.picker.connect")}</span>
    </div>`;
}

/** The footer: "Not listed? {link}" as one sentence (C-11), the link opening the extensions. */
export const pickerFoot = () => te("dialog.picker.notListed").split("{link}")
  .join(`<a href="#" class="text-link" data-extensions>${te("dialog.picker.addExtension")}</a>`);

/**
 * Wires the picker inside `root`.
 * - `onChoose(entry, detected | null)` runs on click or Enter.
 * - `detected()` returns the current detected sources (core `Detected`).
 * - `onRequest()` opens the new-provider issue template.
 * Returns `{ refresh, focus }`.
 */
export function mountPicker(root, { onChoose, detected = () => [], onRequest = () => {}, onExtensions = () => {} }) {
  const input = root.querySelector("#picker-q");
  const clear = root.querySelector("#picker-clear");
  const list = root.querySelector("#picker-list");
  const empty = root.querySelector("#picker-empty");
  const count = root.querySelector("#picker-count");
  let entries = [];
  // V-02 (PRD 0005 review): the core's reason when the query names a provider the picker leaves out.
  let hidden = null;
  const foot = root.querySelector("#picker-foot");
  let active = -1; // no row is highlighted on open (mockup 31); the first arrow key or a query selects one
  const foundFor = (provider) => detected().find((d) => d.provider === provider) ?? null;

  // C-01: always map with an arrow; passing `option` itself would hand it the array as `grouped`.
  const option = (entry, i, grouped = false) => pickOption(entry, i, { active, found: foundFor(entry.provider), grouped });

  function paint() {
    const query = input.value;
    const grouped = query.trim() === "";
    clear.hidden = grouped;
    // DS-CMP-CAT-1: a list, grouped by sign-in method while the search is empty.
    list.classList.toggle("is-grouped", grouped);
    empty.hidden = entries.length > 0;
    const emptyText = hidden ? tl(hidden) : t("dialog.picker.empty", { query });
    root.querySelector("#picker-empty-text").textContent = emptyText;
    // A provider left out on purpose is not a request to make.
    root.querySelector("#picker-request").hidden = Boolean(hidden);
    count.textContent = entries.length ? t("dialog.picker.count", { count: entries.length }) : emptyText;
    if (!grouped) {
      list.innerHTML = entries.map((entry, i) => option(entry, i)).join("");
    } else {
      // DS-CMP-LIST-6: one group per sign-in method, in the core's order.
      const groups = [];
      entries.forEach((entry, i) => {
        const last = groups.at(-1);
        if (last?.method === entry.method) last.items.push([entry, i]);
        else groups.push({ method: entry.method, label: tl(entry.method_label), items: [[entry, i]] });
      });
      list.innerHTML = groups.map((g, gi) => `<div role="group" class="pick-group" aria-labelledby="pick-g-${gi}">
          <div class="pick-group-label" id="pick-g-${gi}" role="presentation">${esc(g.label)}${GROUP_HINTS[g.method]
            ? `<span class="pick-group-hint">${te(GROUP_HINTS[g.method])}</span>` : ""}</div>
          <div class="pick-items">${g.items.map(([entry, i]) => option(entry, i, true)).join("")}</div></div>`).join("");
    }
    // Mockup 31b: the footer always offers an extension.
    foot.innerHTML = entries.length ? pickerFoot() : "";
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
    const notice = !result.length && input.value.trim() ? await pickerHiddenNotice(input.value, currentLocale()).catch(() => null) : null;
    if (mine !== seq) return;
    // The empty search lists the whole catalog, grouped; typing narrows it (owner decision 2026-10-26).
    entries = result;
    hidden = notice;
    // Mockup 31: the empty search names how many providers it covers.
    if (!input.value.trim()) input.placeholder = t("dialog.picker.searchCount", { count: result.length });
    active = input.value.trim() ? 0 : -1;
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
      active = active < 0 ? (e.key === "ArrowDown" ? 0 : entries.length - 1) : (active + (e.key === "ArrowDown" ? 1 : -1) + entries.length) % entries.length;
      syncActive();
    } else if (e.key === "Enter") {
      // C-12: with nothing highlighted, Enter picks nothing.
      e.preventDefault();
      if (active >= 0) choose(active);
    } else if (e.key === "Escape" && input.value) {
      // RF3.2: Esc clears first; the next Esc closes the dialog.
      e.preventDefault();
      e.stopPropagation();
      input.value = "";
      refresh();
    }
  });
  list.addEventListener("click", (e) => {
    const el = e.target.closest(".pick");
    if (el) choose(Number(el.dataset.i));
  });
  // RF3.2: typing anywhere in the picker focuses the search.
  root.addEventListener("keydown", (e) => {
    if (e.target === input || e.ctrlKey || e.metaKey || e.altKey || e.key.length !== 1) return;
    if (e.target.closest("button, input, select, textarea")) return;
    input.focus();
  });
  // PRD 0040 RF8.1: the recommended entry of a provider, for an Add account that names it.
  const entryFor = (provider) => entries.find((e) => e.provider === provider && e.recommended) ?? entries.find((e) => e.provider === provider);
  return { refresh: () => paint(), reload: refresh, focus: () => input.focus(), entryFor };
}
