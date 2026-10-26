#!/usr/bin/env node
// Browser-free UI smoke for PRD 0024 RF1.3, RF4.3, RF6.1 and RF7.3: the pace chip in both units, the forecast
// callout and chip, the reset pair and the best-account chip and advice render from the core's texts in pt-BR,
// en-US, es and it.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
// The shell serves the core catalog through `core_catalog`; the smoke reads the same files.
globalThis.window = {
  __TAURI__: {
    core: {
      invoke: async (command, args) => {
        if (command !== "core_catalog") return undefined;
        try {
          const file = new URL(`crates/cuotai-core/locales/${args.locale}.json`, root);
          return JSON.parse(await readFile(fileURLToPath(file), "utf8"));
        } catch {
          return {};
        }
      },
    },
  },
};
globalThis.document = {
  hidden: false,
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: () => {},
};
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
globalThis.setInterval = () => 0;
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
await i18n.initI18n();
// Two instances of the library: one that starts with the default unit (points), one that starts on percent.
const points = await import("../apps/desktop/ui/js/lib.js?points");
storage.set("cuotai.pace.unit", "percent");
const percent = await import("../apps/desktop/ui/js/lib.js?percent");
storage.delete("cuotai.pace.unit");
const { metricCardHtml } = await import("../apps/desktop/ui/js/detail.js");

const text = (id, params, fallback) => ({ id, params, fallback });
const minutes = (m) => text("text.duration.minutes", { m: String(m) }, `${m} min`);

const metric = {
  label: "Session (5h)",
  state: "available",
  uncapped: false,
  no_cap: false,
  ratio: 0.4,
  used: 60,
  limit: 100,
  unit: "%",
  resets_at: new Date(Date.now() + 3 * 3_600_000).toISOString(),
  pace: {
    stage: "ahead",
    tone: "warn",
    expected_used_fraction: 0.5,
    label: text("pace.ahead", { delta: "10" }, "10 points above the pace"),
    outlook: null,
    chips: {
      points: { glyph: "↑", text: text("pace.chip.ahead", { d: "10" }, "10 pts ahead") },
      percent: { glyph: "↑", text: text("pace.chip.ahead_pct", { p: "20" }, "20% faster than pace") },
    },
  },
  forecast: {
    callout: text("alerts.body.projected", { metric: "Session (5h)", at: "18:53", margin: minutes(37) },
      "At this rate Session (5h) runs out ≈18:53, 37 min before the reset"),
    chip: { glyph: "↑", text: text("pace.chip.runs_out", { at: "18:53" }, "Runs out ≈18:53") },
    at: "18:53",
  },
  resets_label: text("resets.pair", {
    at: "19:30",
    in: text("text.countdown", { duration: text("text.duration.hours_minutes", { h: "1", m: "35" }, "1h 35m") }, "in 1h 35m"),
  }, "Resets 19:30 · in 1h 35m"),
};
const report = { provider: "open_ai_api", account: "work", binding: null, metrics: [metric], index: 0, status: "ready" };
const account = (name, extra) => ({ ...report, account: name, ...extra });

const expected = {
  "pt-BR": {
    points: "10 pts acima", percent: "20% acima do ritmo",
    callout: "Neste ritmo Session (5h) acaba ≈18:53, 37 min antes da renovação", chip: "Acaba ≈18:53",
    reset: "Renova 19:30 · em 1h 35m", best: "Melhor agora",
    advice: "Work esgotou · Personal tem 64% sobrando",
  },
  "en-US": {
    points: "10 pts ahead", percent: "20% faster than pace",
    callout: "At this rate Session (5h) runs out ≈18:53, 37 min before the reset", chip: "Runs out ≈18:53",
    reset: "Resets 19:30 · in 1h 35m", best: "Best now",
    advice: "Work is exhausted · Personal has 64% left",
  },
  es: {
    points: "10 pts por encima", percent: "20% sobre el ritmo",
    callout: "A este ritmo Session (5h) se agota ≈18:53, 37 min antes del reinicio", chip: "Se agota ≈18:53",
    reset: "Se reinicia 19:30 · en 1h 35m", best: "Mejor ahora",
    advice: "Work está agotada · Personal tiene 64% disponible",
  },
  it: {
    points: "10 pt sopra", percent: "20% sopra il ritmo",
    callout: "A questo ritmo Session (5h) finisce ≈18:53, 37 min prima del rinnovo", chip: "Finisce ≈18:53",
    reset: "Si rinnova 19:30 · tra 1h 35m", best: "Migliore ora",
    advice: "Work è esaurito · Personal ha il 64% disponibile",
  },
  de: {
    points: "10 Pkt. voraus", percent: "20% über dem Tempo",
    callout: "In diesem Tempo ist Session (5h) ≈18:53 erschöpft, 37 Min. vor der Erneuerung", chip: "Reicht bis ≈18:53",
    reset: "Erneuert 19:30 · in 1 Std. 35 Min.", best: "Jetzt am besten",
    advice: "Work ist erschöpft · Personal hat noch 64%",
  },
  ru: {
    points: "+10 п. п.", percent: "на 20% выше темпа",
    callout: "В этом темпе Session (5h) закончится ≈18:53, за 37 мин до обновления", chip: "Закончится ≈18:53",
    reset: "Обновится 19:30 · через 1 ч 35 мин", best: "Лучшая сейчас",
    advice: "Work исчерпан · у Personal осталось 64%",
  },
  ko: {
    points: "10pt 초과", percent: "페이스보다 20% 빠름",
    callout: "이 속도라면 초기화보다 37분 앞서 ≈18:53에 Session (5h)이(가) 소진됩니다", chip: "≈18:53 소진",
    reset: "19:30에 갱신 · 1시간 35분 후", best: "지금 최적",
    advice: "Work은(는) 소진됨 · Personal은(는) 64% 남음",
  },
  "hi-IN": {
    points: "10 अंक आगे", percent: "गति से 20% तेज़",
    callout: "इस गति से Session (5h) रीसेट से 37 मिनट पहले ≈18:53 पर खत्म हो जाएगा", chip: "≈18:53 पर खत्म",
    reset: "19:30 को नवीनीकरण · 1 घं 35 मि में", best: "अभी सर्वश्रेष्ठ",
    advice: "Work खत्म हो गया · Personal में 64% बचा है",
  },
};

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const want = expected[code];
  // RF1.3: the same core chip, in the unit the user chose.
  assert.equal(points.paceText(metric.pace, { short: false }).startsWith(want.points), true,
    `${code}: points chip: ${points.paceText(metric.pace)}`);
  assert.equal(percent.paceText(metric.pace).startsWith(want.percent), true,
    `${code}: percent chip: ${percent.paceText(metric.pace)}`);
  // RF4.3 / RF6.1: the callout, the chip and the reset pair in the details.
  const card = metricCardHtml(report, metric);
  assert.ok(card.includes(`<p class="call warn" role="note">`), `${code}: callout missing`);
  assert.ok(card.includes(want.callout), `${code}: callout text: ${card}`);
  assert.ok(card.includes(want.reset), `${code}: reset pair`);
  assert.ok(points.forecastChipHtml(metric).includes(want.chip), `${code}: forecast chip`);
  // No forecast, no callout.
  assert.ok(!metricCardHtml(report, { ...metric, forecast: null }).includes('class="call'), `${code}: stray callout`);
  // RF7.3: the best chip and the advice with its one action.
  const best = account("Personal", { best_now: text("advice.best", {}, "Best now"), group_label: "Coding" });
  assert.ok(points.bestChipHtml(best).includes(want.best), `${code}: best chip`);
  assert.equal(points.bestChipHtml(account("Work", {})), "", `${code}: a non-best account has no chip`);
  const advised = account("Work", {
    advice: {
      text: text("advice.switch.exhausted", { from: "Work", to: "Personal", pct: "64" }, "Work is exhausted · Personal has 64% left"),
      best_index: 1,
      best_name: "Personal",
    },
  });
  const html = points.adviceHtml(advised);
  assert.ok(html.includes(want.advice), `${code}: advice text: ${html}`);
  assert.ok(html.includes('data-show-account="1"'), `${code}: the one action focuses the best card`);
  assert.equal(points.adviceHtml(advised, { action: false }).includes("data-show-account"), false);
  console.log(`${code}: ${want.points} | ${want.percent} | ${want.chip}`);
}

console.log("PASS: pace chips (points and percent), forecast callout, reset pair and best-account advice render in pt-BR, en-US, es and it");
