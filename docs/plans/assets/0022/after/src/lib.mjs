// Shared helpers of the PRD 0022 "AFTER" mockup generator. Node 22+, no dependencies.
// The generated HTML is static: icons, provider tiles, segmented meters and the ring are inlined.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { here } from "./lib-paths.mjs";
import { ago, cur, t, wd } from "./i18n.mjs";
export { here };
export const UI = "../../../../../../apps/desktop/ui"; // relative to src/ (served from the worktree root)
const libJs = readFileSync(join(here, "../../../../../../apps/desktop/ui/js/lib.js"), "utf8");

// ---- icons: the real set from js/lib.js, plus glyphs the design system proposes to add (DS-TOK-ICO-1) ----
const real = new Function(`return ({${/const paths = \{([\s\S]*?)\n\};/.exec(libJs)[1]}\n})`)();
const added = {
  chevDown: '<path d="m6 9 6 6 6-6"/>',
  chevUp: '<path d="m18 15-6-6-6 6"/>',
  tick: '<path d="M20 6 9 17l-5-5"/>',
  chart: '<path d="M3 3v16a2 2 0 0 0 2 2h16"/><path d="m19 9-5 5-4-4-3 3"/>',
  chartBar: '<path d="M3 3v16a2 2 0 0 0 2 2h16"/><path d="M18 17V9"/><path d="M13 17V5"/><path d="M8 17v-3"/>',
  info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
  download: '<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>',
  puzzle: '<path d="M15.39 4.39a1 1 0 0 0 1.68-.474 2.5 2.5 0 1 1 3.014 3.015 1 1 0 0 0-.474 1.68l1.683 1.682a2.414 2.414 0 0 1 0 3.414L19.61 15.39a1 1 0 0 1-1.68-.474 2.5 2.5 0 1 0-3.014 3.015 1 1 0 0 1 .474 1.68l-1.683 1.682a2.414 2.414 0 0 1-3.414 0L8.61 19.61a1 1 0 0 0-1.68.474 2.5 2.5 0 1 1-3.014-3.015 1 1 0 0 0 .474-1.68l-1.683-1.682a2.414 2.414 0 0 1 0-3.414L4.39 8.61a1 1 0 0 1 1.68.474 2.5 2.5 0 1 0 3.014-3.015 1 1 0 0 1-.474-1.68l1.683-1.682a2.414 2.414 0 0 1 3.414 0z"/>',
  sliders: '<line x1="21" x2="14" y1="4" y2="4"/><line x1="10" x2="3" y1="4" y2="4"/><line x1="21" x2="12" y1="12" y2="12"/><line x1="8" x2="3" y1="12" y2="12"/><line x1="21" x2="16" y1="20" y2="20"/><line x1="12" x2="3" y1="20" y2="20"/><line x1="14" x2="14" y1="2" y2="6"/><line x1="8" x2="8" y1="10" y2="14"/><line x1="16" x2="16" y1="18" y2="22"/>',
  shield: '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>',
  plug: '<path d="M12 22v-5"/><path d="M9 8V2"/><path d="M15 8V2"/><path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"/>',
  file: '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>',
  menubar: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 9h18"/>',
  history: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>',
  zap: '<path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/>',
  globe: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>',
  ext: '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  more: '<circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/><circle cx="5" cy="12" r="1"/>',
  dollar: '<line x1="12" x2="12" y1="2" y2="22"/><path d="M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6"/>',
  terminal: '<polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/>',
  db: '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/><path d="M3 12A9 3 0 0 0 21 12"/>',
  arrowUp: '<path d="m5 12 7-7 7 7"/><path d="M12 19V5"/>',
  filter: '<path d="M3 6h18"/><path d="M7 12h10"/><path d="M10 18h4"/>',
  rocket: '<path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z"/><path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z"/><path d="M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0"/><path d="M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5"/>',
  returns: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>',
  arrowDown: '<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>',
  arrowRight: '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
  mapPin: '<path d="M20 10c0 6-8 12-8 12s-8-6-8-12a8 8 0 0 1 16 0Z"/><circle cx="12" cy="10" r="3"/>',
};
export const paths = { ...real, ...added };
export const ic = (name, cls = "") => {
  if (!paths[name]) throw new Error(`icon ${name}`);
  return `<svg class="i ${cls}" viewBox="0 0 24 24" aria-hidden="true">${paths[name]}</svg>`;
};

// ---- provider tiles: [glyph file, tile color, glyph color] copied from js/lib.js `brands` + `glyphs` ----
const T = {
  claude: ["claude", "#d97757"], codex: ["codex", "#111113"], cursor: ["cursor", "#1c1c1e"], kimi: ["kimi", "#1783ff"],
  groq: ["groq", "#f55036"], anthropic: ["anthropic", "#d97757"], openai: ["openai", "#10a37f"], gemini: ["gemini", "#4285f4"],
  copilot: ["githubcopilot", "#6e40c9"], windsurf: ["windsurf", "#09b6a2"], mistral: ["mistral", "#fa520f"],
  perplexity: ["perplexity", "#20b8cd"], ollama: ["ollama", "#f4f4f5", "#111113"], kiro: ["kiro", "#8a5cf6"], qwen: ["qwen", "#615ced"],
  grok: ["grok", "#111113"], antigravity: ["antigravity", "#7c6cf0"], zed: ["zed", "#084ccf"], cline: ["cline", "#18181b"],
  nvidia: ["nvidia", "#0c0c0e", "#76b900"], bedrock: ["bedrock", "#232f3e", "#ff9900"], azure: ["azure", "#0078d4"],
  vertex: ["vertexai", "#4285f4"], cloudflare: ["cloudflare", "#f6821f"], minimax: ["minimax", "#e73b77"], opencode: ["opencode", "#111113"], qoder: ["qoder", "#0c0c0e", "#22c55e"],
};
export const tile = (p, s = 32, mono = "") => {
  const [f, c, g = "#fff"] = T[p];
  const mh = Math.max(12, Math.round(s * 0.4)); // DS-CMP-MONO-1: badge height 40% of the tile, min 12 px
  const fs = s >= 40 ? 10 : 9;
  const badge = mono ? `<span class="mg" style="height:${mh}px;font-size:${fs}px" aria-hidden="true">${mono}</span>` : "";
  return `<span class="tile" style="--s:${s}px;--c:${c};--g:${g};--m:url(${UI}/img/providers/${f}.svg)"><i></i>${badge}</span>`;
};
/** Monochrome provider glyph (menu bar items are template images: no tile color, DS-CMP-MENU-1). */
export const glyph = (p, s = 12) => `<span class="mgl" style="--s:${s}px;--m:url(${UI}/img/providers/${T[p][0]}.svg)"></span>`;
export const LABEL = { claude: "Claude Code", codex: "OpenAI Codex", cursor: "Cursor", kimi: "Kimi", groq: "Groq", anthropic: "Anthropic API" };

// ---- C-ring (DS-CMP-RING): the "C" of Cuotai, one open arc, static, one value ----
const RING = (pct, level, sw, size, extra = "") => {
  const cx = 14, r = 11, a0 = 45, a1 = 315;
  const pt = (deg) => [cx + r * Math.cos((deg * Math.PI) / 180), cx + r * Math.sin((deg * Math.PI) / 180)].map((v) => v.toFixed(2));
  const seg = (from, to) => { const [x1, y1] = pt(from), [x2, y2] = pt(to); return `M${x1} ${y1}A${r} ${r} 0 ${to - from > 180 ? 1 : 0} 1 ${x2} ${y2}`; };
  const track = `<path d="${seg(a0, a1)}" stroke="var(--track)"/>`;
  const fill = pct > 0 ? `<path d="${seg(a0, a0 + (a1 - a0) * Math.max(pct, 3) / 100)}" stroke="var(--${level})"/>` : ""; // never an invisible arc for > 0
  return `<svg class="ring" width="${size}" height="${size}" viewBox="0 0 28 28" role="img" aria-label="${Math.round(pct)}%"><g fill="none" stroke-width="${sw}" stroke-linecap="round">${extra}${track}${fill}</g></svg>`;
};
/** Gauge ring (All tab 16 px, per-account tile 64 px, menu-bar style). pct null = unknown (empty track). */
export function ring(pct, size = 22, role = null, sw = 4.2) {
  const level = role ?? levelOf(pct ?? 0);
  return RING(pct ?? 0, level, sw, size);
}
/** Account-state glyph (DS-CMP-RING-6), 12-16 px: arc = share; exhausted = tinted track; unknown = plain 40% track; stale = dotted + dim. */
export function stateRing(kind, pct = 0, size = 12) {
  const sw = size <= 12 ? 5.4 : 4.8;
  if (kind === "exhausted") return RING(0, "bad", sw, size).replace('stroke="var(--track)"', 'stroke="var(--bad)" stroke-opacity=".55"');
  if (kind === "unknown") return RING(0, "info", sw, size).replace("<g ", '<g opacity=".4" ');
  if (kind === "stale") return RING(pct, levelOf(pct), sw, size).replace("<g ", '<g opacity=".5" stroke-dasharray="0.1 4.2" ').replace(/stroke-linecap="round"/, 'stroke-linecap="round"');
  return RING(pct, levelOf(pct), sw, size);
}
export const stateIcon = (name, role, size = "s12") => ic(name, `${size} r-${role}`);

// ---- continuous meter (DS-CMP-MTR): one solid bar; pace corridor on 6 px bars, tick on 4 px bars ----
export const levelOf = (p) => (p >= 70 ? "good" : p >= 30 ? "warn" : "bad");
export function meter(left, { pace, role, none = false, dim = false, thin = false, label = "" } = {}) {
  const lv = role ?? levelOf(left ?? 0);
  const fill = left == null || left <= 0 ? "" : `<span class="mf" style="width:${Math.max(left, 0.8)}%"></span>`; // > 0 never draws nothing (DS-CMP-MTR-4)
  let pm = "";
  if (pace != null && !dim && !none) { // DS-CMP-MTR-6b: no pace mark for stale or unknown
    if (thin) pm = `<span class="pm" style="left:${pace}%"></span>`;
    else { const lo = Math.max(pace - 5, 0), hi = Math.min(pace + 5, 100); pm = `<span class="pm" style="left:${lo}%;width:${hi - lo}%"></span>`; }
  }
  const cls = ["mtr", none ? "none" : lv, dim ? "dim" : "", thin ? "thin" : ""].join(" ");
  const val = left == null ? "" : ` aria-valuemin="0" aria-valuemax="100" aria-valuenow="${left}"`;
  return `<div class="${cls}" role="meter"${val} aria-valuetext="${label}">${fill}${pm}</div>`;
}
export const bar = (left, role) => `<div class="bar ${role ?? levelOf(left)}"><span style="width:${Math.max(1, left)}%"></span></div>`;

// ---- DS-CMP-STL-5: the status line yields in a fixed order until it fits one line ----
// 1 label -> compact form ("↑ 20 pts"), 2 drop the detail (and the runs-out chip, which is detail), 3 returns stays compact text, 4 truncate the label.
const FIT = `(() => { document.querySelectorAll(".rc > .stl").forEach((s) => { const l = s.querySelector(".stl-l"); if (!l) return; const over = () => l.scrollHeight > l.clientHeight + 1 || l.scrollWidth > l.clientWidth + 1; const steps = []; const lbl = l.querySelector(".lbl");
  if (over() && lbl && lbl.dataset.c) { lbl.textContent = lbl.dataset.c; steps.push("compact label"); }
  if (over()) { const d = []; l.querySelectorAll(".det, .chip").forEach((e) => { d.push(e.textContent.trim()); e.style.display = "none"; }); if (d.length) steps.push("dropped: " + d.join(" | ")); }
  if (over() && lbl) { lbl.style.cssText += ";overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:100%"; steps.push("truncated label"); }
  if (steps.length) s.dataset.fit = steps.join("; "); }); })();`;

// ---- page shell ----
export function page({ title, w, h, theme = "dracula", mode = "dark", body, css = "", bodyClass = "" }) {
  return `<!doctype html>
<html lang="${cur.loc}" data-theme="${theme}" data-mode="${mode}">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=${w}, initial-scale=1" />
<title>${title}</title>
<link rel="stylesheet" href="${UI}/css/themes.css" />
<link rel="stylesheet" href="${UI}/css/themes/index.css" />
<link rel="stylesheet" href="common.css" />
<style>
html, body { width: ${w}px; height: ${h}px !important; overflow: hidden; }
${css}
</style>
</head>
<body class="${bodyClass}">
${body}
<script>${FIT}</script>
</body>
</html>
`;
}

// ---- tray (popover) pieces ----
export function trayFooter({ min = 1, ageRole = "", notice = "" } = {}) {
  const age = ago(min);
  return `${notice}<footer class="pf">
  <button class="btn ghost sm" aria-keyshortcuts="Meta+,">${ic("gear")} <span>${t("settings")}</span></button>
  <button class="btn ghost sm">${ic("chartBar")} <span>${t("stats")}</span></button>
  <span class="grow"></span>
  <span class="mono sm ${ageRole || "muted"} nowrap" title="${age}">${age}</span>
  <button class="ib" aria-label="${t("common.refreshAll")}">${ic("refresh")}</button>
</footer>`;
}

// tabs: [{p, n, st}] with st = {kind: "pct"|"exhausted"|"stale"|"icon", pct, icon, role}: the worst account's state (DS-CMP-TAB-2)
export const tabState = (st) => !st ? "" : `<i class="tst" aria-hidden="true">${st.kind === "icon" ? stateIcon(st.icon, st.role) : stateRing(st.kind, st.pct ?? 0, 10)}</i>`;
export function trayHeader({ tabs, selected = "all", ringPct = 22, total = 7, title = null, back = false, right = "" }) { // DS-CMP-HEAD-2: the tray header holds only the filter tabs (add account lives in the main window and Settings)
  if (back) {
    return `<header class="ph"><button class="ib" aria-label="${t("common.back")}">${ic("back", "s18")}</button><h1>${title}</h1><span class="grow"></span>${right}<span class="kbd" aria-hidden="true">esc</span></header>`;
  }
  const all = `<button class="tab all ${selected === "all" ? "on" : ""}" role="tab" aria-label="${t("tabs.allAria", { pct: ringPct })}">${ring(ringPct, 16, null, 5)}<span>${t("tabs.all")}</span><span class="cnt">${total}</span></button>`;
  const tabHtml = tabs.map((x) => `<button class="tab ${selected === x.p ? "on" : ""}" role="tab" aria-label="${LABEL[x.p]}${x.n > 1 ? `, ${x.n}` : ""}"><span class="tw">${tile(x.p, 20)}${tabState(x.st)}</span>${x.n > 1 ? `<span class="cnt">${x.n}</span>` : ""}</button>`).join("");
  return `<header class="ph"><div class="tabs" role="tablist">${all}${tabHtml}</div><span class="grow"></span>${right}</header>`;
}
const S_PCT = (pct) => ({ kind: "pct", pct });
export const TABS = [
  { p: "claude", n: 2, st: S_PCT(22) }, { p: "codex", n: 1 }, { p: "cursor", n: 1, st: { kind: "exhausted" } },
  { p: "kimi", n: 1 }, { p: "groq", n: 1, st: { kind: "stale", pct: 74 } }, { p: "anthropic", n: 1, st: { kind: "icon", icon: "key", role: "accent" } },
];

// ---- shared time chips (DS-CMP-TIME): TUI pace shapes, wording from core keys ----
const GLYPH = { reserve: "arrowDown", on_pace: "arrowRight", ahead: "arrowUp", exhausted: "x", runs_out: "arrowUp", lasts: "arrowDown", returns: "returns", reserve_pct: "arrowDown", ahead_pct: "arrowUp" };
const ROLE = { reserve: "good", on_pace: "good", ahead: "warn", exhausted: "bad", runs_out: "warn", lasts: "good", returns: "", reserve_pct: "good", ahead_pct: "warn" };
/** Time/pace chip (DS-CMP-TIME). `plain`: no chip background (compact "↺ dom 17:20" at the end of a status line). */
export function tchip(kind, vars = {}, { compact = false, role = "", plain = false } = {}) {
  const role_ = role || ROLE[kind];
  const full = t(`chip.${kind}`, vars);
  const text = compact && vars.at ? (kind === "runs_out" ? `≈${vars.at}` : vars.at) : full;
  if (plain) return `<span class="ret ${role_ ? "r-" + role_ : ""}" title="${full}">${ic(GLYPH[kind], "s12")}<span>${text}</span></span>`;
  return `<span class="chip tc ${role_}" title="${full}">${ic(GLYPH[kind], "s12")}<span>${text}</span></span>`;
}

// account row (DS-CMP-ROW). o.open adds the expanded body.
export function row(o) {
  const chev = ic(o.open ? "chevDown" : "chev");
  const fig = o.figHtml ?? `<b class="${o.figRole ?? ""}">${o.fig}</b>${o.unit && !o.hideUnit ? `<small>${o.unit}</small>` : ""}`;
  const stt = o.status;
  // one line, never three: the left group keeps what fits and DROPS the rest in order (detail, then chips); the returns text stays
  const st = stt
    ? `<div class="stl" title="${[stt.label, stt.det].filter(Boolean).join(" · ")}"><span class="stl-l">${stt.ring ?? ""}${stt.label ? `<span class="lbl ${stt.fgc ? "fgc" : `r-${stt.role}`}"${stt.labelC ? ` data-c="${stt.labelC}"` : ""}>${stt.label}</span>` : ""}${(stt.chips ?? []).join("")}${stt.det ? `<span class="det">· ${stt.det}</span>` : ""}</span>${stt.ret ? `<span class="retw">${stt.ret}</span>` : ""}</div>`
    : "";
  const extra = o.extra ? `<div class="stl x r-${o.extra.role}" title="${o.extra.label} · ${o.extra.det ?? ""}"><span class="stl-l">${ic(o.extra.icon, "s14")}<span class="lbl">${o.extra.label}</span>${o.extra.det ? `<span class="det">· ${o.extra.det}</span>` : ""}</span></div>` : "";
  const sublim = o.sublim ? `<div class="sublim muted">${o.sublim}</div>` : "";
  return `<article class="row ${o.open ? "open" : ""}">
  <button class="rh" aria-expanded="${!!o.open}">
    ${tile(o.p, 32, o.mono)}
    <div class="rc">
      <div class="r1"><span class="nm ${o.acct ? "" : "tr"}" ${o.provider ? `title="${o.provider}"` : ""}>${o.name}</span>${o.plan ? `<span class="plan">${o.plan}</span>` : ""}${o.acct ? `<span class="ac trunc">${o.acct}</span>` : ""}<span class="fg ${o.figDim ? "dim" : ""} ${o.unit || o.hideUnit ? "" : "wrapfig"}">${fig}</span>${chev}</div>
      ${o.meter ? `<div class="rm">${o.meter}</div>` : ""}
      ${st}${sublim}${extra}
    </div>
  </button>
  ${o.open ? `<div class="rb">${o.open}</div>` : ""}
</article>`;
}

// sub-limit row (DS-CMP-SUB)
export function sub({ label, binding = false, fig, unit = t("left"), figRole = "", meterHtml, pace, paceRole = "good", paceIcon = "check", reset = "", figHtml }) {
  return `<div class="sub">
  <div class="s1"><span class="sl">${label}</span>${binding ? `<span class="chip accent">${t("binding")}</span>` : ""}<span class="grow"></span><span class="sf">${figHtml ?? `<b class="${figRole}">${fig}</b><small>${unit}</small>`}</span></div>
  ${meterHtml}
  <div class="s3">${pace ? `<span class="r-${paceRole} pc">${ic(paceIcon, "s14")} ${pace}</span>` : "<span></span>"}<span class="mono muted">${reset}</span></div>
</div>`;
}

// reset timeline (DS-CHT-6)
export function timeline({ range = "7d", lanes, later = [], unknown = [], withHeader = true }) {
  const max = range === "7d" ? 168 : 24;
  const ticks = range === "7d"
    ? [[23, 5.65], [24, 29.65], [25, 53.65], [26, 77.65], [27, 101.65], [28, 125.65], [29, 149.65]].map(([d, h]) => [wd(d), h])
    : [["20:00", 1.65], ["00:00", 5.65], ["04:00", 9.65], ["08:00", 13.65], ["12:00", 17.65], ["16:00", 21.65]];
  const lanesHtml = lanes.map((l) => {
    const x = (l.h / max) * 100;
    const right = x > 60;
    return `<div class="tl-lane"><span class="tl-t">${tile(l.p, 14)}</span><div class="tl-plot"><span class="tl-base"></span><span class="tl-line r-${l.role}" style="width:${x}%"></span><span class="tl-dot r-${l.role}" style="left:${x}%"></span><span class="tl-lbl mono ${right ? "left" : ""}" style="left:${x}%">${l.label}</span></div></div>`;
  }).join("");
  const laterHtml = later.map((l) => `<div class="tl-lane"><span class="tl-t">${tile(l.p, 14)}</span><div class="tl-plot"><span class="tl-base"></span><span class="tl-later mono">${l.label} ${ic("chev", "s12")}</span></div></div>`).join("");
  const ax = ticks.map(([tk, h]) => `<span style="left:${(h / max) * 100}%">${tk}</span>`).join("");
  const unk = unknown.length ? `<div class="tl-unk sm muted">${unknown.map((u) => `<span class="u">${tile(u.p, 14)} <span>${u.label}</span></span>`).join("")}</div>` : "";
  return `<section class="tl" aria-label="${t("tl.title")}">
  ${withHeader ? `<div class="tl-h"><div><h2>${t("tl.title")}</h2><span class="sm muted">${t("tl.sub")}</span></div><div class="seg mono" role="radiogroup"><button class="${range === "24h" ? "on" : ""}">24h</button><button class="${range === "7d" ? "on" : ""}">7d</button></div></div>` : ""}
  <div class="tl-body"><div class="tl-now"><span class="sm">${t("now")}</span></div>${lanesHtml}${laterHtml}<div class="tl-ax axis">${ax}</div></div>
  ${unk}
</section>`;
}

export const TRAY_CSS = `
.pop { display: flex; flex-direction: column; width: 380px; height: 600px; border: var(--border-w) solid var(--border); border-radius: var(--radius-lg); background: var(--bg); overflow: hidden; }
.ph { display: flex; align-items: center; gap: var(--space-2); height: 48px; padding: 0 var(--space-3) 0 var(--space-3); background: var(--surface); border-bottom: var(--border-w) solid var(--border); flex: none; }
.ph h1 { margin: 0; font-size: var(--text-lg); line-height: 1.3; font-weight: 650; }
.tabs { display: flex; align-items: center; gap: var(--space-0); }
.tab { position: relative; display: inline-flex; align-items: center; justify-content: center; gap: 3px; height: 28px; min-width: 28px; padding: 0 4px; border: var(--border-w) solid transparent; border-radius: var(--radius-sm); background: transparent; color: var(--fg-muted); font-size: var(--text-base); }
.tab.all { gap: 6px; padding: 0 var(--space-2); color: var(--fg); font-weight: 500; }
.tab.on { background: var(--surface-2); border-color: var(--border); color: var(--fg); }
.tab .cnt { font-size: var(--text-sm); color: var(--fg-muted); font-weight: 500; }
.tab .tw { display: inline-flex; align-items: flex-start; gap: 2px; }
.tab .tst { display: grid; place-items: center; width: 10px; height: 10px; margin-top: -3px; } /* outside the tile, top right, never over the logo */
.pbody { flex: 1; min-height: 0; overflow: hidden; position: relative; }
.pf { display: flex; align-items: center; gap: var(--space-1); height: 44px; padding: 0 var(--space-2); background: var(--surface); border-top: var(--border-w) solid var(--border); flex: none; }
.pf .btn { padding: 0 var(--space-2); flex: none; }
.pf .btn.warn { color: var(--warn); }
.notice { display: flex; align-items: center; gap: var(--space-2); padding: var(--space-2) var(--space-4); background: var(--surface); border-top: var(--border-w) solid var(--border); font-size: var(--text-sm); flex: none; }
.row { border-bottom: var(--border-w) solid var(--border); }
.rh { display: flex; gap: var(--space-3); width: 100%; padding: var(--space-3); border: 0; background: transparent; text-align: left; align-items: flex-start; }
.rc { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; }
.r1 { display: flex; align-items: flex-start; gap: var(--space-2); min-width: 0; }
.r1 .nm { font-size: var(--text-base); line-height: 22px; font-weight: 600; white-space: nowrap; }
.r1 .plan { margin-top: 2px; }
.r1 .nm.tr { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
.r1 .ac { flex: 1; min-width: 0; font-size: var(--text-sm); line-height: 22px; color: var(--fg-muted); }
.r1 > svg.i { color: var(--fg-muted); margin-left: -4px; align-self: center; }
.fg { display: inline-flex; flex-direction: column; align-items: flex-end; gap: 0; white-space: nowrap; margin-left: auto; flex: none; } /* owner 2026-10-23: unit word below the figure */
.fg b { font-size: var(--text-xl); line-height: 1.1; font-weight: 700; }
.fg.wrapfig { white-space: normal; text-align: right; max-width: 52%; }
.fg.wrapfig b { font-size: var(--text-lg); line-height: 1.2; }
.fg small { font-size: var(--text-2xs); line-height: 1.1; color: var(--fg-muted); font-weight: 400; letter-spacing: 0; }
.rm { margin: 2px 0; }
.stl.x { font-weight: 500; }
.stl > svg.ring { margin-top: 2px; }
.rc > .stl { flex-wrap: nowrap; gap: var(--space-2); height: 20px; overflow: hidden; align-items: flex-start; }
.stl-l { flex: 1 1 0; min-width: 0; height: 20px; display: flex; flex-wrap: wrap; align-content: flex-start; align-items: center; gap: 0 6px; overflow: hidden; }
.stl-l > * { height: 20px; display: inline-flex; align-items: center; }
.stl-l .det { white-space: nowrap; }
.retw { flex: none; margin-left: auto; height: 20px; display: inline-flex; align-items: center; }
.ret { display: inline-flex; align-items: center; gap: 3px; font-size: var(--text-sm); font-weight: 500; color: var(--fg-muted); white-space: nowrap; }
.sublim { font-size: var(--text-sm); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.tc { gap: 4px; padding: 0 var(--space-2); }
.row.open { background: var(--surface); border-radius: var(--radius-md); margin: var(--space-1) var(--space-2); border: var(--border-w) solid var(--border); }
.row.open .rh { padding: var(--space-3) var(--space-3); }
.rb { padding: var(--space-1) var(--space-3) var(--space-3); display: flex; flex-direction: column; gap: var(--space-3); }
.sub { display: flex; flex-direction: column; gap: 6px; }
.s1 { display: flex; flex-wrap: wrap; align-items: center; gap: 2px var(--space-2); }
.sl { font-size: var(--text-base); font-weight: 500; white-space: nowrap; }
.s1 .sf { text-align: right; min-width: 0; }
.sf b { font-size: var(--text-lg); font-weight: 700; } .sf small { font-size: var(--text-sm); color: var(--fg-muted); margin-left: 3px; }
.s3 { display: flex; flex-wrap: wrap; justify-content: space-between; gap: 2px var(--space-2); font-size: var(--text-sm); }
.s3 .pc { white-space: nowrap; } .s3 .mono { margin-left: auto; white-space: nowrap; }
.pc { display: inline-flex; align-items: center; gap: 4px; }
.s3 .mono { font-size: var(--text-micro); }
.lines { display: flex; flex-direction: column; gap: 6px; font-size: var(--text-sm); }
.lines > div { display: flex; align-items: center; gap: 6px; min-width: 0; }
.acts { display: flex; gap: var(--space-1); margin-left: calc(-1 * var(--space-2)); }
.sechead { padding: var(--space-3) var(--space-4) var(--space-1); }

/* reset timeline */
.tl { padding: var(--space-4) var(--space-4) var(--space-3); border-top: var(--border-w) solid var(--border); }
.tl-h { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: var(--space-3); }
.tl-h h2 { margin: 0; font-size: var(--text-base); font-weight: 650; }
.tl-body { position: relative; padding-left: 28px; }
.tl-now { height: 16px; position: relative; } .tl-now span { position: absolute; left: -2px; top: 0; color: var(--fg-muted); font-size: var(--text-micro); }
.tl-body::before { content: ""; position: absolute; left: 28px; top: 14px; bottom: 22px; width: 1px; background: var(--fg-muted); }
.tl-lane { position: relative; display: flex; align-items: center; height: 28px; }
.tl-t { position: absolute; left: -28px; width: 28px; display: grid; place-items: center; }
.tl-plot { position: relative; flex: 1; height: 100%; }
.tl-base { position: absolute; left: 0; right: 0; top: 50%; height: 1px; background: var(--border); }
.tl-line { position: absolute; left: 0; top: calc(50% - 1px); height: 2px; background: currentColor; }
.tl-dot { position: absolute; top: calc(50% - 4px); width: 8px; height: 8px; margin-left: -4px; border-radius: 50%; background: currentColor; }
.tl-lbl { position: absolute; top: 2px; margin-left: 10px; font-size: var(--text-micro); color: var(--fg); line-height: 1; white-space: nowrap; }
.tl-lbl.left { margin-left: 0; transform: translateX(calc(-100% - 10px)); }
.tl-later { position: absolute; right: 0; top: 7px; font-size: var(--text-micro); color: var(--fg-muted); display: inline-flex; align-items: center; gap: 2px; background: var(--bg); padding-left: 6px; }
.tl-ax { position: relative; height: 18px; margin-top: 4px; }
.tl-ax span { position: absolute; top: 4px; }
.tl-unk { display: flex; flex-direction: column; gap: 4px; margin-top: var(--space-2); padding-left: 28px; }
.tl-unk .u { display: inline-flex; align-items: center; gap: 6px; }
`;
