#!/usr/bin/env node
// Browser-free UI smoke for PRD 0033: the collapsed "Renewed at ..." metric of a
// preserved reading in the viewer's local time (details, a card whose other
// window is current, a tray row), the core-style clock, the "data as of" age
// text and the pace line label rule, in every built-in language.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: () => {},
};
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", {
  value: { languages: ["en-US"], language: "en-US" },
  configurable: true,
});
globalThis.location = { search: "" };
globalThis.localStorage = {
  getItem: (key) => storage.get(key) ?? null,
  setItem: (key, value) => storage.set(key, value),
};
globalThis.fetch = async (relative) => {
  try {
    const path = new URL(`apps/desktop/ui/${relative}`, root);
    const data = JSON.parse(await readFile(fileURLToPath(path), "utf8"));
    return { ok: true, json: async () => data };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { metricCardHtml } = await import("../apps/desktop/ui/js/detail.js");
const { summaryHtml } = await import("../apps/desktop/ui/js/card-summary.js");
const { rowHtml } = await import("../apps/desktop/ui/js/components.js");
await i18n.initI18n();

const resetsAt = new Date(Date.now() - 3 * 3600 * 1000).toISOString();
const collapsed = {
  label: "Week", state: "unmeasured", used: null, limit: null, remaining: null, unit: "%",
  window: "weekly", resets_at: resetsAt, expired: true, pace: null,
};
const live = {
  label: "5h", state: "available", ratio: 0.5, used: 50, limit: 100, unit: "%",
  window: "window_5h", resets_at: null, pace: { stage: "reserve", tone: "good", label: { id: "x", fallback: "8 points in reserve" }, expected_used_fraction: 0.4 },
};
const base = { provider: "claude", account: "work", status: "ready", checked_at: new Date().toISOString(), notes: [], freshness: "fresh" };
const wording = (code, clock) => ({
  "pt-BR": `Renovada às ${clock}; aguardando a próxima leitura`,
  "en-US": `Renewed at ${clock}; waiting for the next reading`,
  es: `Renovada a las ${clock}; esperando la próxima lectura`,
  it: `Rinnovata alle ${clock}; in attesa della prossima lettura`,
  de: `Erneuert um ${clock}; wartet auf den nächsten Messwert`,
  ru: `Обновлено в ${clock}; ожидается следующее показание`,
  ko: `${clock}에 초기화됨, 다음 조회값을 기다리는 중`,
  "hi-IN": `${clock} पर रीसेट हुआ; अगली रीडिंग का इंतज़ार`,
  "id-ID": `Diperbarui pada ${clock}; menunggu pembacaan berikutnya`,
  tr: `${clock} saatinde yenilendi; sonraki okuma bekleniyor`,
  vi: `Đã làm mới lúc ${clock}; đang chờ lần đọc tiếp theo`,
  "zh-CN": `已于 ${clock} 重置；等待下一次读数`,
  fr: `Réinitialisé à ${clock} ; en attente du prochain relevé`,
  ja: `${clock} にリセット。次の読み取り待ち`,
})[code];
// V-09: the core's clock (`text::clock::format_time`) for 1:05 today: 12-hour locales never pad the hour.
const early = new Date();
early.setHours(1, 5, 0, 0);
const coreTime = { "en-US": "1:05 AM", "hi-IN": "1:05 am", ko: "오전 1:05", "id-ID": "01.05" };
// The renewed 5 h window of a mixed card (the week is still current) and a tray row whose only quota renewed.
const collapsed5h = { ...collapsed, label: "5h", window: "window_5h" };
const liveWeek = { label: "Week", state: "available", ratio: 0.6, used: 60, limit: 100, unit: "%", window: "weekly", resets_at: null, pace: null };
// Test-only text extraction (never a sanitizer): drop tags until none is left, then collapse spaces.
const stripTags = (html) => {
  let text = html, prev;
  do { prev = text; text = text.replace(/<[^<>]*>/g, ""); } while (text !== prev);
  return text.replace(/\s+/g, " ");
};
const ENTITIES = { amp: "&", quot: '"', "#39": "'", lt: "<", gt: ">" };
const unescape = (s) => s.replace(/&(amp|quot|#39|lt|gt);/g, (_, name) => ENTITIES[name]);
const asOfWording = { "pt-BR": "dados de", "en-US": "data as of", es: "datos de", it: "dati delle", de: "Daten vom", ru: "данные от", ko: "기준 데이터", "hi-IN": "तक का डेटा", "id-ID": "data per", tr: "veriler", vi: "dữ liệu tại", "zh-CN": "数据截至", fr: "données du", ja: "時点のデータ" };

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const clock = lib.coreClock(resetsAt);
  assert.ok(clock && !/^\d{4}-/.test(clock), `${code}: the renewed time is a clock, never an ISO date`);
  const text = wording(code, clock);
  const one = lib.coreClock(early.toISOString());
  assert.equal(one.replace(/\u202f|\u00a0/g, " "), (coreTime[code] ?? "01:05").replace(/\u00a0/g, " "), `${code}: core-style clock`);
  const report = { ...base, binding: null, metrics: [collapsed] };
  const html = metricCardHtml(report, collapsed);
  assert.ok(html.includes("data-renewed-waiting"), `${code}: collapsed marker missing`);
  assert.ok(html.includes(text), `${code}: renewed text drifted: ${html}`);
  assert.ok(!html.includes('class="resets"'), `${code}: a collapsed metric keeps no reset line`);
  assert.equal(lib.summaryOf({ ...report, last_good: null }).kind, "renewed", `${code}: card summary`);

  // V-01: a card with a renewed 5 h window beside a current week names the renewal in that slot, never "—".
  const mixed = { ...base, binding: { index: 1 }, metrics: [collapsed5h, liveWeek] };
  const card = summaryHtml(mixed);
  const slot = card.match(/<div class="win none renewed"[^>]*>[\s\S]*?<\/div>\s*<p class="win-renewed">[\s\S]*?<\/p><\/div>/)?.[0];
  assert.ok(slot, `${code}: the renewed slot is missing from the mixed card`);
  assert.ok(slot.includes("data-renewed-waiting"), `${code}: renewed slot marker`);
  assert.ok(unescape(stripTags(slot)).includes(text), `${code}: the renewed slot text drifted`);
  assert.ok(unescape(slot.match(/aria-label="([^"]*)"/)[1]).endsWith(text), `${code}: the slot's name carries the renewed text`);
  assert.ok(/<svg[^>]*>[\s\S]*?<\/svg> <span>/.test(slot), `${code}: one space between the clock icon and the text (V-08)`);
  assert.ok(!slot.includes(">—<"), `${code}: a renewed slot never reads "—"`);
  assert.equal((card.match(/class="win[ "]/g) ?? []).length, 2, `${code}: both window slots are drawn`);

  // V-02: a tray row of a preserved reading whose only quota renewed says so on its extra line and in its name.
  const stale = { ...base, status: "network_error", metrics: [], binding: null, last_good: { checked_at: new Date(Date.now() - 5 * 3600e3).toISOString(), metrics: [collapsed] } };
  const row = rowHtml(stale, { title: "Kimi" });
  const xline = row.match(/<span class="xline muted" data-renewed-waiting title="([^"]*)">[\s\S]*?<span>([^<]*)<\/span><\/span>/);
  assert.ok(xline && unescape(xline[2]) === text && unescape(xline[1]) === text, `${code}: the tray row's renewed line drifted`);
  assert.match(row, /<span class="lbl warn">[^<]+<\/span><span class="det">· [^<]+<\/span>/, `${code}: the status line keeps "Stale · age"`);
  assert.ok(unescape(row.match(/aria-label="([^"]*)"/)[1]).includes(text), `${code}: the tray row's name lacks the renewed text`);

  const lagging = { ...base, metrics: [live], data_as_of: new Date(Date.now() - 6 * 3600 * 1000).toISOString() };
  assert.ok(lib.ageText(lagging).includes(asOfWording[code]), `${code}: data-as-of text missing`);
  assert.ok(!lib.ageText({ ...base, metrics: [live] }).includes(asOfWording[code]), `${code}: no as-of without data_as_of`);
  console.log(`${code}: ${text}`);
}

// RF4.1: the pace line names its metric unless the binding chip says it already.
await i18n.setLocale("en-US");
const week = { ...live, label: "Week", window: "weekly", pace: null };
const report = { ...base, metrics: [week, live], binding: { index: 0 } };
report.metrics = [week, live];
assert.equal(lib.paceLineLabel(report, live), "5h", "a pace on another metric than the chip's keeps its label");
assert.equal(lib.paceLineLabel({ ...report, binding: { index: 1 } }, live), "", "same metric as the chip: label omitted");
assert.equal(lib.paceLabelOf({ window: "other", label: "Custom" }), "Custom");

console.log("PASS: renewed text (details, mixed card, tray row), core clock, data-as-of and pace labels render in every language");
