import { ic, meter, page, sub, tile, TRAY_CSS } from "./lib.mjs";
import { t } from "./i18n.mjs";

const CSS = TRAY_CSS + `
body { background: var(--bg); }
.dlg { width: 100%; height: 100%; }
.dlg-b { flex: 1; min-height: 0; overflow: hidden; }
.dh-t { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.dh-t .n { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-xl); font-weight: 650; line-height: 1.25; }
.kv { display: flex; align-items: center; gap: var(--space-3); min-height: 36px; padding: var(--space-2) var(--space-3); border-top: var(--border-w) solid var(--border); font-size: var(--text-md); }
.kv:first-child { border-top: 0; }
.kv .k { width: 120px; color: var(--fg-muted); flex: none; }
.lim { padding: var(--space-3); border-top: var(--border-w) solid var(--border); display: flex; gap: var(--space-2); align-items: flex-start; }
.lim:first-child { border-top: 0; }
.lim .sub { flex: 1; min-width: 0; }
.lim.hid .sub { opacity: 0.45; }
.sech { display: flex; align-items: baseline; justify-content: space-between; margin-bottom: var(--space-2); padding: 0 var(--space-1); }
.sech h2 { margin: 0; font-size: var(--text-lg); font-weight: 650; }
.kpis { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space-3); }
.chartbox { padding: var(--space-3); border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: var(--surface); margin-top: var(--space-3); }
.bars { position: relative; height: 112px; margin-left: 40px; display: flex; align-items: flex-end; gap: var(--space-1); border-bottom: 1px solid var(--border); }
.bars .g { position: absolute; left: 0; right: 0; border-top: 1px dashed var(--border); }
.bars .gl { position: absolute; left: -40px; width: 34px; text-align: right; transform: translateY(-50%); }
.bars .b { position: relative; flex: 1; border-radius: var(--radius-xs) var(--radius-xs) 0 0; background: var(--chart-neutral); min-height: 1px; }
.bars .b.hatch { background: repeating-linear-gradient(135deg, var(--track) 0 2px, transparent 2px 5px), color-mix(in srgb, var(--track) 40%, transparent); }
.bars .b.today { background: var(--accent); }
.bars .b span { position: absolute; top: -16px; left: 0; right: 0; text-align: center; color: var(--fg-muted); font: var(--text-micro) var(--font-mono); }
.bars .b.today span { color: var(--fg); }
.bx { display: flex; gap: var(--space-1); margin-left: 40px; margin-top: 4px; }
.bx span { flex: 1; text-align: center; }
.bx span.t { color: var(--fg); font-weight: 600; }
.disc2 { display: flex; align-items: center; gap: var(--space-2); height: 44px; padding: 0 var(--space-3); border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: var(--surface); font-size: var(--text-base); width: 100%; text-align: left; }
.disc2 .v { margin-left: auto; color: var(--fg-muted); font-size: var(--text-sm); }
.lab { display: flex; flex-direction: column; gap: 6px; }
.lab > .t { font-size: var(--text-md); font-weight: 500; }
.lab > .h { font-size: var(--text-sm); color: var(--fg-muted); }
.pv { display: grid; place-items: center; width: 28px; height: 28px; border-radius: var(--radius-sm); background: var(--surface-2); color: var(--fg-muted); flex: none; }
.btn .cnt { color: var(--fg-muted); }
.issue { margin: 0; max-height: 240px; overflow: auto; padding: var(--space-3); border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: var(--input-bg); font-size: var(--text-sm); line-height: 1.5; white-space: pre; }
.disc3 > summary { list-style: none; cursor: pointer; }
.cwrap { font-size: var(--text-md); color: var(--fg-muted); line-height: 1.35; }
`;
const dlgPage = (title, w, h, inner) => page({ title, w, h, css: CSS, body: `<div class="dlg" role="dialog" aria-modal="true">${inner}</div>` });
const closeBtn = `<button class="ib w" aria-label="Close">${ic("x", "s18")}</button>`;
const link = (t) => `<a class="r-accent" style="text-decoration:underline;text-underline-offset:2px">${t}</a>`;

// -------- history chart (DS-CHT area/line, 7d, one reset marker, one gap) --------
function history() {
  const W = 480, H = 150, L = 34, R = 8, T = 8, B = 20;
  const pw = W - L - R, ph = H - T - B;
  const X = (t) => L + (t / 168) * pw, Y = (v) => T + (1 - v / 100) * ph;
  const rnd = (i) => ((Math.sin(i * 12.9898) * 43758.5453) % 1 + 1) % 1;
  const pts = [];
  for (let t = 0; t <= 168; t += 3) {
    let v;
    if (t < 71) v = 38 - (t / 71) * 30 + (rnd(t) - 0.5) * 3; else v = 100 - ((t - 71) / 97) * 78 + (rnd(t) - 0.5) * 3;
    if (t >= 71 && t === 72) v = 99;
    pts.push([t, Math.max(2, Math.min(100, v))]);
  }
  pts[pts.length - 1][1] = 22;
  // runs: gap while the computer slept (Fri 02:00-15:00 = t 7.7..20.7), and a break at the reset (jump)
  const runs = [[], [], []];
  for (const [t, v] of pts) {
    if (t > 8 && t < 21) continue;
    if (t < 8) runs[0].push([t, v]); else if (t < 71) runs[1].push([t, v]); else runs[2].push([t, v]);
  }
  const path = (r) => r.map(([t, v], i) => `${i ? "L" : "M"}${X(t).toFixed(1)} ${Y(v).toFixed(1)}`).join("");
  const area = (r) => `${path(r)}L${X(r[r.length - 1][0]).toFixed(1)} ${Y(0)}L${X(r[0][0]).toFixed(1)} ${Y(0)}Z`;
  const ticks = [["Fri 16", 5.65], ["Sun 18", 53.65], ["Tue 20", 101.65], ["Thu 22", 149.65]];
  return `<svg viewBox="0 0 ${W} ${H}" width="100%" role="img" aria-label="Weekly limit, percent left over 7 days. Lowest 8 percent before the Sunday reset, highest 100 percent, last 22 percent." style="display:block">
  <g class="axis" fill="var(--fg-muted)" font-family="var(--font-mono)" font-size="11">
    ${[100, 50, 0].map((v) => `<text x="${L - 6}" y="${Y(v) + 4}" text-anchor="end">${v}</text>`).join("")}
    ${ticks.map(([t, h]) => `<text x="${X(h)}" y="${H - 4}" text-anchor="middle">${t}</text>`).join("")}
  </g>
  ${[100, 50].map((v) => `<line x1="${L}" x2="${W - R}" y1="${Y(v)}" y2="${Y(v)}" stroke="var(--border)" stroke-dasharray="3 3"/>`).join("")}
  <line x1="${L}" x2="${W - R}" y1="${Y(0)}" y2="${Y(0)}" stroke="var(--border)"/>
  ${runs.filter((r) => r.length > 1).map((r) => `<path d="${area(r)}" fill="color-mix(in srgb, var(--info) 18%, transparent)"/><path d="${path(r)}" fill="none" stroke="var(--info)" stroke-width="1.5" vector-effect="non-scaling-stroke" stroke-linejoin="round"/>`).join("")}
  <line x1="${X(71)}" x2="${X(71)}" y1="${T}" y2="${Y(0)}" stroke="var(--fg-muted)" stroke-dasharray="4 3"/>
  <circle cx="${X(168)}" cy="${Y(22)}" r="3.5" fill="var(--info)"/>
</svg>`;
}

function dailyBars() {
  const vals = [["Fri", 0.31], ["Sat", 0.24], ["Sun", 0.47], ["Mon", 0.19], ["Tue", null], ["Wed", 0.09], ["Today", 0.11]];
  const max = 0.5, h = 112;
  const bars = vals.map(([d, v]) => v == null
    ? `<div class="b unk hatch" style="height:${h}px" title="Cost unknown"><span>—</span></div>`
    : `<div class="b ${d === "Today" ? "today" : ""}" style="height:${(v / max) * h}px"><span>≈$${v.toFixed(2)}</span></div>`).join("");
  const grid = [[0.5, "$0.50"], [0.25, "$0.25"]].map(([v, t]) => `<div class="g" style="bottom:${(v / max) * h}px"><span class="gl axis">${t}</span></div>`).join("");
  return `<div class="chartbox"><div style="margin-bottom:22px" class="sm muted">Estimated cost per day, last 7 days</div>
  <div class="bars" role="img" aria-label="Estimated cost per day: Fri $0.31, Sat $0.24, Sun $0.47, Mon $0.19, Tue unknown, Wed $0.09, Today $0.11">${grid}${bars}<span class="gl axis" style="position:absolute;left:-40px;width:34px;text-align:right;bottom:-7px">$0</span></div>
  <div class="bx axis">${vals.map(([d]) => `<span class="${d === "Today" ? "t" : ""}">${d}</span>`).join("")}</div>
  <div class="sm muted" style="margin-top:var(--space-2)">Tuesday has no price for one model, so its cost is unknown, not zero. Amounts are estimates.</div></div>`;
}

export function screens() {
  const out = [];

  // ---------------- 20 details ----------------
  const eye = (off) => `<button class="ib" aria-label="${off ? "Show this limit" : "Hide this limit"}">${ic(off ? "eyeOff" : "eye", "s14")}</button>`;
  const detail = `
  <div class="dlg-h">${tile("claude", 40, "WO")}<div class="dh-t grow"><div class="n">work@example.com <span class="plan">Max</span></div><span class="muted md">Claude Code</span></div>
    <button class="ib w" aria-label="Edit account">${ic("edit", "s18")}</button><button class="ib w" aria-label="Remove account">${ic("trash", "s18")}</button>${closeBtn}</div>
  <div class="dlg-b">
    <div class="call critical">${ic("alert")}<div><b>Partial outage</b> · Elevated errors on claude.ai & Claude Code<div class="sm muted" style="margin-top:2px">Since 17:32 · read 2 min ago. This may not be caused by your account.</div></div><button class="btn ghost sm act">Open status page ${ic("ext", "s14")}</button></div>
    <section>
      <div class="sech"><h2>Limits</h2><span class="sm muted">Binding limit: Weekly</span></div>
      <div class="grp">
        <div class="lim"><div class="sub"><div class="s1"><span class="sl">5-hour session</span><span class="grow"></span><span class="sf"><b>66%</b><small>left</small></span></div>${meter(66, { pace: 44, label: "66% left" })}<div class="s3"><span class="r-good pc">${ic("check", "s14")} 22 points in reserve</span><span class="mono muted">Resets 20:32 · in 2h 11m</span></div></div>${eye(false)}</div>
        <div class="lim"><div class="sub"><div class="s1"><span class="sl">Weekly</span><span class="chip accent">Binding</span><span class="grow"></span><span class="sf"><b>22%</b><small>left</small></span></div>${meter(22, { pace: 42, label: "22% left" })}<div class="s3"><span class="r-warn pc">${ic("warn", "s14")} 20 points ahead of pace</span><span class="mono muted">Resets Sun 17:20 · in 2d 23h</span></div></div>${eye(false)}</div>
        <div class="lim hid"><div class="sub"><div class="s1"><span class="sl">Sonnet weekly</span><span class="grow"></span><span class="sf"><b>88%</b><small>left</small></span></div>${meter(88, { pace: 70, label: "88% left" })}<div class="s3"><span class="r-good pc">${ic("check", "s14")} 18 points in reserve</span><span class="mono muted">Resets Sun 17:20 · in 2d 23h</span></div></div><button class="ib" aria-label="Show this limit">${ic("eyeOff", "s14")}</button></div>
      </div>
      <div class="call warning" style="margin-top:var(--space-3)">${ic("clock")}<div>At this rate the weekly limit runs out around <b>Sat 10:20</b>, 1d 7h before it resets.</div><button class="btn ghost sm act">${ic("bell", "s14")} Alert me</button></div>
    </section>
    <section>
      <div class="sech"><h2>Reading</h2></div>
      <div class="grp">
        <div class="kv"><span class="k">Last reading</span><span>18:20 · 1 min ago</span><span class="grow"></span><span class="chip good">${ic("check", "s14")} Fresh</span></div>
        <div class="kv"><span class="k">Access level</span><span>Undocumented · usage page of the web app</span><span class="grow"></span><span class="chip plain" title="Verified 20 Aug 2026">${ic("check", "s12")} Verified 2 months ago</span></div>
        <div class="kv"><span class="k">Next refresh</span><span class="mono">in 38 s</span><span class="grow"></span><span class="muted sm">every 5 min</span></div>
      </div>
    </section>
    <section>
      <div class="sech"><h2>Local cost</h2><span class="sm muted">Estimate at API prices, on this computer</span></div>
      <div class="kpis">
        <div class="kpi"><div class="l">Today</div><div class="v">≈$0.11</div><div class="c">468K tokens · plus unpriced</div></div>
        <div class="kpi"><div class="l">Yesterday</div><div class="v">—</div><div class="c">No usage</div></div>
        <div class="kpi"><div class="l">7 days</div><div class="v">≈$1.41</div><div class="c">2M tokens · plus unpriced</div></div>
      </div>
      ${dailyBars()}
      <div style="margin-top:var(--space-3)"><button class="disc2">${ic("chev", "s14")}<span>Models and daily table</span><span class="v">6 models</span></button></div>
    </section>
    <section>
      <div class="sech"><h2>History</h2><div class="seg mono" role="radiogroup"><button>24h</button><button class="on">7d</button><button>30d</button></div></div>
      <div class="chartbox" style="margin-top:0">
        <div class="sm muted" style="margin-bottom:var(--space-2)">Weekly limit, share left</div>
        ${history()}
        <div class="sm muted" style="display:flex;align-items:center;gap:var(--space-4);margin-top:var(--space-2)"><span style="display:inline-flex;align-items:center;gap:6px"><svg width="16" height="8"><line x1="0" x2="16" y1="4" y2="4" stroke="var(--fg-muted)" stroke-dasharray="4 3"/></svg> Reset</span><span class="grow"></span><span>Lowest <b class="mono" style="color:var(--fg);font-weight:600">8%</b></span><span>Highest <b class="mono" style="color:var(--fg);font-weight:600">100%</b></span><span>Last <b class="mono" style="color:var(--fg);font-weight:600">22%</b></span></div>
      </div>
      <div class="sm muted" style="margin-top:var(--space-2)">No readings between Fri 02:00 and 15:00: the computer was asleep. History is kept for 30 days.</div>
    </section>
    <button class="disc2">${ic("chev", "s14")}<span>Models</span><span class="v">6 available</span></button>
  </div>
  <div class="dlg-f"><button class="btn">${ic("refresh", "s14")} Refresh now</button><button class="btn">${ic("edit", "s14")} Edit account</button></div>`;
  out.push({ name: "20-detail-claude", w: 560, h: 1620, html: dlgPage("20 detail Claude", 560, 1620, detail) });

  // ---------------- 31 catalog ----------------
  const prow = (p, name, hint, extra = "") => `<div class="li two">${tile(p, 28)}<div class="grow"><div class="ttl" style="display:flex;flex-wrap:wrap;align-items:center;gap:4px 6px">${name} ${extra}</div><div class="sub cwrap">${hint}</div></div><button class="btn outline sm" style="flex:none">${t("cat.connect")}</button></div>`;
  const foundChip = `<span class="chip info">${ic("check", "s12")}<span>${t("found")}</span></span>`;
  const hint = (auth, lvl) => `${t("auth." + auth)} · ${t("lvl." + lvl)}`;
  const catalog = (query = null) => {
    const search = `<div class="field" style="margin:0 var(--space-5)">${ic("search", "s14")}${query ? `<span>${query}</span><span class="grow"></span><button class="ib" style="width:22px;height:22px" aria-label="Clear">${ic("x", "s12")}</button>` : `<span class="hold">${t("cat.search", { n: 37 })}</span>`}</div>`;
    const body = query ? `
      <div><div class="grp-h"><span class="micro">${t("cat.results", { n: 3 })}</span></div><div class="grp">
        ${prow("claude", "Claude Code", hint("browser", "undocumented"))}
        ${prow("cline", "Cline", hint("keyorlocal", "official"))}
        ${prow("cloudflare", "Cloudflare AI", hint("token", "official"))}
      </div></div>` : `
      <div><div class="grp-h"><span class="micro">${t("cat.g.browser")}</span><span class="hint">${t("cat.h.local")}</span></div><div class="grp">
        ${prow("claude", "Claude Code", hint("browser", "undocumented"), foundChip)}
        ${prow("codex", "OpenAI Codex", hint("browser", "undocumented"), foundChip)}
        ${prow("cursor", "Cursor", hint("browser", "official"))}
        ${prow("copilot", "GitHub Copilot", hint("device", "undocumented"))}
        ${prow("antigravity", "Antigravity", hint("local", "undocumented"))}
      </div></div>
      <div><div class="grp-h"><span class="micro">${t("cat.g.key")}</span><span class="hint">${t("cat.h.key")}</span></div><div class="grp">
        ${prow("anthropic", "Anthropic API", hint("admin", "official"))}
        ${prow("openai", "OpenAI API", hint("admin", "official"))}
        ${prow("groq", "Groq", hint("key", "official"))}
        ${prow("bedrock", "AWS Bedrock", hint("aws", "official"), `<span class="chip warn" style="margin-left:6px">${t("cat.billed")}</span>`)}
      </div></div>`;
    return `<div class="dlg-h"><h1 class="grow">${t("common.add")}</h1>${closeBtn}</div>
    <div style="padding-bottom:var(--space-3)">${search}</div>
    <div class="dlg-b" style="padding-top:var(--space-1)">${body}
      <div class="sm muted" style="text-align:center">${query ? t("cat.notlisted") : t("cat.more", { n: 28 })} ${link(t("cat.ext"))}</div></div>`;
  };
  out.push({ name: "31-account-add-catalog", w: 560, h: 720, html: dlgPage("31 catalog", 560, 720, catalog()) });
  out.push({ name: "31b-account-add-catalog-search", w: 560, h: 400, html: dlgPage("31b catalog search", 560, 400, catalog("cl")) });

  // ---------------- 32 form step ----------------
  const form = `
  <div class="dlg-h"><button class="ib w" aria-label="Back">${ic("back", "s18")}</button>${tile("claude", 32)}<div class="dh-t grow"><div class="n" style="font-size:var(--text-xl);flex-wrap:wrap">Add Claude Code <span class="chip info">${ic("check", "s12")}<span>${t("found")}</span></span></div><span class="muted sm">Undocumented access · usage page of the web app</span></div>${closeBtn}</div>
  <div class="dlg-b" style="gap:var(--space-4)">
    <div class="lab"><span class="t">Name</span><div class="field"><span>work@example.com</span></div><span class="h">Shown on cards and in the menu bar.</span></div>
    <div class="call info">${ic("globe")}<div><b>Browser sign-in</b> is the recommended way. A browser window opens to sign in; the token goes to the system keychain, never to the config file.</div></div>
    <div class="lab"><span class="t">Refresh interval</span><div class="seg fill tall"><button class="on">Default · 5 min</button><button>1 min</button><button>15 min</button><button>Custom</button></div></div>
    <details class="disc3"><summary class="disc2">${ic("chev", "s14")}<span>${t("otherways")}</span><span class="v">Environment variable, pasted session</span></summary></details>
    <button class="disc2">${ic("chev", "s14")}<span>Advanced</span><span class="v">Models, session</span></button>
  </div>
  <div class="dlg-f"><button class="btn lg">Cancel</button><button class="btn primary lg">${ic("globe", "s14")} Sign in with browser</button></div>`;
  out.push({ name: "32-account-add-claude", w: 560, h: 560, html: dlgPage("32 add Claude", 560, 560, form) });

  // ---------------- 07c report a problem ----------------
  const report = `
  <div class="dlg-h"><h1 class="grow">Report a problem</h1>${closeBtn}</div>
  <div class="dlg-b" style="gap:var(--space-4)">
    <div class="lab"><span class="t">This text will be opened as a GitHub issue</span>
      <pre class="issue mono">Cuotai 0.72.0 · macOS 15.1 · desktop
Account: Kimi (kimi) · Official API
Status: unexpected response (schema)

What changed
  five_hour.utilization   number -> string
  weekly.resets_at        string -> null
  weekly.limit            present -> missing

Last good reading: Thu 22 Oct, 16:02
Values, URLs, tokens and e-mail addresses are not included.</pre></div>
    <div class="sm muted">Nothing is sent by Cuotai; GitHub opens with this text.</div>
  </div>
  <div class="dlg-f"><button class="btn">${ic("copy", "s14")} Copy</button><button class="btn primary">Open on GitHub ${ic("ext", "s14")}</button></div>`;
  out.push({ name: "07c-report-problem", w: 560, h: 440, html: dlgPage("07c report a problem", 560, 440, report) });

  // ---------------- 40 extensions ----------------
  const ext = (name, ver, chip, desc, last, id, actions) => `<div class="li two" style="align-items:flex-start;padding:var(--space-3)"><span class="pv">${ic("puzzle", "s18")}</span><div class="grow"><div class="ttl" style="display:flex;align-items:center;gap:var(--space-2)">${name}<span class="plan">v${ver}</span>${chip}</div><div class="sub">${desc}</div><div class="sm muted" style="margin-top:4px"><span class="mono">${id}</span> · ${last}</div></div><div style="display:flex;gap:var(--space-1)">${actions}</div></div>`;
  const extensions = `
  <div class="dlg-h"><div class="grow"><h1>Extensions</h1><div class="muted md" style="margin-top:2px">Local extensions add providers. Cuotai asks before running one.</div></div>${closeBtn}</div>
  <div class="dlg-b" style="gap:var(--space-4)">
    <div class="grp">
      ${ext("Acme usage", "1.2.0", `<span class="chip good">${ic("check", "s14")} Enabled</span>`, "Reads the Acme AI usage page of the example workspace.", "last run 22 Oct, 18:10", "acme-usage", `<button class="btn outline sm">Test</button><button class="btn ghost sm">Disable</button>`)}
      ${ext("Lab quota", "0.3.1", `<span class="chip info">${ic("info", "s14")} Needs approval</span>`, "Quota of an internal inference gateway.", "never run", "lab-quota", `<button class="btn outline sm">Review and enable</button>`)}
    </div>
    <div class="sm muted">Extensions run on this computer, with the access you approve. ${link("How extensions work")}</div>
  </div>
  <div class="dlg-f" style="justify-content:space-between"><button class="btn ghost">${ic("folder", "s14")} Open extensions folder</button><button class="btn">Close</button></div>`;
  out.push({ name: "40-extensions", w: 680, h: 390, html: dlgPage("40 extensions", 680, 390, extensions) });
  return out;
}
