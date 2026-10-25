// PRD 0022 shared components (docs/design-system.md section 4): the continuous meter with the pace corridor
// (DS-CMP-MTR), the figure block (DS-CMP-ROW-1b), the account-state ring glyph (DS-CMP-RING-6), the monogram
// (DS-CMP-MONO), the status line with its overflow order (DS-CMP-STL) and the honest-data marks (DS-CMP-HON).
// Tray rows, main-window cards and the details dialog draw the same markup; every interpretive text comes from
// the core (`tl`) or from the UI catalogs.
import { t, te, tl } from "./i18n.js";
import {
  ALERTS, bindingPct, blockedText, brand, esc, fmtAge, icon, incidentBadge, isBlocked, isStaleReading, level, logo, paceOf, reportKey,
  shownAt, staleView, statusLabel, summaryOf,
} from "./lib.js";
import { currentLocale } from "./i18n.js";

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
export function meterHtml(pct, { state = "fresh", expected = null, thin = false, role = null, text = "" } = {}) {
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
  const now = value == null ? "" : ` aria-valuenow="${Math.round(value)}"`;
  const label = text || (value == null ? t("meter.noData") : t("meter.left", { pct: Math.round(value) }));
  return `<div class="${cls}" role="meter" aria-valuemin="0" aria-valuemax="100"${now} aria-valuetext="${esc(label)}">${fill}${mark}</div>`;
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
/** Monogram letters for every account of a provider with more than one configured account (MONO-1..3). */
export function monograms(reports) {
  const byProvider = new Map();
  for (const r of [...reports].sort((a, b) => a.index - b.index)) {
    if (!byProvider.has(r.provider)) byProvider.set(r.provider, []);
    byProvider.get(r.provider).push(r);
  }
  const out = new Map();
  for (const list of byProvider.values()) {
    if (list.length < 2) continue;
    const used = new Set();
    for (const r of list) {
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
  const time = d.toLocaleTimeString(loc, { hour: "2-digit", minute: "2-digit" });
  const today = new Date(now);
  if (d.toDateString() === today.toDateString()) return time;
  const days = (d.getTime() - now) / 864e5;
  if (days > -1 && days < 6.5) return `${d.toLocaleDateString(loc, { weekday: "short" })} ${time}`;
  return `${d.toLocaleDateString(loc, { day: "numeric", month: "short" })}, ${time}`;
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
    label = t(`freshness.stale`);
    role = "warn";
    detail = fmtAge(shownAt(r));
  } else if (st.kind === "status") {
    label = statusLabel(r.status);
  } else if (st.kind === "exhausted") {
    label = blockedText(r) ? t("metric.exhausted") : t("tray.cat.exhausted");
    role = "bad";
    reset = isBlocked(r) ? r.binding.blocked_until : binding?.resets_at ?? null;
  } else if (pace) {
    label = tl(pace.label);
    role = { good: "good", warn: "warn", bad: "bad", neutral: "muted" }[pace.tone] ?? "muted";
    detail = pace.outlook ? tl(pace.outlook) : "";
    reset = binding.resets_at;
  } else if (binding) {
    const w = r.binding.window;
    label = w && w !== "other" ? t(`card.binding.${w}`) : r.binding.label ?? binding.label;
    role = "fg";
    reset = binding.resets_at;
  } else if (st.kind === "unknown") {
    label = t("meter.noData");
    role = "muted";
  }
  const at = fmtAt(reset);
  if (at) resetFull = t("stl.resets", { time: at });
  const full = [label, detail, resetFull].filter(Boolean).join(" · ");
  return { label, role, detail, reset: at, resetFull, full };
}
/** One-line status line (DS-CMP-STL-1): glyph, label, detail and the right-aligned reset; `fitStatusLines`
 *  applies the overflow order after layout. */
export function statusLineHtml(r, st = accountState(r), { glyph = true } = {}) {
  const p = statusParts(r, st);
  if (!p.label && !p.reset) return "";
  const lbl = p.label ? `<span class="lbl ${p.role}">${esc(p.label)}</span>` : "";
  const det = p.detail ? `<span class="det">· ${esc(p.detail)}</span>` : "";
  const rst = p.reset ? `<span class="rst" title="${esc(p.resetFull)}">${icon("returns")}<span>${esc(p.reset)}</span></span>` : "";
  return `<div class="stl" title="${esc(p.full)}"><span class="stl-l">${glyph ? stateGlyph(st) : ""}${lbl}${det}</span>${rst}</div>`;
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
    const lbl = left.querySelector(".lbl"), det = left.querySelector(".det");
    const avail = left.clientWidth;
    const width = (parts) => {
      if (det) det.hidden = !parts.detail;
      if (lbl && parts.label !== lbl.textContent) lbl.textContent = parts.label;
      return left.scrollWidth;
    };
    const steps = planFit(avail, { label: lbl?.textContent ?? "", compact: lbl?.dataset.c ?? "", detail: det?.textContent ?? "" }, width);
    line.classList.toggle("truncated", steps.includes("truncate"));
    if (steps.length) line.dataset.fit = steps.join(" ");
    else delete line.dataset.fit;
  }
}

/** Visually hidden live region (DS-A11Y-3): one per window, polite. */
export const liveRegionHtml = () => `<div class="sr-only" role="status" aria-live="polite" data-live></div>`;
export { te };
