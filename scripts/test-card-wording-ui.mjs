#!/usr/bin/env node
// Browser-free UI smoke for PRD 0022 RF17.7: render the remaining-share
// figure with every built-in locale and keep UI-owned catalog keys in parity.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  documentElement: {
    lang: "",
    dataset: {},
    toggleAttribute: () => {},
  },
  addEventListener: () => {},
};
globalThis.matchMedia = () => ({
  matches: false,
  addEventListener: () => {},
});
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
const { detailHtml, metricCardHtml, openReference, referenceHtml } = await import("../apps/desktop/ui/js/detail.js");
const { esc } = await import("../apps/desktop/ui/js/lib.js");
await i18n.initI18n();

const expected = {
  "pt-BR": {
    unit: "disponível",
    levels: {
      good: "75% disponível: com folga (70% ou mais).",
      warn: "Atenção: 50% disponível (entre 30% e 70%).",
      bad: "Crítico: só 20% disponível (menos de 30%).",
    },
    rateLimit: "Rate limit: 42% disponível",
    sortAsc: "Disponível: menor primeiro",
    sortDesc: "Disponível: maior primeiro",
  },
  "en-US": {
    unit: "left",
    levels: {
      good: "75% left: plenty remains (70% or more).",
      warn: "Attention: 50% left (between 30% and 70%).",
      bad: "Critical: only 20% left (under 30%).",
    },
    rateLimit: "Rate limit: 42% left",
    sortAsc: "Left: lowest first",
    sortDesc: "Left: highest first",
  },
  es: {
    unit: "disponible",
    levels: {
      good: "75% disponible: con margen (70% o más).",
      warn: "Atención: 50% disponible (entre 30% y 70%).",
      bad: "Crítico: solo 20% disponible (menos de 30%).",
    },
    rateLimit: "Rate limit: 42% disponible",
    sortAsc: "Disponible: menor primero",
    sortDesc: "Disponible: mayor primero",
  },
  it: {
    unit: "disponibile",
    levels: {
      good: "75% disponibile: ne resta in abbondanza (70% o più).",
      warn: "Attenzione: 50% disponibile (tra 30% e 70%).",
      bad: "Critico: solo 20% disponibile (meno di 30%).",
    },
    rateLimit: "Limite di frequenza: 42% disponibile",
    sortAsc: "Disponibile: prima il minore",
    sortDesc: "Disponibile: prima il maggiore",
  },
  de: {
    unit: "übrig",
    levels: {
      good: "75% übrig: reichlich vorhanden (70% oder mehr).",
      warn: "Achtung: 50% übrig (zwischen 30% und 70%).",
      bad: "Kritisch: nur noch 20% übrig (unter 30%).",
    },
    rateLimit: "Ratenlimit: 42% übrig",
    sortAsc: "Übrig: niedrigste zuerst",
    sortDesc: "Übrig: höchste zuerst",
  },
  ru: {
    unit: "осталось",
    levels: {
      good: "Осталось 75%: запас достаточный (70% и более).",
      warn: "Внимание: осталось 50% (от 30% до 70%).",
      bad: "Критично: осталось всего 20% (меньше 30%).",
    },
    rateLimit: "Лимит частоты: осталось 42%",
    sortAsc: "Осталось: сначала наименьшее",
    sortDesc: "Осталось: сначала наибольшее",
  },
  ko: {
    unit: "남음",
    levels: {
      good: "75% 남음: 여유가 충분합니다(70% 이상).",
      warn: "주의: 50% 남음(30%~70%).",
      bad: "위험: 20%만 남음(30% 미만).",
    },
    rateLimit: "요청 제한: 42% 남음",
    sortAsc: "남음: 적은 순",
    sortDesc: "남음: 많은 순",
  },
  "hi-IN": {
    unit: "बचा",
    levels: {
      good: "75% बचा: काफ़ी बचा है (70% या ज़्यादा)।",
      warn: "ध्यान दें: 50% बचा (30% से 70% के बीच)।",
      bad: "गंभीर: केवल 20% बचा (30% से कम)।",
    },
    rateLimit: "रेट लिमिट: 42% बचा",
    sortAsc: "बचा: पहले कम वाला",
    sortDesc: "बचा: पहले ज़्यादा वाला",
  },
  "id-ID": {
    unit: "tersisa",
    levels: {
      good: "Sisa 75%: masih banyak (70% atau lebih).",
      warn: "Perhatian: sisa 50% (antara 30% dan 70%).",
      bad: "Kritis: hanya sisa 20% (di bawah 30%).",
    },
    rateLimit: "Batas laju: sisa 42%",
    sortAsc: "Sisa: paling sedikit dulu",
    sortDesc: "Sisa: paling banyak dulu",
  },
  tr: {
    unit: "kaldı",
    levels: {
      good: "%75 kaldı: bol miktarda var (%70 veya daha fazla).",
      warn: "Dikkat: %50 kaldı (%30 ile %70 arası).",
      bad: "Kritik: yalnızca %20 kaldı (%30&#39;un altında).",
    },
    rateLimit: "Hız sınırı: %42 kaldı",
    sortAsc: "Kalan: en az önce",
    sortDesc: "Kalan: en çok önce",
  },
  vi: {
    unit: "còn lại",
    levels: {
      good: "Còn 75%: còn nhiều (70% trở lên).",
      warn: "Chú ý: còn 50% (từ 30% đến 70%).",
      bad: "Nguy cấp: chỉ còn 20% (dưới 30%).",
    },
    rateLimit: "Giới hạn tốc độ: còn 42%",
    sortAsc: "Còn lại: ít nhất trước",
    sortDesc: "Còn lại: nhiều nhất trước",
  },
  "zh-CN": {
    unit: "剩余",
    levels: {
      good: "剩余 75%：余量充足（70% 或以上）。",
      warn: "注意：剩余 50%（30% 到 70% 之间）。",
      bad: "严重：仅剩 20%（低于 30%）。",
    },
    rateLimit: "速率限制：剩余 42%",
    sortAsc: "剩余：最少优先",
    sortDesc: "剩余：最多优先",
  },
  fr: {
    unit: "restant",
    levels: {
      good: "75 % restant : il en reste largement (70 % ou plus).",
      warn: "Attention : 50 % restant (entre 30 % et 70 %).",
      bad: "Critique : seulement 20 % restant (moins de 30 %).",
    },
    rateLimit: "Limite de débit : 42 % restant",
    sortAsc: "Restant : le plus bas d’abord",
    sortDesc: "Restant : le plus haut d’abord",
  },
  ja: {
    unit: "残り",
    levels: {
      good: "残り 75%：十分に残っています（70% 以上）。",
      warn: "注意：残り 50%（30%〜70%）。",
      bad: "危険：残りわずか 20%（30% 未満）。",
    },
    rateLimit: "レート制限：残り 42%",
    sortAsc: "残り：少ない順",
    sortDesc: "残り：多い順",
  },
};

const coreCatalog = JSON.parse(await readFile(
  fileURLToPath(new URL("crates/cuotai-core/locales/pt-BR.json", root)),
  "utf8",
));
const coreKeys = new Set(Object.keys(coreCatalog));
let expectedUiKeys;

const metric = {
  label: "5h",
  state: "available",
  uncapped: false,
  no_cap: false,
  ratio: 0.75,
  used: 25,
  limit: 100,
  unit: "%",
  resets_at: null,
  pace: null,
};
const report = {
  provider: "open_ai_api",
  account: "work",
  binding: null,
  metrics: [metric],
};

for (const { code } of i18n.LOCALES) {
  const catalogPath = new URL(`apps/desktop/ui/locales/${code}.json`, root);
  const catalog = JSON.parse(await readFile(fileURLToPath(catalogPath), "utf8"));
  const uiKeys = Object.keys(catalog).filter((key) => !coreKeys.has(key)).sort();
  expectedUiKeys ??= uiKeys;
  assert.deepEqual(uiKeys, expectedUiKeys, `${code}: UI-owned catalog keys drifted`);

  const wording = expected[code];
  assert.equal(catalog["card.available"], wording.unit, `${code}: card.available drifted`);

  await i18n.setLocale(code);
  // The window's name titles the only limit of a window; several keep the model or the group that tells them apart.
  const h5 = catalog["card.window.long.h5"];
  const week = catalog["card.window.long.week"];
  const title = (m, metrics) => metricCardHtml({ ...report, metrics }, { ...metric, ...m }).match(/<h3>([^<]*)/)[1].trim();
  const pro = { label: "Gemini Pro", window: "window_5h", scope: { label: "Gemini Pro" } };
  const flash = { label: "Gemini Flash", window: "window_5h", scope: { label: "Gemini Flash" } };
  const group = { label: "Gemini (5h)", label_text: "Gemini (5h)", window: "window_5h" };
  const weekly = { label: "Gemini (Semanal)", label_text: "Gemini (Weekly)", window: "weekly" };
  assert.equal(title(pro, [pro, flash, group, weekly]), esc(`Gemini Pro · ${h5}`), `${code}: model name lost`);
  assert.equal(title(flash, [pro, flash, group, weekly]), esc(`Gemini Flash · ${h5}`), `${code}: model name lost`);
  assert.equal(title(group, [pro, flash, group, weekly]), "Gemini (5h)", `${code}: group name lost`);
  assert.equal(title(weekly, [pro, flash, group, weekly]), esc(week), `${code}: the only weekly limit lost the window's name`);
  assert.equal(title(pro, [pro]), esc(h5), `${code}: the only 5-hour limit lost the window's name`);
  for (const [tone, ratio] of [["good", 0.75], ["warn", 0.5], ["bad", 0.2]]) {
    const rendered = metricCardHtml(report, { ...metric, ratio });
    assert.ok(
      rendered.includes(`<small>${wording.unit}</small>`),
      `${code}/${tone}: remaining-share unit did not render`,
    );
    assert.ok(
      rendered.includes(`title="${wording.levels[tone]}"`),
      `${code}/${tone}: remaining-share level sentence did not render`,
    );
  }
  assert.equal(
    i18n.t("card.rateLimit", { pct: 42 }),
    wording.rateLimit,
    `${code}: rate-limit composition drifted`,
  );
  assert.equal(i18n.t("sort.pct-asc"), wording.sortAsc, `${code}: ascending sort composition drifted`);
  assert.equal(i18n.t("sort.pct-desc"), wording.sortDesc, `${code}: descending sort composition drifted`);

  // PRD 0032 RF8.2: a level crossed while Cuotai was closed shows its own line in the details, in the language;
  // a metric without it shows none.
  const crossed = metricCardHtml(report, { ...metric, ratio: 0.08, alert: { muted: false, active: true, missed: 90 } });
  assert.ok(crossed.includes("data-missed"), `${code}: missed line did not render`);
  const line = crossed.match(/data-missed><svg[\s\S]*?<\/svg><span>([^<]*)<\/span>/)?.[1];
  assert.equal(line, i18n.t("alerts.missed", { percent: 90 }), `${code}: missed line text`);
  assert.ok(line && !line.includes("{") && /90/.test(line), `${code}: missed line not translated: ${line}`);
  assert.ok(!metricCardHtml(report, { ...metric, alert: { muted: false, active: false } }).includes("data-missed"), `${code}: marker without a crossing`);

  console.log(`${code}: 75% ${wording.unit}; ${wording.rateLimit}`);
}

console.log("PASS: remaining-share wording renders in pt-BR, en-US, es and it; UI catalog keys match");

// PRD 0005 RF4.2 (DS-MSG-9): a changed interface explains itself without an address; the details link the core's
// reference contract, and the link opens through `openExternal` (the shell's `open_external` allow-list), never
// through `open_dashboard`.
{
  const reference = "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/zed.js";
  const changed = {
    index: 0, provider: "zed", account: "Personal", status: "error", class: "unknown", binding: null, metrics: [],
    notes: ["x"], notes_l10n: [{ id: "query.schema", params: {}, fallback: "x" }], checked_at: new Date().toISOString(),
    access: {
      explanation: { id: "access.interface_changed", params: {}, fallback: "The response does not follow the known internal interface: it may have changed without notice." },
      reference, reference_label: { id: "access.reference", params: {}, fallback: "Reference contract" }, session: null,
    },
  };
  const html = detailHtml(changed, { eye: true });
  const cause = html.match(/<div class="call [^"]*">[\s\S]*?<div>([^<]*)<\/div><\/div>/)?.[1] ?? "";
  assert.ok(cause === esc(i18n.tl(changed.access.explanation)) && cause && !cause.includes("https://"), `cause carries no address: ${cause}`);
  assert.ok(html.includes(`data-reference-url="${reference}"`), "details link the reference");
  const label = i18n.tl(changed.access.reference_label);
  assert.ok(label && label !== "access.reference" && html.includes(`<span>${esc(label)}</span>`), "the link is labelled by the core");
  assert.equal(referenceHtml({ ...changed, access: { ...changed.access, reference: null } }), "", "no reference, no link");
  let opened = null;
  globalThis.window.open = (url) => { opened = url; return null; };
  const button = { dataset: { referenceUrl: reference } };
  assert.equal(openReference({ closest: (selector) => (selector === "[data-reference-url]" ? button : null) }), true);
  assert.equal(opened, reference, "the reference opens through openExternal");
  assert.equal(openReference({ closest: () => null }), false, "other clicks fall through");
  const detailSource = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/detail.js", root)), "utf8");
  const opener = detailSource.slice(detailSource.indexOf("export function openReference"), detailSource.indexOf("export function detailHtml"));
  assert.ok(opener.includes("openExternal(") && !opener.includes("openDashboard") && !opener.includes("open_dashboard"), "reference opener uses openExternal only");
  console.log("PASS: the changed-interface reference is a details link opened through openExternal");
}

// PRD 0009 re-validation V-01, V-04, V-05, V-07: binding names come from the core's translated label, shares are
// rounded honestly, and the browser demo agrees with the core's binding rule.
{
  const lib = await import("../apps/desktop/ui/js/lib.js");
  const label_text = { id: "metric.label.rolling_window", params: {}, fallback: "Rolling window" };
  const report = (used_ratio, extra = {}) => ({ status: "ready", metrics: [], binding: { index: 0, label: "Janela móvel", label_text, window: "other",
    used_ratio, remaining: 1, resets_at: null, reason: used_ratio >= 1 ? "exhausted" : "tightest", blocked_until: null, ...extra } });
  for (const locale of ["en-US", "de", "pt-BR"]) {
    await i18n.setLocale(locale);
    const name = i18n.tl(label_text);
    assert.ok(name && !name.includes("metric.label"), `${locale}: the catalog names the rolling window`);
    assert.equal(lib.bindingLabel(report(0.5)), name, `${locale}: the binding name is the translated label`);
    if (locale !== "pt-BR") assert.ok(!lib.bindingLabel(report(0.5)).includes("Janela"), `${locale}: no raw adapter label`);
    assert.equal(lib.metricName({ label: "Janela móvel", label_text }), name, `${locale}: column names use the translated label`);
  }
  await i18n.setLocale("en-US");
  assert.equal(lib.bindingLabel(report(0.5, { label_text: undefined })), "Janela móvel", "without a translation the raw label stays");
  assert.ok(lib.bindingChipText(report(0.996)).endsWith(" · 99%"), "99.6 % used never reads 100%");
  assert.ok(lib.bindingChipText(report(0.004)).endsWith(" · < 1%"), "a sliver used reads < 1%");
  assert.ok(lib.bindingChipText(report(1)).endsWith(" · 100%"), "an exhausted limit reads 100%");
  assert.deepEqual([lib.fmtShare(0.4), lib.fmtShare(99.6), lib.fmtShare(100), lib.fmtShare(0)], ["< 1%", "99%", "100%", "0%"]);
  const src = async (file) => readFile(fileURLToPath(new URL(`apps/desktop/ui/js/${file}`, root)), "utf8");
  const main = await src("main.js"), tray = await src("tray-menu.js"), comp = await src("components.js");
  assert.ok(!main.includes("${m.label} ·") && !main.includes("s.m.label, s.m") && !main.includes("Math.round(pct)}%`"), "card columns use metricName and fmtShare");
  assert.ok(tray.includes("fmtShare(pct)") && !tray.includes("Math.round(pct)}%"), "the native menu rounds honestly");
  assert.ok(!comp.includes("r.binding.label ??"), "the status line uses the translated binding name");
  // V-07: the demo follows `metrics::binding` / `classify_report`.
  const { reports } = await lib.getView();
  for (const r of reports.filter((x) => x.status === "ready")) {
    if (r.binding?.reason === "exhausted") assert.equal(r.class, "exhausted", `${r.provider}: a blocked binding implies the exhausted class`);
    const measurable = r.metrics.some((m) => m.kind === "quota" && !m.scope && m.limit > 0 && m.used != null);
    if (measurable && !r.scope_blocks) assert.ok(r.binding, `${r.provider}: a measurable quota has a binding`);
  }
  console.log("PASS: binding names are translated, shares are rounded honestly and the demo follows the core rule");
}
