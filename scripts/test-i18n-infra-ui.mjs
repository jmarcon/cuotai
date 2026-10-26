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
// A language in progress cannot be chosen; the language list shows it as pending.
await i18n.setLocale("ja");
assert.equal(i18n.currentLocale(), "it");
assert.ok(i18n.pendingLocales().some((l) => l.code === "ja"));
await i18n.setLocale("en-US");
assert.equal(i18n.tl(alerts(1)), "Cuotai: 1 usage alerts", "English renders the core's own fallback");
assert.equal(durationText(95 * 60), "1h 35m");
console.log("PASS: registry normalization, folding, durations and core plurals in the desktop modules");
