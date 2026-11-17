#!/usr/bin/env node
// PRD 0050 RF3 acceptance (theme half): the desktop loader registers a theme the core already parsed and a language
// pack the shell already stripped of its protected ids, inserts only the canonical CSS it received, keeps the built-in
// security wording, reports refused files with the parser's reason, and never requests a remote resource.
// PRD 0035 re-validation S-02: a pack arrives as the core parsed and filtered it (no file text); the webview never
// parses one, refuses an item that carries `text`, and registers exactly the messages the core produced
// (`crates/cuotai-core/tests/fixtures/user_pack/filtered.json`, written by the core's tests).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "en-US" });
/** A locale item as `user_content.rs` sends it. */
function pack(code, label, messages, extra = {}) {
  return { name: `${code}.json`, code, label, flag: null, digest: "0".repeat(64), messages, ignored: [], ...extra };
}
const requests = [];
const localFetch = globalThis.fetch;
globalThis.fetch = async (url, ...rest) => {
  requests.push(String(url));
  return localFetch(url, ...rest);
};
const styles = [];
document.head = { append: (node) => styles.push(node) };
document.createElement = (tag) => ({ tag, dataset: {}, textContent: "" });

// What `load_user_content` returns once `user_content.rs` ran `cuotai_core::user_theme::parse` and
// `user_pack::load`: canonical theme CSS, a parsed pack without protected ids, and the refusals.
const oceanCss = ':root[data-theme="ocean"][data-mode="dark"] {\n  color-scheme: dark;\n  --bg: #0b1a24;\n}\n'
  + ':root[data-theme="ocean"][data-mode="light"] {\n  color-scheme: light;\n  --bg: #eef6fb;\n}\n';
const payload = {
  dir: "/tmp/cuotai",
  themes: [{ name: "ocean.css", id: "ocean", label: "Ocean", group: "custom", modes: ["dark", "light"], text: oceanCss }],
  locales: [
    pack("fr", "Français", { "tray.ok": "Tout fonctionne" }, { ignored: ["serve.banner", "serve.token"] }),
    // PRD 0034 re-validation S-05: codes that fit the shape but not BCP 47, which would make every Intl call throw (the
    // core refuses them first; the webview repeats the check).
    ...["en-US-US", "en-a1", "xx-ab-cd-ef"].map((code) => pack(code, code, { "tray.ok": "x" })),
    // S-02: an item with a file's text, or without a messages object, never went through the core: refused whole.
    { ...pack("it", "Italiano", { "tray.ok": "x" }), text: '{"_meta":{"name":"Italiano"},"tray.ok":"x"}' },
    { name: "es.json", text: JSON.stringify({ _meta: { name: "Español" }, "tray.ok": "x" }) },
    { ...pack("es", "Español", {}), messages: '{"tray.ok":"x"}' },
    // L-02 and the other shape checks, as defence in depth.
    pack("de", "Deutsch", { "tray.ok": "x" }, { flag: ["de"] }),
    pack("nl", "", { "tray.ok": "x" }),
    pack("pl", "Polski", { "a b": "x" }),
    pack("ru", "Русский", { "tray.ok": ["x"] }),
    pack("tr", "Türkçe", { "tray.ok": { one: "x" } }),
  ],
  errors: [
    { kind: "theme", name: "pixel.css", reason: "externalUrl" },
    { kind: "theme", name: "wide.css", reason: "property" },
    { kind: "locale", name: "fr.json", reason: "protected", count: 2 },
  ],
};
window.__TAURI__ = {
  core: {
    invoke: async (command) => {
      if (command === "load_user_content") return payload;
      if (command === "core_catalog") return {};
      return null;
    },
  },
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const { THEMES } = await import("../apps/desktop/ui/js/themes.js");
const { loadUserContent, skippedMessage } = await import("../apps/desktop/ui/js/usercontent.js");
await i18n.initI18n();
const builtinBanner = i18n.t("serve.banner");
const content = await loadUserContent();
const refusals = content.errors.length;

// The theme: registered from the core's entry, inserted as the exact canonical text.
const ocean = THEMES.find((x) => x.id === "ocean");
assert.deepEqual({ ...ocean }, { id: "ocean", label: "Ocean", group: "custom", modes: ["dark", "light"], user: true });
assert.equal(styles.length, 1);
assert.equal(styles[0].textContent, oceanCss);
assert.equal(styles[0].dataset.userTheme, "ocean");

// The pack: ordinary text replaced, the protected banner keeps the built-in words.
await i18n.setLocale("fr");
assert.equal(i18n.t("tray.ok"), "Tout fonctionne");
// R-01: the pack overlays the shipped French catalog key by key. A text it does not set (or may not set: the core
// dropped the protected ids) reads the shipped French translation, never English or the raw key.
const shippedFr = JSON.parse(await readFile(new URL("../apps/desktop/ui/locales/fr.json", import.meta.url), "utf8"));
assert.equal(i18n.t("serve.banner"), shippedFr["serve.banner"]);
assert.notEqual(i18n.t("serve.banner"), builtinBanner);
assert.equal(i18n.t("confirm.cancel"), shippedFr["confirm.cancel"]);
assert.equal(i18n.t("settings.page.general"), shippedFr["settings.page.general"]);
assert.equal(i18n.machineTranslated(), shippedFr._meta.reviewed === false, "the shipped _meta is read under a pack");

// Refusals carry the parser's reason, each with a translated message in every catalog.
assert.deepEqual(content.errors.map((e) => e.reason), ["externalUrl", "property", "protected", "invalidCode", "invalidCode",
  "invalidCode", "unreadable", "unreadable", "unreadable", "invalidMeta", "invalidMeta", "invalidKey", "invalidValue",
  "invalidValue"]);
assert.deepEqual(content.errors.slice(3, 6).map((e) => e.name), ["en-US-US.json", "en-a1.json", "xx-ab-cd-ef.json"]);
assert.ok(!i18n.hasLocale("en-US-US"), "a pack Intl cannot read is not offered");
for (const code of ["it", "es", "de", "nl", "pl", "ru", "tr"]) {
  assert.ok(!content.locales.some((l) => l.code === code), `${code}: a refused item registers nothing`);
}
assert.deepEqual(content.locales.map((l) => [l.code, l.label, l.ignored]), [["fr", "Français", 2]]);
// S-05: a tag Intl cannot read formats as en-US instead of throwing.
for (const code of ["en-US", "pt-BR", "fr", "sr-Latn-RS", "zh-Hant-TW"]) assert.ok(i18n.isIntlTag(code), code);
for (const code of ["en-US-US", "en-a1", "", "x"]) assert.ok(!i18n.isIntlTag(code), code);
assert.equal(i18n.intlTag("en-US-US"), "en-US");
const { intlFormatter } = await import("../apps/desktop/ui/js/lib.js");
assert.equal(intlFormatter("NumberFormat", "en-US-US", { maximumFractionDigits: 1 }).format(1234.5), "1,234.5");
await i18n.setLocale("en-US");
assert.match(skippedMessage(content.errors[1]), /not a theme token/);
const reasons = ["syntax", "atRule", "externalUrl", "function", "scope", "property", "value", "modes", "invalidMeta", "invalidId",
  "invalidCode", "invalidKey", "invalidValue", "invalidJson", "tooLarge", "unreadable"];
for (const code of ["en-US", "pt-BR", "es", "it", "fr", "de", "ja", "zh-CN", "ko", "hi-IN", "id-ID", "tr", "ru", "vi"]) {
  const catalog = JSON.parse(await readFile(new URL(`../apps/desktop/ui/locales/${code}.json`, import.meta.url), "utf8"));
  for (const reason of reasons) assert.ok(catalog[`userContent.reason.${reason}`], `${code}: userContent.reason.${reason}`);
}

// S-02: every case of the core's fixture. A pack the core loaded registers exactly the core's messages; a refused one
// registers nothing, and the same file handed over as text (a shell that skipped the core) is refused too.
const fixture = JSON.parse(await readFile(new URL("../crates/cuotai-core/tests/fixtures/user_pack/filtered.json", import.meta.url), "utf8"));
assert.ok(fixture.length >= 20 && fixture.some((c) => c.output.refused) && fixture.some((c) => !c.output.refused));
for (const { name, input, output } of fixture) {
  if (output.refused) {
    payload.themes = [];
    payload.errors = [{ kind: "locale", name, reason: output.refused }];
    payload.locales = [{ name, text: input }];
    const got = await loadUserContent();
    assert.deepEqual(got.locales, [], `${name}: a refused pack registers nothing`);
    assert.deepEqual(got.errors.map((e) => e.reason), [output.refused, "unreadable"], name);
    continue;
  }
  payload.themes = [];
  payload.errors = [];
  payload.locales = [{ name, ...output }];
  const got = await loadUserContent();
  assert.deepEqual(got.errors, [], `${name}: the webview accepts what the core accepted`);
  assert.equal(got.locales.length, 1, name);
  const [registered] = got.locales;
  assert.deepEqual(registered.messages, output.messages, `${name}: the registered catalog is the core's output`);
  assert.deepEqual([registered.code, registered.label, registered.flag, registered.digest],
    [output.code, output.label, output.flag, output.digest], name);
  for (const id of output.ignored) assert.ok(!Object.hasOwn(registered.messages, id), `${name}: ${id} was filtered`);
}
// The webview applies what it registered: the last fixture pack's texts are what `t` returns in that language.
const last = fixture.findLast((c) => !c.output.refused && Object.values(c.output.messages).some((v) => typeof v === "string" && v));
const [lastId, lastText] = Object.entries(last.output.messages).find(([, v]) => typeof v === "string" && v);
payload.locales = [{ name: last.name, ...last.output }];
await loadUserContent();
await i18n.setLocale(last.output.code);
assert.equal(i18n.t(lastId), lastText);
await i18n.setLocale("en-US");

// Static: the loader never parses a pack and never reads a locale item's text.
const source = await readFile(new URL("../apps/desktop/ui/js/usercontent.js", import.meta.url), "utf8");
assert.ok(!/JSON\.parse/.test(source), "usercontent.js must not parse a pack (JSON.parse)");
const addLocaleSource = source.slice(source.indexOf("function addLocale("), source.indexOf("export async function loadUserContent"));
assert.ok(addLocaleSource.length > 100, "addLocale found");
assert.ok(!/\.text\b|\btext\s*[,}]|\["text"\]/.test(addLocaleSource.replace(/Object\.hasOwn\(item, "text"\)/, "")),
  "addLocale must not read a locale item's text");

// L-02: only a two-letter string is an explicit flag.
const { flagCountry } = await import("../apps/desktop/ui/js/flags.js");
assert.equal(flagCountry("fr", ["de"]), "fr");
assert.equal(flagCountry("fr", { toLowerCase: () => "x" }), "fr");
assert.equal(flagCountry("fr", "DE"), "de");

// No remote request: only bundled catalogs were read.
assert.ok(requests.length > 0, "the bundled catalogs were read");
for (const url of requests) assert.ok(!/^(https?:)?\/\//i.test(url) && !/^(data|blob):/i.test(url), `remote request: ${url}`);
console.log(`user content: 1 theme, 1 pack, ${refusals} refusals, ${fixture.length} core fixture cases, ${requests.length} local requests`);
