// PRD 0022 shared components (docs/design-system.md section 4): the continuous meter with the pace corridor
// (DS-CMP-MTR), the figure block (DS-CMP-ROW-1b), the account-state ring glyph (DS-CMP-RING-6), the monogram
// (DS-CMP-MONO), the status line with its overflow order (DS-CMP-STL) and the honest-data marks (DS-CMP-HON).
// Tray rows, main-window cards and the details dialog draw the same markup; every interpretive text comes from
// the core (`tl`) or from the UI catalogs.
import { t, te, tl } from "./i18n.js";
import {
  ALERTS, billingOverage, bindingPct, brand, fmtExtra, fmtLocal, paceUnit, esc, fmtAge, icon, incidentBadge, isBlocked, isStaleReading, level, logo, paceOf, reportKey,
  shownAt, staleView, statusLabel, summaryOf, windowsOf,
  planBadgeHtml,
} from "./lib.js";
import { currentLocale } from "./i18n.js";
import { oneStatus, rowLayout } from "./density.js";

const clamp = (v) => Math.max(0, Math.min(100, v));

/** DS-CMP-MTR-6 geometry: the corridor (expected ± 5 points, clamped, at least 4 px wide at an end) in percent. */
export function corridor(expectedLeft) {
  if (expectedLeft == null || !Number.isFinite(expectedLeft)) return null;
  const c = clamp(expectedLeft);
  const lo = clamp(c - 5), hi = clamp(c + 5);
  return { center: c, left: lo, width: hi - lo };
}

/**
 * The continuous meter (DS-CMP-MTR-1..9). `pct` is the remaining share (null = unknown);
 * `state`: "fresh" | "stale" | "unknown" | "exhausted" | "overage"; `expected`: the expected remaining share from
 * the core's pace (only drawn on fresh meters); `thin`: the 4 px form with a tick instead of the corridor;
 * `text`: the accessible value text (DS-CMP-MTR-9), built by the caller from core texts.
 */
export function meterHtml(pct, { state = "fresh", expected = null, thin = false, role = null, text = "", name = "" } = {}) {
  const unknown = state === "unknown" || (pct == null && state !== "exhausted");
  const value = state === "exhausted" ? 0 : unknown ? null : clamp(pct);
  const lv = role ?? (state === "exhausted" ? "bad" : level(value));
  // DS-CMP-MTR-4: a value above 0 always shows a sliver; a value under 100 never fills the bar.
  const width = value == null || value <= 0 ? 0 : value < 1 ? 1 : value >= 100 ? 100 : Math.min(value, 99.5);
  const fill = width ? `<span class="mf" style="width:${width}%"></span>` : "";
  // DS-CMP-MTR-6b: no pace mark for stale, unknown, exhausted or overage meters.
  const band = state === "fresh" && value != null ? corridor(expected) : null;
  const mark = !band ? "" : thin
    ? `<span class="pm tick" style="left:${band.center}%"></span>`
    : `<span class="pm" style="left:${band.left}%;width:${band.width}%"></span>`;
  const cls = ["mtr", unknown ? "none" : lv, state === "stale" ? "stale" : "", thin ? "thin" : ""].filter(Boolean).join(" ");
  const label = text || (value == null ? t("meter.noData") : t("meter.left", { pct: Math.round(value) }));
  // PRD 0020 RNF (axe aria-required-attr): a meter without a value is not a meter; it is an image named "No data".
  if (value == null) return `<div class="${cls}" role="img" aria-label="${esc(name || label)}">${fill}${mark}</div>`;
  // axe aria-meter-name: every meter carries an accessible name (the caller's metric name, else its value text).
  return `<div class="${cls}" role="meter" aria-label="${esc(name || label)}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${Math.round(value)}" aria-valuetext="${esc(label)}">${fill}${mark}</div>`;
}

/** Expected remaining share (0–100) of a metric's core pace, or null. */
export const expectedLeft = (m) => {
  const p = m && paceOf(m);
  return p && Number.isFinite(p.expected_used_fraction) ? (1 - p.expected_used_fraction) * 100 : null;
};

/** DS-CMP-ROW-1b: figure on line 1, the unit word below it at --text-2xs; a word figure has no unit line. */
export const figureHtml = (figure, unit = "", { role = "", dim = false, cls = "" } = {}) =>
  `<span class="fig${dim ? " stale" : ""}${cls ? ` ${cls}` : ""}"><b class="${role}">${esc(figure)}</b>${unit ? `<small>${esc(unit)}</small>` : ""}</span>`;

// ---------- DS-CMP-RING-6: the static C-ring as the account-state glyph ----------
const RING_R = 11, RING_A0 = 45, RING_A1 = 315;
const ringPoint = (deg) => [14 + RING_R * Math.cos((deg * Math.PI) / 180), 14 + RING_R * Math.sin((deg * Math.PI) / 180)].map((v) => v.toFixed(2));
const ringArc = (from, to) => {
  const [x1, y1] = ringPoint(from), [x2, y2] = ringPoint(to);
  return `M${x1} ${y1}A${RING_R} ${RING_R} 0 ${to - from > 180 ? 1 : 0} 1 ${x2} ${y2}`;
};
/** The open C ring (DS-ID-8): `kind` = "pct" | "exhausted" | "unknown" | "stale"; always `aria-hidden`, the owner
 *  element carries the text. Static: no transition, no animation (DS-CMP-RING-3). */
export function ringHtml(kind, pct = null, size = 12, cls = "") {
  const sw = size <= 12 ? 5.4 : size <= 16 ? 4.8 : 3.4;
  const lv = kind === "exhausted" ? "bad" : level(pct);
  const share = kind === "pct" || kind === "stale" ? clamp(pct ?? 0) : 0;
  const track = `<path class="rt" d="${ringArc(RING_A0, RING_A1)}"/>`;
  const fill = share > 0 ? `<path class="ra" d="${ringArc(RING_A0, RING_A0 + ((RING_A1 - RING_A0) * Math.max(share, 3)) / 100)}"/>` : "";
  return `<svg class="ring ${kind} ${lv}${cls ? ` ${cls}` : ""}" width="${size}" height="${size}" viewBox="0 0 28 28" aria-hidden="true"><g fill="none" stroke-width="${sw}" stroke-linecap="round">${track}${fill}</g></svg>`;
}

// ---------- account state ----------
const STATE_ICON = { error: "alert", auth: "key", rate: "gauge", config: "wrench", unsupported: "ban" };
/**
 * One account's visual state, shared by rows, cards, tabs and the compact view.
 * `pct` = binding remaining share (or the preserved stale one), `kind` = meter state, `role` = level role,
 * `icon` set for non-quota states (the ring is replaced by that icon, DS-CMP-RING-6).
 */
export function accountState(r) {
  const stale = isStaleReading(r);
  if (stale) {
    const s = summaryOf(staleView(r));
    return { kind: "stale", pct: s.kind === "bar" ? s.pct : null, role: s.kind === "bar" ? level(s.pct) : "unknown", stale: true };
  }
  if (r.status !== "ready") {
    const a = ALERTS.find((c) => c.match(r));
    return { kind: "status", pct: null, role: a?.tone ?? "bad", icon: STATE_ICON[a?.id] ?? "alert" };
  }
  if (isBlocked(r) || r.class === "exhausted") return { kind: "exhausted", pct: 0, role: "bad" };
  const pct = bindingPct(r) ?? (summaryOf(r).kind === "bar" ? summaryOf(r).pct : null);
  if (pct == null) return { kind: "unknown", pct: null, role: "unknown" };
  return { kind: "pct", pct, role: level(pct) };
}
/** The 12–16 px glyph of an account state (ring, or the state icon in its role). */
export function stateGlyph(st, size = 12) {
  if (st.icon) return icon(st.icon, `sg ${st.role}`);
  return ringHtml(st.kind === "status" ? "unknown" : st.kind, st.pct, size);
}
/** Needs attention (DS-CMP-TAB-2): critical level, exhausted, stale, a failed query or a provider incident. */
export const needsAttention = (r, st = accountState(r)) =>
  st.kind === "status" || st.kind === "exhausted" || st.kind === "stale" || st.role === "bad" || !!incidentBadge(r);
const RANK = { status: 5, exhausted: 4, stale: 3 };
/** Worst state among accounts, for a tab badge. */
export function worstState(reports) {
  let worst = null;
  for (const r of reports) {
    const st = accountState(r);
    const score = RANK[st.kind] ?? (st.role === "bad" ? 2 : st.role === "warn" ? 1 : 0);
    if (!worst || score > worst.score || (score === worst.score && (st.pct ?? 101) < (worst.st.pct ?? 101))) worst = { st, score };
  }
  return worst?.st ?? null;
}

// ---------- DS-CMP-MONO: account monograms ----------
const fold = (s) => s.normalize("NFKD").replace(/[̀-ͯ]/g, "");
/** DS-CMP-MONO-4: the user's monogram override of a report (the view's `monogram`, from `desktop.json`): at most two
 *  characters, upper-cased; "" when there is none. */
export function monogramOverride(r) {
  const raw = typeof r?.monogram === "string" ? r.monogram.trim() : "";
  return [...raw].slice(0, 2).join("").toLocaleUpperCase();
}
/** Monogram letters for every account of a provider with more than one configured account (MONO-1..3); a user
 *  override (MONO-4) always wins and is shown even on a provider's only account, since the user asked for it. The
 *  derived letters of the provider's other accounts avoid it. */
export function monograms(reports) {
  const byProvider = new Map();
  for (const r of [...reports].sort((a, b) => a.index - b.index)) {
    if (!byProvider.has(r.provider)) byProvider.set(r.provider, []);
    byProvider.get(r.provider).push(r);
  }
  const out = new Map();
  for (const list of byProvider.values()) {
    if (list.length < 2) continue;
    const used = new Set(list.map(monogramOverride).filter(Boolean));
    for (const r of list) {
      if (monogramOverride(r)) continue;
      const label = fold(String(r.account ?? "")).replace(/@.*$/, "").trim();
      const words = label.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
      const letters = [...(words.join(""))].map((c) => c.toUpperCase());
      let mono = words.length >= 2 ? (words[0][0] + words[1][0]).toUpperCase() : letters.slice(0, 2).join("");
      if (!mono) mono = "?";
      if (used.has(mono)) {
        const first = mono[0];
        const next = letters.slice(1).find((c) => !used.has(first + c));
        mono = next ? first + next : first + ([2, 3, 4, 5, 6, 7, 8, 9].find((d) => !used.has(first + d)) ?? "");
      }
      used.add(mono);
      out.set(reportKey(r), mono);
    }
  }
  for (const r of reports) if (monogramOverride(r)) out.set(reportKey(r), monogramOverride(r));
  return out;
}
/** Provider tile with the monogram badge on its bottom-right corner (decorative, `aria-hidden`). */
export const tileHtml = (provider, size, mono = "") => {
  const tile = logo(provider, size);
  if (!mono) return tile;
  const h = Math.max(12, Math.round(size * 0.4));
  return `<span class="tile-wrap" style="width:${size}px;height:${size}px">${tile}<span class="mono-badge" style="height:${h}px;font-size:${size >= 40 ? 10 : 9}px" aria-hidden="true">${esc(mono)}</span></span>`;
};

// ---------- DS-CMP-ROW-1: titles ----------
/** Providers with more than one listed account: their rows take the account label as title. */
export const repeatedProviders = (reports) => {
  const seen = new Set(), repeated = new Set();
  for (const r of reports) (seen.has(r.provider) ? repeated : seen).add(r.provider);
  return repeated;
};

// ---------- DS-MSG-8: absolute times ----------
/** "19:30" today, "Sat 11:54" within 6 days, else "22 Oct, 19:30". */
export function fmtAt(iso, now = Date.now()) {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  const loc = currentLocale();
  const time = fmtLocal(d, { hour: "2-digit", minute: "2-digit" }, loc);
  const today = new Date(now);
  if (d.toDateString() === today.toDateString()) return time;
  const days = (d.getTime() - now) / 864e5;
  if (days > -1 && days < 6.5) return `${fmtLocal(d, { weekday: "short" }, loc)} ${time}`;
  return `${fmtLocal(d, { day: "numeric", month: "short" }, loc)}, ${time}`;
}

// ---------- DS-CMP-STL: the status line ----------
/**
 * The parts of an account's status line: `label` (role), `detail`, `reset` (absolute), `full` (tooltip and
 * accessible text). Label priority (DS-CMP-STL-3): blocked/exhausted > query failure > pace stage > binding window.
 */
export function statusParts(r, st = accountState(r)) {
  const binding = r.status === "ready" && r.binding ? r.metrics[r.binding.index] : null;
  const pace = binding ? paceOf(binding) : null;
  let label = "", role = st.role, detail = "", reset = null, resetFull = null;
  if (st.kind === "stale") {
    label = t("freshness.stale");
    role = "warn";
    detail = fmtAge(shownAt(r));
  } else if (st.kind === "status") {
    label = r.status === "authentication_error" ? t("state.auth") : statusLabel(r.status);
  } else if (st.kind === "exhausted") {
    label = t("stage.exhausted"); // the figure already reads "Exhausted"; the reset carries the return time
    role = "bad";
    reset = isBlocked(r) ? r.binding.blocked_until : binding?.resets_at ?? null;
    // Fa gap 4: on paid extra usage the one line says so ("Limit hit · on extra balance"); the money is the figure.
    if (billingOverage(r)) detail = t("stl.onExtra");
  } else if (pace) {
    // DS-CMP-STL-3: the pace stage names the line; the core's sentence stays in the tooltip.
    label = t(`stage.${pace.stage}`);
    role = { good: "good", warn: "warn", bad: "bad", neutral: "muted" }[pace.tone] ?? "muted";
    detail = [tl(pace.label), pace.outlook ? tl(pace.outlook) : ""].filter(Boolean).join(" · ");
    reset = binding.resets_at;
  } else if (binding) {
    // Fa gap 1: without a pace the line names the level (never the window, which the meter's limit already is); the
    // window stays in the detail and the tooltip.
    const w = r.binding.window;
    const lv = st.role === "good" || st.role === "warn" || st.role === "bad" ? st.role : null;
    label = lv ? t(`stage.level.${lv}`) : w && w !== "other" ? t(`card.binding.${w}`) : r.binding.label ?? binding.label;
    role = lv ?? "fg";
    if (lv) detail = w && w !== "other" ? t(`card.binding.${w}`) : r.binding.label ?? binding.label ?? "";
    reset = binding.resets_at;
  } else if (st.kind === "unknown") {
    label = t("meter.noData");
    role = "muted";
  }
  const at = fmtAt(reset);
  if (at) resetFull = t("stl.resets", { time: at });
  const full = [label, detail, resetFull].filter(Boolean).join(" · ");
  return { label, role, detail, reset: at, resetFull, full, pace, binding };
}

// ---------- DS-CMP-TIME: time chips (core texts, TUI shapes) ----------
const CHIP_ROLE = { "↑": "warn", "↓": "good", "→": "good", "✕": "bad", "↺": "muted" };
/** A time chip: glyph + text from the core (`Chip`), compact form when asked (DS-CMP-TIME-3); the full text is
 *  the tooltip and the accessible name. */
export function timeChipHtml(chip, { compact = false, role = null } = {}) {
  if (!chip) return "";
  const full = tl(chip.text);
  const text = compact && chip.compact ? chip.compact : full;
  return `<span class="tchip ${role ?? CHIP_ROLE[chip.glyph] ?? "muted"}" title="${esc(full)}" aria-label="${esc(full)}"><span aria-hidden="true">${esc(chip.glyph)}</span><span>${esc(text)}</span></span>`;
}
/** The stage chip in the user's unit (points by default, DS-CMP-TIME-6). */
export const stageChip = (pace) => pace?.chips?.[paceUnit()] ?? null;
/** The outlook chip: "↑ Runs out ≈18:53" when the core projects a stop before the reset, else "↓ Lasts to reset". */
export function outlookChip(m, pace) {
  if (m?.forecast?.chip) return m.forecast.chip;
  if (pace?.lasts_until_reset) return { glyph: "↓", text: { id: "pace.chip.lasts", params: {}, fallback: t("pace.chip.lasts") }, compact: null };
  return null;
}
/** The chips of a card's status line (mockup 10): the stage when it is not "on pace" and nothing projects, then the
 *  outlook. */
export function paceChipsHtml(m, pace) {
  if (!pace || pace.stage === "exhausted") return "";
  const outlook = outlookChip(m, pace);
  const stage = pace.stage !== "on_pace" && !m?.forecast ? timeChipHtml(stageChip(pace)) : "";
  return stage + timeChipHtml(outlook);
}

/** One-line status line (DS-CMP-STL-1): glyph, label, chips, detail and the right-aligned reset; `fitStatusLines`
 *  applies the overflow order after layout. `compact`: the tray row form (the projected run-out as a compact chip);
 *  `reset: false` leaves the reset to the card's windows. */
export function statusLineHtml(r, st = accountState(r), { glyph = true, compact = true, reset = true } = {}) {
  const p = statusParts(r, st);
  if (!p.label && !p.reset) return "";
  const lbl = p.label ? `<span class="lbl ${p.role}">${esc(p.label)}</span>` : "";
  const chips = !p.pace ? "" : compact ? timeChipHtml(p.binding?.forecast?.chip, { compact: true }) : paceChipsHtml(p.binding, p.pace);
  const det = (st.kind === "stale" || st.kind === "exhausted") && p.detail ? `<span class="det">· ${esc(p.detail)}</span>` : "";
  const rst = reset && p.reset ? `<span class="rst" title="${esc(p.resetFull)}">${icon("returns")}<span>${esc(p.reset)}</span></span>` : "";
  return `<span class="stl" title="${esc(p.full)}"><span class="stl-l">${glyph ? stateGlyph(st) : ""}${lbl}${chips}${det}</span>${rst}</span>`;
}

/**
 * DS-CMP-STL-5 overflow order as a pure plan: given the available width and a `measure(parts)` of the left group,
 * returns the steps to apply ("compact" label, "drop" detail, "truncate" label) until it fits. The returns time
 * never truncates (it is outside the measured group). Steps are applied in order and only when needed.
 */
export function planFit(avail, parts, measure) {
  const steps = [];
  let cur = { ...parts };
  if (measure(cur) <= avail) return steps;
  if (cur.compact) { cur = { ...cur, label: cur.compact }; steps.push("compact"); if (measure(cur) <= avail) return steps; }
  if (cur.detail) { cur = { ...cur, detail: "" }; steps.push("drop"); if (measure(cur) <= avail) return steps; }
  steps.push("truncate");
  return steps;
}
/** Applies `planFit` to every rendered status line under `root` (one measurement pass per render). */
export function fitStatusLines(root) {
  for (const line of root.querySelectorAll(".stl")) {
    const left = line.querySelector(".stl-l");
    if (!left) continue;
    const lbl = left.querySelector(".lbl"), det = left.querySelector(".det"), chips = [...left.querySelectorAll(".tchip")];
    const avail = left.clientWidth;
    const width = (parts) => {
      if (det) det.hidden = !parts.detail;
      for (const c of chips) c.hidden = !parts.detail;
      if (lbl && parts.label !== lbl.textContent) lbl.textContent = parts.label;
      return left.scrollWidth;
    };
    const steps = planFit(avail, { label: lbl?.textContent ?? "", compact: lbl?.dataset.c ?? "", detail: [det?.textContent, ...chips.map((c) => c.textContent)].filter(Boolean).join(" ") }, width);
    line.classList.toggle("truncated", steps.includes("truncate"));
    if (steps.length) line.dataset.fit = steps.join(" ");
    else delete line.dataset.fit;
  }
}


// ---------- PRD 0022 RF12.3 / RF13.4: the static change marker and the polite announcement ----------
// A refresh is identified by the view's `finished_at`. When a figure or level changed against the previous
// refresh, the account gets a static marker (an accent dot beside the figure, the previous value in its tooltip)
// until the next refresh or until the user opens it, and the window's live region announces the changes once.
let seenRefresh = null;
let previous = new Map(); // key → { pct, role, name }
const changes = new Map(); // key → previous { pct, role, name }
let pendingAnnouncement = "";
const snapshot = (reports) => new Map(reports.map((r) => {
  const st = accountState(r);
  return [reportKey(r), { pct: st.pct == null ? null : Math.round(st.pct), role: st.role, name: figureOf(r, st).fig, label: r.account }];
}));
/** Compares this view with the previous completed refresh; call before rendering. */
export function trackChanges(view) {
  if (!view?.finished_at || view.finished_at === seenRefresh) return;
  const now = snapshot(view.reports ?? []);
  changes.clear();
  const lines = [];
  if (seenRefresh != null) {
    for (const [key, cur] of now) {
      const old = previous.get(key);
      if (!old || (old.pct === cur.pct && old.role === cur.role && old.name === cur.name)) continue;
      changes.set(key, old);
      const r = view.reports.find((x) => reportKey(x) === key);
      const was = old.pct != null ? `${old.pct}%` : old.name;
      const value = cur.pct != null ? t("meter.left", { pct: cur.pct }) : cur.name;
      lines.push(t("live.changed", { account: `${brand(r.provider).label} · ${r.account}`, value, was }));
    }
  }
  pendingAnnouncement = lines.join(". ");
  seenRefresh = view.finished_at;
  previous = now;
}
/** The previous value when `key` changed in the last refresh (and was not opened since), else null. */
export const changeOf = (key) => changes.get(key) ?? null;
export const clearChange = (key) => changes.delete(key);
/** The marker beside a changed figure (decorative; the change is in the live region and the tooltip). */
export const changeMarkHtml = (key) => {
  const old = changeOf(key);
  if (!old) return "";
  const was = old.pct != null ? `${old.pct}%` : old.name;
  return `<i class="chg" aria-hidden="true" title="${esc(t("live.was", { value: was }))}"></i>`;
};
/** The window's one polite live region, outside the re-rendered root so assistive technology keeps it. */
function liveRegion() {
  let live = document.getElementById?.("cuotai-live");
  if (!live && document.body?.append) {
    live = document.createElement("div");
    live.id = "cuotai-live";
    live.className = "sr-only";
    live.setAttribute("role", "status");
    live.setAttribute("aria-live", "polite");
    document.body.append(live);
  }
  return live;
}
/** Writes the pending announcement into the window's live region once per refresh (DS-A11Y-3). */
export function announceChanges() {
  if (!pendingAnnouncement) return;
  const live = liveRegion();
  if (!live) return;
  live.textContent = pendingAnnouncement;
  pendingAnnouncement = "";
}
export { te };

// ---------- DS-CMP-TAB: filter tabs (tray header and main topbar) ----------
/** The tightest binding share among the listed accounts (exhausted counts as 0), or null. */
export function worstShare(reports) {
  let min = null;
  for (const r of reports) {
    const st = accountState(r);
    if (st.kind === "pct" || st.kind === "exhausted") min = min == null ? st.pct : Math.min(min, st.pct);
  }
  return min;
}

/** The filter id of an aggregator's own tab (PRD 0036 RF8.1): never a provider id. */
export const aggregatorTabId = (r) => `agg:${reportKey(r)}`;
/** Whether report `r` is in filter `filter` of the tabs ("all", a provider, or an aggregator's own tab). */
export const inTabFilter = (r, filter, aggregatorTabs = false) => filter === "all"
  || (filter.startsWith("agg:") ? aggregatorTabId(r) === filter : r.provider === filter && !(aggregatorTabs && r.aggregator));

/** Filter tabs (DS-CMP-TAB-1..5): "All" with the tightest ring and the count, then one icon tab per provider and,
 *  with `aggregatorTabs` (PRD 0036 RF8.1), one tab per aggregator account after them: its tile, the pool's ring and
 *  the connection count. An aggregator's connections add no tab of their upstream providers. */
export function filterTabsHtml(reports, { selected: filter = "all", moreOpen = false, maxTabs = 6, aggregatorTabs = false } = {}) {
  const providers = [];
  for (const r of reports) if (!(aggregatorTabs && r.aggregator) && !providers.includes(r.provider)) providers.push(r.provider);
  const aggs = aggregatorTabs ? reports.filter((r) => r.aggregator) : [];
  const ring = worstShare(reports);
  const allName = ring == null ? t("tabs.allAriaNoPct", { count: reports.length }) : t("tabs.allAria", { count: reports.length, pct: Math.round(ring) });
  const tab = (id, inner, name, extra = "") =>
    `<button class="tab${extra}" role="tab" data-tab="${esc(id)}" aria-selected="${filter === id}" tabindex="${filter === id ? 0 : -1}" aria-label="${esc(name)}" title="${esc(name)}">${inner}</button>`;
  const all = tab("all", `${ringHtml(ring == null ? "unknown" : "pct", ring, 16)}<span>${te("tabs.all")}</span><span class="cnt">${reports.length}</span>`, allName, " all");
  const providerTab = (p) => {
    const list = reports.filter((r) => r.provider === p);
    const attention = list.filter((r) => needsAttention(r));
    const worst = attention.length ? worstState(attention) : null;
    let name = t("tabs.provider", { provider: brand(p).label, count: list.length });
    if (worst) name = t("tabs.attention", { name, state: statusParts(attention.find((r) => accountState(r).kind === worst.kind) ?? attention[0]).label });
    const badge = worst ? `<i class="tst" aria-hidden="true">${stateGlyph(worst, 10)}</i>` : "";
    return tab(p, `<span class="tw">${tileHtml(p, 20)}${badge}</span>${list.length > 1 ? `<span class="cnt">${list.length}</span>` : ""}`, name, badge ? " badged" : "");
  };
  const aggTab = (r) => {
    const a = r.aggregator;
    const pct = r.status === "ready" && a.total ? (a.available / a.total) * 100 : null;
    const name = [brand(r.provider).label, r.account, t("agg.connections", { count: a.connections }), ...(a.pools ?? []).map((p) => tl(p.summary))].join(", ");
    return tab(aggregatorTabId(r), `<span class="tw">${tileHtml(r.provider, 20)}<i class="tst" aria-hidden="true">${ringHtml(pct == null ? "unknown" : "pct", pct, 10)}</i></span><span class="cnt">${a.connections}</span>`, name, " badged agg");
  };
  const entries = [...providers.map((p) => ({ id: p, provider: p, label: brand(p).label, html: () => providerTab(p) })),
    ...aggs.map((r) => ({ id: aggregatorTabId(r), provider: r.provider, label: `${brand(r.provider).label} · ${r.account}`, html: () => aggTab(r) }))];
  const overflow = entries.length > maxTabs + 1;
  const shown = overflow ? entries.slice(0, maxTabs) : entries;
  const rest = overflow ? entries.slice(maxTabs) : [];
  const restSel = rest.find((e) => e.id === filter);
  const more = rest.length ? `<span class="tab-more" role="presentation">
      <button class="tab" role="tab" data-more aria-haspopup="menu" aria-expanded="${moreOpen}" aria-selected="${!!restSel}" tabindex="${restSel ? 0 : -1}" aria-label="${te("tabs.more")}" title="${te("tabs.more")}">${restSel ? tileHtml(restSel.provider, 20) : icon("more")}</button>
      <span class="menu" role="menu" ${moreOpen ? "" : "hidden"}>${rest.map((e) => `<button role="menuitemradio" aria-checked="${filter === e.id}" data-tab="${esc(e.id)}">${tileHtml(e.provider, 16)}<span>${esc(e.label)}</span>${filter === e.id ? icon("tick") : ""}</button>`).join("")}</span>
    </span>` : "";
  return `<div class="tabs" role="tablist" aria-label="${te("tabs.label")}">${all}${shown.map((e) => e.html()).join("")}${more}</div>`;
}

// ---------- DS-CMP-ROW: the account row (tray list, previews) ----------
/** The figure of an account (DS-CMP-ROW-1a/-2): neutral "22%" with "left" below; a word in its role otherwise. */
export function figureOf(r, st = accountState(r)) {
  if (st.kind === "status") return { fig: statusLabel(r.status), unit: "", role: st.role, word: true };
  if (st.kind === "exhausted") {
    // DS-CMP-ROW-2, DS-COL-3: on extra balance the figure is the money being charged ("$6.25 extra").
    const o = billingOverage(r);
    if (o?.spent != null) return { fig: fmtExtra(o.spent, o.unit, o.currency), unit: t("figure.extra"), role: "warn" };
    // Fa gap 5: the figure stays a number ("0% left"); the status line already names the state in its role.
    return { fig: "0%", unit: t("card.available"), role: "" };
  }
  if (st.pct != null) return { fig: `${Math.round(st.pct)}%`, unit: t("card.available"), role: "", dim: st.kind === "stale" || isAging(r) };
  const s = summaryOf(r);
  if (s.kind === "text") return { fig: s.text, unit: "", role: "", word: true };
  return { fig: "—", unit: "", role: "" };
}
const INCIDENT_ROLE = { bad: "bad", warn: "warn", info: "info", muted: "muted" };
/** DS-CMP-ROW-4: at most one extra line: provider incident > aging age > active usage alert (a stale reading
 *  already says "Stale · age" on its status line). A fresh age is never repeated per row. */
export function extraLineHtml(r, st = accountState(r)) {
  const inc = incidentBadge(r);
  if (inc) return `<span class="xline ${INCIDENT_ROLE[inc.tone] ?? "muted"}" data-incident title="${esc(inc.text)}">${icon(inc.glyph)}<span>${esc(inc.text)}</span></span>`;
  if (!st.stale && r.status === "ready" && r.freshness === "aging") {
    const text = `${t("freshness.aging")} · ${fmtAge(shownAt(r))}`;
    return `<span class="xline warn" data-age>${icon("clock")}<span>${esc(text)}</span></span>`;
  }
  if (r.usage_alerts?.length) {
    const a = ALERTS.find((c) => c.id === "usage");
    return `<span class="xline warn">${icon("bell")}<span>${esc(a.label(r))}</span></span>`;
  }
  // PRD 0025 RF9.2: "Freshness age" on shows how old a fresh reading is, too.
  if (rowLayout().elements.age && !st.stale && r.status === "ready" && shownAt(r)) {
    return `<span class="xline muted" data-age>${icon("clock")}<span>${esc(fmtAge(shownAt(r)))}</span></span>`;
  }
  return "";
}
/** PRD 0025 RF9.2: the sub-limit preview line ("5h 66% · Week 22%"); empty when the account has neither window. */
export function subLimitsText(r) {
  if (r.status !== "ready") return "";
  const w = windowsOf(r);
  return [["window_5h", w.h5], ["weekly", w.week]].filter(([, x]) => x).map(([k, x]) => `${t(`card.binding.${k}`)} ${Math.round(x.pct)}%`).join(" · ");
}
export const subLimitsHtml = (r) => { const s = subLimitsText(r); return s ? `<span class="subl">${esc(s)}</span>` : ""; };
/** PRD 0025 RF9.4 (DS-CMP-DENS-3): Minimal's single status item, by priority; the full status stays in the row's
 *  accessible name and tooltip. */
export function oneStatusHtml(r, st = accountState(r)) {
  const inc = incidentBadge(r);
  const p = statusParts(r, st);
  const chip = p.binding?.forecast?.chip ?? null;
  const kind = oneStatus({ incident: !!inc, stale: st.kind === "stale", exhausted: st.kind === "exhausted" || st.kind === "status", runsOut: !!chip, pace: !!p.pace });
  let inner = "";
  if (kind === "incident") inner = `<span class="lbl ${INCIDENT_ROLE[inc.tone] ?? "muted"}">${esc(inc.text)}</span>`;
  else if (kind === "stale" || (kind === "exhausted" && !p.reset)) inner = `<span class="lbl ${p.role}">${esc(p.label)}</span>${p.detail && kind === "stale" ? `<span class="det">· ${esc(p.detail)}</span>` : ""}`;
  else if (kind === "exhausted") inner = `<span class="lbl bad">${esc(p.label)}</span><span class="rst" title="${esc(p.resetFull)}">${icon("returns")}<span>${esc(p.reset)}</span></span>`;
  else if (kind === "runs_out") inner = timeChipHtml(chip, { compact: true });
  else if (kind === "pace") inner = `<span class="lbl ${p.role}">${esc(p.label)}</span>`;
  return inner ? `<span class="stl one" title="${esc(p.full)}"><span class="stl-l">${inner}</span></span>` : "";
}
/** Accessible name of a row (DS-CMP-RING-5): "Claude Code, work, 22% left, Ahead of pace, resets 19:30". */
export function rowName(r, st = accountState(r)) {
  const f = figureOf(r, st), p = statusParts(r, st);
  const value = f.unit ? t("meter.left", { pct: Math.round(st.pct) }) : f.fig;
  return [brand(r.provider).label, r.account, value, p.label !== f.fig ? p.label : "", p.detail, p.resetFull].filter(Boolean).join(", ");
}
/** Fa gap 9 (mockup 06): an aging reading is drawn like a stale one: dimmed figure, dotted meter, no pace mark. */
const isAging = (r) => r.status === "ready" && r.freshness === "aging";
/** The row's meter with its accessible value text (DS-CMP-MTR-9); none for a failed query without a reading. */
export function accountMeterHtml(r, st = accountState(r), { thin = false } = {}) {
  if (st.kind === "status") return "";
  const binding = r.status === "ready" && r.binding ? r.metrics[r.binding.index] : null;
  const p = statusParts(r, st);
  return meterHtml(st.pct, {
    thin,
    state: st.kind === "stale" || (st.kind === "pct" && isAging(r)) ? "stale" : st.kind === "exhausted" ? "exhausted" : st.pct == null ? "unknown" : "fresh",
    expected: st.kind === "pct" && !isAging(r) ? expectedLeft(binding) : null,
    text: st.pct == null ? t("meter.noData") : p.full ? t("meter.valuetext", { pct: Math.round(st.pct), status: p.full }) : "",
  });
}
/**
 * One account row (DS-CMP-ROW-1..7). `title`/`sub`: the title line (the account label is the title when the provider
 * has several accounts); `mono`: monogram; `open`: expanded body HTML (accordion) or null; `chips`: the row's
 * feature chips (extra usage, best account, a 9router connection's state), on one wrapping line.
 */
export function rowHtml(r, { title, sub = "", mono = "", open = null, key = reportKey(r), chips = "" } = {}) {
  const st = accountState(r);
  const f = figureOf(r, st);
  const meter = accountMeterHtml(r, st);
  // PRD 0025 RF9 / PRD 0022 RF8.2: the row display; hidden elements are left out by the root's CSS hook, Minimal
  // shows one status. The plan badge and the sub-limit line exist only when the account has them; the accessible
  // name carries every element whatever the layout (DS-CMP-DENS-2).
  const lay = rowLayout();
  const plan = r.plan ?? r.connection?.plan ?? "";
  const dup = r.connection?.duplicate_of ? " dup" : "";
  const name = [rowName(r, st), plan, subLimitsText(r)].filter(Boolean).join(", ");
  const status = lay.one ? oneStatusHtml(r, st) : `${statusLineHtml(r, st)}${extraLineHtml(r, st)}`;
  return `<article class="row${open != null ? " open" : ""}${dup}" data-row="${esc(key)}">
    <button class="rh" data-open="${esc(key)}" aria-expanded="${open != null}" aria-label="${esc(name)}" title="${esc(name)}">
      ${tileHtml(r.provider, 32, mono)}
      <span class="rc">
        <span class="r1"><span class="nm">${esc(title)}</span>${planBadgeHtml({ plan })}${sub ? `<span class="ac">${esc(sub)}</span>` : ""}${changeMarkHtml(key)}${figureHtml(f.fig, f.unit, { role: f.role, dim: f.dim || isAging(r), cls: f.word ? "wordfig" : "" })}${icon(open != null ? "chevDown" : "chev", "chev")}</span>
        ${meter ? `<span class="rm">${meter}</span>` : ""}
        ${status}
        ${lay.elements.sublimits ? subLimitsHtml(r) : ""}
        ${chips ? `<span class="row-chips">${chips}</span>` : ""}
      </span>
    </button>
    ${open != null ? `<div class="rb">${open}</div>` : ""}
  </article>`;
}
