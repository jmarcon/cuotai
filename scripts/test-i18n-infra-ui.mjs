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
