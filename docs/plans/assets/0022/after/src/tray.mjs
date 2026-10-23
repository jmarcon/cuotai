import { ic, levelOf, meter, page, ring, row, stateIcon, stateRing, sub, tchip, tile, timeline, trayFooter, trayHeader, TABS, TRAY_CSS } from "./lib.mjs";
import { A, ORDER, PRESETS, displayRows, resetsIn, rows, subsClaude, incidents } from "./fixtures.mjs";
import { ago, t, usd, when } from "./i18n.mjs";


const VIEW_CSS = `
.rgrid { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space-2); padding: var(--space-3); }
.rt { display: flex; flex-direction: column; align-items: center; gap: 6px; min-width: 0; padding: var(--space-3) var(--space-1) var(--space-2); border: var(--border-w) solid var(--border); border-radius: var(--radius-md); background: var(--surface); }
.rw { position: relative; display: grid; place-items: center; }
.rw svg.ring { position: absolute; inset: 0; }
.rw .rv { position: relative; font-size: var(--text-base); font-weight: 700; line-height: 1; }
.rw .rv.s { font-size: var(--text-md); }
.rt .t { display: flex; align-items: center; gap: 4px; max-width: 100%; padding: 0 var(--space-1); font-size: var(--text-sm); font-weight: 600; }
.rt .s { display: flex; align-items: center; gap: 3px; max-width: 100%; padding: 0 var(--space-1); font-size: var(--text-micro); font-weight: 600; white-space: nowrap; }
.cr { display: flex; align-items: center; gap: var(--space-2); height: 44px; padding: 0 var(--space-3); border-bottom: var(--border-w) solid var(--border); }
.cr .nm { flex: 1; min-width: 0; font-size: var(--text-base); font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cr .cb { position: relative; width: 72px; flex: none; }
.cr .cb .mtr { margin: 0; }
.cr .cb .pt { position: absolute; top: -3px; bottom: -3px; width: 2px; margin-left: -1px; background: var(--fg); border-radius: 1px; }
.cr .cf { width: 78px; flex: none; text-align: right; white-space: nowrap; font-size: var(--text-base); font-weight: 700; }
.cr .cf small { font-size: var(--text-sm); font-weight: 400; color: var(--fg-muted); margin-left: 2px; }
.cr > svg.i:last-child { color: var(--fg-muted); }
`;

const pop = ({ header, body, footer }) => `<div class="pop">${header}<div class="pbody">${body}</div>${footer}</div>`;
const shell = (title, inner, mode = "dark", extra = "") => page({ title, w: 380, h: 600, mode, css: TRAY_CSS + VIEW_CSS + extra, body: inner });

const lanes = () => [
  { p: "claude", h: 71, role: "bad", label: `work · ${when(25, 17, 20)}` },
  { p: "claude", h: 86.65, role: "good", label: `side · ${when(26, 9, 0)}` },
  { p: "kimi", h: 108.98, role: "warn", label: when(27, 7, 20) },
  { p: "codex", h: 119.98, role: "warn", label: when(27, 18, 20) },
];
const laterAll = () => [{ p: "cursor", label: when(31, 2, 20) }];
const unknownAll = () => [{ p: "groq", label: t("tl.unkStale") }, { p: "anthropic", label: t("tl.unkAuth") }];
const tlAll = () => timeline({ lanes: lanes(), later: laterAll(), unknown: unknownAll() });

const lines = (items) => `<div class="lines">${items.join("")}</div>`;
const mutedLine = (icon, html) => `<div class="muted ln">${ic(icon, "s14")}<span class="txt">${html}</span></div>`;
const acts = () => `<div class="acts"><button class="btn ghost sm">${t("card.details")}</button><button class="btn ghost sm">${ic("refresh", "s14")} ${t("common.refresh")}</button></div>`;
const link = (x) => `<a class="r-accent" style="text-decoration:underline;text-underline-offset:2px">${x} ${ic("ext", "s12")}</a>`;
const access = (lvl, sum) => mutedLine("shield", `${t("lvl." + lvl)} · ${t("sum." + sum)}`);

// ---- alternative tray views (owner decision 1): Rings and Compact ----
const ringTile = ({ p, title, pct = null, role, centre, status, statusRole, statusIcon, dim = false, kind = "pct" }) => {
  const lv = role ?? (pct == null ? "info" : levelOf(pct));
  const rg = ring(pct ?? 0, 64, lv, 3.1); // one open C arc per account (DS-CMP-RING-2)
  const inner = kind === "icon" ? ic(centre, "s22 r-" + statusRole) : `<span class="rv ${kind === "text" ? "s" : ""} ${centre.cls ?? ""}">${centre.t}</span>`;
  return `<div class="rt"><div class="rw ${dim ? "dim" : ""}" style="width:64px;height:64px">${pct == null ? rg.replace("<g ", '<g opacity="0.4" ') : rg}${inner}</div>
  <div class="t">${tile(p, 14)}<span class="trunc" title="${title}">${title}</span></div>
  <div class="s r-${statusRole}">${statusIcon}<span>${status}</span></div></div>`;
};
const ringsBody = () => `<div class="rgrid">
  ${ringTile({ p: "claude", title: "work@example.com", pct: 22, centre: { t: "22%" }, status: t("st.ahead"), statusRole: "warn", statusIcon: ic("warn", "s14") })}
  ${ringTile({ p: "claude", title: "side@example.com", pct: 71, centre: { t: "71%" }, status: t("pace.on_pace"), statusRole: "good", statusIcon: ic("check", "s14") })}
  ${ringTile({ p: "codex", title: "Codex", pct: 63, centre: { t: "63%" }, status: t("st.reserve"), statusRole: "good", statusIcon: ic("check", "s14") })}
  ${ringTile({ p: "cursor", title: "Cursor", pct: 0, role: "bad", centre: { t: usd(6.25), cls: "r-warn" }, kind: "text", status: t("st.limithit"), statusRole: "bad", statusIcon: ic("xcircle", "s14") })}
  ${ringTile({ p: "kimi", title: "Kimi", pct: 59, centre: { t: "59%" }, status: t("pace.on_pace"), statusRole: "muted", statusIcon: `<i class="dot r-muted"></i>` })}
  ${ringTile({ p: "groq", title: "Groq", pct: 74, dim: true, centre: { t: "74%" }, status: `${t("stale")} ${ago(42)}`, statusRole: "warn", statusIcon: ic("clock", "s14") })}
  ${ringTile({ p: "anthropic", title: "Anthropic", pct: null, centre: "key", kind: "icon", status: t("st.keyrej"), statusRole: "accent", statusIcon: "" })}
</div>`;
const crow = ({ p, title, mark, left, pace, none, dim, fig, unit = t("left"), figCls = "", mono = "" }) => `<div class="cr">${tile(p, 24, mono)}<span class="nm" title="${title}">${title}</span>${mark}${left == null && !none ? "" : `<div class="cb">${none ? `<div class="mtr thin none"></div>` : meter(left, { thin: true, pace: dim ? null : pace, dim })}</div>`}<span class="cf ${figCls} ${dim ? "dim" : ""}">${fig}${unit ? `<small>${unit}</small>` : ""}</span>${ic("chev")}</div>`;
const compactBody = () => `
  ${crow({ p: "claude", title: "work@example.com", mono: "WO", mark: stateRing("pct", 22), left: 22, pace: 42, fig: "22%" })}
  ${crow({ p: "claude", title: "side@example.com", mono: "SI", mark: stateRing("pct", 71), left: 71, pace: 70, fig: "71%" })}
  ${crow({ p: "codex", title: "OpenAI Codex", mark: stateRing("pct", 63), left: 63, pace: 55, fig: "63%" })}
  ${crow({ p: "cursor", title: "Cursor", mark: stateRing("exhausted"), none: true, fig: usd(6.25), unit: t("extra"), figCls: "r-warn" })}
  ${crow({ p: "kimi", title: "Kimi", mark: stateRing("pct", 59), left: 59, pace: 56, fig: "59%" })}
  ${crow({ p: "groq", title: "Groq", mark: stateRing("stale", 74), left: 74, dim: true, fig: "74%" })}
  ${crow({ p: "anthropic", title: "Anthropic API", mark: stateIcon("key", "accent"), fig: t("st.auth"), unit: "", figCls: "r-accent" })}
`;

export function screens() {
  const out = [];
  const all = () => ({ header: trayHeader({ tabs: TABS }), footer: trayFooter() });
  const tl = tlAll();

  out.push({ name: "01-tray-overview", w: 380, h: 600, html: shell("01 tray overview", pop({ ...all(), body: rows() + tl })) });
  out.push({ name: "01b-tray-overview-scrolled", w: 380, h: 600, html: shell("01b tray overview scrolled", pop({ ...all(), body: `<div style="margin-top:-322px">${rows()}${tl}</div>` })) });
  out.push({ name: "01c-tray-rings-view", w: 380, h: 600, html: shell("01c tray rings view", pop({ ...all(), body: ringsBody() + tl })) });
  out.push({ name: "01d-tray-compact-view", w: 380, h: 600, html: shell("01d tray compact view", pop({ ...all(), body: compactBody() + tl })) });

  // 01e minimal preset (figure + bar + one status); 01f percent mode (pace deviation as percent)
  out.push({ name: "01e-tray-minimal-view", w: 380, h: 600, html: shell("01e tray minimal view", pop({ ...all(), body: displayRows(PRESETS.minimal) + tl })) });
  const pctOpen = `
    ${subsClaude(true)}
    <div class="call warning">${ic("clock")}<div>${t("fc.weekly", { when: `<b>≈${when(24, 10, 20)}</b>`, margin: "1d 7h" })}</div><button class="btn ghost sm act">${ic("bell", "s14")} ${t("alertme")}</button></div>
    ${acts()}`;
  // percent mode (DS-CMP-TIME-6): the status label itself becomes the percent chip text; compact form "↑ 34%"
  const withPct = (o, kind, d, glyph) => ({ ...o, status: { ...o.status, label: t(`chip.${kind}`, { d }), labelC: `${glyph} ${d}%` } });
  out.push({
    name: "01f-tray-percent-mode", w: 380, h: 600,
    html: shell("01f tray percent mode", pop({
      ...all(),
      body: row({ ...withPct(A.claude1(), "ahead_pct", 34, "↑"), open: pctOpen }) + row(A.claude2()) + row(withPct(A.codex(), "reserve_pct", 18, "↓")) + row(A.cursor()) + row(A.kimi()) + row(A.groq()) + row(A.anthropic()),
    })),
  });

  const tabsInc = [
    { p: "claude", n: 2, st: { kind: "icon", icon: "alert", role: "bad" } }, { p: "codex", n: 1 }, { p: "cursor", n: 1, st: { kind: "icon", icon: "alert", role: "bad" } },
    { p: "kimi", n: 1, st: { kind: "icon", icon: "wrench", role: "info" } }, { p: "groq", n: 1, st: { kind: "icon", icon: "alert", role: "warn" } }, { p: "anthropic", n: 1, st: { kind: "icon", icon: "key", role: "accent" } },
  ];
  out.push({ name: "02-tray-incidents", w: 380, h: 600, html: shell("02 tray incidents", pop({ header: trayHeader({ tabs: tabsInc }), footer: trayFooter(), body: rows({ all: { inc: true } }) })) });

  // 03 Claude expanded: sub-limits + forecast callout
  const claudeBody = `
    ${subsClaude()}
    <div class="call warning">${ic("clock")}<div>${t("fc.weekly", { when: `<b>≈${when(24, 10, 20)}</b>`, margin: "1d 7h" })}</div><button class="btn ghost sm act">${ic("bell", "s14")} ${t("alertme")}</button></div>
    ${lines([
      `<div class="r-bad ln">${ic("alert", "s14")}<span class="txt"><b style="font-weight:600">${t("inc.partial")}</b> <span class="muted">· Elevated errors on claude.ai & Claude Code · ${t("incident.since", { time: when(22, 17, 32) })}</span></span></div>`,
      `<div class="muted ln" style="padding-left:20px"><span class="txt">${t("note.incident.ongoing")} <span class="nowrap">${link(t("incident.open"))}</span></span></div>`,
      access("undocumented", "claude"),
      mutedLine("dollar", t("lc.line", { today: usd(0.11), week: usd(1.41) })),
    ])}
    ${acts()}`;
  out.push({
    name: "03-tray-account-claude-expanded", w: 380, h: 600,
    html: shell("03 tray Claude expanded", pop({
      header: trayHeader({ tabs: TABS, selected: "claude" }), footer: trayFooter(),
      body: row({ ...A.claude1({ inc: false }), open: claudeBody, extra: null }) + row(A.claude2()) + timeline({ lanes: lanes().slice(0, 2), withHeader: true }),
    })),
  });

  // 05 Cursor on extra balance
  const cursorBody = `
    ${sub({ label: t("sub.fast"), figHtml: `<b class="r-bad">${t("metric.exhausted")}</b>`, meterHtml: meter(0, { role: "bad", label: t("metric.exhausted") }), pace: t("cur.limit", { n: "500" }), paceRole: "bad", paceIcon: "xcircle", reset: resetsIn(when(31, 2, 20), "8d 7h") })}
    <div class="sub">
      <div class="s1"><span class="sl">${t("sub.extrausage")}</span><span class="chip warn">${ic("zap", "s12")}<span>${t("ov.active", { amt: usd(6.25) })}</span></span><span class="grow"></span><span class="sf"><b>${usd(6.25)}</b><small>${t("cur.leftof", { b: usd(20) })}</small></span></div>
      ${meter(31, { role: "warn", label: usd(6.25) })}
      <div class="s3"><span class="r-warn pc">${ic("zap", "s14")} ${t("cur.since", { when: when(22, 10, 44) })}</span><span class="muted">${t("cur.budget")}</span></div>
    </div>
    <div class="call warning">${ic("zap")}<div>${t("cur.callout", { left: `<b>${t("sub.extra").toLowerCase()}</b>`, amt: usd(6.25) })}</div><button class="btn ghost sm act">${ic("bell", "s14")} ${t("alertme")}</button></div>
    ${lines([access("official", "cursor_personal")])}
    ${acts()}`;
  out.push({
    name: "05-tray-detail-cursor-extra", w: 380, h: 600,
    html: shell("05 tray Cursor extra", pop({
      header: trayHeader({ tabs: TABS, selected: "cursor" }), footer: trayFooter(),
      body: row({ ...A.cursor(), open: cursorBody, extra: null }),
    })),
  });

  // 06 Groq stale
  const groqBody = `
    ${sub({ label: t("sub.rpd"), fig: "74%", meterHtml: meter(74, { dim: true, label: "74%" }), pace: "", reset: t("reading.from", { when: when(22, 17, 39) }) }).replace('class="sub"', 'class="sub dim"')}
    <div class="call warning">${ic("clock")}<div><b>${t("stale")} · ${ago(42)}.</b> ${t("groq.callout", { when: when(22, 17, 39), d: "4 min" })}</div></div>
    ${lines([
      mutedLine("info", t("stale.ignored")),
      `<div class="r-warn ln">${ic("alert", "s14")}<span class="txt"><b style="font-weight:600">${t("inc.degraded")}</b> <span class="muted">· Slow responses in eu-west</span></span></div>`,
      access("official", "groq"),
    ])}
    <div class="acts"><button class="btn ghost sm">${t("card.details")}</button><button class="btn ghost sm">${ic("refresh", "s14")} ${t("retry")}</button></div>`;
  out.push({
    name: "06-tray-groq-stale", w: 380, h: 600,
    html: shell("06 tray Groq stale", pop({ header: trayHeader({ tabs: TABS, selected: "groq" }), footer: trayFooter(), body: row({ ...A.groq(), open: groqBody }) })),
  });

  // 07 Anthropic API auth error
  const errBody = `
    <div class="call critical">${ic("key")}<div>${t("err.callout", { b: `<b>${t("err.replace")}</b>` })}</div><button class="btn primary sm act">${t("btn.updatekey")}</button></div>
    ${lines([
      mutedLine("clock", t("err.last", { when: when(22, 18, 20), d: "5 min" })),
      mutedLine("info", t("err.noreading")),
      access("official", "anthropic_api"),
    ])}
    <div class="acts"><button class="btn ghost sm">${t("card.details")}</button><button class="btn ghost sm">${ic("refresh", "s14")} ${t("tryagain")}</button></div>`;
  out.push({
    name: "07-tray-error", w: 380, h: 600,
    html: shell("07 tray error", pop({ header: trayHeader({ tabs: TABS, selected: "anthropic" }), footer: trayFooter(), body: row({ ...A.anthropic(), open: errBody }) })),
  });

  // 07b contract change: diagnostic block + report entry (PRD 0028)
  const schemaBody = `
    <div class="call critical">${ic("alert")}<div>${t("sc.callout", { b: `<b>${t("sc.report")}</b>` })}</div></div>
    <div class="call neutral">${ic("info")}<div><b style="font-weight:650">${t("sc.what")}</b><div style="margin-top:4px">${t("sc.sentence")}</div>
      <div class="mono sm" style="margin-top:6px;display:flex;flex-direction:column;gap:2px"><span>five_hour.utilization</span><span>weekly.resets_at</span><span>weekly.limit</span></div>
      <div class="sm muted" style="margin-top:6px">${t("sc.never")}</div></div></div>
    ${lines([access("official", "kimi"), `<div class="muted ln">${ic("check", "s14")}<span class="txt"><span class="chip plain" title="">${t("ver.chip", { n: 2 })}</span></span></div>`])}
    <div class="acts"><button class="btn secondary sm" style="margin-left:var(--space-2)">${ic("ext", "s14")} ${t("sc.report")}</button><button class="btn ghost sm">${ic("refresh", "s14")} ${t("tryagain")}</button></div>`;
  out.push({
    name: "07b-tray-error-schema", w: 380, h: 600,
    html: shell("07b tray schema error", pop({
      header: trayHeader({ tabs: TABS.map((x) => x.p === "kimi" ? { ...x, st: { kind: "icon", icon: "alert", role: "bad" } } : x), selected: "kimi" }), footer: trayFooter(),
      body: row({ ...A.kimi(), fig: t("st.error"), unit: "", figRole: "r-bad", meter: null, status: { ring: stateIcon("alert", "bad"), role: "bad", label: t("sc.label"), det: t("sc.det") }, open: schemaBody }),
    })),
  });

  out.push({ name: "09-tray-overview-light", w: 380, h: 600, html: shell("09 tray overview light", pop({ ...all(), body: rows() + tl }), "light") });
  return out;
}
