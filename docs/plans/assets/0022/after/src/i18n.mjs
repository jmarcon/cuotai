// Locale layer of the mockup generator (PRD 0022 i18n validation).
// English (en-US) is the unsuffixed set and uses the wording written in the mockups.
// For pt-BR, es and it a string comes, in this order, from:
//   1. the REAL catalogs: apps/desktop/ui/locales/<loc>.json ("ui:key"),
//   2. the core catalogs: crates/cuotai-core/locales/<loc>.json ("core:key"); the core has no pt-BR file because its
//      Portuguese lives in Rust fallbacks, which are copied here from the sources and tagged "rust:key",
//   3. a DRAFT translation written for this document (the feature or wording is new).
// `usage` records which of the three each rendered string used, for the report in AFTER.md.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { here } from "./lib-paths.mjs";

const read = (p) => JSON.parse(readFileSync(join(here, "../../../../../..", p), "utf8"));
export const LOCS = ["en-US", "pt-BR", "es", "it"];
const IDX = { "en-US": 0, "pt-BR": 1, es: 2, it: 3 };
const UI = Object.fromEntries(LOCS.map((l) => [l, read(`apps/desktop/ui/locales/${l}.json`)]));
const CORE = Object.fromEntries(["en-US", "es", "it"].map((l) => [l, read(`crates/cuotai-core/locales/${l}.json`)]));
// Portuguese core fallbacks, copied from crates/cuotai-core/src/{pace,access}.rs and incidents/status.rs
const RUST_PT = {
  "pace.on_pace": "No ritmo",
  "pace.ahead": "{delta} pontos acima do ritmo",
  "pace.reserve": "{delta} pontos de reserva",
  "pace.lasts_until_reset": "dura até a renovação",
  "pace.runs_out_at": "acaba às {at}, {margin} antes da renovação",
  "incident.state.partial_outage": "Interrupção parcial",
  "incident.state.major_outage": "Interrupção grave",
  "incident.state.degraded": "Desempenho degradado",
  "incident.state.maintenance": "Em manutenção",
  "note.incident.ongoing": "O provedor informa um incidente em andamento; a falha pode não ser da sua conta.",
  "access.level.official": "Oficial",
  "access.level.undocumented": "Não documentada",
  "access.summary.claude": "Janelas de uso do plano Claude pela interface interna usada pelos clientes oficiais.",
  "access.summary.chat_gpt": "Janelas de limite do Codex pela interface interna do Codex CLI.",
  "access.summary.cursor_personal": "Resumo da assinatura pessoal do Cursor pela interface interna do site.",
  "access.summary.kimi": "Saldo da plataforma internacional do Kimi pelo endpoint de saldo documentado.",
  "access.summary.groq": "Rate limits da organização lidos nos cabeçalhos x-ratelimit-* da listagem de modelos.",
  "access.summary.anthropic_api": "Custos e uso da organização pela Usage and Cost Admin API da Anthropic.",
};

export const cur = { loc: "en-US" };
export const setLoc = (l) => { cur.loc = l; };
export const usage = new Map(); // loc -> Map(id -> {src, key})

const S = {};
/** def(id, [en, pt, es, it], real?) ; real = "ui:key" | "core:key" (pt-BR from Rust) | undefined (draft) */
export const def = (id, tr, real) => { S[id] = { tr, real }; };

const interp = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (v && v[k] != null ? v[k] : m));
const note = (id, src, key) => {
  if (!usage.has(cur.loc)) usage.set(cur.loc, new Map());
  usage.get(cur.loc).set(id, { src, key });
};
export function t(id, vars) {
  const e = S[id];
  if (!e) throw new Error(`missing string id: ${id}`);
  const loc = cur.loc;
  if (loc === "en-US") return interp(e.tr[0], vars);
  if (e.real) {
    const [kind, key] = e.real.split(":");
    let v;
    if (kind === "ui") v = UI[loc][key];
    else if (loc === "pt-BR") v = RUST_PT[key];
    else v = CORE[loc][key];
    if (v && typeof v === "object") v = v[new Intl.PluralRules(loc).select(vars?.count ?? 1)] ?? v.other; // catalog plural forms
    if (typeof v === "string") { note(id, kind === "ui" ? "catalog" : loc === "pt-BR" ? "rust" : "core", key); return interp(v, vars); }
  }
  const v = e.tr[IDX[loc]];
  if (v == null) throw new Error(`no ${loc} text for ${id}`);
  note(id, "draft", "");
  return interp(v, vars);
}
/** Real catalog string for a UI key regardless of the id table (used for data such as endonyms). */
export const ui = (key, vars) => { const v = UI[cur.loc][key] ?? UI["en-US"][key]; return interp(v, vars); };

// ---------------- dates, ages, money (what the app does with Intl) ----------------
const TODAY = 22;
const DT = (d, h, m) => new Date(2026, 9, d, h, m);
export const timeOf = (d, h, m) => DT(d, h, m).toLocaleTimeString(cur.loc, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
export const wd = (d) => DT(d, 12, 0).toLocaleDateString(cur.loc, { weekday: "short" });
/** DS-MSG-8: today -> time; within 6 days -> weekday + time; otherwise day, month + time. */
export function when(d, h, m) {
  const time = timeOf(d, h, m);
  if (d === TODAY) return time;
  if (d - TODAY <= 6) return `${wd(d)} ${time}`;
  const dm = cur.loc === "en-US" ? `${d} Oct` : DT(d, h, m).toLocaleDateString(cur.loc, { day: "numeric", month: "short" });
  return `${dm}, ${time}`;
}
export const ago = (min) => (cur.loc === "en-US" ? `${min} min ago` : new Intl.RelativeTimeFormat(cur.loc, { style: "short" }).format(-min, "minute"));
export const usd = (n) => (cur.loc === "en-US" ? `$${n.toFixed(2)}` : new Intl.NumberFormat(cur.loc, { style: "currency", currency: "USD" }).format(n));
export const num = (n) => new Intl.NumberFormat(cur.loc).format(n);
