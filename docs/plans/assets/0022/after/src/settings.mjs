import { glyph, ic, meter, page, ring, row as accRow, stateRing, tchip, tile, TRAY_CSS, UI } from "./lib.mjs";
import { PRESETS, display } from "./fixtures.mjs";
import { flagCountry, flagSvg } from "../../../../../../apps/desktop/ui/js/flags.js"; // the app's own flags (region wins: pt-BR -> br)
import { cur, t, usd, when } from "./i18n.mjs";

const CSS = TRAY_CSS + `
.sg { display: grid; grid-template-columns: 220px 1fr; height: 100%; border: var(--border-w) solid var(--border); border-radius: var(--radius-lg); overflow: hidden; background: var(--bg); }
.side { display: flex; flex-direction: column; gap: var(--space-3); padding: 44px var(--space-3) var(--space-3); background: var(--surface); border-right: var(--border-w) solid var(--border); }
.side .field { height: 30px; font-size: var(--text-md); color: var(--fg-muted); }
.side .field .kbd { margin-left: auto; }
.nav { display: flex; flex-direction: column; gap: 2px; flex: 1; }
.ni { position: relative; display: flex; align-items: center; gap: var(--space-2); height: 34px; padding: 0 var(--space-3); border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--fg); font-size: var(--text-base); text-align: left; width: 100%; }
.ni svg.i { color: var(--fg-muted); }
.ni.on { background: var(--surface-2); font-weight: 600; }
.ni.on svg.i { color: var(--fg); }
.ni.on::before { content: ""; position: absolute; left: -4px; top: 8px; bottom: 8px; width: 3px; border-radius: 2px; background: var(--accent); }
.ni.child { padding-left: 34px; }
.ni.grp-btn { color: var(--fg-muted); font-size: var(--text-sm); font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; }
.ni .cnt { margin-left: auto; color: var(--fg-muted); font-size: var(--text-sm); }
.sp { height: 1px; background: var(--border); margin: var(--space-2) var(--space-1); }
.content { position: relative; overflow: hidden; padding: 44px var(--space-6) var(--space-5); }
.in { max-width: 640px; margin: 0 auto; display: flex; flex-direction: column; gap: var(--space-5); }
.in > h1 { margin: 0; font-size: var(--text-xl); font-weight: 650; line-height: 1.25; }
.sr { display: flex; align-items: center; gap: var(--space-4); min-height: 52px; padding: var(--space-2) var(--space-4); border-top: var(--border-w) solid var(--border); }
.sr:first-child { border-top: 0; }
.sr .tx { flex: 1; min-width: 0; }
.sr .lb { font-size: var(--text-base); line-height: 1.3; }
.sr .ds { font-size: var(--text-md); line-height: 1.35; color: var(--fg-muted); }
.sr.dep { padding-left: calc(var(--space-4) + var(--space-5)); background: color-mix(in srgb, var(--surface-2) 45%, var(--surface)); }
.sr.off { opacity: 0.45; }
.sr .val { color: var(--fg-muted); font-size: var(--text-md); display: inline-flex; align-items: center; gap: 4px; }
.chips { display: flex; gap: var(--space-1); flex-wrap: wrap; justify-content: flex-end; }
.swatch { display: inline-flex; gap: 3px; }
.swatch i { width: 10px; height: 10px; border-radius: 50%; display: block; border: var(--border-w) solid var(--border); }
.toast { position: absolute; left: var(--space-6); bottom: var(--space-5); display: inline-flex; align-items: center; gap: var(--space-2); height: 32px; padding: 0 var(--space-3); border: var(--border-w) solid var(--border); border-radius: var(--radius-sm); background: var(--surface); box-shadow: var(--shadow); font-size: var(--text-md); }
.opt { display: flex; flex-direction: column; gap: var(--space-2); align-items: center; justify-content: space-between; padding: var(--space-3) var(--space-3) var(--space-2); height: 84px; border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: var(--surface); color: var(--fg-muted); font-size: var(--text-md); }
.opt.on { border: 2px solid var(--accent); padding: calc(var(--space-3) - 1px) calc(var(--space-3) - 1px) calc(var(--space-2) - 1px); color: var(--fg); font-weight: 600; }
.opt .pv2 { flex: 1; display: flex; align-items: center; gap: var(--space-2); color: var(--fg); font-size: var(--text-md); font-weight: 600; }
.opt .pv2 .p { display: inline-flex; align-items: center; gap: 4px; }
.prev { display: flex; align-items: center; justify-content: space-between; padding: var(--space-4); border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: var(--surface); }
.mb { display: inline-flex; align-items: center; gap: var(--space-3); height: 28px; padding: 0 var(--space-3); border-radius: var(--radius-sm); background: var(--surface-2); font-size: var(--text-base); font-weight: 500; }
.mb .p { display: inline-flex; align-items: center; gap: 5px; }
.mbm { gap: var(--space-3); }
.mi { display: inline-flex; align-items: center; gap: 5px; }
.mbar { display: block; width: 38px; height: 4px; border-radius: 99px; background: color-mix(in srgb, var(--fg) 28%, transparent); overflow: hidden; }
.mbar > span { display: block; height: 100%; border-radius: inherit; background: var(--fg); }
.mbar.hatch { background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--fg) 45%, transparent) 0 2px, transparent 2px 4px); }
.li.dim .tile, .li.dim .ttl, .li.dim .sub { opacity: .55; }
.notif { display: flex; gap: var(--space-3); align-items: flex-start; width: 360px; padding: var(--space-3); border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: var(--surface); box-shadow: var(--shadow); }
.notif .ni-ring { margin-top: 2px; flex: none; display: grid; }
.notif .nt { font-size: var(--text-base); font-weight: 600; line-height: 1.3; }
.notif .nx { margin-top: 2px; font-size: var(--text-md); color: var(--fg-muted); line-height: 1.35; }
.opts { display: grid; grid-template-columns: 1fr 1fr; gap: var(--space-3); }
.opts.c3 { grid-template-columns: repeat(3, 1fr); }
.opts.c3 .opt { height: 116px; }
.li .grip2 { color: var(--fg-muted); opacity: .7; width: 16px; display: grid; place-items: center; }
.sw-i { display: inline-flex; margin-left: 4px; color: var(--fg-muted); }
.numf { display: inline-flex; align-items: center; gap: var(--space-1); }
.numf .field { width: 96px; justify-content: flex-end; }
.flag { display: block; border-radius: 3px; flex: none; }
.flag.hair { box-shadow: 0 0 0 var(--border-w) var(--border); } /* DS-CMP-SEL-3: only white-field flags (jp, kr, id) */
.lmenu { position: absolute; z-index: 5; width: 260px; padding: var(--space-1); border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: var(--surface); box-shadow: var(--shadow); }
.lmenu button { display: flex; align-items: center; gap: var(--space-3); width: 100%; height: 34px; padding: 0 var(--space-3); border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--fg); font-size: var(--text-base); text-align: left; }
.lmenu button.on { background: var(--surface-2); font-weight: 600; }
.lmenu button svg.i:last-child { margin-left: auto; color: var(--accent); }
`;

const NAV = (selected, advOpen) => {
  const item = (id, icon, label, extra = "") => `<button class="ni ${selected === id ? "on" : ""}" ${selected === id ? 'aria-current="page"' : ""}>${ic(icon)}<span class="trunc" title="${label}">${label}</span>${extra}</button>`;
  const child = (id, icon, label, extra = "") => `<button class="ni child ${selected === id ? "on" : ""}" ${selected === id ? 'aria-current="page"' : ""}>${ic(icon)}<span class="trunc" title="${label}">${label}</span>${extra}</button>`;
  return `<aside class="side"><div class="field">${ic("search", "s14")}<span class="hold trunc">${t("nav.search")}</span><span class="kbd">⌘F</span></div>
  <nav class="nav" aria-label="${t("settings")}">
    ${item("general", "sliders", t("nav.general"))}${item("dashboard", "grid", t("nav.dashboard"))}${item("menubar", "menubar", t("nav.menubar"))}${item("alerts", "bell", t("nav.alerts"))}${item("data", "shield", t("nav.data"))}
    <button class="ni grp-btn" aria-expanded="${advOpen}">${ic(advOpen ? "chevDown" : "chev", "s14")}<span class="trunc">${t("nav.advanced")}</span></button>
    ${advOpen ? `${child("integrations", "plug", t("nav.integrations"))}${child("cost", "dollar", t("nav.cost"))}${child("extensions", "puzzle", t("nav.extensions"), '<span class="cnt">2</span>')}${child("files", "file", t("nav.files"))}` : ""}
    <span style="flex:1"></span><div class="sp"></div>
    ${item("about", "info", t("nav.about"))}
  </nav></aside>`;
};
const win = (selected, advOpen, content, extra = "") => `<div class="sg">${NAV(selected, advOpen)}<main class="content"><div class="in">${content}</div>${extra}</main></div>`;
const grp = (label, rows, hint = "") => `<section><div class="grp-h"><span class="micro">${label}</span>${hint ? `<span class="hint">${hint}</span>` : ""}</div><div class="grp">${rows}</div></section>`;
const row = (label, desc, ctl, cls = "") => `<div class="sr ${cls}"><div class="tx"><div class="lb">${label}</div>${desc ? `<div class="ds">${desc}</div>` : ""}</div>${ctl}</div>`;
const sw = (on = true) => `<span class="sw ${on ? "on" : ""}" role="switch" aria-checked="${on}"></span>`;
const seg = (opts, on, mono = false) => `<div class="seg tall ${mono ? "mono" : ""}" role="radiogroup">${opts.map((o, i) => `<button class="${i === on ? "on" : ""}">${o}</button>`).join("")}</div>`;
const info = `<span class="sw-i">${ic("info", "s14")}</span>`;


// language selector with flags (owner feedback 2026-10-23): flag + endonym (the catalog's _meta.name), region wins (pt-BR -> Brazil)
const LANGS = [["pt-BR", "Português"], ["en-US", "English"], ["es", "Español"], ["it", "Italiano"]];
const star = (cx, cy, r, fill) => `<polygon fill="${fill}" points="${Array.from({ length: 10 }, (_, i) => { const a = (-90 + i * 36) * Math.PI / 180, q = i % 2 ? r * 0.4 : r; return `${(cx + q * Math.cos(a)).toFixed(2)},${(cy + q * Math.sin(a)).toFixed(2)}`; }).join(" ")}"/>`;
const hb = (c) => c.map((x, i) => `<rect y="${(20 / c.length) * i}" width="30" height="${20 / c.length + 0.05}" fill="${x}"/>`).join("");
// artwork for the languages PRD 0035 adds (flags.js must gain these, DS-CMP-SEL-3); simplified, same 30x20 box
const EXTRA = {
  cn: `<rect width="30" height="20" fill="#de2910"/>${star(5.5, 5.5, 3.2, "#ffde00")}${star(11, 2.5, 1, "#ffde00")}${star(13, 5, 1, "#ffde00")}${star(13, 8, 1, "#ffde00")}${star(11, 10.5, 1, "#ffde00")}`,
  kr: `<rect width="30" height="20" fill="#fff"/><path d="M11.5 10a3.5 3.5 0 0 1 7 0z" fill="#cd2e3a"/><path d="M18.5 10a3.5 3.5 0 0 1-7 0z" fill="#0047a0"/><g fill="#000"><path d="M4 3.5l3-1.6.7 1.2-3 1.6zM22.3 16.9l3-1.6.7 1.2-3 1.6zM22.3 3.1l.7-1.2 3 1.6-.7 1.2zM4 16.5l.7-1.2 3 1.6-.7 1.2z"/></g>`,
  in: `${hb(["#ff9933", "#fff", "#138808"])}<circle cx="15" cy="10" r="2.3" fill="none" stroke="#000080" stroke-width=".6"/>`,
  id: hb(["#e70011", "#fff"]),
  tr: `<rect width="30" height="20" fill="#e30a17"/><circle cx="11.5" cy="10" r="5" fill="#fff"/><circle cx="12.9" cy="10" r="4" fill="#e30a17"/>${star(16.5, 10, 2, "#fff")}`,
  vn: `<rect width="30" height="20" fill="#da251d"/>${star(15, 10.5, 6, "#ff0")}`,
};
const HAIR = new Set(["jp", "kr", "id"]);
const flag = (code, w = 22) => {
  const c = flagCountry(code) || ({ ko: "kr", vi: "vn", tr: "tr" })[code]; // flagCountry() gains ko, vi, tr, zh, hi, id with PRD 0035
  const art = EXTRA[c];
  const svg = art ? `<svg class="flag" viewBox="0 0 30 20" width="${w}" height="${Math.round((w * 2) / 3)}" aria-hidden="true">${art}</svg>` : flagSvg(c, w);
  return HAIR.has(c) ? svg.replace('class="flag"', 'class="flag hair"') : svg;
};
const langName = (code) => LANGS.find(([c]) => c === code)[1];
const langField = (code) => `<div class="field sel" style="min-width:210px">${flag(code)}<span class="grow nowrap" lang="${code}">${langName(code)}</span>${ic("chevDown", "s14")}</div>`;
const langMenu = (code, style, list = LANGS) => `<div class="lmenu" role="menu" style="${style}">${list.map(([c, n]) => `<button role="menuitemradio" aria-checked="${c === code}" class="${c === code ? "on" : ""}">${flag(c)}<span lang="${c}">${n}</span>${c === code ? ic("tick", "s14") : ""}</button>`).join("")}</div>`;
// the 14 languages of PRD 0035 with their endonyms (DS-CMP-SEL-3); only the first four ship today
const LANGS14 = [["pt-BR", "Português"], ["en-US", "English"], ["es", "Español"], ["it", "Italiano"], ["zh-CN", "中文（简体）"], ["fr", "Français"], ["de", "Deutsch"], ["ja", "日本語"], ["ko", "한국어"], ["hi-IN", "हिन्दी"], ["id-ID", "Bahasa Indonesia"], ["ru", "Русский"], ["tr", "Türkçe"], ["vi", "Tiếng Việt"]];

export function screens() {
  const out = [];
  const shot = (name, w, h, body, title) => out.push({ name, w, h, html: page({ title, w, h, css: CSS, body }) });
  const modeSeg = seg([`${ic("monitor", "s14")} ${t("theme.mode.system")}`, `${ic("sun", "s14")} ${t("theme.mode.light")}`, `${ic("moon", "s14")} ${t("theme.mode.dark")}`], 0);
  const general = (extra = "") => win("general", false, `
    <h1>${t("nav.general")}</h1>
    ${grp(t("settings.appearance"), `
      ${row(t("g.mode"), "", modeSeg)}
      ${row(t("g.theme"), t("g.theme.d"), `<div class="field sel"><span class="swatch"><i style="background:var(--bg)"></i><i style="background:var(--surface-2)"></i><i style="background:var(--accent)"></i><i style="background:var(--good)"></i></span><span>Dracula</span>${ic("chevDown", "s14")}</div>`)}
      ${row(t("settings.motion.label"), t("settings.motion.help"), seg([t("settings.motion.system"), t("settings.motion.reduced"), t("settings.motion.off")], 0))}
    `)}
    ${grp(t("lang.label"), row(t("lang.label"), t("g.language.d"), langField(cur.loc)))}
    ${grp(t("g.startup"), `
      ${row(t("g.login"), t("g.login.d"), sw(true))}
      ${row(t("g.dock"), "", sw(false))}
    `)}
  `, extra);
  shot("50-settings-general", 860, 640, general(), "50 settings general");
  shot("50-settings-general-language", 860, 640, general(langMenu(cur.loc, "top:358px;right:24px")), "50 settings general language");
  if (cur.loc === "en-US") shot("50-settings-general-language-14", 860, 900, general(langMenu("en-US", "top:300px;right:24px", LANGS14)), "50 settings general language, the 14 planned languages");
  // ---------- Dashboard (moved controls) ----------
  const miniRow = (p, v, withFig = false) => `<div style="display:flex;align-items:center;gap:6px">${tile(p, 10)}<div class="bar ${v >= 70 ? "good" : v >= 30 ? "warn" : "bad"}" style="flex:1"><span style="width:${v}%"></span></div>${withFig ? `<span style="font-size:var(--text-micro);font-weight:700;min-width:22px;text-align:right">${v}%</span>` : ""}</div>`;
  shot("50-settings-dashboard", 860, 1620, win("dashboard", false, `
    <h1>Dashboard</h1>
    <section><div class="grp-h"><span class="micro">Tray view</span><span class="hint">Shown when you open the tray</span></div>
      <div class="opts c3" role="radiogroup">
        <div class="opt on" role="radio" aria-checked="true"><div class="pv2" style="flex-direction:column;align-items:stretch;justify-content:center;gap:7px;width:92px">${miniRow("claude", 22)}${miniRow("codex", 63)}${miniRow("kimi", 59)}</div><span>List</span></div>
        <div class="opt" role="radio" aria-checked="false"><div class="pv2" style="gap:4px">${ring(22, 30, "bad", 3.4)}${ring(63, 30, "warn", 3.4)}${ring(59, 30, "warn", 3.4)}</div><span>Rings</span></div>
        <div class="opt" role="radio" aria-checked="false"><div class="pv2" style="flex-direction:column;align-items:stretch;justify-content:center;gap:4px;width:92px">${miniRow("claude", 22, true)}${miniRow("codex", 63, true)}${miniRow("kimi", 59, true)}${miniRow("groq", 74, true)}</div><span>Compact</span></div>
      </div>
    </section>
    ${grp("Row display", `
      ${row("Detail level", "How much each account row shows.", seg(["Detailed", "Standard", "Minimal", "Custom"], 1))}
      ${row("Choose what a row shows", "Plan badge, monogram, chips, pace corridor and more.", `<span class="val">${ic("chev", "s14")}</span>`)}
      ${row("Use a different layout in the main window", "Off: the tray and the main window show the same row content.", sw(false))}
      ${row("Main window detail level", "", seg(["Detailed", "Standard", "Minimal", "Custom"], 1), "dep off")}
    `)}
    ${grp("Accounts", [
      ["claude", "work@example.com", "Claude Code · Max", "WO", true], ["claude", "side@example.com", "Claude Code · Pro", "SI", true], ["codex", "OpenAI Codex", "me@example.com · Plus", "", true],
      ["cursor", "Cursor", "dev@example.com · Pro", "", true], ["kimi", "Kimi", "ops@example.com", "", true], ["groq", "Groq", "team@example.com", "", true], ["anthropic", "Anthropic API", "api@example.com", "", false],
    ].map(([p, nm, sub, mono, on]) => `<div class="li two ${on ? "" : "dim"}"><span class="grip2">${ic("grip")}</span>${tile(p, 28, mono)}<div class="grow"><div class="ttl trunc" title="${nm}">${nm}</div><div class="sub">${sub}</div></div><button class="ib" aria-label="Edit ${nm}">${ic("edit", "s14")}</button><button class="ib" aria-label="Remove ${nm}">${ic("trash", "s14")}</button>${sw(on)}</div>`).join("")
      + `<div class="li" style="background:var(--surface-2)"><button class="btn outline sm">${ic("plus", "s14")} <span>Add account</span></button><span class="grow"></span><span class="sm muted" style="text-align:right">Drag to set the order. The monogram letters can be changed in the account form.</span></div>`, "7 accounts · the order is shared with the tray")}
    <section><div class="grp-h"><span class="micro">Pace</span><span class="hint">Applies everywhere pace is shown</span></div>
      <div class="grp">
        ${row("Show pace as", "Points or percent, in chips, status lines, details and notifications.", seg(["Points", "Percent"], 0))}
        <div class="sr" style="align-items:stretch;flex-direction:column;gap:var(--space-3);background:var(--surface-2)">
          <div class="sm muted">Example: expected use 50%, actual use 60%. Points: difference from the expected share. Percent: how much faster or slower than expected.</div>
          <div style="display:grid;grid-template-columns:1fr 1fr;gap:var(--space-3)">
            <div style="display:flex;flex-direction:column;gap:6px"><span class="micro">Points</span><span class="chip warn" style="align-self:flex-start">${ic("arrowUp", "s12")}<span>10 pts ahead</span></span><span class="sm muted">10 points above the expected share</span></div>
            <div style="display:flex;flex-direction:column;gap:6px"><span class="micro">Percent</span><span class="chip warn" style="align-self:flex-start">${ic("arrowUp", "s12")}<span>20% faster than pace</span></span><span class="sm muted">Uses 20% more than expected (10 of 50)</span></div>
          </div>
        </div>
      </div>
    </section>
    ${grp("Order and grouping", `
      ${row("Sort accounts", "Custom order is set by dragging in the main window.", `<div class="field sel">${ic("sort", "s14")}<span>Custom order</span>${ic("chevDown", "s14")}</div>`)}
      ${row("Group exhausted accounts", "Moves them under Recharging, with their return time.", sw(false))}
      ${row("Show consumption pace", "Pace marker and outlook on meters.", sw(true))}
      ${row("Refresh interval", "Providers billed per query have a higher minimum.", seg(["1 min", "5 min", "15 min", "Custom"], 1, true))}
    `)}
    ${grp("Quick access", `
      ${row("Mode", "Same as General.", seg([`${ic("monitor", "s14")} System`, `${ic("sun", "s14")} Light`, `${ic("moon", "s14")} Dark`], 0))}
      ${row("Language", "Same as General.", `${langField("en-US")}`)}
    `, "Moved from the window headers")}
  `), "50 settings dashboard");



  // ---------- Custom display (sub-page of Dashboard) ----------
  const TOG = [
    ["plan", "Plan badge", "Max, Pro, Plus"], ["account", "Account label", "The name or e-mail of the account"], ["mono", "Account monogram", "Two letters on the tile when a provider has several accounts"],
    ["unit", "Unit word", "\"left\" under the figure"], ["corridor", "Pace corridor", "The band on the bar that marks on-pace"], ["label", "Status label", "Ahead of pace, On pace, Limit hit"],
    ["ring", "State ring", "The small ring before the status"], ["runsout", "Runs-out chip", "When the limit is projected to run out"], ["returns", "Returns time", "When the limit comes back"],
    ["age", "Freshness age", "Always show how old the reading is"], ["incident", "Incident mark", "Provider outage or maintenance"], ["extrausage", "Extra-usage row", "Extra balance under the plan limits"], ["sublimits", "Sub-limit preview", "5 h and weekly values under the bar"],
  ];
  const cfg = PRESETS.custom;
  shot("50-settings-display-custom", 860, 1080, win("dashboard", false, `
    <div style="display:flex;align-items:center;gap:var(--space-2)"><button class="ib" aria-label="Back">${ic("back", "s18")}</button><h1 style="margin:0">Row display</h1></div>
    <div style="display:flex;align-items:center;justify-content:space-between;gap:var(--space-4)"><span class="muted md">Start from a preset, then adjust each element.</span>${seg(["Detailed", "Standard", "Minimal", "Custom"], 3)}</div>
    <div class="prev" style="flex-direction:column;align-items:stretch;gap:var(--space-3);padding:var(--space-3) 0 0"><span class="muted md" style="padding:0 var(--space-4)">Live preview</span>
      <div style="width:380px;max-width:100%;margin:0 auto;border-top:var(--border-w) solid var(--border);background:var(--bg)">${accRow(display("claude1", cfg))}</div></div>
    ${grp("Elements", TOG.map(([k, label, desc]) => row(label, desc, sw(cfg[k]))).join(""), "Changes apply at once")}
  `), "50 settings display custom");

  // ---------- Menu bar (new) ----------
  const pct = (p, txt, cls = "") => `<span class="p ${cls}">${glyph(p, 13)}<span>${txt}</span></span>`;
  // menu-bar item, Meters style: monochrome glyph + one continuous bar of the remaining share (template image: foreground colour only)
  const mbItem = (p, v, hatch = false) => `<span class="mi">${glyph(p, 13)}<span class="mbar ${hatch ? "hatch" : ""}">${hatch ? "" : `<span style="width:${Math.max(v, 1)}%"></span>`}</span></span>`;
  const mrow = (p, name, acct, status, checked, statusRole = "", mono = "") => `<div class="li two"><span class="grip2">${ic("grip")}</span>${tile(p, 28, mono)}<div class="grow"><div class="ttl trunc" title="${name}">${name} <span class="muted sm">${acct}</span></div><div class="sub ${statusRole ? "r-" + statusRole : ""}">${status}</div></div><span class="cc ${checked ? "on" : ""}" role="checkbox" aria-checked="${checked}">${ic("tick")}</span></div>`;
  const left = (n) => `${n}% ${t("left")}`;
  shot("50-settings-menubar", 860, 960, win("menubar", false, `
    <h1>${t("nav.menubar")}</h1>
    <div class="prev"><span class="muted md">${t("mb.preview")}</span><div class="mb mbm">${mbItem("claude", 22)}${mbItem("codex", 63)}${mbItem("cursor", 0)}</div></div>
    <section><div class="grp-h"><span class="micro">${t("mb.style")}</span></div>
      <div class="opts" role="radiogroup">
        <div class="opt on" role="radio" aria-checked="true"><div class="pv2 mbm" style="gap:var(--space-3)">${mbItem("claude", 22)}${mbItem("codex", 63)}${mbItem("cursor", 0)}</div><span>${t("mb.meters")}</span></div>
        <div class="opt" role="radio" aria-checked="false"><div class="pv2">${pct("claude", "22%")}${pct("codex", "63%")}${pct("cursor", usd(6), "r-warn")}</div><span>${t("mb.logos")}</span></div>
        <div class="opt" role="radio" aria-checked="false"><div class="pv2">${pct("claude", "22%")}</div><span>${t("mb.tightest")}</span></div>
        <div class="opt" role="radio" aria-checked="false"><div class="pv2">${ring(22, 18, "fg", 5)}</div><span>${t("mb.icon")}</span></div>
      </div>
      <div class="sm muted" style="margin-top:var(--space-2);padding:0 var(--space-1)">${t("mb.showing", { n: 3, total: 7 })} ${t("mb.ring")}</div>
    </section>
    ${grp(t("mb.appearance"), row(t("mb.colored"), t("mb.colored.d"), sw(false)))}
    <section><div class="grp-h"><span class="micro">${t("mb.accounts")}</span><span class="hint">${t("mb.count", { n: 3, total: 7 })}</span></div>
      <div class="grp">
        ${mrow("claude", "work@example.com", "Claude Code", left(22), true, "", "WO")}
        ${mrow("codex", "OpenAI Codex", "me@example.com", left(63), true)}
        ${mrow("cursor", "Cursor", "dev@example.com", t("mb.onextra"), true, "warn")}
        <div class="li inset"><span class="insetl">${t("mb.not")}</span></div>
        ${mrow("claude", "side@example.com", "Claude Code", left(71), false, "", "SI")}
        ${mrow("kimi", "Kimi", "ops@example.com", left(59), false)}
        ${mrow("groq", "Groq", "team@example.com", `${t("stale")} · ${left(74)}`, false)}
        ${mrow("anthropic", "Anthropic API", "api@example.com", t("st.auth"), false, "accent")}
      </div>
      <div class="sm muted" style="margin-top:var(--space-2);padding:0 var(--space-1)">${t("mb.drag")}</div>
    </section>
  `), "50 settings menu bar");

  // ---------- Alerts (new rows) ----------
  const chip = (x) => `<span class="chip plain" style="height:28px;padding:0 var(--space-3);color:var(--fg);background:var(--surface-2);font-size:var(--text-md)">${x} ${ic("x", "s12")}</span>`;
  shot("50-settings-alerts", 860, 960, win("alerts", false, `
    <h1>${t("nav.alerts")}</h1>
    ${grp(t("al.pace.g"), `
      ${row(t("al.pace"), t("al.pace.d"), sw(true))}
      ${row(t("al.lead"), t("al.lead.d"), seg(["15 min", "30 min", "60 min"], 1, true), "dep")}
    `)}
    ${grp(t("sub.extra"), `
      ${row(t("al.extra"), t("al.extra.d"), sw(true))}
      ${row(t("al.remind"), "", `<span class="numf"><span class="field s30"><span>${usd(2)}</span></span></span>`, "dep")}
    `)}
    <section><div class="grp-h"><span class="micro">${t("al.prev")}</span><span class="hint">${t("al.prev.d")}</span></div>
      <div class="prev" style="justify-content:center;background:var(--surface-2)"><div class="notif" role="img" aria-label="${t("al.prev.t")}"><span class="ni-ring">${stateRing("pct", 22, 16)}</span><div class="nb"><div class="nt">${t("al.prev.t")}</div><div class="nx">${t("al.prev.b", { at: when(25, 10, 20) })}</div></div></div></div>
    </section>
    ${grp(t("al.little"), row(t("settings.alerts.thresholds"), t("al.thr.d"), `<div class="chips">${chip("50%")}${chip("20%")}${chip("10%")}<button class="btn outline sm">${ic("plus", "s14")} ${t("settings.alerts.add")}</button></div>`))}
    ${grp(t("al.resets"), `
      ${row(t("settings.notify.on"), "", sw(true))}
      ${row(t("al.which"), "", seg([t("settings.notify.when.exhausted"), t("settings.notify.when.always")], 0), "dep")}
      ${row(t("al.test"), "", `<button class="btn outline sm">${ic("bell", "s14")} <span>${t("settings.notify.test")}</span></button>`)}
    `)}
  `, `<div class="toast" role="status">${ic("check", "s14 r-good")} ${t("saved")}</div>`), "50 settings alerts");
  // ---------- Advanced: Integrations ----------
  shot("50-settings-advanced", 860, 640, win("integrations", true, `
    <h1>Integrations</h1>
    ${grp("Local server", `
      ${row("Run the local server", "Lets other tools on this computer read your usage.", `<span style="display:inline-flex;align-items:center;gap:var(--space-3)"><span class="chip good"><i class="dot"></i> Listening</span>${sw(true)}</span>`)}
      <div class="sr" style="padding:0"><details open style="width:100%"><summary class="sr" style="border-top:var(--border-w) solid var(--border);list-style:none;min-height:44px"><span class="lb" style="display:inline-flex;align-items:center;gap:var(--space-2)">${ic("chevDown", "s14")} Connection details</span></summary>
        <div class="sr dep"><div class="tx"><div class="lb">Address</div></div><span class="mono muted sm">127.0.0.1:8787</span><button class="ib" aria-label="Copy address">${ic("copy", "s14")}</button></div>
        <div class="sr dep"><div class="tx"><div class="lb">Access token</div><div class="ds">Never shown. Copy it into the tool that needs it.</div></div><button class="btn outline sm">${ic("copy", "s14")} Copy token</button></div>
        <div class="sr dep"><div class="tx"><div class="lb">Web pages may read usage ${info}</div></div>${sw(false)}</div>
      </details></div>
    `)}
    ${grp("Events", `
      ${row("Run hooks on events", "Starts your commands when a limit changes state.", sw(true))}
      ${row("Hook rules", "", `<span class="val">3 rules ${ic("chev", "s14")}</span>`)}
    `)}
  `), "50 settings advanced");
  return out;
}
