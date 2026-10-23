// Fixture data (example.com only). "Now" is Thu 22 Oct 2026, 18:21 local. All visible text goes through t().
import { ic, meter, row, stateIcon, stateRing, sub, tchip } from "./lib.mjs";
import { ago, t, usd, when } from "./i18n.mjs";

const dotMark = (role) => `<i class="dot r-${role}"></i>`;
export const resetsIn = (w, d) => `${t("resetsAt", { when: w })} · ${t("in", { d })}`;

export const incidents = () => ({
  claude1: { role: "bad", icon: "alert", label: t("inc.partial"), det: "Elevated errors on claude.ai" },
  claude2: { role: "bad", icon: "alert", label: t("inc.partial"), det: "Elevated errors on claude.ai" },
  cursor: { role: "bad", icon: "alert", label: t("inc.major"), det: "Chat and completions down" },
  groq: { role: "warn", icon: "alert", label: t("inc.degraded"), det: "Slow responses in eu-west" },
  kimi: { role: "info", icon: "wrench", label: t("inc.maintenance"), det: "23:00 – 23:30" },
});

// Each builder returns the collapsed-row options; `inc` adds the provider incident line (DS-CMP-ROW-4).
export const A = {
  claude1: (o = {}) => ({
    p: "claude", name: "work@example.com", mono: "WO", provider: "Claude Code", plan: "Max", acct: "", fig: "22%", unit: t("left"), figRole: "",
    meter: meter(22, { pace: 42, label: "22%" }),
    status: { ring: stateRing("pct", 22), role: "warn", label: t("st.ahead"), labelC: t("lblc.ahead", { d: 20 }), chips: [tchip("runs_out", { at: when(24, 10, 20) }, { compact: true })], ret: tchip("returns", { at: when(25, 17, 20) }, { compact: true, plain: true }) },
    extra: o.inc ? incidents().claude1 : null, ...o,
  }),
  claude2: (o = {}) => ({
    p: "claude", name: "side@example.com", mono: "SI", provider: "Claude Code", plan: "Pro", acct: "", fig: "71%", unit: t("left"),
    meter: meter(71, { pace: 70, label: "71%" }),
    status: { ring: stateRing("pct", 71), role: "good", label: t("pace.on_pace"), ret: tchip("returns", { at: when(26, 9, 0) }, { compact: true, plain: true }) },
    extra: o.inc ? incidents().claude2 : null, ...o,
  }),
  codex: (o = {}) => ({
    p: "codex", name: "OpenAI Codex", plan: "Plus", acct: "me@example.com", fig: "63%", unit: t("left"),
    meter: meter(63, { pace: 55, label: "63%" }),
    status: { ring: stateRing("pct", 63), role: "good", label: t("st.reserve"), labelC: t("lblc.reserve", { d: 8 }), ret: tchip("returns", { at: when(27, 18, 20) }, { compact: true, plain: true }) }, ...o,
  }),
  cursor: (o = {}) => ({
    p: "cursor", name: "Cursor", plan: "Pro", acct: "dev@example.com", fig: usd(6.25), unit: t("extra"), figRole: "r-warn",
    meter: meter(0, { role: "bad", label: t("metric.exhausted") }),
    status: { ring: stateRing("exhausted"), role: "bad", label: t("st.limithit"), det: t("st.onextra"), ret: tchip("returns", { at: when(31, 2, 20) }, { compact: true, plain: true }) },
    extra: o.inc ? incidents().cursor : null, ...o,
  }),
  kimi: (o = {}) => ({
    p: "kimi", name: "Kimi", acct: "ops@example.com", fig: "59%", unit: t("left"),
    meter: meter(59, { pace: 56, label: "59%" }),
    status: { ring: stateRing("pct", 59), role: "muted", label: t("pace.on_pace"), ret: tchip("returns", { at: when(27, 7, 20) }, { compact: true, plain: true }) },
    extra: o.inc ? incidents().kimi : null, ...o,
  }),
  groq: (o = {}) => ({
    p: "groq", name: "Groq", acct: "team@example.com", fig: "74%", unit: t("left"), figDim: true,
    meter: meter(74, { dim: true, label: "74%" }),
    status: { ring: stateRing("stale", 74), role: "warn", label: `${t("stale")} · ${ago(42)}`, det: t("st.timeout") },
    extra: o.inc ? incidents().groq : null, ...o,
  }),
  anthropic: (o = {}) => ({
    p: "anthropic", name: "Anthropic API", acct: "api@example.com", fig: t("st.auth"), unit: "", figRole: "r-accent",
    meter: null,
    status: { ring: stateIcon("key", "accent"), role: "accent", label: t("st.keyrej"), det: t("st.replace") }, ...o,
  }),
};

export const ORDER = ["claude1", "claude2", "codex", "cursor", "kimi", "groq", "anthropic"];
export const rows = (o = {}) => ORDER.map((k) => row(A[k](o[k] ?? o.all ?? {}))).join("\n");

// sub-limit set used by expanded rows; pct = "show pace as percent" (DS-CMP-TIME-6)
export const subsClaude = (pct = false) => [
  sub({ label: t("sub.session"), fig: "66%", meterHtml: meter(66, { pace: 44, label: "66%" }), pace: pct ? t("pace.reserve_pct", { d: 39 }) : t("pace.reserve", { delta: 22 }), paceRole: "good", paceIcon: "check", reset: resetsIn(when(22, 20, 32), "2h 11m") }),
  sub({ label: t("sub.weekly"), binding: true, fig: "22%", meterHtml: meter(22, { pace: 42, label: "22%" }), pace: pct ? t("pace.ahead_pct", { d: 34 }) : t("pace.ahead", { delta: 20 }), paceRole: "warn", paceIcon: "warn", reset: resetsIn(when(25, 17, 20), "2d 23h") }),
].join("");

// ---- row display presets and per-element toggles (Settings > Dashboard > Row display) ----
export const TOGGLES = ["plan", "account", "mono", "unit", "corridor", "label", "ring", "runsout", "returns", "age", "incident", "extrausage", "sublimits"];
export const PRESETS = {
  detailed: Object.fromEntries(TOGGLES.map((k) => [k, true])),
  standard: { plan: true, account: true, mono: true, unit: true, corridor: true, label: true, ring: true, runsout: true, returns: true, age: false, incident: true, extrausage: true, sublimits: false },
  minimal: { plan: false, account: false, mono: true, one: true, unit: false, corridor: false, label: true, ring: true, runsout: false, returns: false, age: false, incident: false, extrausage: false, sublimits: false },
  custom: { plan: true, account: true, mono: true, unit: true, corridor: true, label: true, ring: true, runsout: false, returns: true, age: false, incident: false, extrausage: true, sublimits: true },
};
const SUBLIM = { claude1: () => t("dsp.sub", { a: "66%", b: "22%" }), claude2: () => t("dsp.sub", { a: "94%", b: "71%" }), codex: () => t("dsp.sub", { a: "88%", b: "63%" }) };
/** Apply a display configuration to the options of one collapsed row. `key` = fixture key. */
export function display(key, cfg, o = {}) {
  const r = A[key]({ ...o, inc: cfg.incident });
  if (!cfg.plan) delete r.plan;
  if (!cfg.mono) delete r.mono;
  if (!cfg.account && r.acct) r.acct = "";
  if (!cfg.unit) r.hideUnit = true;
  if (!cfg.corridor && r.meter) r.meter = r.meter.replace(/<span class="pm"[^>]*><\/span>/, "");
  if (r.status && cfg.one) { // Minimal: ONE status chosen by priority incident > stale > exhausted (+ return) > runs out > pace stage
    const s = r.status;
    const chips = s.chips ?? [];
    r.status = { role: s.role, label: s.label, ring: "" };
    if (key === "claude1") r.status = { role: "warn", label: "", chips: [tchip("runs_out", { at: when(24, 10, 20) })] };
    if (key === "cursor") r.status.ret = s.ret;
    r.status.label ||= "";
    void chips;
  } else if (r.status) {
    if (!cfg.label) r.status.label = "";
    if (!cfg.ring) r.status.ring = "";
    if (!cfg.runsout) r.status.chips = [];
    if (!cfg.returns) r.status.ret = "";
    if (cfg.age) r.status.det = [r.status.det, ago(1)].filter(Boolean).join(" · ");
  }
  if (cfg.sublimits && SUBLIM[key]) r.sublim = SUBLIM[key]();
  return r;
}
export const displayRows = (cfg) => ORDER.map((k) => row(display(k, cfg))).join("\n");
