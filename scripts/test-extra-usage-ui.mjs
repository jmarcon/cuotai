#!/usr/bin/env node
// Browser-free UI smoke for PRD 0023 RF7: paid extra usage (overage) on the card chip, the card column, the status
// line, the tray row chip and the details, in all 14 built-in locales with their core catalogs, for an active,
// capped, unknown, armed, disabled and provider-unit state. The core owns the texts (`overage.*`) and sends amounts
// as `amount.*` notes with machine params; the UI formats every number in its own locale (PRD 0023 re-validation
// V-01, V-02, V-03, V-05, V-06, V-07, V-09).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const read = async (relative) => JSON.parse(await readFile(fileURLToPath(new URL(relative, root)), "utf8"));
const storage = new Map();
globalThis.window = {
  __TAURI__: {
    core: {
      // en-US uses the English fallback of each core text; every other language has its core catalog.
      invoke: async (_command, { locale }) => (locale && locale !== "en-US" ? read(`crates/cuotai-core/locales/${locale}.json`).catch(() => ({})) : {}),
    },
  },
};
globalThis.document = { hidden: false, documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} }, addEventListener: () => {} };
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) };
globalThis.fetch = async (relative) => {
  try {
    return { ok: true, json: async () => read(`apps/desktop/ui/${relative}`) };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { extraColumnHtml, statusParts } = await import("../apps/desktop/ui/js/components.js");
const { detailHtml, metricCardHtml } = await import("../apps/desktop/ui/js/detail.js");
await i18n.initI18n();

const text = (id, fallback, params = {}) => ({ id, params, fallback });
const money = (value) => text("amount.money", `$${value.toFixed(2)}`, { value: String(value), currency: "USD" });
const units = (value) => text("overage.amount.provider_units", `${value.toLocaleString("en-US")} provider units`,
  { value: text("amount.number", value.toLocaleString("en-US"), { value: String(value) }) });
const reset = new Date(Date.now() + 5 * 3600e3).toISOString();
const meter = (used) => ({ label: "weekly", used, limit: 100, remaining: 100 - used, unit: "%", kind: "quota", window: "weekly", period: null,
  resets_at: reset, state: used >= 100 ? "exhausted" : "available", ratio: (100 - used) / 100, uncapped: false, no_cap: false, pace: null });
const extraMetric = (over) => ({
  label: "Extra usage", state: "available", uncapped: true, no_cap: false, ratio: null, used: 6.25, limit: 50, remaining: null,
  unit: "USD", kind: "spend", window: "monthly", resets_at: null, pace: null, billing: "overage", ...over,
});
/** A ready report; `exhausted` makes the plan limit the binding, exhausted one (the account is blocked). */
const report = (overage, extra = extraMetric({}), { exhausted = true } = {}) => {
  const metrics = [meter(exhausted ? 100 : 60), ...(extra ? [extra] : [])];
  return {
    provider: "claude", account: "Work", status: "ready", class: exhausted ? "exhausted" : "available", metrics, overage, notes: [], notes_l10n: [],
    binding: { index: 0, used_ratio: exhausted ? 1 : 0.6, window: "weekly", label: "weekly", reason: exhausted ? "exhausted" : "tightest", blocked_until: exhausted ? reset : null },
    usage_alerts: [], checked_at: new Date().toISOString(), freshness: "fresh", clock_adjusted: false, last_good: null, pending: false,
    access: null, incident: null, session_expiry: null, identity: { email: null, badges: [], copy: null },
  };
};
const cycle = "2026-11-01T00:00:00Z";
const usd = { unit: "USD", currency: "USD", cycle_ends_at: cycle };
const cap = text("overage.cap_reached", "Extra budget used up");
const cases = {
  active: report({
    state: "active", spent: 6.25, cap: 50, ...usd,
    text: text("overage.on_extra", "On extra · $6.25", { amount: money(6.25) }),
    summary: text("overage.limit_hit_on_extra", "Limit hit · on extra $6.25", { amount: money(6.25) }), caveat: null,
  }),
  // The core sends the cap text as the summary too (PRD 0023 re-validation V-03).
  capped: report({ state: "cap_reached", spent: 50, cap: 50, ...usd, text: cap, summary: cap, caveat: null }, extraMetric({ used: 50 })),
  cappedAvailable: report({ state: "cap_reached", spent: 50, cap: 50, ...usd, text: cap, summary: cap, caveat: null }, extraMetric({ used: 50 }), { exhausted: false }),
  unknown: report({
    state: "unknown", spent: null, cap: 50, unit: "USD", currency: "USD", cycle_ends_at: null,
    text: text("overage.unknown", "Extra usage amount unknown"), summary: text("overage.unknown", "Extra usage amount unknown"), caveat: null,
  }, extraMetric({ used: null }), { exhausted: false }),
  armed: report({
    state: "armed", spent: 0, cap: 50, ...usd,
    text: text("overage.armed", "Extra usage on · nothing spent"), summary: text("overage.armed", "Extra usage on · nothing spent"), caveat: null,
  }, extraMetric({ used: 0 }), { exhausted: false }),
  disabled: report({
    state: "disabled", spent: null, cap: null, unit: null, currency: null, cycle_ends_at: null,
    text: text("overage.disabled", "Extra usage off"), summary: text("overage.disabled", "Extra usage off"), caveat: null,
  }, null, { exhausted: false }),
  units: report({
    state: "active", spent: 1234.5, cap: 5000, unit: "unid. provedor", currency: null, cycle_ends_at: cycle,
    text: text("overage.on_extra", "On extra · 1,234.5 provider units", { amount: units(1234.5) }),
    summary: text("overage.limit_hit_on_extra", "Limit hit · on extra 1,234.5 provider units", { amount: units(1234.5) }),
    caveat: text("overage.units_caveat", "Amounts are in provider units, not money."),
  }, extraMetric({ unit: "unid. provedor", kind: "quota", used: 1234.5, limit: 5000 })),
};

const flat = (value) => value.replace(/[  ]/g, " ");
// `&amp;` last, so an escaped entity such as `&amp;#39;` is not decoded twice.
const decode = (html) => flat(html).replaceAll("&#39;", "'").replaceAll("&quot;", '"').replaceAll("&amp;", "&");
const withoutTitles = (html) => html.replace(/ (title|aria-label|aria-valuetext)="[^"]*"/g, "");
const count = (haystack, needle) => haystack.split(needle).length - 1;
const coreText = async (code, id, fallback) => (code === "en-US" ? fallback : flat((await read(`crates/cuotai-core/locales/${code}.json`))[id] ?? fallback));

assert.equal(i18n.LOCALES.length, 14, "every built-in language is checked");
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const usdOf = (value) => flat(new Intl.NumberFormat(code, { style: "currency", currency: "USD" }).format(value));
  const num = (value) => flat(new Intl.NumberFormat(code, { maximumFractionDigits: 2 }).format(value));
  const capText = await coreText(code, "overage.cap_reached", "Extra budget used up");
  const armedText = await coreText(code, "overage.armed", "Extra usage on · nothing spent");
  const unknownText = await coreText(code, "overage.unknown", "Extra usage amount unknown");
  const unitsWord = await coreText(code, "metric.unit.provider_units", "provider units");

  // V-02: the core amount is formatted in the UI locale, like every other figure; never the en-US fallback.
  assert.equal(flat(i18n.tl(money(6.25))), usdOf(6.25), `${code}: amount.money`);
  assert.equal(flat(i18n.tl(text("amount.number", "1,234.5", { value: "1234.5" }))), num(1234.5), `${code}: amount.number`);
  assert.equal(flat(i18n.tl(text("amount.money", "$6.25", { value: "six", currency: "USD" }))), "$6.25", `${code}: malformed value keeps the fallback`);

  // Active: the warning chip carries the core summary with the amount in the locale's format.
  const active = decode(lib.overageChipHtml(cases.active));
  assert.ok(active.includes('class="mini warn"') && active.includes("data-overage-chip"), `${code}: active chip ${active}`);
  assert.ok(active.includes(usdOf(6.25)), `${code}: active chip amount ${active}`);

  // V-03: capped is critical and says the extra budget is used up, never "on extra".
  const capped = decode(lib.overageChipHtml(cases.cappedAvailable));
  assert.ok(capped.includes('class="mini bad"'), `${code}: capped chip is critical ${capped}`);
  assert.ok(capped.includes(`>${capText}<`), `${code}: capped chip text ${capped}`);
  const stlCapped = statusParts(cases.capped);
  assert.equal(stlCapped.role, "bad", `${code}: capped status line role`);
  assert.equal(flat(stlCapped.detail), capText, `${code}: capped status line detail`);
  assert.notEqual(stlCapped.detail, i18n.t("stl.onExtra"), `${code}: capped is not on extra`);
  assert.equal(statusParts(cases.active).detail, i18n.t("stl.onExtra"), `${code}: active status line is on extra`);

  // V-05: armed is a muted chip, unknown a muted dashed chip; disabled has none.
  const armed = decode(lib.overageChipHtml(cases.armed));
  assert.ok(armed.includes('class="mini muted"') && armed.includes(`>${armedText}<`), `${code}: armed chip ${armed}`);
  const unknown = decode(lib.overageChipHtml(cases.unknown));
  assert.ok(unknown.includes('class="mini muted unknown"') && unknown.includes(`>${unknownText}<`), `${code}: unknown chip ${unknown}`);
  assert.ok(!unknown.includes("$0") && !unknown.includes(usdOf(0)), `${code}: unknown is never 0`);
  assert.equal(lib.overageChipHtml(cases.disabled), "", `${code}: disabled has no chip`);

  // The card column (mockup 10): V-01 figure and unit, V-06 named meter, V-07 the cap under it, not the amount again.
  const column = decode(extraColumnHtml(cases.active));
  assert.ok(column.includes(`<b class="warn">${usdOf(6.25)}</b><small>${flat(i18n.t("figure.extra"))}</small>`), `${code}: active figure ${column}`);
  assert.match(column, /class="mtr warn" role="meter"/, `${code}: the extra budget is a real meter`);
  assert.ok(column.includes(`aria-label="${flat(i18n.t("card.extra.title"))}"`), `${code}: the meter is named`);
  assert.ok(column.includes(`aria-valuetext="${flat(i18n.t("card.extra.meterLeft", { pct: 88 }))}"`), `${code}: the meter says what is left`);
  assert.equal(count(withoutTitles(column), usdOf(6.25)), 1, `${code}: the amount shows once ${column}`);
  assert.ok(withoutTitles(column).includes(usdOf(50)), `${code}: the cap is under the figure`);
  const cappedColumn = decode(extraColumnHtml(cases.capped));
  assert.match(cappedColumn, /<b class="bad">/, `${code}: capped figure is critical`);
  assert.match(cappedColumn, /class="mtr bad" role="meter"/, `${code}: capped meter is critical`);
  const unitsColumn = decode(extraColumnHtml(cases.units));
  assert.ok(unitsColumn.includes(`<b class="warn">${num(1234.5)}</b><small>${unitsWord}</small>`), `${code}: provider units figure ${unitsColumn}`);
  assert.ok(!unitsColumn.includes("$"), `${code}: provider units never show a dollar sign`);
  assert.equal(extraColumnHtml(cases.armed), "", `${code}: no column unless money is billing`);

  // Details: a section for every state, with the cap and the cycle end; the amount once in the section (V-07).
  const detail = decode(detailHtml(cases.active));
  const section = decode(lib.overageDetailHtml(cases.active));
  assert.ok(detail.includes("data-overage-detail") && detail.includes(`<h3>${flat(i18n.t("card.extra.title"))}</h3>`), `${code}: details section`);
  assert.equal(count(section, usdOf(6.25)), 1, `${code}: details section repeats no amount ${section}`);
  assert.ok(decode(detailHtml(cases.disabled)).includes(await coreText(code, "overage.disabled", "Extra usage off")), `${code}: disabled details text`);
  assert.ok(decode(detailHtml(cases.capped)).includes(capText), `${code}: capped details text`);
  const unknownDetail = decode(detailHtml(cases.unknown));
  assert.ok(unknownDetail.includes(">—<"), `${code}: unknown amount is a dash`);
  assert.ok(!unknownDetail.includes("$0"), `${code}: unknown amount is never $0`);

  // The overage metric is a textual figure in the list, never a meter that implies "left".
  const metricHtml = decode(metricCardHtml(cases.active, cases.active.metrics[1]));
  assert.ok(metricHtml.includes(usdOf(6.25)), `${code}: metric figure ${metricHtml}`);
  assert.doesNotMatch(metricHtml, /class="mtr|role="meter"/, `${code}: overage metric has no meter`);
  assert.ok(!metricHtml.includes('class="pct'), `${code}: overage metric has no left%`);

  // Claude keeps provider units and says so: no $ sign anywhere.
  const unitsAll = decode(detailHtml(cases.units) + lib.overageChipHtml(cases.units));
  assert.ok(unitsAll.includes(await coreText(code, "overage.units_caveat", "Amounts are in provider units, not money.")), `${code}: units caveat`);
  assert.ok(!unitsAll.includes("$"), `${code}: provider units never show a dollar sign`);

  // PRD 0034 re-validation (G13): an adapter unit (`créditos`, an id) reads as the overage metric's `unit_text` on the
  // card column, the line, the details and the cap; the core's chip text carries its own nested unit text.
  const creditsWord = await coreText(code, "metric.unit.credits", "credits");
  const creditsText = text("metric.unit.credits", "credits");
  const creditsAmount = (value) => text("amount.units", `${value} credits`, { value: String(value), unit: creditsText });
  const credits = report({ state: "active", spent: 12, unit: "créditos", currency: null, cap: 40, cycle_ends_at: null,
    text: text("overage.on_extra", "On extra · 12 credits", { amount: creditsAmount(12) }),
    summary: text("overage.limit_hit_on_extra", "Limit hit · on extra 12 credits", { amount: creditsAmount(12) }), caveat: null },
  extraMetric({ unit: "créditos", unit_text: creditsText, kind: "quota", used: 12, limit: 40 }));
  const creditsAll = decode(extraColumnHtml(credits) + lib.overageChipHtml(credits) + detailHtml(credits) + lib.overageLineHtml(credits));
  assert.ok(creditsAll.includes(`${num(12)} ${flat(creditsWord)}`) && creditsAll.includes(`${num(40)} ${flat(creditsWord)}`), `${code}: credits read in the language`);
  if (flat(creditsWord) !== "créditos") assert.ok(!creditsAll.includes("créditos"), `${code}: the raw unit never shows`);

  // Overage never decides the card summary or the sort order.
  assert.equal(lib.worstRemaining(cases.unknown), 40, `${code}: worst remaining ignores overage`);
  console.log(`${code}: ${flat(lib.overageLineText(cases.active.overage))}; ${capText}`);
}

assert.equal(lib.summaryOf(report(null, extraMetric({}), { exhausted: false })).kind, "bar", "the plan meter stays the summary");
console.log("PASS: extra usage renders chips, column, status line and details in all 14 languages; overage never becomes a plan meter");
