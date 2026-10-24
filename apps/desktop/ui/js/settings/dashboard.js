// Settings › Dashboard (PRD 0020 RF2.5): account order, the "Recharging" group, consumption pace and the refresh
// interval. Account management (PRD 0025 / DS-CMP-SEG-5) joins this page when it moves out of the main window.
import { t, te } from "../i18n.js";
import { currentSort, esc, groupingRecharging, setGroupRecharging, setRefreshSeconds, setShowPace, setSort } from "../lib.js";
import { errorText, group, l10nError } from "./common.js";

const PRESETS = [[60, "1 min"], [300, "5 min"], [900, "15 min"], [3600, "1 h"]];

export function html({ settings: s, view }) {
  return group("order", te("settings.order.title"), `
      <label class="check"><input id="order-binding" type="checkbox" ${currentSort() === "binding" ? "checked" : ""} /> ${te("settings.order.binding")}</label>
      <p class="hint">${te("settings.order.help")}</p>`)
    + group("display", te("settings.display.title"), `
      <label class="check"><input id="group-recharging" type="checkbox" ${groupingRecharging() ? "checked" : ""} /> ${te("settings.group.recharging")}</label>
      <p class="hint">${te("settings.group.help")}</p>
      <label class="check"><input id="show-pace" type="checkbox" ${s.show_pace !== false ? "checked" : ""} /> ${te("settings.pace.show")}</label>
      <p class="hint">${te("settings.pace.help")}</p>
      <p class="err" id="pace-msg" role="alert"></p>`)
    + group("refresh", te("settings.refresh.title"), `
      <p class="hint">${te("settings.refresh.help")}</p>
      <label class="f"><span>${te("settings.refresh.label")}</span>
        <input id="refresh-seconds" type="number" inputmode="numeric" min="30" step="1" value="${esc(view()?.refresh_seconds || 300)}" /><small>${te("settings.refresh.min")}</small></label>
      <div class="dlg-actions left">
        ${PRESETS.map(([sec, label]) => `<button class="btn" data-preset="${sec}">${label}</button>`).join("")}
        <button class="btn primary" data-apply-refresh>${te("settings.refresh.save")}</button>
      </div>
      <p class="err" id="refresh-msg" role="alert"></p>`);
}

export function bind(root, ctx) {
  // PRD 0009 RF2.3: display-only ordering; the manual order (TOML, shared with the TUI) is never rewritten.
  root.querySelector("#order-binding").onchange = (e) => { setSort(e.target.checked ? "binding" : "custom"); ctx.changed("sort"); };
  root.querySelector("#group-recharging").onchange = (e) => { setGroupRecharging(e.target.checked); ctx.changed("group_recharging"); };
  // PRD 0007 RF3.4: saved in desktop.json; on failure the box goes back to the saved value.
  root.querySelector("#show-pace").onchange = (e) => setShowPace(e.target.checked).then(
    () => { root.querySelector("#pace-msg").textContent = ""; ctx.changed("show_pace"); },
    (x) => { e.target.checked = !e.target.checked; root.querySelector("#pace-msg").textContent = errorText(x); });
  const input = root.querySelector("#refresh-seconds");
  const msg = root.querySelector("#refresh-msg");
  for (const b of root.querySelectorAll("[data-preset]")) b.onclick = () => { input.value = b.dataset.preset; msg.textContent = ""; };
  root.querySelector("[data-apply-refresh]").onclick = async () => {
    msg.classList.remove("ok");
    try {
      await setRefreshSeconds(Number(input.value));
      msg.classList.add("ok");
      msg.textContent = t("settings.refresh.saved");
      ctx.changed("refresh_seconds");
    } catch (e) { msg.textContent = l10nError(e); }
  };
}
