// Settings › Dashboard (PRD 0020 RF2.5, PRD 0025 RF5.2/RF9): the accounts (add, edit, remove and order; the dialogs
// stay in the main window, which owns them), the sort, the tray view, the row display, the "Recharging" group,
// consumption pace and the refresh interval.
import { t, te } from "../i18n.js";
import { accountLabel, brand, esc, getView, logo, moveAccount, requestAccountAction, adviceShown, currentSort, groupingRecharging, icon, setAdviceEnabled, setGroupRecharging, setPaceUnit, setRefreshSeconds, setShowPace, setSort, setTrayView, TRAY_VIEWS, trayViewOf } from "../lib.js";
import { errorText, group, l10nError } from "./common.js";
import { commitField, disc, num, row, rowError, select, seg, sw, toast } from "./rows.js";
import { AGG_MODES, aggregatorsInAll, setAggregatorsInAll } from "../aggregator.js";
import { bind as bindRows, rowsHtml } from "./rowdisplay.js";

const PRESETS = [[60, "1 min"], [300, "5 min"], [900, "15 min"], [3600, "1 h"]];

export function html({ settings: s, view }) {
  const seconds = view()?.refresh_seconds || 300;
  return group("accounts", te("settings.accounts.title"),
    `<div data-accounts><p class="hint">${te("common.loading")}</p></div><p class="err" data-accounts-err role="alert"></p>`
    + `<div class="dlg-actions left"><button type="button" class="btn" data-account-add>${icon("plus")} ${te("settings.accounts.add")}</button></div>`)
    + group("order", te("settings.order.title"),
    row({ key: "order", label: te("settings.order.binding"), info: te("settings.order.help"),
      control: (a) => sw("order-binding", currentSort() === "binding", a) }))
    + group("display", te("settings.display.title"),
      // PRD 0022 RF8.1 (DS-CMP-SHELL-5): the tray view; the list is the default, Rings and Compact are alternatives.
      row({ key: "trayview", label: te("settings.trayView.label"), info: te("settings.trayView.desc"), msg: "trayview-msg",
        body: optionCards("tray-view", TRAY_VIEWS.map((v) => [v, te(`settings.trayView.${v}`), VIEW_PREVIEW[v]]), trayViewOf(s.tray?.popover_view), `aria-labelledby="trayview-label"`) })
      + rowsHtml(s.rows)
      // PRD 0036 RF8.3, PRD 0024 RF1.3/RF7.5: the rarer display choices sit under a remembered disclosure (RF2.2).
      + disc("display-more", te("settings.display.more"), ""
      + row({ key: "grouping", label: te("settings.group.recharging"), info: te("settings.group.help"),
        control: (a) => sw("group-recharging", groupingRecharging(), a) })
      + row({ key: "pace", label: te("settings.pace.show"), info: te("settings.pace.help"), msg: "pace-msg",
        control: (a) => sw("show-pace", s.show_pace !== false, a) })
      // PRD 0036 RF8.3: how the popover's "All" shows 9router connections.
      + row({ key: "agg-in-all", label: te("settings.agg.all"), info: te("settings.agg.allHelp"),
        control: (a) => select("agg-in-all", AGG_MODES.map((m) => [m, te(`settings.agg.all.${m}`)]), aggregatorsInAll(), a) })
      + row({ key: "pace-unit", label: te("settings.pace.unit"), info: te("settings.pace.unitHelp"), msg: "pace-unit-msg",
        // The example lives in the info popover (PRD 0020 RF5.1 copy budget).
        control: (a) => seg("pace-unit", [["points", te("settings.pace.points")], ["percent", te("settings.pace.percent")]], s.pace_unit ?? "points", a) })
      + row({ key: "advice", label: te("settings.advice.show"), info: te("settings.advice.help"), msg: "advice-msg",
        control: (a) => sw("advice-on", s.advice_enabled !== false, a) })))
    + group("refresh", te("settings.refresh.title"),
      row({ key: "refresh", label: te("settings.refresh.label"), desc: te("settings.refresh.min"), info: te("settings.refresh.help"), msg: "refresh-msg",
        control: (a) => num("refresh-seconds", { value: seconds, min: 30, unit: "s", apply: "data-apply-refresh" }, a),
        body: seg("refresh-presets", PRESETS.map(([sec, label]) => [sec, label, `data-preset="${sec}"`]), seconds, `aria-label="${te("settings.refresh.presets")}"`) }));
}

/** PRD 0025 RF5.2: the configured accounts in their order (the TOML order, shared with the TUI), each with Edit and
 *  Remove; the handle moves it with Alt+↑/↓ (DS-CMP-DRAG-2). 9router connections are not accounts of this list. */
export function accountsHtml(reports) {
  const list = reports.filter((r) => Number.isInteger(r.index) && r.index >= 0 && !r.connection).sort((a, b) => a.index - b.index);
  if (!list.length) return `<p class="hint">${te("settings.accounts.none")}</p>`;
  return `<ul class="mb-list acc-list" aria-label="${te("settings.accounts.title")}">${list.map((r, i) => {
    const name = accountLabel(r);
    return `<li class="mb-acc" data-index="${r.index}">
      <button type="button" class="icon mb-grip" data-acc-grip aria-label="${te("settings.menubar.move", { name })}" title="${te("settings.menubar.move.hint")}" aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown" data-pos="${i}">${icon("grip")}</button>
      ${logo(r.provider, 28)}
      <span class="mb-acc-text"><span class="mb-acc-name">${esc(name)}</span><span class="mb-acc-provider">${esc(brand(r.provider).label)}</span></span>
      <button type="button" class="icon" data-account-edit="${r.index}" aria-label="${te("settings.accounts.edit", { name })}" title="${te("common.edit")}">${icon("edit")}</button>
      <button type="button" class="icon danger" data-account-remove="${r.index}" aria-label="${te("settings.accounts.remove", { name })}" title="${te("common.remove")}">${icon("trash")}</button></li>`;
  }).join("")}</ul>`;
}

/** DS-CMP-CHK-3 option cards: a static preview on top, the label below; one radio group with arrow keys. */
const VIEW_PREVIEW = {
  list: `<span class="pv pv-list" aria-hidden="true"><i></i><i></i><i></i></span>`,
  rings: `<span class="pv pv-rings" aria-hidden="true">${'<b></b>'.repeat(3)}</span>`,
  compact: `<span class="pv pv-compact" aria-hidden="true">${'<i></i>'.repeat(5)}</span>`,
};
const optionCards = (id, options, value, a) =>
  `<div class="opt-cards" role="radiogroup" id="${id}" ${a}>${options.map(([v, label, preview]) =>
    `<button type="button" role="radio" class="opt-card" data-value="${v}" aria-checked="${v === value}" tabindex="${v === value ? 0 : -1}">${preview}<span>${label}</span>${v === value ? icon("tick") : ""}</button>`).join("")}</div>`;

export function bind(root, ctx) {
  // PRD 0025 RF5.2: account management; the order moves through the core (`move_account`), like the main window.
  const box = root.querySelector("[data-accounts]");
  const err = root.querySelector("[data-accounts-err]");
  const repaint = async (focusIndex = null) => {
    try { ctx.setView(await getView()); } catch {}
    box.innerHTML = accountsHtml(ctx.view()?.reports ?? []);
    if (focusIndex != null) box.querySelector(`[data-index="${focusIndex}"] [data-acc-grip]`)?.focus();
  };
  box.innerHTML = accountsHtml(ctx.view()?.reports ?? []);
  root.querySelector("[data-account-add]").onclick = () => requestAccountAction("add");
  box.addEventListener("click", (e) => {
    const edit = e.target.closest("[data-account-edit]");
    if (edit) return requestAccountAction("edit", Number(edit.dataset.accountEdit));
    const remove = e.target.closest("[data-account-remove]");
    if (remove) return requestAccountAction("remove", Number(remove.dataset.accountRemove));
  });
  box.addEventListener("keydown", async (e) => {
    const grip = e.target.closest?.("[data-acc-grip]");
    if (!grip || !["ArrowUp", "ArrowDown"].includes(e.key)) return;
    e.preventDefault();
    const items = [...box.querySelectorAll(".mb-acc")].map((li) => Number(li.dataset.index));
    const at = Number(grip.dataset.pos);
    const to = at + (e.key === "ArrowUp" ? -1 : 1);
    if (to < 0 || to >= items.length) return;
    try {
      await moveAccount(items[at], items[to]);
      err.textContent = "";
      toast();
      await repaint(items[to]);
    } catch (x) { err.textContent = errorText(x); }
  });
  bindRows(root, ctx);
  // PRD 0022 RF8.1: the tray view saves on choice (desktop.json) and the tray repaints at once.
  const cards = root.querySelector("#tray-view");
  const choose = (button) => {
    const value = button.dataset.value;
    setTrayView(value).then(() => {
      for (const b of cards.querySelectorAll("[role=radio]")) {
        const on = b === button;
        b.setAttribute("aria-checked", on);
        b.tabIndex = on ? 0 : -1;
        b.querySelector("svg.i")?.remove();
        if (on) b.insertAdjacentHTML("beforeend", icon("tick"));
      }
      rowError(cards, "");
      toast();
      ctx.changed("tray_view");
    }, (x) => rowError(cards, errorText(x)));
  };
  cards.addEventListener("click", (e) => { const b = e.target.closest("[role=radio]"); if (b) choose(b); });
  cards.addEventListener("keydown", (e) => {
    const list = [...cards.querySelectorAll("[role=radio]")];
    const i = list.indexOf(document.activeElement);
    const next = { ArrowLeft: i - 1, ArrowUp: i - 1, ArrowRight: i + 1, ArrowDown: i + 1 }[e.key];
    if (i < 0 || next === undefined) return;
    e.preventDefault();
    const b = list[(next + list.length) % list.length];
    b.focus();
    choose(b);
  });
  // PRD 0009 RF2.3: display-only ordering; the manual order (TOML, shared with the TUI) is never rewritten.
  root.querySelector("#order-binding").onchange = (e) => { setSort(e.target.checked ? "binding" : "custom"); toast(); };
  root.querySelector("#group-recharging").onchange = (e) => { setGroupRecharging(e.target.checked); toast(); };
  // PRD 0036 RF8.3: how the popover's "All" shows 9router connections (a per-device display choice).
  root.querySelector("#agg-in-all").onchange = (e) => setAggregatorsInAll(e.target.value).then(
    () => { rowError(e.target, ""); toast(); ctx.changed("aggregators_in_all"); },
    (x) => { e.target.value = aggregatorsInAll(); rowError(e.target, errorText(x)); });
  // PRD 0007 RF3.4: saved in desktop.json; on failure the switch goes back to the saved value.
  root.querySelector("#show-pace").onchange = (e) => setShowPace(e.target.checked).then(
    () => { rowError(e.target, ""); toast(); ctx.changed("show_pace"); },
    (x) => { e.target.checked = !e.target.checked; rowError(e.target, errorText(x)); e.target.focus(); });
  // PRD 0024 RF1.3: the unit is the user's choice; saved on change, put back when refused.
  const unit = root.querySelector("#pace-unit");
  let savedUnit = unit.value;
  unit.addEventListener("change", () => setPaceUnit(unit.value).then(
    () => { savedUnit = unit.value; rowError(unit, ""); toast(); ctx.changed("pace_unit"); },
    (x) => { unit.value = savedUnit; rowError(unit, errorText(x)); }));
  // PRD 0024 RF7.5: advisory only; saved at once, put back when refused.
  root.querySelector("#advice-on").onchange = (e) => setAdviceEnabled(e.target.checked).then(
    () => { rowError(e.target, ""); toast(); ctx.changed("advice"); },
    (x) => { e.target.checked = !e.target.checked; rowError(e.target, errorText(x)); e.target.focus(); });
  // RF4.2: the interval saves on Enter, on leaving the field or with the row's Apply; a preset saves at once.
  const input = root.querySelector("#refresh-seconds");
  const presets = root.querySelector("#refresh-presets");
  const field = commitField(input, (value) => setRefreshSeconds(Number(value)).then(() => ctx.changed("refresh_seconds"), (e) => { throw l10nError(e); }),
    { onSaved: (value) => { presets.value = value; toast(t("settings.refresh.saved")); } });
  presets.addEventListener("change", () => field.set(presets.value).then(() => { presets.value = input.value; }));
  input.addEventListener("input", () => { presets.value = input.value; });
}

/** A new snapshot (an account added, edited, removed or moved in any window) redraws the account list in place. */
const accountsSig = (v) => (v?.reports ?? []).map((r) => `${r.index}:${r.provider}:${r.account}`).join("|");
export function viewChanged(view, prev) {
  const box = document.querySelector("[data-accounts]");
  if (box && accountsSig(view) !== accountsSig(prev)) box.innerHTML = accountsHtml(view?.reports ?? []);
  return false;
}
