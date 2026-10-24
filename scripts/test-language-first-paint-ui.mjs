#!/usr/bin/env node
// Browser-free UI smoke for PRD 0022 RF17.6: execute both production
// pre-paint bootstraps, then exercise the shared production boot and renderer.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = new URL("../", import.meta.url);
const storage = new Map();
let storageGetThrows = false;

globalThis.window = {};
globalThis.document = {
  hidden: false,
  documentElement: {
    lang: "en-US",
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
globalThis.location = { search: "" };
globalThis.localStorage = {
  getItem: (key) => {
    if (storageGetThrows) throw new Error("storage unavailable");
    return storage.get(key) ?? null;
  },
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

const setLanguages = (languages) => Object.defineProperty(globalThis, "navigator", {
  value: { languages, language: languages[0] },
  configurable: true,
});

async function runPrePaintBootstrap(page, storedLocale, systemLanguages, storageThrows = false, userLocales = []) {
  storage.clear();
  if (storedLocale) storage.set("cuotai.locale", storedLocale);
  if (userLocales.length) storage.set("cuotai.userLocales", JSON.stringify(userLocales));
  storageGetThrows = storageThrows;
  setLanguages(systemLanguages);
  document.documentElement.lang = "en-US";
  document.documentElement.dataset = {};

  const htmlUrl = new URL(`apps/desktop/ui/${page}`, root);
  const html = await readFile(fileURLToPath(htmlUrl), "utf8");
  const bootstrap = html.split("<script>", 2)[1]?.split("</script>", 1)[0];
  assert.ok(bootstrap, `${page}: missing pre-paint bootstrap`);
  try {
    vm.runInThisContext(bootstrap, { filename: fileURLToPath(htmlUrl) });
    return document.documentElement.lang;
  } finally {
    storageGetThrows = false;
  }
}

const userLocaleFile = {
  name: "fr.json",
  text: JSON.stringify({ _meta: { name: "Français" }, "tray.ok": "Tout fonctionne" }),
};
window.__TAURI__ = {
  core: {
    invoke: async (command) => command === "load_user_content"
      ? { dir: "/tmp/cuotai", errors: [], themes: [], locales: [userLocaleFile] }
      : {},
  },
};
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const { loadUserContent } = await import("../apps/desktop/ui/js/usercontent.js");
await loadUserContent();
assert.deepEqual(JSON.parse(storage.get("cuotai.userLocales")), ["fr"], "validated user locale mirror was not persisted");

const scenarios = [
  { stored: "it", system: ["pt-BR"], expected: "it", label: "stored locale" },
  { stored: "fr", system: ["en-US"], userLocales: ["fr"], expected: "fr", label: "stored user locale" },
  { stored: "bogus", system: ["es-MX"], expected: "es", label: "invalid stored locale" },
  { stored: null, system: ["it"], expected: "it", label: "unavailable storage", storageThrows: true },
  { stored: null, system: ["pt-BR"], expected: "pt-BR", label: "exact pt-BR detection" },
  { stored: null, system: ["en-US"], expected: "en-US", label: "exact en-US detection" },
  { stored: null, system: ["es"], expected: "es", label: "exact es detection" },
  { stored: null, system: ["it"], expected: "it", label: "exact it detection" },
  { stored: null, system: ["es-MX"], expected: "es", label: "same-language detection" },
  { stored: null, system: ["ja"], expected: "en-US", label: "unsupported locale fallback" },
];

for (const page of ["index.html", "tray.html", "settings.html"]) {
  for (const scenario of scenarios) {
    const preferred = scenario.storageThrows ? null : scenario.stored;
    const productionExpected = i18n.pickLocale(preferred, scenario.system);
    assert.equal(productionExpected, scenario.expected, `${scenario.label}: invalid explicit expectation`);
    const lang = await runPrePaintBootstrap(page, scenario.stored, scenario.system, scenario.storageThrows, scenario.userLocales);
    assert.equal(lang, productionExpected, `${page}: ${scenario.label} diverged from production locale resolution`);
    console.log(`${page} pre-paint (${scenario.label}): lang=${lang}`);
  }
}

await runPrePaintBootstrap("index.html", "fr", ["en-US"], false, ["fr"]);
const { boot } = await import("../apps/desktop/ui/js/boot.js");
const { langMenuHtml } = await import("../apps/desktop/ui/js/lib.js");
await boot();

const frenchCatalog = JSON.parse(userLocaleFile.text);
assert.equal(document.documentElement.lang, "fr", "shared boot replaced the stored user locale");
assert.equal(i18n.currentLocale(), "fr", "shared boot did not initialize the stored user locale");
assert.equal(i18n.t("tray.ok"), frenchCatalog["tray.ok"], "first translated string did not use the user catalog");
const firstRenderedMenu = langMenuHtml({ inline: true });
assert.match(firstRenderedMenu, /data-locale="fr"[^>]*class="on"|class="on"[^>]*data-locale="fr"/, "stored user locale is not selected in the production menu");
console.log(`production boot first render: lang=fr; tray.ok=${i18n.t("tray.ok")}`);

await i18n.setLocale("es");
const spanishCatalog = JSON.parse(await readFile(
  fileURLToPath(new URL("apps/desktop/ui/locales/es.json", root)),
  "utf8",
));
assert.equal(document.documentElement.lang, "es", "runtime language change did not update document.lang");
assert.equal(storage.get("cuotai.locale"), "es", "runtime language change was not persisted");
assert.ok(langMenuHtml({ inline: true }).includes(`aria-label="${spanishCatalog["lang.label"]}"`), "runtime language change did not re-render the production menu");
console.log(`runtime selector render: lang=es; tray.ok=${i18n.t("tray.ok")}`);

console.log("PASS: every HTML root (main, tray, settings) applies stored/detected language before module boot; production boot and runtime selection stay synchronized");
