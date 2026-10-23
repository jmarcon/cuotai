import { ic, page, stateIcon, stateRing, tile, trayFooter, trayHeader, TRAY_CSS } from "./lib.mjs";
import { t, usd, when, wd } from "./i18n.mjs";

const CSS = TRAY_CSS + `
.sbody { padding: var(--space-3); display: flex; flex-direction: column; gap: var(--space-4); }
.kpis3 { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space-2); }
.kpis3 .kpi { padding: var(--space-3) var(--space-3); }
.kpis3 .kpi .v { font-size: var(--text-figure); }
.pcard { padding: var(--space-3); border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: var(--surface); }
.pc-h { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: var(--space-3); }
.pc-h h2 { margin: 0; font-size: var(--text-base); font-weight: 650; }
.pk { position: relative; height: 120px; margin-left: 30px; display: flex; align-items: flex-end; gap: var(--space-1); border-bottom: 1px solid var(--border); }
.pk .g { position: absolute; left: 0; right: 0; border-top: 1px dashed var(--border); }
.pk .g span { position: absolute; left: -30px; width: 24px; text-align: right; top: -7px; }
.pk .z { position: absolute; left: -30px; width: 24px; text-align: right; bottom: -7px; }
.pk .b { position: relative; flex: 1; border-radius: var(--radius-xs) var(--radius-xs) 0 0; background: var(--chart-neutral); }
.pk .b.today { background: var(--accent); }
.pk .b em { position: absolute; top: -15px; left: 0; right: 0; text-align: center; font: normal var(--text-micro) var(--font-mono); color: var(--fg); }
.pkx { display: flex; gap: var(--space-1); margin: 4px 0 0 30px; }
.pkx span { flex: 1; text-align: center; }
.pkx span.t { color: var(--fg); font-weight: 600; }
.spark { display: inline-flex; align-items: flex-end; gap: 2px; height: 22px; width: 62px; }
.spark i { flex: 1; display: block; border-radius: var(--radius-xs) var(--radius-xs) 0 0; background: var(--chart-neutral); min-height: 1px; }
.spark i.last { background: var(--accent); } .spark i.hit { background: var(--bad); }
.sum { text-align: right; font-size: var(--text-md); line-height: 1.25; min-width: 64px; }
.sum b { font-weight: 600; } .sum span { display: block; font-size: var(--text-sm); color: var(--fg-muted); }
.ev { display: flex; align-items: center; gap: var(--space-3); min-height: 52px; padding: var(--space-2) var(--space-3); border-top: var(--border-w) solid var(--border); }
.ev:first-child { border-top: 0; }
.evm { width: 14px; display: grid; place-items: center; flex: none; }
.pk .b.ghost { background: repeating-linear-gradient(135deg, var(--track) 0 2px, transparent 2px 5px), color-mix(in srgb, var(--track) 40%, transparent); }
.pk .b.ghost em { color: var(--fg-muted); top: -15px; }
.sel { display: inline-flex; align-items: center; gap: var(--space-2); height: 28px; padding: 0 var(--space-2); border: var(--border-w) solid var(--border); border-radius: var(--radius-sm); background: var(--input-bg); font-size: var(--text-md); }
`;

const spark = (vals, hit = false, none = false) => none ? `<span class="spark muted sm" style="width:62px;height:auto;justify-content:flex-end">${t("st.noread")}</span>` : `<span class="spark" role="img" aria-label="Peak per day, last 7 days">${vals.map((v, i) => `<i class="${i === 6 ? (hit ? "hit" : "last") : ""}" style="height:${Math.max(1, (v / 100) * 22)}px"></i>`).join("")}</span>`;
const acct = (p, name, sub, vals, avg, hits, opts = {}) => `<div class="li two">${tile(p, 28, opts.mono)}<div class="grow"><div class="ttl trunc" title="${name}">${name}</div><div class="sub sm trunc" title="${sub}">${sub}</div></div>${opts.none ? "" : spark(vals, opts.hit)}<div class="sum">${opts.none ? `<b class="muted">—</b><span>${t("st.noreadyet")}</span>` : `<b>${t("st.avg", { n: avg })}</b><span>${hits === 0 ? t("st.nohits") : t("st.hit1")}</span>`}</div></div>`;
const evt = (mark, title, sub, at) => `<div class="ev"><span class="evm">${mark}</span><div class="grow"><div class="ttl" style="font-size:var(--text-base)">${title}</div><div class="sub muted md">${sub}</div></div><span class="mono muted sm nowrap">${at}</span></div>`;

const content = () => {
  const days = [[wd(16), 38], [wd(17), 31], [wd(18), 44], [wd(19), null], [wd(20), 41], [wd(21), 62], [t("today"), 100]];
  const h = 120;
  const today = t("today");
  return `<div class="sbody">
  <div class="kpis3">
    <div class="kpi"><div class="l">${t("st.k.hits")}</div><div class="v">1</div><div class="c">${t("st.k.in7")}</div></div>
    <div class="kpi"><div class="l">${t("st.k.avg")}</div><div class="v">≈52%</div><div class="c">${t("st.k.tight")}</div></div>
    <div class="kpi"><div class="l">${t("st.k.extra")}</div><div class="v" ${usd(6.25).length > 6 ? 'style="font-size:var(--text-xl)"' : ""}>${usd(6.25)}</div><div class="c">${t("sub.extrausage").toLowerCase()}</div></div>
  </div>
  <div class="pcard">
    <div class="pc-h"><div><h2>${t("st.peak")}</h2><span class="sm muted">${t("st.peak.d")}</span></div><div class="seg mono"><button class="on">7d</button><button>30d</button></div></div>
    <div class="pk" role="img" aria-label="${t("st.peak")}">
      <div class="g" style="bottom:${h}px"><span class="axis">100</span></div><div class="g" style="bottom:${h / 2}px"><span class="axis">50</span></div><span class="z axis">0</span>
      ${days.map(([d, v]) => v == null ? `<div class="b ghost" style="height:${h}px" title="${t("nodata")}"><em>—</em></div>` : `<div class="b ${d === today ? "today" : ""}" style="height:${(v / 100) * h}px">${d === today ? `<em>${v}%</em>` : ""}</div>`).join("")}
    </div>
    <div class="pkx axis">${days.map(([d]) => `<span class="${d === today ? "t" : ""}">${d}</span>`).join("")}</div>
  </div>
  <section><div class="grp-h"><span class="micro">${t("st.by")}</span><span class="hint">${t("st.by.h")}</span></div><div class="grp">
    ${acct("claude", "work@example.com", "Claude Code", [52, 48, 63, 70, 66, 81, 78], 61, 0, { mono: "WO" })}
    ${acct("claude", "side@example.com", "Claude Code", [20, 25, 31, 22, 34, 30, 29], 27, 0, { mono: "SI" })}
    ${acct("codex", "OpenAI Codex", "me@example.com", [35, 40, 38, 46, 41, 52, 37], 42, 0)}
    ${acct("cursor", "Cursor", "dev@example.com", [40, 52, 48, 57, 66, 60, 100], 58, 1, { hit: true })}
    ${acct("kimi", "Kimi", "ops@example.com", [28, 30, 35, 31, 36, 38, 41], 34, 0)}
    ${acct("groq", "Groq", "team@example.com", [18, 22, 20, 25, 24, 27, 26], 23, 0)}
    ${acct("anthropic", "Anthropic API", "api@example.com", [], 0, 0, { none: true })}
  </div></section>
  <section><div class="grp-h"><span class="micro">${t("st.events")}</span></div><div class="grp">
    ${evt(stateRing("exhausted"), t("st.e.hit"), t("st.e.hit.d"), `${t("today")} ${when(22, 10, 44)}`)}
    ${evt(stateIcon("arrowUp", "warn"), t("st.e.pace"), t("st.e.pace.d", { when: when(24, 10, 20) }), `${t("today")} ${when(22, 17, 50)}`)}
    ${evt(stateIcon("alert", "bad"), t("st.e.inc"), t("st.e.inc.d", { state: t("inc.partial").toLowerCase() }), `${t("today")} ${when(22, 17, 32)}`)}
    ${evt(stateRing("stale", 74), t("st.e.stale"), t("st.e.stale.d"), `${t("today")} ${when(22, 17, 39)}`)}
  </div></section>
  <div class="sm muted" style="padding:0 var(--space-1)">${t("st.keep")} <a class="r-accent" style="text-decoration:underline;text-underline-offset:2px">${t("st.change")}</a></div>
  </div>`;
};

const pop = (inner) => `<div class="pop">${trayHeader({ back: true, title: t("stats"), right: `<div class="sel">${t("st.all")} ${ic("chevDown", "s14")}</div>` })}<div class="pbody">${inner}</div>${trayFooter()}</div>`;

export function screens() {
  const mk = (name, inner, title) => ({ name, w: 380, h: 600, html: page({ title, w: 380, h: 600, css: CSS, body: pop(inner) }) });
  return [
    mk("70-stats", content(), "70 stats"),
    mk("70b-stats-scrolled", `<div style="margin-top:-452px">${content()}</div>`, "70b stats scrolled"),
  ];
}
