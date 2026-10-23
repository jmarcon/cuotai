import { UI, ic, meter, page, ring, stateIcon, stateRing, tabState, tchip, tile, TABS, TRAY_CSS, LABEL } from "./lib.mjs";
import { resetsIn } from "./fixtures.mjs";
import { ago, t, usd, when } from "./i18n.mjs";

export const MAIN_CSS = TRAY_CSS + `
body { background: var(--body-bg); }
.tb { display: flex; align-items: center; gap: var(--space-3); height: 56px; padding: 0 var(--space-5) 0 76px; background: var(--bg); border-bottom: var(--border-w) solid var(--border); flex: none; }
.tb .brand { display: flex; align-items: center; gap: var(--space-2); }
.tb .brand img { width: 24px; height: 24px; display: block; }
.tb h1 { margin: 0; font-size: var(--text-lg); font-weight: 650; }
.tb .search { flex: 1 1 140px; max-width: 216px; min-width: 120px; color: var(--fg-muted); }
.tb .search .kbd { margin-left: auto; }
.tb .tabs .tab { height: 32px; }
.tb.narrow { flex-direction: column; align-items: stretch; height: auto; padding: var(--space-2) var(--space-4) var(--space-2) 76px; gap: var(--space-2); }
.tb.narrow .l1, .tb.narrow .l2 { display: flex; align-items: center; gap: var(--space-2); }
.tb.narrow { padding-left: var(--space-4); }
.tb.narrow .l1 { padding-left: 60px; }
.banners { padding: var(--space-4) var(--space-5) 0; }
.upd { display: flex; gap: var(--space-3); padding: var(--space-3) var(--space-4); border-radius: var(--radius-md); background: color-mix(in srgb, var(--info) 12%, var(--surface)); }
.upd > svg.i { color: var(--info); margin-top: 2px; }
.upd .t { font-weight: 600; } .upd .d { color: var(--fg-muted); font-size: var(--text-sm); }
.upd .wn { margin-top: var(--space-2); font-size: var(--text-md); display: flex; flex-direction: column; gap: 6px; }
.upd .wn summary { list-style: none; display: inline-flex; align-items: center; gap: 4px; font-size: var(--text-md); font-weight: 500; }
.grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-4); padding: var(--space-5); align-items: stretch; }
.grid.one { grid-template-columns: minmax(0, 1fr); padding: var(--space-4); }
.card { display: flex; flex-direction: column; border: var(--border-w) solid var(--border); border-radius: var(--radius-lg); background: var(--card-bg); box-shadow: var(--shadow-card); overflow: hidden; }
.card.stripe-bad { box-shadow: inset 3px 0 0 var(--bad), var(--shadow-card); }
.card.off { opacity: 0.55; }
.ch { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-4) var(--space-4) var(--space-3) var(--space-2); }
.ch .grip { color: var(--fg-muted); opacity: 0.6; display: grid; place-items: center; width: 24px; height: 32px; flex: none; }
.ch .nm { font-size: var(--text-xl); line-height: 1.25; font-weight: 650; display: flex; align-items: center; gap: var(--space-2); }
.ch .ac { font-size: var(--text-md); color: var(--fg-muted); }
.ch .acts { display: flex; align-items: center; gap: var(--space-1); margin: 0; }
.ch .sw { margin-left: var(--space-2); }
.sumr { display: grid; grid-template-columns: 1fr 1fr; gap: var(--space-5); padding: 0 var(--space-4) var(--space-3) var(--space-5); }
.sumr.one { grid-template-columns: 1fr; }
.win { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
.win .w1 { display: flex; align-items: center; gap: var(--space-2); min-height: 20px; }
.win .w2 { display: flex; flex-direction: column; align-items: flex-start; gap: 0; }
.win .fig { font-size: var(--text-figure); line-height: 1.05; font-weight: 700; }
.win .w2 small { font-size: var(--text-micro); line-height: 1.1; color: var(--fg-muted); }
.win .rs { font: var(--text-micro) var(--font-mono); color: var(--fg-muted); }
.win .mtr { margin: 3px 0; }
.win.nodata .fig { color: var(--fg-muted); font-weight: 400; }
.cst { display: flex; flex-direction: column; gap: 6px; padding: 0 var(--space-5) var(--space-3); }
.cst .stl { font-size: var(--text-md); }
.cst .stl .lbl { font-size: var(--text-md); }
.cst .meta { display: flex; align-items: center; gap: 6px; font-size: var(--text-sm); color: var(--fg-muted); }
.disc { display: flex; align-items: center; gap: var(--space-2); margin: 0 var(--space-4) var(--space-3); padding: 0 var(--space-3); height: 36px; border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: transparent; font-size: var(--text-md); text-align: left; }
.disc .k { font-weight: 500; } .disc .v { margin-left: auto; color: var(--fg-muted); font-size: var(--text-sm); }
.cf { display: flex; align-items: center; gap: var(--space-2); margin-top: auto; padding: var(--space-2) var(--space-4) var(--space-3) var(--space-5); font-size: var(--text-sm); color: var(--fg-muted); }
.cf .btn { margin-left: auto; }
.state { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: var(--space-3); text-align: center; padding: var(--space-8); }
`;

export const topbar = ({ narrow = false, count = 7, withTools = true, ringPct = 22 } = {}) => {
  const age = ago(1);
  const brand = `<div class="brand"><h1 class="wordmark main"><span class="wm-c" aria-hidden="true"></span><span aria-hidden="true">uot<span class="wm-ai">AI</span></span><span class="wm-name">Cuotai</span></h1></div>`;
  const tabs = `<div class="tabs" role="tablist"><button class="tab all on" role="tab">${ring(ringPct, 16, null, 5)}<span>${t("tabs.all")}</span><span class="cnt">${count}</span></button>${TABS.map((x) => `<button class="tab" role="tab" aria-label="${LABEL[x.p]}"><span class="tw">${tile(x.p, 20)}${tabState(x.st)}</span>${x.n > 1 ? `<span class="cnt">${x.n}</span>` : ""}</button>`).join("")}</div>`;
  const search = `<div class="field search">${ic("search", "s14")}<span class="hold">${t("filter.placeholder")}</span><span class="kbd">/</span></div>`;
  const sort = `<button class="btn outline" aria-haspopup="menu" aria-label="${t("sort.custom")}" title="${t("sort.custom")}" style="padding:0 var(--space-2)">${ic("sort", "s14")}${ic("chevDown", "s14")}</button>`;
  if (!withTools) {
    return `<header class="tb">${brand}<span class="grow"></span><button class="ib w" aria-label="Settings">${ic("gear", "s18")}</button></header>`;
  }
  if (narrow) {
    return `<header class="tb narrow"><div class="l1">${brand}<span class="grow"></span><button class="ib w" aria-label="Filter accounts">${ic("search", "s18")}</button><button class="ib w" aria-label="Add account">${ic("plus", "s18")}</button><button class="ib w" aria-label="Refresh, last refreshed ${age}">${ic("refresh", "s18")}</button><button class="ib w" aria-label="More">${ic("more", "s18")}</button></div>
    <div class="l2">${tabs}<span class="grow"></span><button class="ib w" aria-label="Sort: custom order">${ic("sort", "s18")}</button></div></header>`;
  }
  return `<header class="tb">${brand}${tabs}${search}${sort}<span class="grow"></span>
    <button class="btn ghost mono" style="font-size:var(--text-sm)" aria-label="Refresh, last refreshed ${age}">${ic("refresh", "s14")} ${age}</button>
    <button class="btn outline">${ic("plus", "s14")} <span>${t("common.add")}</span></button>
    <button class="ib w" aria-label="Extensions">${ic("puzzle", "s18")}</button>
    <button class="ib w" aria-label="Settings">${ic("gear", "s18")}</button></header>`;
};

const win = ({ label, binding, fig, unit = t("left"), figRole = "", meterHtml, rs, nodata, dim }) => `<div class="win ${nodata ? "nodata" : ""} ${dim ? "dim" : ""}">
  <div class="w1"><span class="micro">${label}</span>${binding ? `<span class="chip accent">${t("binding")}</span>` : ""}</div>
  <div class="w2"><b class="fig ${figRole}">${fig}</b>${unit ? `<small>${unit}</small>` : ""}</div>
  ${meterHtml}
  <div class="rs">${rs ?? "&nbsp;"}</div>
</div>`;

export const card = (o) => `<article class="card ${o.cls ?? ""}">
  <div class="ch"><span class="grip">${ic("grip")}</span>${tile(o.p, 40, o.mono)}
    <div class="grow"><div class="nm"><span>${o.name}</span>${o.plan ? `<span class="plan">${o.plan}</span>` : ""}</div><div class="ac trunc">${o.acct}</div></div>
    <div class="acts"><button class="ib w" aria-label="Refresh this account">${ic("refresh", "s18")}</button><button class="ib w" aria-label="More actions">${ic("more", "s18")}</button><span class="sw on" role="switch" aria-checked="true" aria-label="Enabled"></span></div>
  </div>
  <div class="sumr ${o.wins.length === 1 ? "one" : ""}">${o.wins.join("")}</div>
  <div class="cst">${o.lines.join("")}</div>
  ${o.cost ? `<button class="disc">${ic("chev", "s14")}<span class="k">${t("lc.title")}</span><span class="muted">· ${t("estimate")}</span><span class="v">${o.cost}</span></button>` : ""}
  <div class="cf">${o.foot}<button class="btn ghost sm">${t("card.details")} ${ic("chev", "s14")}</button></div>
</article>`;

const stlLine = (ringHtml, role, label, det, chips = []) => `<div class="stl">${ringHtml}<span class="lbl r-${role}">${label}</span>${det ? `<span class="det">· ${det}</span>` : ""}${chips.join("")}</div>`;
const access = (txt) => `<div class="meta">${ic("shield", "s14")}<span>${txt}</span></div>`;
const wi = (name) => ic(name, "s14");

const accessM = (lvl, sum) => access(`${t("lvl." + lvl)} · ${t("sum." + sum)}`);
const limitsFoot = (n, models) => `<span>${n === 1 ? t("lim.1") : t("lim.n", { n })}${models ? ` · ${t("card.modelCount", { count: models })}` : ""}</span>`;
export const cards = () => {
  const M = (v, o = {}) => meter(v, o);
  return ({
  claude1: card({
    p: "claude", name: "work@example.com", mono: "WO", plan: "Max", acct: "Claude Code",
    wins: [
      win({ label: t("sub.session"), fig: "66%", meterHtml: M(66, { pace: 44, label: "66%" }), rs: resetsIn(when(22, 20, 32), "2h 11m") }),
      win({ label: t("sub.weekly"), binding: true, fig: "22%", meterHtml: M(22, { pace: 42, label: "22%" }), rs: resetsIn(when(25, 17, 20), "2d 23h") }),
    ],
    lines: [stlLine(stateRing("pct", 22, 14), "warn", t("st.ahead"), "", [tchip("runs_out", { at: when(24, 10, 20) })]), accessM("undocumented", "claude")],
    cost: t("lc.main", { today: usd(0.11), week: usd(1.41) }), foot: limitsFoot(2, 6),
  }),
  claude2: card({
    p: "claude", name: "side@example.com", mono: "SI", plan: "Pro", acct: "Claude Code",
    wins: [
      win({ label: t("sub.session"), fig: "94%", meterHtml: M(94, { pace: 90, label: "94%" }), rs: resetsIn(when(22, 21, 10), "2h 49m") }),
      win({ label: t("sub.weekly"), binding: true, fig: "71%", meterHtml: M(71, { pace: 70, label: "71%" }), rs: resetsIn(when(26, 9, 0), "3d 15h") }),
    ],
    lines: [stlLine(stateRing("pct", 71, 14), "good", t("pace.on_pace"), "", [tchip("lasts")]), accessM("undocumented", "claude")],
    foot: limitsFoot(2, 4),
  }),
  codex: card({
    p: "codex", name: "OpenAI Codex", plan: "Plus", acct: "me@example.com",
    wins: [
      win({ label: t("sub.session"), fig: "88%", meterHtml: M(88, { pace: 84, label: "88%" }), rs: resetsIn(when(22, 21, 44), "3h 23m") }),
      win({ label: t("sub.weekly"), binding: true, fig: "63%", meterHtml: M(63, { pace: 55, label: "63%" }), rs: resetsIn(when(27, 18, 20), "4d 23h") }),
    ],
    lines: [stlLine(stateRing("pct", 63, 14), "good", t("st.reserve"), "", [tchip("reserve", { d: 8 }), tchip("lasts")]), accessM("undocumented", "chat_gpt")],
    cost: t("lc.codex", { yday: usd(0.19), week: usd(0.83) }), foot: limitsFoot(2, 4),
  }),
  cursor: card({
    p: "cursor", name: "Cursor", plan: "Pro", acct: "dev@example.com", cls: "stripe-bad",
    wins: [
      win({ label: t("sub.fast"), binding: true, fig: t("metric.exhausted"), unit: "", figRole: "r-bad", meterHtml: M(0, { role: "bad", label: t("metric.exhausted") }), rs: t("cur.used", { n: "500", when: when(31, 2, 20) }) }),
      win({ label: t("sub.extrausage"), fig: usd(6.25), unit: t("cur.leftof", { b: usd(20) }), figRole: "r-warn", meterHtml: M(31, { role: "warn", label: usd(6.25) }), rs: t("cur.since", { when: when(22, 10, 44) }) }),
    ],
    lines: [stlLine(stateRing("exhausted", 0, 14), "bad", t("st.limithit"), t("st.onextra.long")), accessM("official", "cursor_personal")],
    foot: limitsFoot(2, 0) + `<span class="chip warn">${wi("zap")} ${t("ov.active", { amt: usd(6.25) })}</span>`,
  }),
  kimi: card({
    p: "kimi", name: "Kimi", acct: "ops@example.com",
    wins: [
      win({ label: t("sub.session"), fig: "—", unit: "", nodata: true, meterHtml: M(null, { none: true }), rs: t("nodata") }),
      win({ label: t("sub.weekly"), binding: true, fig: "59%", meterHtml: M(59, { pace: 56, label: "59%" }), rs: resetsIn(when(27, 7, 20), "4d 13h") }),
    ],
    lines: [stlLine(stateRing("pct", 59, 14), "muted", t("pace.on_pace"), "", [tchip("lasts")]), accessM("official", "kimi")],
    foot: limitsFoot(1, 0),
  }),
  groq: card({
    p: "groq", name: "Groq", acct: "team@example.com",
    wins: [win({ label: t("sub.rpd"), fig: "74%", dim: true, meterHtml: M(74, { dim: true, label: "74%" }), rs: t("reading.from", { when: when(22, 17, 39) }) })],
    lines: [stlLine(stateRing("stale", 74, 14), "warn", `${t("stale")} · ${ago(42)}`, t("st.timeout.long")), accessM("official", "groq")],
    foot: limitsFoot(1, 0),
  }),
  anthropic: card({
    p: "anthropic", name: "Anthropic API", acct: "api@example.com",
    wins: [`<div class="win nodata"><div class="w1"><span class="micro">${t("usage")}</span></div><div class="w2"><b class="fig r-accent" style="font-weight:700">${t("st.auth")}</b></div><div class="rs">&nbsp;</div></div>`],
    lines: [stlLine(stateIcon("key", "accent", "s14"), "accent", t("st.keyrej"), t("st.replace.long")), accessM("official", "anthropic_api")],
    foot: `<button class="btn outline sm" style="margin-left:0">${ic("key", "s14")} ${t("btn.updatekey")}</button>`,
  }),
});
};

const win1200 = (inner, extra = "") => `<div class="mw">${inner}</div>`;

export function screens() {
  const out = [];
  const c = cards();
  const cn = c;
  const grid = `<main class="grid">${[c.claude1, c.claude2, c.codex, c.cursor, c.kimi, c.groq, c.anthropic].join("")}</main>`;
  const css = MAIN_CSS + `.mw { height: 100%; display: flex; flex-direction: column; overflow: hidden; }`;
  out.push({ name: "10-main-overview", w: 1200, h: 820, html: page({ title: "10 main overview", w: 1200, h: 820, css, body: win1200(topbar() + grid) }) });
  out.push({ name: "11-main-overview-full", w: 1200, h: 1310, html: page({ title: "11 main overview full", w: 1200, h: 1310, css, body: win1200(topbar() + grid) }) });

  const banner = `<div class="banners"><div class="upd" role="status">${ic("download", "s22")}<div class="grow">
    <div class="t">Update 0.72.0 available</div><div class="d">Released 22 Oct 2026</div>
    <details open class="wn"><summary>${ic("chevDown", "s14")} What's new</summary><div>Measurement fixes and a new automatic-update option.</div></details></div>
    <div style="display:flex;gap:8px;align-items:flex-start"><button class="btn ghost">Remind me later</button><button class="btn primary">Update now</button></div></div></div>`;
  out.push({ name: "13-main-update-available", w: 1200, h: 820, html: page({ title: "13 update available", w: 1200, h: 820, css, body: win1200(topbar() + banner + grid) }) });

  const empty = `<div class="state" style="flex:1">
    <span style="display:grid;place-items:center;width:56px;height:56px;border-radius:50%;background:var(--surface-2);color:var(--fg-muted)">${ic("plus", "s22")}</span>
    <h2 style="margin:0;font-size:var(--text-xl);font-weight:650">No accounts yet</h2>
    <p class="muted" style="margin:0;max-width:380px">Add an account to see how much is left, until when, and whether you can trust the number.</p>
    <button class="btn primary lg">${ic("plus", "s14")} Add account</button>
    <div class="sm muted" style="margin-top:var(--space-4)">Popular providers</div>
    <div style="display:flex;gap:var(--space-2)">${["claude", "codex", "cursor", "kimi", "groq", "anthropic"].map((p) => `<button class="tab" style="width:44px;height:44px;border-color:var(--border);background:var(--surface)" aria-label="Connect ${LABEL[p]}">${tile(p, 28)}</button>`).join("")}</div>
    <div class="sm muted" style="margin-top:var(--space-4)">Accounts are read from <span class="mono">~/Library/Application Support/cuotai/cuotai.toml</span></div>
  </div>`;
  out.push({ name: "14-main-empty", w: 1200, h: 820, html: page({ title: "14 main empty", w: 1200, h: 820, css, body: win1200(topbar({ withTools: false }) + empty) }) });

  const grid1 = `<main class="grid one">${[cn.claude1, cn.cursor, cn.groq].join("")}</main>`;
  out.push({ name: "15-main-narrow", w: 520, h: 820, html: page({ title: "15 main narrow", w: 520, h: 820, css, body: win1200(topbar({ narrow: true }) + grid1) }) });
  return out;
}
