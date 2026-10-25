// Settings › Menu bar (PRD 0025 RF4): the four styles as option cards (DS-CMP-CHK-3) with a live preview each
// (DS-CMP-PREV-1), the colored variant (RF3.6) and the accounts the menu bar shows, in their own order
// (DS-CMP-CHK-1, DS-CMP-DRAG, "Not in menu bar" per DS-CMP-LIST-3). Figures, levels, marks and the width budget come
// from the core through `tray_preview`; this page only lays them out and saves the choice.
import { t, te, tl } from "../i18n.js";
import { esc, icon, setTrayPrefs, trayAccounts, trayPreview } from "../lib.js";
import { errorText, group } from "./common.js";
import { check, row, rowError, sw, toast } from "./rows.js";

export const STYLES = ["tightest_percent", "logos_percent", "meters", "icon_only"];

const kw = () => `data-keywords="${te("settings.menubar.keywords")}"`;

export function html({ settings: s }) {
  const tray = s.tray ?? {};
  const style = STYLES.includes(tray.style) ? tray.style : "tightest_percent";
  const cards = `<div class="seg opt-cards" role="radiogroup" id="tray-style" aria-labelledby="tray-style-label">${STYLES.map((v) => {
    const on = v === style;
    return `<button type="button" role="radio" data-value="${v}" aria-checked="${on}" tabindex="${on ? 0 : -1}">
      <span class="opt-preview" data-tray-preview="${v}" aria-hidden="true"></span><span class="opt-label">${te(`settings.menubar.style.${v}`)}</span></button>`;
  }).join("")}</div>`;
  return group("menubar-style", te("settings.menubar.style"),
    row({ key: "tray-style", label: te("settings.menubar.style"), info: te("settings.menubar.style.help"), attrs: kw(), body: cards })
    + `<div class="prev-card" data-tray-live role="img" aria-label=""><span class="prev-label">${te("settings.menubar.preview")}</span><span class="prev-out" data-tray-live-out></span></div>`
    + row({ key: "tray-colored", label: te("settings.menubar.colored"), info: te("settings.menubar.colored.help"), attrs: kw(),
      control: (a) => sw("tray-colored", !!tray.colored, a) }))
    + group("menubar-accounts", te("settings.menubar.accounts"),
      row({ key: "tray-auto", label: te("settings.menubar.auto"), info: te("settings.menubar.auto.help"), attrs: kw(),
        control: (a) => sw("tray-auto", tray.accounts == null, a) })
      + `<div data-tray-accounts><p class="hint">${te("common.loading")}</p></div>`);
}

/** RF4.1: the preview of one style: the title as system text beside the mark, or continuous meters. */
export function previewHtml(summary, colored) {
  if (!summary) return "";
  if (summary.style === "meters") {
    const bars = summary.items.length ? summary.items : [{ remaining: null }];
    return `<span class="mb-meters${colored ? " colored" : ""}">${bars.map((i) => {
      const pct = i.remaining == null ? 0 : Math.max(i.remaining > 0 ? 6 : 0, Math.round(i.remaining * 100));
      const cls = i.exhausted ? "out" : i.remaining == null ? "unknown" : i.level ?? "";
      return `<span class="mb-bar ${cls}"><span style="width:${i.exhausted ? 0 : Math.min(pct, i.remaining < 1 ? 99 : 100)}%"></span></span>`;
    }).join("")}</span>`;
  }
  const badge = summary.badge && summary.badge !== "none" ? `<span class="mb-badge ${summary.badge}"></span>` : "";
  return `<span class="mb-item${colored ? " colored" : ""}"><span class="mb-mark">${icon("gauge")}${badge}</span>${summary.title ? `<span class="mb-title">${esc(summary.title)}</span>` : ""}</span>`;
}

/** RF4.2: the account list: the menu-bar accounts in their order (with handles), then "Not in menu bar". */
export function accountsHtml(accounts, selected, auto) {
  if (!accounts.length) return `<p class="hint">${te("settings.menubar.none")}</p>`;
  const inBar = selected.map((k) => accounts.find((a) => a.key === k)).filter(Boolean);
  const rest = accounts.filter((a) => !selected.includes(a.key));
  const item = (a, on) => `<li class="mb-acc" data-key="${esc(a.key)}" ${on && !auto ? 'draggable="true"' : ""}>
    ${on && !auto ? `<button type="button" class="icon mb-grip" data-grip aria-label="${te("settings.menubar.move", { name: a.name })}" title="${te("settings.menubar.move.hint")}">${icon("grip")}</button>` : '<span class="mb-grip-gap"></span>'}
    ${check(`data-tray-pick="${esc(a.key)}" aria-label="${te("settings.menubar.pick", { name: a.name })}"`, on)}
    <span class="mb-acc-name">${esc(a.name)}</span><span class="mb-acc-provider">${esc(a.provider_label)}</span></li>`;
  return `<ul class="mb-list" aria-label="${te("settings.menubar.accounts")}">${inBar.map((a) => item(a, true)).join("")}</ul>
    ${rest.length ? `<p class="mb-sub">${te("settings.menubar.not_in")}</p><ul class="mb-list">${rest.map((a) => item(a, false)).join("")}</ul>` : ""}
    ${inBar.length > 1 && !auto ? `<p class="hint">${te("settings.menubar.drag")}</p>` : ""}`;
}

/** Moves `key` one place up (`-1`) or down in `order`; the same array when it cannot move. */
export function moved(order, key, step) {
  const at = order.indexOf(key);
  const to = at + step;
  if (at < 0 || to < 0 || to >= order.length) return order;
  const next = [...order];
  [next[at], next[to]] = [next[to], next[at]];
  return next;
}

export function bind(root, ctx) {
  const tray = ctx.settings.tray ?? {};
  const state = {
    style: STYLES.includes(tray.style) ? tray.style : "tightest_percent",
    colored: !!tray.colored,
    accounts: tray.accounts ?? null,
    popover_view: tray.popover_view ?? null,
  };
  let all = [];
  const box = root.querySelector("[data-tray-accounts]");
  const styleEl = root.querySelector("#tray-style");
  const selected = () => state.accounts ?? autoSelection();
  const autoSelection = () => all.map((a) => a.key).slice(0, state.style === "meters" ? 4 : state.style === "logos_percent" ? 6 : Infinity);

  const paintPreviews = async () => {
    const accounts = state.accounts;
    const out = await Promise.all(STYLES.map((style) => trayPreview(style, accounts).catch(() => null)));
    STYLES.forEach((style, i) => {
      const el = root.querySelector(`[data-tray-preview="${style}"]`);
      if (el) el.innerHTML = previewHtml(out[i], state.colored);
    });
    const live = out[STYLES.indexOf(state.style)];
    const card = root.querySelector("[data-tray-live]");
    if (card) {
      card.querySelector("[data-tray-live-out]").innerHTML = previewHtml(live, state.colored);
      card.setAttribute("aria-label", `${t("settings.menubar.preview")}: ${live ? tl(live.accessible_label) : ""}`);
    }
  };
  const paintAccounts = (focusKey = null, focusSel = "[data-grip]") => {
    box.innerHTML = accountsHtml(all, selected(), state.accounts == null);
    if (focusKey) box.querySelector(`[data-key="${CSS.escape(focusKey)}"] ${focusSel}`)?.focus();
  };
  const save = (el) => setTrayPrefs(state).then(
    () => { rowError(el, ""); toast(); ctx.changed("tray"); paintPreviews(); },
    (x) => rowError(el, errorText(x)));

  styleEl.addEventListener("change", () => { state.style = styleEl.value; paintAccounts(); save(styleEl); });
  root.querySelector("#tray-colored").onchange = (e) => { state.colored = e.target.checked; save(e.target); };
  root.querySelector("#tray-auto").onchange = (e) => {
    // "Automatic" restores Auto; turning it off starts a custom list from what the menu bar shows now.
    state.accounts = e.target.checked ? null : autoSelection();
    paintAccounts();
    save(e.target);
  };
  const setOrder = (order, focusKey, focusSel) => {
    state.accounts = order;
    root.querySelector("#tray-auto").checked = false;
    paintAccounts(focusKey, focusSel);
    save(box);
  };
  box.addEventListener("change", (e) => {
    const key = e.target.dataset?.trayPick;
    if (!key) return;
    const order = selected().filter((k) => k !== key);
    setOrder(e.target.checked ? [...order, key] : order, key, "[data-tray-pick]");
  });
  // DS-CMP-DRAG-2: Alt+↑/↓ (or ↑/↓ on the handle) moves the account.
  box.addEventListener("keydown", (e) => {
    const li = e.target.closest?.(".mb-acc");
    const onGrip = e.target.matches?.("[data-grip]");
    if (!li || !["ArrowUp", "ArrowDown"].includes(e.key) || !(e.altKey || onGrip) || state.accounts == null) return;
    const order = moved(selected(), li.dataset.key, e.key === "ArrowUp" ? -1 : 1);
    e.preventDefault();
    if (order !== selected()) setOrder(order, li.dataset.key, onGrip ? "[data-grip]" : "[data-tray-pick]");
  });
  let dragKey = null;
  box.addEventListener("dragstart", (e) => { dragKey = e.target.closest?.(".mb-acc")?.dataset.key ?? null; e.target.classList?.add("dragging"); });
  box.addEventListener("dragend", () => { dragKey = null; box.querySelectorAll(".drop, .dragging").forEach((el) => el.classList.remove("drop", "dragging")); });
  box.addEventListener("dragover", (e) => {
    const li = e.target.closest?.('.mb-acc[draggable="true"]');
    if (!dragKey || !li) return;
    e.preventDefault();
    box.querySelectorAll(".drop").forEach((el) => el.classList.remove("drop"));
    li.classList.add("drop");
  });
  box.addEventListener("drop", (e) => {
    const target = e.target.closest?.('.mb-acc[draggable="true"]')?.dataset.key;
    if (!dragKey || !target || target === dragKey) return;
    e.preventDefault();
    const order = selected().filter((k) => k !== dragKey);
    order.splice(order.indexOf(target), 0, dragKey);
    setOrder(order, dragKey);
  });

  return trayAccounts().catch(() => []).then((accounts) => {
    all = accounts ?? [];
    paintAccounts();
    return paintPreviews();
  });
}

/** A new snapshot redraws the previews from the current readings (no repaint of the page). */
export const viewChanged = () => false;
