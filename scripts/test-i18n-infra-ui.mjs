#!/usr/bin/env node
// Browser-free UI smoke for the PRD 0035 infrastructure: the JS twin of the registry normalization, the search folding,
// the duration patterns and the core plural objects in `tl()`, in the production modules. No browser involved.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "pt-BR" });
const { REGISTRY, normalizeLocale } = await import("../apps/desktop/ui/js/locale-registry.js");
const { fold } = await import("../apps/desktop/ui/js/util.js");
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const { durationText, fmtNext } = await import("../apps/desktop/ui/js/lib.js");

// RF1.3/RF1.4: the same table as `text::registry::tests::normalize_follows_the_alias_table`.
const cases = [
  ["zh", "zh-CN"], ["zh_CN.UTF-8", "zh-CN"], ["zh-Hans-TW", "zh-CN"], ["cmn-Hans-CN", "zh-CN"], ["zh-SG", "zh-CN"],
  ["zh-TW", null], ["zh-HK", null], ["zh-Hant", null], ["zh-Hant-TW", null], ["zh_TW.UTF-8", null],
  ["hi", "hi-IN"], ["hi_IN.UTF-8", "hi-IN"], ["in", "id-ID"], ["in-ID", "id-ID"], ["ms-MY", null],
  ["fr-CA", "fr"], ["de_DE@euro", "de"], ["ja-JP", "ja"], ["ko", "ko"], ["ru-UA", "ru"], ["tr-TR", "tr"], ["vi-VN", "vi"],
  ["pt-PT", "pt-BR"], ["es-419", "es"], ["it-CH", "it"], ["en-GB", "en-US"], ["EN", "en-US"],
  ["est", null], ["ita", null], ["deu", null], ["kor", null], ["", null], ["C", null], ["pt BR; rm", null], ["x-klingon", null],
];
for (const [raw, code] of cases) assert.equal(normalizeLocale(raw)?.code ?? null, code, `normalizeLocale(${JSON.stringify(raw)})`);
assert.equal(REGISTRY.length, 14);
for (const row of REGISTRY) assert.equal(normalizeLocale(row.code)?.code, row.code);

// RF10.5: the twin of `cuotai_core::text::fold`.
assert.equal(fold("Đồng"), "dong");
assert.equal(fold("İstanbul"), "istanbul");
assert.equal(fold("Straße"), "strasse");
assert.equal(fold("Łódź"), "lodz");
assert.equal(fold("Tiếng Việt"), "tieng viet");
assert.equal(fold("한국어"), "한국어", "Hangul stays as typed");
assert.equal(fold("日本語のアカウント"), "日本語のアカウント");

await i18n.initI18n();
assert.equal(i18n.currentLocale(), "pt-BR");
// RF4: a core plural object picks its form from `count` (pt-BR: 0 and 1 are `one`).
const alerts = (count) => ({ id: "alerts.title.accounts", params: { count: String(count) }, fallback: `Cuotai: ${count} usage alerts` });
assert.equal(i18n.tl(alerts(1)), "Cuotai: 1 alerta de uso");
assert.equal(i18n.tl(alerts(3)), "Cuotai: 3 alertas de uso");
// RF10.3: durations come from the catalog patterns.
assert.equal(durationText(37 * 60), "37 min");
assert.equal(durationText(95 * 60), "1h 35m");
assert.equal(durationText(3 * 86400 + 15 * 3600), "3d 15h");
const now = Date.now();
assert.equal(fmtNext(now + 260_000, now), "4m 20s");
assert.equal(fmtNext(now + 3_900_000, now), "1h 05m");
await i18n.setLocale("it");
assert.equal(durationText(3 * 86400 + 15 * 3600), "3g 15h", "Italian days");
assert.equal(i18n.tl(alerts(1)), "Cuotai: 1 avviso di utilizzo");
// A language in progress (none is left once every catalog ships) cannot be chosen.
for (const { code } of i18n.pendingLocales()) {
  await i18n.setLocale(code);
  assert.equal(i18n.currentLocale(), "it", `${code} is still in progress`);
}
await i18n.setLocale("en-US");
assert.equal(i18n.tl(alerts(1)), "Cuotai: 1 usage alerts", "English renders the core's own fallback");
assert.equal(durationText(95 * 60), "1h 35m");
// RF12.3: the shipped example user pack is a partial Dutch pack (a language that is not built in); its keys exist in
// en-US with the same placeholders, it registers as a user language, and missing keys fall back to English.
{
  const { readFileSync } = await import("node:fs");
  const read = (path) => JSON.parse(readFileSync(new URL(path, import.meta.url), "utf8"));
  const pack = read("../apps/desktop/examples/user/locales/nl.json");
  const english = read("../apps/desktop/ui/locales/en-US.json");
  const holes = (v) => JSON.stringify(v).match(/\{[a-zA-Z_]+\}/g)?.sort() ?? [];
  assert.equal(normalizeLocale("nl"), null, "nl is not a built-in language");
  assert.equal(pack._meta.name, "Nederlands");
  for (const [key, value] of Object.entries(pack)) {
    if (key === "_meta") continue;
    assert.ok(key in english, `example pack key ${key} is not an en-US key`);
    assert.deepEqual(holes(value), holes(english[key]), `example pack key ${key} changes the placeholders`);
  }
  const { _meta, ...messages } = pack;
  i18n.registerLocale("nl", _meta.name, messages, _meta.flag);
  await i18n.setLocale("nl");
  assert.equal(i18n.currentLocale(), "nl", "the example pack registers as a user language");
  assert.equal(i18n.t("common.refresh"), "Vernieuwen");
  assert.equal(i18n.t("tray.ok"), english["tray.ok"], "a key the pack lacks falls back to English");
  await i18n.setLocale("en-US");
}
console.log("PASS: registry normalization, folding, durations and core plurals in the desktop modules");

// PRD 0035 RF8.6 (DS-TOK-TYP-2, DS-MSG-7): numbers stay Latin in every registry locale. `Intl` never substitutes
// Devanagari (hi-IN), full-width (ja, zh-CN) or other native digits for the codes the app passes it.
const NATIVE_DIGITS = /[٠-٩۰-۹०-९０-９]/;
for (const { code } of REGISTRY) {
  const number = new Intl.NumberFormat(code);
  assert.equal(number.resolvedOptions().numberingSystem, "latn", `${code} numbers`);
  const samples = [
    number.format(1234567.89),
    new Intl.NumberFormat(code, { style: "percent" }).format(0.42),
    new Intl.NumberFormat(code, { style: "currency", currency: "USD" }).format(12.5),
    new Intl.DateTimeFormat(code, { dateStyle: "short", timeStyle: "short" }).format(new Date(2026, 9, 25, 14, 5)),
  ];
  for (const sample of samples) {
    assert.ok(/[0-9]/.test(sample) && !NATIVE_DIGITS.test(sample), `${code}: ${sample}`);
  }
  assert.equal(new Intl.DateTimeFormat(code).resolvedOptions().numberingSystem, "latn", `${code} dates`);
}
console.log("PASS: Latin digits in Intl output for every registry locale");

// PRD 0035 RF10.1 and decision 8: the registry code reaches `Intl` as is, so the decimal separator and the grouping
// follow the language: a decimal comma in de, fr, ru, tr, vi and id-ID, lakh grouping in hi-IN, a dot elsewhere.
{
  const plain = (s) => s.replace(/[\s  ]/g, " ");
  const decimal = (code) => plain(new Intl.NumberFormat(code, { minimumFractionDigits: 2 }).format(1.5));
  for (const code of ["de", "fr", "ru", "tr", "vi", "id-ID", "es", "it", "pt-BR"]) {
    assert.equal(decimal(code), "1,50", `${code} decimal comma`);
  }
  for (const code of ["en-US", "zh-CN", "ja", "ko", "hi-IN"]) {
    assert.equal(decimal(code), "1.50", `${code} decimal dot`);
  }
  assert.equal(new Intl.NumberFormat("hi-IN").format(1234567), "12,34,567", "hi-IN lakh grouping");
  assert.equal(new Intl.NumberFormat("id-ID").format(1234567), "1.234.567", "id-ID dot grouping");
  assert.equal(plain(new Intl.NumberFormat("de", { style: "percent" }).format(0.82)), "82 %", "de percent");
  assert.equal(new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(6.25), "$6.25");
}
console.log("PASS: decimal separators and grouping follow the registry locale");

// PRD 0035 RF13.3: every desktop plural text renders the counts 0, 1, 2, 5, 11, 21, 22 and 1.5 in every shipped
// language through `t()`, with no placeholder left and the number shown (exactly 1 may be named in words).
{
  const { readFileSync } = await import("node:fs");
  const english = JSON.parse(readFileSync(new URL("../apps/desktop/ui/locales/en-US.json", import.meta.url), "utf8"));
  const plurals = Object.keys(english).filter((key) => english[key] && typeof english[key] === "object" && key !== "_meta");
  assert.ok(plurals.length >= 10, `${plurals.length} plural ids`);
  for (const { code } of REGISTRY) {
    await i18n.setLocale(code);
    assert.equal(i18n.currentLocale(), code);
    for (const key of plurals) {
      for (const count of [0, 1, 2, 5, 11, 21, 22, 1.5]) {
        const text = i18n.t(key, { count: String(count) });
        assert.ok(text && text !== key, `${code} ${key} ${count}: no text`);
        assert.ok(!/\{count\}/.test(text), `${code} ${key} ${count}: ${text}`);
        if (count !== 1) assert.ok(text.includes(String(count)), `${code} ${key} ${count} hides the number: ${text}`);
      }
    }
  }
  await i18n.setLocale("en-US");
}
console.log("PASS: desktop plural texts render every RF13.3 count in every language");

// PRD 0035 RF10.3: the pace outlook carries the margin as the core's localized duration, resolved in the UI language.
{
  const outlook = {
    id: "pace.runs_out_at",
    params: { at: "06:00", margin: { id: "text.duration.hours", params: { h: "18" }, fallback: "18h" } },
    fallback: "runs out at 06:00, 18h before the reset",
  };
  assert.equal(i18n.tl(outlook), "runs out at 06:00, 18h before the reset");
  await i18n.setLocale("ja");
  assert.ok(i18n.tl(outlook).includes("18時間"), i18n.tl(outlook));
  await i18n.setLocale("en-US");
}
console.log("PASS: the pace margin reads in the UI language");

// PRD 0001 re-validation (L5): a nested RF8 note is resolved with its own parameters and escaped once.
{
  const nested = { id: "test.l5.outer", params: { a: { id: "test.l5.inner", params: { name: "R&D" }, fallback: "{name}" }, name: "outer" }, fallback: "[{a}]" };
  assert.equal(i18n.tle(nested), "[R&amp;D]", "escaped once in HTML");
  assert.equal(i18n.tl(nested), "[R&D]", "plain text keeps the raw value");
  const clash = { id: "test.l5.clash", params: { a: { id: "test.l5.inner2", params: { name: "inner" }, fallback: "{name}" }, name: "outer" }, fallback: "{a}/{name}" };
  assert.equal(i18n.tl(clash), "inner/outer", "an outer parameter never overrides the nested note's own");
}
console.log("PASS: nested notes keep their own parameters and one escape");
