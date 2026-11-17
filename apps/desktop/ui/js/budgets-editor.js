// PRD 0041 RF5.1: the account dialog's "Budgets and floors" section. The rules (limits, units, periods, starts) and
// every sentence and error come from the core (`check_budgets`); this module only lays out the rows and collects them.
// F-05: the rows travel as typed and the core reads each amount in the dialog's language.
import { checkBudgets, esc, icon } from "./lib.js";
import { currentLocale, t, te, tl } from "./i18n.js";

const MAX_BUDGETS = 8;
const MAX_FLOORS = 4;
const KINDS = ["spend", "usage", "credits", "account"];
const FLOOR_KINDS = ["", "balance", "credits"];
const PERIODS = ["monthly", "weekly", "daily", "window"];
const WEEKDAYS = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

// F-10: the row input each field path of the core names (`budgets[1].label_contains` → the filter input).
// Ids of the error lines (`aria-describedby`): unique per mounted editor, nothing secret, so a counter.
let mounts = 0;
const FIELD_INPUTS = { kind: "kind", amount: "amount", unit: "unit", period: "period", starts: "start", label_contains: "filter", label: "label" };

/** F-05: a saved amount as its field shows it: the language's decimal sign, no grouping, and a fourth fraction digit
 *  after exactly three (1.1250), so it reads back as the same number (the core's `text::number::amount_input`). */
function amountText(n) {
  if (!(n > 0) || !Number.isFinite(n)) return "";
  const plain = n.toLocaleString("en-US", { useGrouping: false, maximumFractionDigits: 20 });
  const [int, frac = ""] = plain.split(".");
  if (!frac) return int;
  let decimal = ".";
  try { decimal = new Intl.NumberFormat(currentLocale()).formatToParts(1.5).find((p) => p.type === "decimal")?.value ?? "."; } catch {}
  return `${int}${decimal}${frac.length === 3 ? `${frac}0` : frac}`;
}

const option = (value, label, selected) => `<option value="${esc(value)}" ${selected ? "selected" : ""}>${esc(label)}</option>`;

/** A budget as the TOML holds it → the row's form values. */
function budgetRow(b = {}) {
  return {
    kind: b.scope === "account" ? "account" : b.kind ?? "spend",
    filter: b.label_contains ?? "",
    amount: amountText(b.amount),
    unit: b.unit ?? "",
    period: b.period ?? "monthly",
    start: b.starts == null ? "" : String(b.starts),
    label: b.label ?? "",
    // PRD 0045 RF5.2: a budget on a breakdown row keeps its row (scope and id) untouched.
    row: b.row ?? null,
  };
}

/** The row's form values → the core's `BudgetRow`: the texts as typed (the core reads and trims them). */
function budgetOf(row) {
  let start = null;
  if (row.period === "weekly" && WEEKDAYS.includes(row.start)) start = row.start;
  if (row.period === "monthly" && row.start) start = Number(row.start);
  return { target: row.kind, amount: row.amount, unit: row.unit, period: row.period, start, filter: row.filter, label: row.label, row: row.row };
}

const floorRow = (f = {}) => ({ kind: f.kind ?? "", filter: f.label_contains ?? "", amount: amountText(f.amount), unit: f.unit ?? "" });
/** The row's form values → the core's `FloorRow`. */
const floorOf = (row) => ({ target: row.kind || "any", amount: row.amount, unit: row.unit, filter: row.filter });

function startOptions(row) {
  if (row.period === "weekly") return [option("", t("budget.editor.start.default_weekly"), !row.start), ...WEEKDAYS.map((d) => option(d, t(`budget.editor.weekday.${d}`), row.start === d))].join("");
  if (row.period === "monthly") return [option("", t("budget.editor.start.default_monthly"), !row.start), ...Array.from({ length: 28 }, (_, i) => option(String(i + 1), String(i + 1), row.start === String(i + 1)))].join("");
  return "";
}

/** F-10: a field's error line under its control, which `aria-describedby` points at. */
const errLine = (id) => `<small class="field-err" id="${id}" role="alert" hidden></small>`;

function budgetHtml(row, i, uid) {
  const startable = row.period === "weekly" || row.period === "monthly";
  const id = (k) => `${uid}-b${i}-${k}`;
  const field = (k, label, control, attrs = "") => `<label class="f" ${attrs}><span>${te(label)}</span>${control(`aria-describedby="${id(k)}"`)}${errLine(id(k))}</label>`;
  return `<li class="budget-row" data-budget-row="${i}">
    <div class="budget-grid">
      ${field("kind", "budget.editor.metric", (a) => `<select data-k="kind" ${a}>${KINDS.map((k) => option(k, t(`budget.editor.kind.${k}`), row.kind === k)).join("")}</select>`)}
      ${field("amount", "budget.editor.amount", (a) => `<input data-k="amount" inputmode="decimal" ${a} value="${esc(row.amount)}" />`)}
      ${field("unit", "budget.editor.unit", (a) => `<input data-k="unit" spellcheck="false" maxlength="24" ${a} placeholder="${te("budget.editor.unitPlaceholder")}" value="${esc(row.unit)}" />`)}
      ${field("period", "budget.editor.period", (a) => `<select data-k="period" ${a}>${PERIODS.map((p) => option(p, t(`budget.editor.period.${p}`), row.period === p)).join("")}</select>`)}
      ${field("start", "budget.editor.start", (a) => `<select data-k="start" ${a}>${startOptions(row)}</select>`, startable ? "" : "hidden")}
      ${field("filter", "budget.editor.filter", (a) => `<input data-k="filter" maxlength="64" ${a} placeholder="${te("budget.editor.filterPlaceholder")}" value="${esc(row.filter)}" />`)}
      ${field("label", "budget.editor.label", (a) => `<input data-k="label" maxlength="32" ${a} value="${esc(row.label)}" />`)}
    </div>
    ${(row.hints ?? []).map((h) => `<p class="hint" data-budget-hint>${esc(tl(h))}</p>`).join("")}
    ${row.row ? `<p class="hint" data-budget-row-target>${esc(t("budget.editor.row", { scope: t(`budget.editor.rowScope.${row.row.scope}`), id: row.row.id }))}</p>` : ""}
    <p class="hint" data-sentence role="status" aria-live="polite"></p>
    <small class="field-err" role="alert" hidden></small>
    <button type="button" class="btn sm" data-remove-budget="${i}">${icon("x")}${te("budget.editor.remove")}</button>
  </li>`;
}

function floorHtml(row, i, uid) {
  const id = (k) => `${uid}-f${i}-${k}`;
  const field = (k, label, control) => `<label class="f"><span>${te(label)}</span>${control(`aria-describedby="${id(k)}"`)}${errLine(id(k))}</label>`;
  return `<li class="budget-row" data-floor-row="${i}">
    <div class="budget-grid">
      ${field("kind", "budget.editor.balance", (a) => `<select data-k="kind" ${a}>${FLOOR_KINDS.map((k) => option(k, t(`budget.editor.floorKind.${k || "any"}`), row.kind === k)).join("")}</select>`)}
      ${field("amount", "budget.editor.floorAmount", (a) => `<input data-k="amount" inputmode="decimal" ${a} value="${esc(row.amount)}" />`)}
      ${field("unit", "budget.editor.unit", (a) => `<input data-k="unit" spellcheck="false" maxlength="24" ${a} placeholder="${te("budget.editor.unitPlaceholder")}" value="${esc(row.unit)}" />`)}
      ${field("filter", "budget.editor.filter", (a) => `<input data-k="filter" maxlength="64" ${a} placeholder="${te("budget.editor.filterPlaceholder")}" value="${esc(row.filter)}" />`)}
    </div>
    <small class="field-err" role="alert" hidden></small>
    <button type="button" class="btn sm" data-remove-floor="${i}">${icon("x")}${te("budget.editor.remove")}</button>
  </li>`;
}

/** Mounts the section in `host`; `index` is the edited account (`null` for a new one). `budgetDraft` / `floorDraft` (RF5.3) add one prefilled row and open the section;
 *  `budgetHint` (RF6) carries the core's "From your plan price…" and "Last 30 days…" lines shown under that row.
 *  `rows()` returns `{ budgets, floors }` once the person touched the section, else `null` (the account keeps its own). */
export function mountBudgets(host, { index = null, provider, budgets = [], floors = [], budgetDraft = null, budgetHint = null, floorDraft = null }) {
  let bRows = budgets.map(budgetRow);
  let fRows = floors.map(floorRow);
  let touched = false;
  // F-10: a new row's empty amount is not an error until it was typed in or Save was pressed.
  let revealed = false;
  let seq = 0;
  const uid = `bud${++mounts}`;
  // RF5.1: the provider's metric choices from the last check; a new row starts on the first one.
  let targets = null;
  if (budgetDraft && bRows.length < MAX_BUDGETS) {
    const row = { ...budgetRow(budgetDraft), fresh: true };
    // RF6: a reference, never a saved amount; the row is written only by Save.
    row.hints = [budgetHint?.from_plan, budgetHint?.reference].filter(Boolean);
    bRows.push(row);
    touched = true;
  }
  if (floorDraft && fRows.length < MAX_FLOORS) { fRows.push({ ...floorRow(floorDraft), fresh: true }); touched = true; }
  const el = document.createElement("details");
  el.className = "session-box budgets-box";
  el.open = !!(budgetDraft || floorDraft);
  host.replaceChildren(el);

  function render() {
    el.innerHTML = `<summary><h3>${te("budget.editor.title")}</h3></summary>
      <p class="hint">${te("budget.editor.hint")}</p>
      <h4>${te("budget.editor.budgets")}</h4>
      <small class="field-err" data-list-err="budgets" role="alert" hidden></small>
      <ul class="budget-list" aria-label="${te("budget.editor.budgets")}">${bRows.map((row, i) => budgetHtml(row, i, uid)).join("")}</ul>
      <button type="button" class="btn sm" data-add-budget ${bRows.length >= MAX_BUDGETS ? "disabled" : ""}>${icon("plus")}${te("budget.editor.addBudget")}</button>
      <h4>${te("budget.editor.floors")}</h4>
      <small class="field-err" data-list-err="floors" role="alert" hidden></small>
      <ul class="budget-list" aria-label="${te("budget.editor.floors")}">${fRows.map((row, i) => floorHtml(row, i, uid)).join("")}</ul>
      <button type="button" class="btn sm" data-add-floor ${fRows.length >= MAX_FLOORS ? "disabled" : ""}>${icon("plus")}${te("budget.editor.addFloor")}</button>`;
    check();
  }

  async function check() {
    const mine = ++seq;
    let result;
    try {
      // F-07: the edited account's index lets the core check each unit against its last good reading.
      result = await checkBudgets(index, provider, bRows.map(budgetOf), fRows.map(floorOf), currentLocale());
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
    // F-11: a row added here takes its metric's unit from the account's last reading, once.
    let prefilled = false;
    bRows.forEach((row, i) => {
      const unit = result.units?.[i];
      if (!row.prefill || row.unit.trim() || !unit) return;
      row.unit = unit;
      row.prefill = false;
      const input = el.querySelector(`[data-budget-row="${i}"] [data-k="unit"]`);
      if (input) input.value = unit;
      prefilled = true;
    });
    if (prefilled) return check();
    el.querySelectorAll("[data-budget-row]").forEach((li, i) => {
      const sentence = result.sentences[i];
      li.querySelector("[data-sentence]").textContent = sentence ? tl(sentence) : "";
    });
    el.querySelectorAll(".field-err").forEach((e) => { e.hidden = true; e.textContent = ""; });
    el.querySelectorAll("[aria-invalid]").forEach((e) => e.removeAttribute("aria-invalid"));
    // F-10: each problem under its own field (DS-CMP-MSG-1), the first one per field.
    for (const p of result.problems) {
      const m = /^(budgets|floors)(?:\[(\d+)\](?:\.(\w+))?)?$/.exec(p.field);
      if (!m) continue;
      let err;
      let control = null;
      if (m[2] == null) {
        err = el.querySelector(`[data-list-err="${m[1]}"]`);
      } else {
        const budget = m[1] === "budgets";
        const li = el.querySelector(`[data-${budget ? "budget" : "floor"}-row="${m[2]}"]`);
        const data = (budget ? bRows : fRows)[Number(m[2])];
        const key = FIELD_INPUTS[m[3]];
        if (!li || !data) continue;
        if (p.rule === "amount" && key === "amount" && !data.amount.trim() && data.fresh && !revealed) continue;
        control = key ? li.querySelector(`[data-k="${key}"]`) : null;
        err = control?.closest("label")?.querySelector(".field-err") ?? li.querySelector(":scope > .field-err");
      }
      if (!err || !err.hidden) continue;
      err.hidden = false;
      err.textContent = tl(p.note);
      control?.setAttribute("aria-invalid", "true");
    }
  }

  el.addEventListener("click", (e) => {
    const tgt = e.target;
    if (tgt.closest("[data-add-budget]")) {
      const first = targets?.[0];
      bRows.push({ ...budgetRow(first === "account" ? { scope: "account" } : first ? { kind: first } : {}), fresh: true, prefill: true });
      touched = true;
      return render();
    }
    if (tgt.closest("[data-add-floor]")) { fRows.push({ ...floorRow(), fresh: true }); touched = true; return render(); }
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
    if (input.dataset.k === "amount") row.fresh = false;
    // A new period offers its own start (weekday or day of the month).
    if (input.dataset.k === "period") { row.start = ""; return render(); }
    check();
  };
  el.addEventListener("input", onEdit);
  el.addEventListener("change", onEdit);
  render();
  if (budgetDraft || floorDraft) queueMicrotask(() => el.scrollIntoView?.({ block: "nearest" }));

  return {
    rows: () => (touched ? { budgets: bRows.map(budgetOf), floors: fRows.map(floorOf), locale: currentLocale() } : null),
    setProvider(next) { provider = next; check(); },
    /** F-10: Save was pressed: every problem shows, a new row's empty amount included. */
    reveal() { revealed = true; check(); },
  };
}
