#!/usr/bin/env node
// PRD 0050 RF3 acceptance (theme half): the desktop loader registers a theme the core already parsed and a language
// pack the shell already stripped of its protected ids, inserts only the canonical CSS it received, keeps the built-in
// security wording, reports refused files with the parser's reason, and never requests a remote resource.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "en-US" });
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
// `user_pack::strip_protected`: canonical theme CSS, a pack without protected ids, and the refusals.
const oceanCss = ':root[data-theme="ocean"][data-mode="dark"] {\n  color-scheme: dark;\n  --bg: #0b1a24;\n}\n'
  + ':root[data-theme="ocean"][data-mode="light"] {\n  color-scheme: light;\n  --bg: #eef6fb;\n}\n';
const payload = {
  dir: "/tmp/cuotai",
  themes: [{ name: "ocean.css", id: "ocean", label: "Ocean", group: "custom", modes: ["dark", "light"], text: oceanCss }],
  locales: [{ name: "fr.json", text: JSON.stringify({ _meta: { name: "Français" }, "tray.ok": "Tout fonctionne" }) }],
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

// The theme: registered from the core's entry, inserted as the exact canonical text.
const ocean = THEMES.find((x) => x.id === "ocean");
assert.deepEqual({ ...ocean }, { id: "ocean", label: "Ocean", group: "custom", modes: ["dark", "light"], user: true });
assert.equal(styles.length, 1);
assert.equal(styles[0].textContent, oceanCss);
assert.equal(styles[0].dataset.userTheme, "ocean");

// The pack: ordinary text replaced, the protected banner keeps the built-in words.
await i18n.setLocale("fr");
assert.equal(i18n.t("tray.ok"), "Tout fonctionne");
assert.equal(i18n.t("serve.banner"), builtinBanner);

// Refusals carry the parser's reason, each with a translated message in every catalog.
assert.deepEqual(content.errors.map((e) => e.reason), ["externalUrl", "property", "protected"]);
await i18n.setLocale("en-US");
assert.match(skippedMessage(content.errors[1]), /not a theme token/);
const reasons = ["syntax", "atRule", "externalUrl", "function", "scope", "property", "value", "modes", "invalidMeta", "invalidId"];
for (const code of ["en-US", "pt-BR", "es", "it", "fr", "de", "ja", "zh-CN", "ko", "hi-IN", "id-ID", "tr", "ru", "vi"]) {
  const catalog = JSON.parse(await readFile(new URL(`../apps/desktop/ui/locales/${code}.json`, import.meta.url), "utf8"));
  for (const reason of reasons) assert.ok(catalog[`userContent.reason.${reason}`], `${code}: userContent.reason.${reason}`);
}

// No remote request: only bundled catalogs were read.
assert.ok(requests.length > 0, "the bundled catalogs were read");
for (const url of requests) assert.ok(!/^(https?:)?\/\//i.test(url) && !/^(data|blob):/i.test(url), `remote request: ${url}`);
console.log(`user content: 1 theme, 1 pack, ${content.errors.length} refusals, ${requests.length} local requests`);
