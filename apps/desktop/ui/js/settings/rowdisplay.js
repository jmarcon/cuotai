// Settings › Dashboard › Row display (PRD 0025 RF9, PRD 0022 RF8.2–RF8.3; DS-CMP-DENS-1..5, mockup
// `50-settings-display-custom`): the presets as a segmented choice, a live preview row drawn by the same row
// component as the menu bar (DS-CMP-PREV-1, no animation) and the element switches in the DS-CMP-DENS-1 order.
// Choosing a preset sets the switches; changing a switch selects Custom. One layout serves the menu bar and the main
// window unless "Use a different layout in the main window" is on, which adds a choice of which one to edit.
import { t, te } from "../i18n.js";
import { accountLabel, brand, esc, getSettings, setRows } from "../lib.js";
import { applyRowLayout, choosePreset, ELEMENTS, PRESETS, resolve, toggled } from "../density.js";
import { rowHtml } from "../components.js";
import { errorText } from "./common.js";
import { row, rowError, seg, subpage, subRow, sw, toast } from "./rows.js";

const presetLabel = (layout) => t(`settings.rows.preset.${resolve(layout).preset}`);

/** The Dashboard row that opens the sub-page, and the sub-page. */
export function rowsHtml(rows) {
  const r = norm(rows);
  return subRow({ key: "rows", label: te("settings.rows.title"), desc: esc(presetLabel(r.tray)) })
    + subpage("rows", te("settings.rows.title"), `<div data-rows-body>${bodyHtml(r, "tray")}</div>`);
}

const norm = (rows) => ({ tray: rows?.tray ?? { preset: "standard" }, window: rows?.window ?? { preset: "standard" }, separate: !!rows?.separate });

/** The sub-page body for `scope` ("tray" = the shared layout, or the menu bar's when separate; "window"). */
export function bodyHtml(rows, scope) {
  const layout = resolve(rows[scope]);
  const presets = seg("rows-preset", PRESETS.map((p) => [p, te(`settings.rows.preset.${p}`)]), layout.preset, `aria-label="${te("settings.rows.presets")}"`);
  const scopes = rows.separate
    ? seg("rows-scope", [["tray", te("settings.rows.scope.tray")], ["window", te("settings.rows.scope.window")]], scope, `aria-label="${te("settings.rows.scope")}"`)
    : "";
  return `<div class="rows-head"><p class="hint">${te("settings.rows.intro")}</p>${presets}</div>
    <div class="prev-card rows-prev" role="img" aria-label="" data-rows-preview><span class="prev-label">${te("settings.menubar.preview")}</span><div class="rows-prev-out" data-rows-preview-out></div></div>
    ${row({ key: "rows-separate", label: te("settings.rows.separate"), kw: "rows-separate", control: (a) => sw("rows-separate", rows.separate, a), body: scopes })}
    <div class="rows-el-head"><h3>${te("settings.rows.elements")}</h3><span class="hint">${te("settings.rows.live")}</span></div>
    <div class="rows-els">${ELEMENTS.map((k) => row({ key: `rows-el-${k}`, kw: "rows", label: te(`settings.rows.el.${k}`), desc: te(`settings.rows.el.${k}.desc`),
      control: (a) => sw(`rows-el-${k}`, layout.elements[k], a, `data-rows-el="${k}"`) })).join("")}</div>`;
}

/** DS-CMP-DENS-5: the preview account: the first one with a pace, else the first, else a fixed sample labelled as an
 *  example. */
export function previewReport(view) {
  const reports = (view?.reports ?? []).filter((r) => r.status === "ready");
  const paced = reports.find((r) => r.binding && r.metrics?.[r.binding.index]?.pace);
  return paced ?? reports[0] ?? null;
}

const SAMPLE = {
  provider: "claude", account: "work@example.com", account_id: "acc_example", index: -1, status: "ready", class: "available",
  checked_at: new Date().toISOString(), freshness: "fresh", plan: "Max",
  binding: { index: 0, used_ratio: 0.78, window: "weekly", label: "Weekly" },
  metrics: [{ label: "Weekly", kind: "quota", window: "weekly", used: 78, limit: 100, unit: "percent", resets_at: null },
    { label: "5h", kind: "quota", window: "window_5h", used: 34, limit: 100, unit: "percent", resets_at: null }],
};

export function previewHtml(view, layout) {
  const real = previewReport(view);
  const r = real ?? SAMPLE;
  // The row component reads this window's layout: the preview is the only row drawn here.
  applyRowLayout(layout);
  const html = rowHtml(r, { title: brand(r.provider).label, sub: accountLabel(r), mono: "" });
  return real ? html : `${html}<p class="hint">${te("settings.rows.example")}</p>`;
}

export function bind(root, ctx) {
  let rows = norm(ctx.settings.rows);
  let scope = "tray";
  const body = root.querySelector("[data-rows-body]");
  if (!body) return;
  const paint = (focusId = null) => {
    body.innerHTML = bodyHtml(rows, scope);
    const out = body.querySelector("[data-rows-preview-out]");
    out.innerHTML = previewHtml(ctx.view?.(), rows[scope]);
    body.querySelector("[data-rows-preview]").setAttribute("aria-label", `${t("settings.menubar.preview")}: ${out.querySelector(".rh")?.getAttribute("aria-label") ?? ""}`);
    root.querySelector('[data-row="rows"] .set-row-desc')?.replaceChildren(presetLabel(rows.tray));
    if (focusId) body.querySelector(`#${focusId}`)?.focus();
  };
  const save = (next, el, focusId) => {
    const before = rows;
    rows = next;
    paint(focusId);
    return setRows(rows).then(() => { toast(); ctx.changed("rows"); },
      (x) => { rows = before; paint(focusId); rowError(body.querySelector(`#${focusId}`) ?? el, errorText(x)); });
  };
  body.addEventListener("change", (e) => {
    const el = e.target;
    if (el.id === "rows-preset") return save({ ...rows, [scope]: choosePreset(el.value) }, el, "rows-preset");
    if (el.id === "rows-scope") { scope = el.value; return paint("rows-scope"); }
    if (el.id === "rows-separate") {
      if (!el.checked) scope = "tray";
      return save({ ...rows, separate: el.checked, window: el.checked ? rows.window ?? rows.tray : rows.window }, el, "rows-separate");
    }
    const k = el.dataset?.rowsEl;
    if (k) save({ ...rows, [scope]: toggled(rows[scope], k, el.checked) }, el, el.id);
  });
  paint();
  // Another window changed the layout: follow it.
  getSettings().then((s) => { rows = norm(s.rows); paint(); }).catch(() => {});
}
