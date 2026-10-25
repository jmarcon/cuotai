// Settings › Advanced › Local cost (PRD 0015), as rows (PRD 0020 RF3): one switch per source, the period and
// retention, then the directories and the price table as sub-pages (RF2.3) and the index erasure at the end (RF3.4).
import { t, te, tle } from "../i18n.js";
import { costDisable, costEnable, costErase, costPrices, costSetOptions, costSetPaths, costStatus, esc, fmtDate, fmtNumber, fmtReset, icon } from "../lib.js";
import { askDialog, fmtBytes, group, l10nError } from "./common.js";
import { buttons, commitField, num, row, select, subpage, subRow, sw, toast } from "./rows.js";

export function html() {
  return group("cost", te("settings.cost.title"), `<div data-cost-body><p class="hint">${te("common.loading")}</p></div>`, "data-cost");
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
  const dirs = (s) => `${te("settings.cost.dirs", { pattern: s.disclosure.pattern })} ${s.disclosure.directories.map((d) => `<code>${esc(d)}</code>`).join(", ")}${s.disclosure.custom ? ` (${te("settings.cost.pathsChosen")})` : ""}`;
  const sources = cv.sources.map((s) => row({ key: `cost-src-${esc(s.source)}`, kw: "cost-src", label: te("settings.cost.read", { source: s.label }), attrs: `data-cost-source="${esc(s.source)}"`,
    desc: `<span data-cost-state>${s.enabled ? te("settings.cost.since", { at: fmtReset(s.consented_at) ?? "—" }) : te("settings.cost.off")}</span>`,
    info: `${te("settings.cost.help")}<br />${dirs(s)}${s.enabled ? `<br /><span data-cost-indexed>${te("settings.cost.indexed", { files: s.indexed.files, invalid: s.indexed.invalid_lines })}</span>` : ""}`,
    msg: `cost-src-${esc(s.source)}-msg`, control: (a) => sw(`cost-toggle-${esc(s.source)}`, s.enabled, a, `data-cost-toggle="${esc(s.source)}"`),
    body: s.enabled && s.summary ? `<p class="hint" data-cost-summary>${tle(s.summary)}</p>` : "" })).join("");
  const editors = cv.sources.map((s) => `<div class="cost-paths" data-cost-paths-box="${esc(s.source)}">
      <label class="f"><span>${te("settings.cost.paths", { source: s.label })}</span>
        <textarea rows="3" spellcheck="false" autocomplete="off" data-cost-paths="${esc(s.source)}" placeholder="~/…">${esc(s.paths.join("\n"))}</textarea>
        <small>${dirs(s)}</small></label>
      <div class="dlg-actions left"><button class="btn" data-cost-paths-reset="${esc(s.source)}" ${s.paths.length ? "" : "disabled"}>${te("settings.cost.pathsReset")}</button>
        <button class="btn primary" data-cost-paths-save="${esc(s.source)}">${te("settings.cost.pathsSave")}</button></div>
      <p class="err" id="cost-paths-${esc(s.source)}-msg" role="alert"></p>
    </div>`).join("");
  return `${state ? `<p class="hint${cv.state === "paused" ? " warn-txt" : ""}" data-cost-worker role="status">${te(state)}</p>` : ""}
    ${cv.error ? `<p class="hint warn-txt" role="alert">${tle(cv.error)}</p>` : ""}
    ${sources}
    ${row({ key: "cost-period-row", label: te("settings.cost.period"), msg: "cost-period-msg",
      control: (a) => select("cost-period", cv.periods.map((p) => [p.id, tle(p.label)]), cv.period, a) })}
    ${row({ key: "cost-retention-row", label: te("settings.cost.retention"), desc: te("settings.cost.retentionRange", { min: cv.retention_min, max: cv.retention_max }), msg: "cost-retention-msg",
      control: (a) => num("cost-retention", { value: cv.retention_days, min: cv.retention_min, max: cv.retention_max, apply: "data-cost-save" }, a) })}
    ${subRow({ key: "cost-dirs", label: te("settings.cost.pathsEdit") })}
    ${subpage("cost-dirs", te("settings.cost.pathsEdit"), `<p class="hint">${te("settings.cost.pathsHelp")}</p>${editors}`)}
    ${subRow({ key: "cost-prices", label: te("settings.cost.prices") })}
    ${subpage("cost-prices", te("settings.cost.prices"), `<div class="cost-prices" data-cost-prices>${prices ? pricesHtml(prices) : `<p class="hint">${te("common.loading")}</p>`}</div>`)}
    ${row({ key: "cost-erase", label: te("settings.cost.erase"), danger: true, msg: "cost-msg",
      desc: `<span data-cost-size>${te("settings.cost.size", { size: fmtBytes(cv.size) })}</span>`,
      control: () => buttons(`<button class="btn sm danger-btn" data-cost-erase aria-labelledby="cost-erase-label">${icon("trash")}</button>`),
      body: `<div class="confirm" id="cost-erase-confirm" role="alertdialog" aria-labelledby="cost-erase-text" hidden>
          <p id="cost-erase-text">${te("settings.cost.eraseText")}</p>
          <div class="dlg-actions left"><button class="btn danger-btn" data-cost-erase-confirm>${te("settings.cost.eraseConfirm")}</button>
            <button class="btn" data-cost-erase-cancel>${te("common.cancel")}</button></div>
        </div>` })}`;
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
  return `<p class="hint">${te("settings.cost.pricesHelp")}</p>${tables}${state}
    <p class="hint">${te("settings.cost.pricesFile")} <code>${esc(file.path)}</code></p>
    <div class="cost-price-table"><table><thead><tr>${cols}</tr></thead><tbody>${rows}</tbody></table></div>
    <p class="hint">${tle(p.notice)} ${te("settings.cost.unknownModel")}</p>`;
}

/** Paints the group from the backend's state and binds it; while a scan is under way it reads the state again.
 *  `keep`: a repaint the user did not ask for (the scan's state, the prices arriving) keeps the directories being
 *  typed and the focus. A refusal stays in the row of the control that caused it (RF4.3) until the next change. */
async function bindCost(box, ctx) {
  let cv, prices = null;
  try { cv = await costStatus(); } catch (e) { box.querySelector(".hint").textContent = l10nError(e); return; }
  let timer = null;
  let failure = null; // [line id, text]
  ctx.onLeave?.(() => clearTimeout(timer));
  const paint = (next, keep = false) => {
    cv = next;
    clearTimeout(timer);
    const drafts = keep ? [...box.querySelectorAll("[data-cost-paths]")].map((el) => [el.dataset.costPaths, el.value]) : [];
    const focused = keep && document.activeElement?.dataset?.costPaths;
    const caret = focused ? document.activeElement.selectionStart : 0;
    box.innerHTML = costHtml(cv, prices);
    for (const [source, value] of drafts) { const el = box.querySelector(`[data-cost-paths="${source}"]`); if (el) el.value = value; }
    if (focused) {
      const el = box.querySelector(`[data-cost-paths="${focused}"]`);
      el?.focus();
      el?.setSelectionRange(caret, caret);
    }
    if (failure) { const line = box.querySelector(`#${CSS.escape(failure[0])}`); if (line) line.textContent = failure[1]; }
    // A change also repaints the cards' local usage (`settings-changed`); a refused one keeps what was typed.
    const run = (work, line, done = () => t("settings.saved")) => work().then((answer) => {
      failure = null;
      paint(answer);
      const text = done();
      if (text) toast(text);
      ctx.changed("cost");
    }, (e) => { failure = [line, l10nError(e)]; paint(cv, true); });
    for (const input of box.querySelectorAll("[data-cost-toggle]")) input.onchange = async () => {
      const source = cv.sources.find((s) => s.source === input.dataset.costToggle);
      input.checked = source.enabled; // only the confirmed answer changes it
      const line = `cost-src-${source.source}-msg`;
      if (!source.enabled) {
        if (await confirmDisclosure(source.disclosure)) run(() => costEnable(source.source, source.disclosure.digest), line);
      } else if (await confirmCostOff(source)) run(() => costDisable(source.source), line);
    };
    // RF4.1: a different list needs a new consent, so a source that is on turns off (erasing what was derived) first.
    const savePaths = async (source, paths) => {
      const chosen = paths.map((p) => p.trim()).filter(Boolean);
      const changed = JSON.stringify([...new Set(chosen)]) !== JSON.stringify(source.paths);
      if (changed && source.enabled && !(await confirmPathsOff(source))) return;
      run(() => costSetPaths(source.source, chosen), `cost-paths-${source.source}-msg`, () => (changed ? t("settings.cost.pathsSaved", { source: source.label }) : ""));
    };
    for (const button of box.querySelectorAll("[data-cost-paths-save]")) button.onclick = () => {
      const source = cv.sources.find((s) => s.source === button.dataset.costPathsSave);
      savePaths(source, box.querySelector(`[data-cost-paths="${source.source}"]`).value.split("\n"));
    };
    for (const button of box.querySelectorAll("[data-cost-paths-reset]")) button.onclick = () => {
      savePaths(cv.sources.find((s) => s.source === button.dataset.costPathsReset), []);
    };
    // RF4.1/RF4.2: the period saves on change, the retention on Enter, on leaving the field or with its Apply.
    const period = box.querySelector("#cost-period");
    period.onchange = () => run(() => costSetOptions(period.value, cv.retention_days), "cost-period-msg");
    commitField(box.querySelector("#cost-retention"), (value) => costSetOptions(cv.period, Number(value)).then(
      (answer) => { failure = null; paint(answer); ctx.changed("cost"); }, (e) => { throw l10nError(e); }));
    const confirm = box.querySelector("#cost-erase-confirm");
    box.querySelector("[data-cost-erase]").onclick = () => { confirm.hidden = false; confirm.querySelector("[data-cost-erase-cancel]").focus(); };
    confirm.querySelector("[data-cost-erase-cancel]").onclick = () => { confirm.hidden = true; box.querySelector("[data-cost-erase]").focus(); };
    confirm.querySelector("[data-cost-erase-confirm]").onclick = async () => {
      try {
        const count = await costErase();
        failure = null;
        paint(await costStatus());
        toast(t("settings.cost.erased", { count }));
        ctx.changed("cost");
      } catch (e) { failure = ["cost-msg", l10nError(e)]; paint(cv); }
    };
    // The background scan answers after the change: read its state again until it settles.
    const settled = ["paused", "failed"].includes(cv.state);
    const busy = ["starting", "scanning"].includes(cv.state) || (!settled && cv.sources.some((s) => s.enabled && !s.summary));
    if (busy) timer = setTimeout(() => { if (box.isConnected) costStatus().then((answer) => paint(answer, true), () => {}); }, 1000);
  };
  paint(cv);
  // The prices change only with pricing.json; read once per page opening.
  costPrices().then((answer) => {
    prices = answer;
    if (box.isConnected) paint(cv, true);
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

/** RF6.3: what the search indexes for this page: the rows drawn from the core's state, by the same function. */
export async function searchHtml() {
  return costHtml(await costStatus(), await costPrices());
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
