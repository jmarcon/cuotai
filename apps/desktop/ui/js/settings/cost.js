// Settings › Advanced › Local cost (PRD 0015), moved unchanged from the old dialog (PRD 0020 RF2.5).
import { t, te, tle } from "../i18n.js";
import { costDisable, costEnable, costErase, costPrices, costSetOptions, costSetPaths, costStatus, esc, fmtDate, fmtNumber, fmtReset, icon } from "../lib.js";
import { askDialog, fmtBytes, group, l10nError } from "./common.js";

export function html() {
  return group("cost", te("settings.cost.title"), `
      <p class="hint">${te("settings.cost.help")}</p>
      <div data-cost-body><p class="hint">${te("common.loading")}</p></div>`, "data-cost");
}

/** Resolves once the sources are drawn (the shell then lands on an anchor). */
export function bind(root, ctx) {
  return bindCost(root.querySelector("[data-cost-body]"), ctx);
}

/** PRD 0015 RF1/RF2/RF4.1: local cost from the logs of command-line tools, off by default. Per source a switch that
 *  first shows the core's disclosure (exactly the directories read, what for, and that only numbers are extracted)
 *  and turns on with its digest; turning off confirms that everything derived is erased; the directories it reads
 *  (a different list turns a source that is on off, so its new disclosure is reviewed). Then the period, the index
 *  retention, the prices in effect (the embedded table under the user's pricing.json), the index size and its
 *  erasure; the state the core's background scan last reported (paused while saving energy). */
export function costHtml(cv, prices) {
  const state = { starting: "settings.cost.state.starting", scanning: "settings.cost.state.scanning", paused: "settings.cost.state.paused" }[cv.state];
  const sources = cv.sources.map((s) => `<div class="cost-source" data-cost-source="${esc(s.source)}">
        <label class="check"><input type="checkbox" data-cost-toggle="${esc(s.source)}" ${s.enabled ? "checked" : ""} /> ${te("settings.cost.read", { source: s.label })}</label>
        <p class="hint" data-cost-state>${s.enabled ? te("settings.cost.since", { at: fmtReset(s.consented_at) ?? "—" }) : te("settings.cost.off")}</p>
        <p class="hint">${te("settings.cost.dirs", { pattern: s.disclosure.pattern })} ${s.disclosure.directories.map((d) => `<code>${esc(d)}</code>`).join(", ")}${s.disclosure.custom ? ` (${te("settings.cost.pathsChosen")})` : ""}</p>
        ${s.enabled && s.summary ? `<p class="hint" data-cost-summary>${tle(s.summary)}</p>` : ""}
        ${s.enabled ? `<p class="hint" data-cost-indexed>${te("settings.cost.indexed", { files: s.indexed.files, invalid: s.indexed.invalid_lines })}</p>` : ""}
        <details class="cost-paths" data-cost-paths-box="${esc(s.source)}" ${pathsOpen.has(s.source) ? "open" : ""}><summary>${te("settings.cost.pathsEdit")}</summary>
          <label class="f"><span>${te("settings.cost.paths", { source: s.label })}</span>
            <textarea rows="3" spellcheck="false" autocomplete="off" data-cost-paths="${esc(s.source)}" placeholder="~/…">${esc(s.paths.join("\n"))}</textarea>
            <small>${te("settings.cost.pathsHelp")}</small></label>
          <div class="dlg-actions left"><button class="btn" data-cost-paths-save="${esc(s.source)}">${te("settings.cost.pathsSave")}</button>
            <button class="btn" data-cost-paths-reset="${esc(s.source)}" ${s.paths.length ? "" : "disabled"}>${te("settings.cost.pathsReset")}</button></div>
        </details>
      </div>`).join("");
  return `${sources}
        ${state ? `<p class="hint${cv.state === "paused" ? " warn-txt" : ""}" data-cost-worker role="status">${te(state)}</p>` : ""}
        ${cv.error ? `<p class="hint warn-txt" role="alert">${tle(cv.error)}</p>` : ""}
        <label class="f"><span>${te("settings.cost.period")}</span>
          <select id="cost-period">${cv.periods.map((p) => `<option value="${esc(p.id)}" ${p.id === cv.period ? "selected" : ""}>${tle(p.label)}</option>`).join("")}</select></label>
        <label class="f"><span>${te("settings.cost.retention")}</span>
          <input id="cost-retention" type="number" inputmode="numeric" min="${cv.retention_min}" max="${cv.retention_max}" step="1" value="${esc(cv.retention_days)}" />
          <small>${te("settings.cost.retentionRange", { min: cv.retention_min, max: cv.retention_max })}</small></label>
        <div class="dlg-actions left"><button class="btn primary" data-cost-save>${te("settings.cost.save")}</button></div>
        <div class="cost-prices" data-cost-prices>${prices ? pricesHtml(prices) : `<p class="hint">${te("common.loading")}</p>`}</div>
        <p class="hint" data-cost-size>${te("settings.cost.size", { size: fmtBytes(cv.size) })}</p>
        <div class="dlg-actions left"><button class="btn danger-btn" data-cost-erase>${icon("trash")} ${te("settings.cost.erase")}</button></div>
        <div class="confirm" id="cost-erase-confirm" role="alertdialog" aria-labelledby="cost-erase-text" hidden>
          <p id="cost-erase-text">${te("settings.cost.eraseText")}</p>
          <div class="dlg-actions left"><button class="btn danger-btn" data-cost-erase-confirm>${te("settings.cost.eraseConfirm")}</button>
            <button class="btn" data-cost-erase-cancel>${te("common.cancel")}</button></div>
        </div>
        <p class="err" id="cost-msg" role="alert"></p>`;
}

/** RF3/RF4.1: the prices the estimate uses, with each table's date and source and the state of pricing.json. */
function pricesHtml(p) {
  const user = p.models.filter((m) => m.origin === "user").length;
  const tables = p.tables.map((tb) => `<p class="hint">${tb.origin === "embedded"
    ? te("settings.cost.pricesEmbedded", { date: fmtDate(tb.effective) ?? "", source: tb.source ?? "—" })
    : te("settings.cost.pricesUser", { date: fmtDate(tb.effective) ?? "", source: tb.source ?? "—", count: user })}</p>`).join("");
  const file = p.pricing_file;
  const state = file.error ? `<p class="hint warn-txt" role="alert">${tle(file.error)}</p>`
    : file.used ? "" : `<p class="hint">${te("settings.cost.pricesMissing")}</p>`;
  const rate = (r) => fmtNumber(r, 6);
  const rows = p.models.map((m) => `<tr><th scope="row"><code>${esc(m.model)}</code></th><td>${rate(m.rates.input)}</td><td>${rate(m.rates.output)}</td>
      <td>${rate(m.rates.cache_read)}</td><td>${rate(m.rates.cache_write)}</td><td>${te(`cost.origin.${m.origin}`)}</td></tr>`).join("");
  const cols = ["model", "input", "output", "cacheRead", "cacheWrite", "origin"].map((c) => `<th scope="col">${te(`settings.cost.col.${c}`)}</th>`).join("");
  return `<h4>${te("settings.cost.prices")}</h4>
    <p class="hint">${te("settings.cost.pricesHelp")}</p>${tables}${state}
    <p class="hint">${te("settings.cost.pricesFile")} <code>${esc(file.path)}</code></p>
    <details class="cost-price-table" ${pricesOpen ? "open" : ""}><summary>${te("settings.cost.pricesShow", { count: p.models.length })}</summary>
      <table><thead><tr>${cols}</tr></thead><tbody>${rows}</tbody></table></details>
    <p class="hint">${tle(p.notice)} ${te("settings.cost.unknownModel")}</p>`;
}

// What stays open across the group's repaints.
const pathsOpen = new Set();
let pricesOpen = false;

/** Paints the group from the backend's state and binds it; while a scan is under way it reads the state again.
 *  `keep`: a repaint the user did not ask for (the scan's state, the prices arriving) keeps the directories being
 *  typed and the focus. */
async function bindCost(box, ctx) {
  let cv, prices = null;
  try { cv = await costStatus(); } catch (e) { box.querySelector(".hint").textContent = l10nError(e); return; }
  let timer = null;
  ctx.onLeave?.(() => clearTimeout(timer));
  const paint = (next, message = "", ok = false, keep = false) => {
    cv = next;
    clearTimeout(timer);
    const drafts = keep ? [...box.querySelectorAll("[data-cost-paths]")].map((el) => [el.dataset.costPaths, el.value]) : [];
    const focused = keep && document.activeElement?.dataset?.costPaths;
    const caret = focused ? document.activeElement.selectionStart : 0;
    box.innerHTML = costHtml(cv, prices);
    for (const [source, value] of drafts) box.querySelector(`[data-cost-paths="${source}"]`).value = value;
    if (focused) {
      const el = box.querySelector(`[data-cost-paths="${focused}"]`);
      el.focus();
      el.setSelectionRange(caret, caret);
    }
    const msg = box.querySelector("#cost-msg");
    msg.classList.toggle("ok", ok);
    msg.textContent = message;
    // A change also repaints the cards' local usage (`settings-changed`); a refused one keeps what was typed.
    const run = (work, done = () => "") => work().then((answer) => { paint(answer, done(), true); ctx.changed("cost"); }, (e) => paint(cv, l10nError(e), false, true));
    for (const input of box.querySelectorAll("[data-cost-toggle]")) input.onchange = async () => {
      const source = cv.sources.find((s) => s.source === input.dataset.costToggle);
      input.checked = source.enabled; // only the confirmed answer changes it
      if (!source.enabled) {
        if (await confirmDisclosure(source.disclosure)) run(() => costEnable(source.source, source.disclosure.digest));
      } else if (await confirmCostOff(source)) run(() => costDisable(source.source));
    };
    for (const el of box.querySelectorAll("[data-cost-paths-box]")) {
      el.ontoggle = () => (el.open ? pathsOpen.add(el.dataset.costPathsBox) : pathsOpen.delete(el.dataset.costPathsBox));
    }
    // RF4.1: a different list needs a new consent, so a source that is on turns off (erasing what was derived) first.
    const savePaths = async (source, paths) => {
      const chosen = paths.map((p) => p.trim()).filter(Boolean);
      const changed = JSON.stringify([...new Set(chosen)]) !== JSON.stringify(source.paths);
      if (changed && source.enabled && !(await confirmPathsOff(source))) return;
      run(() => costSetPaths(source.source, chosen), () => (changed ? t("settings.cost.pathsSaved", { source: source.label }) : ""));
    };
    for (const button of box.querySelectorAll("[data-cost-paths-save]")) button.onclick = () => {
      const source = cv.sources.find((s) => s.source === button.dataset.costPathsSave);
      savePaths(source, box.querySelector(`[data-cost-paths="${source.source}"]`).value.split("\n"));
    };
    for (const button of box.querySelectorAll("[data-cost-paths-reset]")) button.onclick = () => {
      savePaths(cv.sources.find((s) => s.source === button.dataset.costPathsReset), []);
    };
    const table = box.querySelector(".cost-price-table");
    if (table) table.ontoggle = () => { pricesOpen = table.open; };
    box.querySelector("[data-cost-save]").onclick = () => run(
      () => costSetOptions(box.querySelector("#cost-period").value, Number(box.querySelector("#cost-retention").value)),
      () => t("settings.cost.saved"));
    const confirm = box.querySelector("#cost-erase-confirm");
    box.querySelector("[data-cost-erase]").onclick = () => { confirm.hidden = false; confirm.querySelector("[data-cost-erase-cancel]").focus(); };
    confirm.querySelector("[data-cost-erase-cancel]").onclick = () => { confirm.hidden = true; box.querySelector("[data-cost-erase]").focus(); };
    confirm.querySelector("[data-cost-erase-confirm]").onclick = async () => {
      try {
        const count = await costErase();
        paint(await costStatus(), t("settings.cost.erased", { count }), true);
        ctx.changed("cost");
      } catch (e) { paint(cv, l10nError(e)); }
    };
    // The background scan answers after the change: read its state again until it settles.
    const settled = ["paused", "failed"].includes(cv.state);
    const busy = ["starting", "scanning"].includes(cv.state) || (!settled && cv.sources.some((s) => s.enabled && !s.summary));
    if (busy) timer = setTimeout(() => { if (box.isConnected) costStatus().then((answer) => paint(answer, msg.textContent, msg.classList.contains("ok"), true), () => {}); }, 1000);
  };
  paint(cv);
  // The prices change only with pricing.json; read once per page opening.
  costPrices().then((answer) => {
    prices = answer;
    const msg = box.querySelector("#cost-msg");
    if (msg && box.isConnected) paint(cv, msg.textContent, msg.classList.contains("ok"), true);
  }, (e) => { const slot = box.querySelector("[data-cost-prices]"); if (slot) slot.innerHTML = `<p class="hint warn-txt">${esc(l10nError(e))}</p>`; });
}

/** RF1.1: the core's disclosure of one source before it is turned on; resolves to the user's answer. */
function confirmDisclosure(d) {
  return askDialog("cost-confirm", te("settings.cost.confirm.title", { source: d.label }), `<p>${tle(d.purpose)}</p>
      <p>${te("settings.cost.confirm.dirs", { pattern: d.pattern })}</p>
      <ul class="cost-dirs" data-cost-dirs>${d.directories.map((dir) => `<li><code>${esc(dir)}</code></li>`).join("")}</ul>
      <p>${tle(d.extracted)}</p>
      <p class="hint">${tle(d.never)}</p>
      <p class="hint">${tle(d.off)}</p>`, te("settings.cost.confirm.yes"));
}

/** RF2.4: turning a source off erases everything derived from it; resolves to the user's answer. */
function confirmCostOff(s) {
  return askDialog("cost-off", te("settings.cost.disable.title", { source: s.label }),
    `<p>${te("settings.cost.disable.text", { source: s.label })}</p>`, te("settings.cost.disable.yes"));
}

/** RF4.1: choosing other directories for a source that is on turns it off and erases what was derived from it. */
function confirmPathsOff(s) {
  return askDialog("cost-off", te("settings.cost.pathsOffTitle", { source: s.label }),
    `<p>${te("settings.cost.pathsOffText", { source: s.label })}</p>`, te("settings.cost.pathsOffYes"));
}
