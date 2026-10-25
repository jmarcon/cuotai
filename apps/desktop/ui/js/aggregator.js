// PRD 0036 RF8: aggregator accounts (9router) and their connections. The backend sends each aggregator report with
// `aggregator` (instance summary and the core's pool texts) and `connections` (one report view per connection, keyed by
// its own `account_key`, with `index` pointing at the aggregator so every action acts on it). Nothing here classifies
// or counts on its own: figures and pool sentences come from the core.
import { t, te, tl } from "./i18n.js";
import { bindingPct, brand, esc, icon, level, logo } from "./lib.js";

export const isAggregator = (r) => !!r?.aggregator;
export const isConnection = (r) => !!r?.connection;

/** A connection as the cards draw it: an upstream Cuotai does not know keeps the aggregator's slug as its name and
 *  a neutral tile (its own slug, which has no brand), never another provider's look. */
const shown = (c) => (c.connection.mapped ? c : { ...c, provider: c.connection.upstream });

/** The connections of aggregator `r`, ready to draw. */
export const connectionsOf = (r) => (r?.connections ?? []).map(shown);

/** Every report and, after each aggregator, its connections: what key lookups (details, tray) search. */
export const withConnections = (reports) => reports.flatMap((r) => [r, ...connectionsOf(r)]);

/** On in the aggregator and not a duplicate of a direct account: takes part in pools, rankings and "All". */
export const activeConnection = (c) => c.connection.enabled_upstream !== false && !c.connection.duplicate_of;

/** Display name of the connection's upstream: the mapped provider's brand or the aggregator's slug verbatim. */
export const upstreamName = (c) => (c.connection.mapped ? brand(c.connection.mapped).label : c.connection.upstream);

/** The connections grouped by upstream (the core's order) with each group's pool, when it has one. */
export function groupsOf(r) {
  const groups = [];
  for (const c of connectionsOf(r)) {
    let g = groups.find((x) => x.upstream === c.connection.upstream);
    if (!g) {
      g = { upstream: c.connection.upstream, name: upstreamName(c), pool: r.aggregator.pools.find((p) => p.upstream === c.connection.upstream) ?? null, items: [] };
      groups.push(g);
    }
    g.items.push(c);
  }
  return groups;
}

/** "5/7": active connections with quota left, of all active ones (the core's pool counts). */
export const poolFigure = (r) => `${r.aggregator.available}/${r.aggregator.total}`;

/** "7 connections · 9router 0.5.109". */
export const instanceHint = (r) => [t("agg.connections", { count: r.aggregator.connections }),
  r.aggregator.version ? t("agg.version", { version: r.aggregator.version }) : ""].filter(Boolean).join(" · ");

/** The first pool whose every connection is exhausted, if any (its core sentence carries the return time). */
export const exhaustedPool = (r) => r.aggregator?.pools.find((p) => p.total > 0 && p.exhausted === p.total) ?? null;

/** The active connection with the least room left on its binding metric, among those not used up (an exhausted
 *  pool has a line of its own): the DS-CMP-ROW status line. */
export function tightest(r) {
  let best = null;
  for (const c of connectionsOf(r).filter(activeConnection)) {
    const pct = bindingPct(c);
    if (pct != null && pct > 0 && (best == null || pct < best.pct)) best = { c, pct };
  }
  return best;
}

/** The status line of the grouped row: the exhausted pool, or the tightest connection, or nothing measured. */
export function statusLine(r) {
  const out = exhaustedPool(r);
  if (out) return { tone: "bad", text: tl(out.summary) };
  const tight = tightest(r);
  if (tight) return { tone: level(tight.pct), text: t("agg.tightest", { name: `${upstreamName(tight.c)} · ${tight.c.connection.label}`, pct: Math.round(tight.pct) }) };
  return { tone: "unknown", text: t("agg.none") };
}

/** Accessible name of the grouped row and the 9router view: "9router, 7 connections, Claude: all exhausted". */
export const accessibleName = (r) => [brand(r.provider).label, r.account, t("agg.connections", { count: r.aggregator.connections }),
  ...r.aggregator.pools.map((p) => tl(p.summary))].join(", ");

/** Chips of a connection (text and icon, never color alone): off in the aggregator, also monitored directly. */
export function connectionChips(c, { via = false } = {}) {
  const chips = [];
  if (c.connection.enabled_upstream === false) chips.push(`<span class="mini muted" data-conn-off>${icon("ban")}<span class="mini-t">${te("agg.off")}</span></span>`);
  if (c.connection.duplicate_of) chips.push(`<span class="mini muted" data-conn-duplicate>${icon("copy")}<span class="mini-t">${te("agg.duplicate")}</span></span>`);
  if (via) chips.push(`<span class="mini" data-conn-via>${te("agg.via")}</span>`);
  return chips.join("");
}

/** One pool line ("Claude · 2 of 3 available") for lists and the details. */
export const poolLineHtml = (p) => {
  const out = p.total > 0 && p.exhausted === p.total;
  return `<p class="agg-pool ${out ? "bad" : ""}" data-pool="${esc(p.upstream)}">${icon(out ? "battery" : p.available ? "check" : "clock")}<span>${esc(tl(p.summary))}</span></p>`;
};

/** PRD 0036 RF8.5: what the details add for a connection: its upstream, mapped provider, plan, how it is read and
 *  the duplicate state. Recovery is limited to the aggregator's own dashboard. */
export function connectionInfoHtml(c, parentIndex) {
  const k = c.connection;
  const rows = [
    [te("agg.upstream"), esc(k.upstream)],
    [te("agg.mapped"), k.mapped ? esc(brand(k.mapped).label) : "—"],
    ...(k.plan ? [[te("agg.plan"), `<code>${esc(k.plan)}</code>`]] : []),
    [te("agg.source"), te("agg.via")],
  ];
  return `<div class="agg-info" data-connection-info>
    <dl>${rows.map(([dt, dd]) => `<dt>${dt}</dt><dd>${dd}</dd>`).join("")}</dl>
    ${connectionChips(c) ? `<p class="agg-chips">${connectionChips(c)}</p>` : ""}
    <p class="hint">${te("agg.caveat")}</p>
    <div class="dlg-actions left"><button type="button" class="link" data-agg-dashboard="${parentIndex}">${icon("window")}<span>${te("agg.openDashboard")}</span></button></div>
  </div>`;
}

/** PRD 0036 RF8.5: the instance's details: address (host only), version, connections, pools and the alert mode. */
export function instanceInfoHtml(r) {
  const a = r.aggregator;
  const rows = [
    ...(a.host ? [[te("agg.host"), `<code>${esc(a.host)}</code>`]] : []),
    ...(a.version ? [[te("agg.versionLabel"), `<code>${esc(a.version)}</code>`]] : []),
    [te("agg.connectionsLabel"), esc(String(a.connections))],
  ];
  return `<div class="agg-info" data-instance-info>
    <dl>${rows.map(([dt, dd]) => `<dt>${dt}</dt><dd>${dd}</dd>`).join("")}</dl>
    ${a.pools.length ? `<h3 class="agg-micro">${te("agg.pools")}</h3>${a.pools.map(poolLineHtml).join("")}` : ""}
    <label class="check"><input type="checkbox" data-pool-each="${esc(r.account_key)}" ${a.alert_each ? "checked" : ""} /> ${te("agg.alertEach")}</label>
    <p class="hint">${te("agg.alertEachHelp")}</p>
    ${a.dashboard ? `<div class="dlg-actions left"><button type="button" class="link" data-agg-dashboard="${r.index}">${icon("window")}<span>${te("agg.openDashboard")}</span></button></div>` : ""}
  </div>`;
}

// PRD 0036 RF8.3: how "All" shows aggregators — one grouped row (default), their connections listed with the other
// accounts, or nothing. A per-device display choice, mirrored across windows like the "Recharging" group.
export { AGG_MODES, aggregatorsInAll, onAggregatorModeChange, setAggregatorsInAll } from "./lib.js";

/** Ring of an aggregator tile: the share of active connections with room left. */
export const poolPct = (r) => (r.aggregator.total ? (r.aggregator.available / r.aggregator.total) * 100 : null);

/** The aggregator's header line in its sections: logo, "9ROUTER · HOME" micro label and the hint. */
export const instanceHeadHtml = (r, size = 32) => `${logo(r.provider, size)}
  <div class="who"><h2 class="agg-micro" title="${esc(r.account)}">${esc(brand(r.provider).label)} · ${esc(r.account)}</h2>
  <p class="hint">${esc(instanceHint(r))}</p></div>`;
