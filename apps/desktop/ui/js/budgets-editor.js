// PRD 0041 RF5.1: the account dialog's "Budgets and floors" section. The rules (limits, units, periods, starts) and
// every sentence and error come from the core (`check_budgets`); this module only lays out the rows and collects them.
import { checkBudgets, esc, icon } from "./lib.js";
import { t, te, tl } from "./i18n.js";

const MAX_BUDGETS = 8;
const MAX_FLOORS = 4;
const KINDS = ["spend", "usage", "credits", "account"];
const FLOOR_KINDS = ["", "balance", "credits"];
const PERIODS = ["monthly", "weekly", "daily", "window"];
const WEEKDAYS = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

const option = (value, label, selected) => `<option value="${esc(value)}" ${selected ? "selected" : ""}>${esc(label)}</option>`;

/** A budget as the TOML holds it → the row's form values. */
function budgetRow(b = {}) {
  return {
    kind: b.scope === "account" ? "account" : b.kind ?? "spend",
    filter: b.label_contains ?? "",
    amount: b.amount ? String(b.amount) : "",
    unit: b.unit ?? "",
    period: b.period ?? "monthly",
    start: b.starts == null ? "" : String(b.starts),
    label: b.label ?? "",
    // PRD 0045 RF5.2: a budget on a breakdown row keeps its row (scope and id) untouched.
    row: b.row ?? null,
  };
}

/** The row's form values → the budget the core reads (absent fields omitted). */
function budgetOf(row) {
  const b = row.kind === "account" ? { scope: "account" } : { kind: row.kind };
  if (row.filter.trim()) b.label_contains = row.filter.trim();
  b.amount = Number(row.amount.replace(",", ".")) || 0;
  if (row.unit.trim()) b.unit = row.unit.trim();
  b.period = row.period;
  if (row.period === "weekly" && WEEKDAYS.includes(row.start)) b.starts = row.start;
  if (row.period === "monthly" && row.start) b.starts = Number(row.start);
  if (row.label.trim()) b.label = row.label.trim();
  if (row.row) b.row = row.row;
  return b;
}

const floorRow = (f = {}) => ({ kind: f.kind ?? "", filter: f.label_contains ?? "", amount: f.amount ? String(f.amount) : "", unit: f.unit ?? "" });
function floorOf(row) {
  const f = {};
  if (row.kind) f.kind = row.kind;
  if (row.filter.trim()) f.label_contains = row.filter.trim();
  f.amount = Number(row.amount.replace(",", ".")) || 0;
  if (row.unit.trim()) f.unit = row.unit.trim();
  return f;
}

function startOptions(row) {
  if (row.period === "weekly") return [option("", t("budget.editor.start.default_weekly"), !row.start), ...WEEKDAYS.map((d) => option(d, t(`budget.editor.weekday.${d}`), row.start === d))].join("");
  if (row.period === "monthly") return [option("", t("budget.editor.start.default_monthly"), !row.start), ...Array.from({ length: 28 }, (_, i) => option(String(i + 1), String(i + 1), row.start === String(i + 1)))].join("");
  return "";
}

function budgetHtml(row, i) {
  const startable = row.period === "weekly" || row.period === "monthly";
  return `<li class="budget-row" data-budget-row="${i}">
    <div class="budget-grid">
      <label class="f"><span>${te("budget.editor.metric")}</span><select data-k="kind">${KINDS.map((k) => option(k, t(`budget.editor.kind.${k}`), row.kind === k)).join("")}</select></label>
      <label class="f"><span>${te("budget.editor.amount")}</span><input data-k="amount" inputmode="decimal" value="${esc(row.amount)}" /></label>
      <label class="f"><span>${te("budget.editor.unit")}</span><input data-k="unit" spellcheck="false" maxlength="24" placeholder="${te("budget.editor.unitPlaceholder")}" value="${esc(row.unit)}" /></label>
      <label class="f"><span>${te("budget.editor.period")}</span><select data-k="period">${PERIODS.map((p) => option(p, t(`budget.editor.period.${p}`), row.period === p)).join("")}</select></label>
      <label class="f" ${startable ? "" : "hidden"}><span>${te("budget.editor.start")}</span><select data-k="start">${startOptions(row)}</select></label>
      <label class="f"><span>${te("budget.editor.filter")}</span><input data-k="filter" maxlength="64" placeholder="${te("budget.editor.filterPlaceholder")}" value="${esc(row.filter)}" /></label>
      <label class="f"><span>${te("budget.editor.label")}</span><input data-k="label" maxlength="32" value="${esc(row.label)}" /></label>
    </div>
    ${(row.hints ?? []).map((h) => `<p class="hint" data-budget-hint>${esc(tl(h))}</p>`).join("")}
    ${row.row ? `<p class="hint" data-budget-row-target>${esc(t("budget.editor.row", { scope: t(`budget.editor.rowScope.${row.row.scope}`), id: row.row.id }))}</p>` : ""}
    <p class="hint" data-sentence role="status" aria-live="polite"></p>
    <small class="field-err" role="alert" hidden></small>
    <button type="button" class="btn sm" data-remove-budget="${i}">${icon("x")}${te("budget.editor.remove")}</button>
  </li>`;
}

function floorHtml(row, i) {
  return `<li class="budget-row" data-floor-row="${i}">
    <div class="budget-grid">
      <label class="f"><span>${te("budget.editor.balance")}</span><select data-k="kind">${FLOOR_KINDS.map((k) => option(k, t(`budget.editor.floorKind.${k || "any"}`), row.kind === k)).join("")}</select></label>
      <label class="f"><span>${te("budget.editor.floorAmount")}</span><input data-k="amount" inputmode="decimal" value="${esc(row.amount)}" /></label>
      <label class="f"><span>${te("budget.editor.unit")}</span><input data-k="unit" spellcheck="false" maxlength="24" placeholder="${te("budget.editor.unitPlaceholder")}" value="${esc(row.unit)}" /></label>
      <label class="f"><span>${te("budget.editor.filter")}</span><input data-k="filter" maxlength="64" placeholder="${te("budget.editor.filterPlaceholder")}" value="${esc(row.filter)}" /></label>
    </div>
    <small class="field-err" role="alert" hidden></small>
    <button type="button" class="btn sm" data-remove-floor="${i}">${icon("x")}${te("budget.editor.remove")}</button>
  </li>`;
}

/** Mounts the section in `host`. `budgetDraft` / `floorDraft` (RF5.3) add one prefilled row and open the section;
 *  `budgetHint` (RF6) carries the core's "From your plan price…" and "Last 30 days…" lines shown under that row.
 *  `rows()` returns `{ budgets, floors }` once the person touched the section, else `null` (the account keeps its own). */
export function mountBudgets(host, { provider, budgets = [], floors = [], budgetDraft = null, budgetHint = null, floorDraft = null }) {
  let bRows = budgets.map(budgetRow);
  let fRows = floors.map(floorRow);
  let touched = false;
  let seq = 0;
  // RF5.1: the provider's metric choices from the last check; a new row starts on the first one.
  let targets = null;
  if (budgetDraft && bRows.length < MAX_BUDGETS) {
    const row = budgetRow(budgetDraft);
    // RF6: a reference, never a saved amount; the row is written only by Save.
    row.hints = [budgetHint?.from_plan, budgetHint?.reference].filter(Boolean);
    bRows.push(row);
    touched = true;
  }
  if (floorDraft && fRows.length < MAX_FLOORS) { fRows.push(floorRow(floorDraft)); touched = true; }
  const el = document.createElement("details");
  el.className = "session-box budgets-box";
  el.open = !!(budgetDraft || floorDraft);
  host.replaceChildren(el);

  function render() {
    el.innerHTML = `<summary><h3>${te("budget.editor.title")}</h3></summary>
      <p class="hint">${te("budget.editor.hint")}</p>
      <h4>${te("budget.editor.budgets")}</h4>
      <ul class="budget-list" aria-label="${te("budget.editor.budgets")}">${bRows.map(budgetHtml).join("")}</ul>
      <button type="button" class="btn sm" data-add-budget ${bRows.length >= MAX_BUDGETS ? "disabled" : ""}>${icon("plus")}${te("budget.editor.addBudget")}</button>
      <h4>${te("budget.editor.floors")}</h4>
      <ul class="budget-list" aria-label="${te("budget.editor.floors")}">${fRows.map(floorHtml).join("")}</ul>
      <button type="button" class="btn sm" data-add-floor ${fRows.length >= MAX_FLOORS ? "disabled" : ""}>${icon("plus")}${te("budget.editor.addFloor")}</button>`;
    check();
  }

  async function check() {
    const mine = ++seq;
    let result;
    try {
      result = await checkBudgets(provider, bRows.map(budgetOf), fRows.map(floorOf));
    } catch {
      return;
    }
    if (mine !== seq) return;
    targets = Array.isArray(result.targets) ? result.targets : null;
    // RF5.1: the metric picker offers what the provider can report (the row's own choice stays visible).
    const offered = (rows, list) => el.querySelectorAll(`${rows} select[data-k="kind"]`).forEach((select) => {
      if (!Array.isArray(list)) return;
      for (const opt of select.options) opt.hidden = !list.includes(opt.value === "" ? "any" : opt.value) && opt.value !== select.value;
    });
    offered("[data-budget-row]", result.targets);
    offered("[data-floor-row]", result.floor_targets);
    el.querySelectorAll("[data-budget-row]").forEach((li, i) => {
      const sentence = result.sentences[i];
      li.querySelector("[data-sentence]").textContent = sentence ? tl(sentence) : "";
    });
    el.querySelectorAll(".budget-row .field-err").forEach((e) => { e.hidden = true; e.textContent = ""; });
    for (const p of result.problems) {
      const m = /^(budgets|floors)\[(\d+)\]/.exec(p.field);
      const row = m ? el.querySelector(`[data-${m[1] === "budgets" ? "budget" : "floor"}-row="${m[2]}"]`) : null;
      const err = row?.querySelector(".field-err");
      if (err && err.hidden) { err.hidden = false; err.textContent = tl(p.note); }
    }
  }

  el.addEventListener("click", (e) => {
    const tgt = e.target;
    if (tgt.closest("[data-add-budget]")) {
      const first = targets?.[0];
      bRows.push(budgetRow(first === "account" ? { scope: "account" } : first ? { kind: first } : {}));
      touched = true;
      return render();
    }
    if (tgt.closest("[data-add-floor]")) { fRows.push(floorRow()); touched = true; return render(); }
    const rb = tgt.closest("[data-remove-budget]");
    if (rb) { bRows.splice(Number(rb.dataset.removeBudget), 1); touched = true; return render(); }
    const rf = tgt.closest("[data-remove-floor]");
    if (rf) { fRows.splice(Number(rf.dataset.removeFloor), 1); touched = true; return render(); }
  });
  const onEdit = (e) => {
    const input = e.target.closest("[data-k]");
    const li = e.target.closest("[data-budget-row],[data-floor-row]");
    if (!input || !li) return;
    touched = true;
    const row = li.dataset.budgetRow != null ? bRows[Number(li.dataset.budgetRow)] : fRows[Number(li.dataset.floorRow)];
    row[input.dataset.k] = input.value;
    // A new period offers its own start (weekday or day of the month).
    if (input.dataset.k === "period") { row.start = ""; return render(); }
    check();
  };
  el.addEventListener("input", onEdit);
  el.addEventListener("change", onEdit);
  render();
  if (budgetDraft || floorDraft) queueMicrotask(() => el.scrollIntoView?.({ block: "nearest" }));

  return {
    rows: () => (touched ? { budgets: bRows.map(budgetOf), floors: fRows.map(floorOf) } : null),
    setProvider(next) { provider = next; check(); },
  };
}
