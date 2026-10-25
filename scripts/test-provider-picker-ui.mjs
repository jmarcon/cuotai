#!/usr/bin/env node
// Browser-free UI smoke: the provider picker, the cards and the details
// render no integration-status label (Official, Undocumented, Derived,
// Unavailable) in any built-in locale; that transparency lives in
// docs/providers.md. The picker receives the core's `pickable` flag instead,
// and the details show no "verified N months ago" chip either.
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
const lib = await import("../apps/desktop/ui/js/lib.js");
await i18n.initI18n();

const LEVELS = ["official", "undocumented", "derived", "unavailable"];
const session = {
  level: { id: "access.credential.user_session", params: {}, fallback: "Sessão do usuário" },
  warning: { id: "user_session.warning", params: {}, fallback: "Sessão completa." },
};
const report = (level, withSession) => ({
  provider: "open_ai_api",
  account: "work",
  status: "ready",
  notes: [],
  notes_l10n: [],
  access: {
    level,
    label: { id: `access.level.${level}`, params: {}, fallback: level },
    summary: { id: "access.summary.open_ai_api", params: {}, fallback: "Resumo." },
    reason: null,
    explanation: null,
    session: withSession ? session : null,
    // PRD 0028 RF6.2 data the core still sends; the details never show it.
    verified_on: "2026-01-02",
    verification: { id: "access.verified_months", params: { months: 9 }, fallback: "Verificada há 9 meses." },
    verification_stale: true,
  },
});

const providers = await lib.listProviders();
assert.ok(providers.length > 0, "the demo lists providers");
for (const p of providers) {
  assert.equal(typeof p.pickable, "boolean", `${p.id}: pickable flag missing`);
  assert.equal(p.level, undefined, `${p.id}: the picker still receives an access level`);
}
assert.equal(providers.find((p) => p.id === "poolside")?.pickable, false, "a provider without any query is not pickable");
assert.equal(providers.find((p) => p.id === "perplexity")?.pickable, true, "a provider with one queried mode stays pickable");

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  const labels = LEVELS.map((level) => i18n.tl({ id: `access.level.${level}`, params: {}, fallback: level }));
  for (const level of LEVELS) {
    for (const withSession of [false, true]) {
      const r = report(level, withSession);
      // The details call it with `full`; the cards without. Neither renders status.
      for (const html of [lib.accessHtml(r), lib.accessHtml(r, { full: true })]) {
        assert.ok(!html.includes("data-verified"), `${code}/${level}: verified chip rendered`);
        assert.ok(!html.includes(i18n.tl(r.access.verification)), `${code}/${level}: verification text rendered`);
        assert.ok(!html.includes(i18n.tl(r.access.summary)), `${code}/${level}: access summary rendered`);
        assert.ok(!html.includes("2026-01-02"), `${code}/${level}: verification date rendered`);
      }
      const html = lib.accessHtml(r, { full: true });
      for (const label of labels) {
        assert.ok(!html.includes(label), `${code}/${level}: access label "${label}" rendered: ${html}`);
      }
      assert.ok(!html.includes("data-access="), `${code}/${level}: access line rendered`);
      assert.equal(html.includes("data-session-notice"), withSession, `${code}/${level}: session notice`);
    }
  }
}
console.log("provider picker UI smoke: ok");
