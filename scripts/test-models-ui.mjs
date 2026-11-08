#!/usr/bin/env node
// Browser-free UI smoke for PRD 0002 RF6 (and its 2026-11-08 re-validation, V-04 to V-07): the desktop Models
// section's view of every core state, in en-US, pt-BR and es (the RF6 acceptance languages). It checks the status
// line, its DS-COL-3 role and icon, the meta line built from whole translated keys (DS-MSG-12), the shared DS-MSG-8
// date, the search filter, and that the sync bar is hidden for an unsupported account. No backend is called.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "en-US" });
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const { modelsView } = await import("../apps/desktop/ui/js/detail.js");
const { fmtAt } = await import("../apps/desktop/ui/js/components.js");

const fixed = (id, fallback, params) => ({ id, fallback, ...(params ? { params } : {}) });
const at = new Date(Date.now() - 3 * 864e5).toISOString();
const catalog = {
  models: [
    { id: "llama-3.3-70b", name: "Llama 3.3 70B" },
    { id: "gemma2-9b-it", name: null },
    { id: "whisper-large-v3", name: "Whisper" },
  ],
  source: fixed("models.source.groq", "Groq GET /openai/v1/models with the account key."),
  fetched_at: at,
};
const states = {
  notFetched: { state: "not_fetched" },
  ready: { state: "ready", ...catalog },
  empty: { state: "ready", ...catalog, models: [] },
  failedPrev: { state: "failed", status: "authentication_error", error: fixed("query.http_status", "Query returned HTTP 401."), at, previous: catalog },
  failedNone: { state: "failed", status: "error", error: fixed("query.transport", "Network failure or query timeout."), at, previous: null },
  rateLimited: { state: "failed", status: "rate_limited", error: fixed("query.rate_limited", "Rate limited."), at, previous: catalog },
  interrupted: { state: "interrupted", note: fixed("models.interrupted", "The last model query was interrupted."), at, previous: catalog },
  unsupported: { state: "unsupported", reason: fixed("models.unsupported", "ElevenLabs: there is no model catalog. No query is made.") },
};

// Per language: words that must appear in the attempt and meta lines (whole keys, never English in pt-BR/es).
const words = {
  "en-US": { attempt: "attempted", source: "Source:", fetched: "fetched", of: "of" },
  "pt-BR": { attempt: "tentativa:", source: "Fonte:", fetched: "obtido:", of: "de" },
  es: { attempt: "intento:", source: "Fuente:", fetched: "obtenido:", of: "de" },
};

for (const [code, w] of Object.entries(words)) {
  await i18n.setLocale(code);
  const when = fmtAt(at);
  assert.ok(when, "fixture date formats");
  const view = (name, opts) => modelsView(states[name], opts);
  const noPlaceholder = (text, what) => assert.ok(!/[{}]/.test(text), `${code} ${what}: raw placeholder in "${text}"`);

  // Not fetched and ready: neutral status, no role.
  const nf = view("notFetched");
  assert.equal(nf.status, i18n.t("models.notFetched"));
  assert.equal(nf.tone, null);
  assert.equal(nf.meta, "");
  assert.equal(nf.searchHidden, true);
  assert.equal(nf.barHidden, false, "sync is offered before the first query");

  const ready = view("ready");
  assert.equal(ready.status, i18n.t("models.count", { count: 3 }));
  assert.equal(ready.tone, null);
  assert.equal(ready.meta, `${w.source} ${i18n.tl(catalog.source)} · ${w.fetched} ${when}`, `${code} meta`);
  assert.ok(ready.meta.includes(when), "DS-MSG-8 date in the meta line");
  assert.equal(ready.shown.length, 3);
  assert.equal(ready.searchHidden, false);
  assert.equal(ready.barHidden, false);

  // Empty catalog: dated, not an error.
  const empty = view("empty");
  assert.equal(empty.status, i18n.t("models.empty"));
  assert.equal(empty.tone, null);
  assert.ok(empty.meta.includes(w.source));
  assert.equal(empty.searchHidden, true);

  // Failed with a preserved catalog: role, icon and the whole translated line; the list stays.
  const failed = view("failedPrev");
  assert.deepEqual(failed.tone, ["accent", "key"], "an authentication failure takes the accent role");
  assert.ok(failed.status.includes(i18n.tl(states.failedPrev.error)), `${code} failed error text`);
  assert.ok(failed.status.includes(`${w.attempt} ${when}`), `${code} failed attempt: ${failed.status}`);
  noPlaceholder(failed.status, "failed");
  assert.equal(failed.shown.length, 3, "a failure keeps the previous catalog");
  assert.ok(failed.meta.includes(w.fetched), "the preserved catalog keeps its date");

  const failedNone = view("failedNone");
  assert.deepEqual(failedNone.tone, ["bad", "alert"], "a query error takes the bad role");
  assert.equal(failedNone.meta, "");
  assert.equal(failedNone.shown.length, 0);
  assert.deepEqual(view("rateLimited").tone, ["warn", "clock"], "rate limited takes the warn role");

  const interrupted = view("interrupted");
  assert.deepEqual(interrupted.tone, ["warn", "clock"]);
  assert.ok(interrupted.status.includes(`${w.attempt} ${when}`), `${code} interrupted: ${interrupted.status}`);
  noPlaceholder(interrupted.status, "interrupted");

  // Unsupported: the reason only, no sync bar.
  const unsupported = view("unsupported");
  assert.equal(unsupported.barHidden, true, "no sync for an account that cannot list models");
  assert.equal(unsupported.tone, null);
  assert.equal(unsupported.meta, "");

  // Search: folded, local, with the filtered meta key; no match says so.
  const search = view("ready", { query: "LLAMA" });
  assert.deepEqual(search.shown.map((m) => m.id), ["llama-3.3-70b"]);
  assert.ok(search.meta.endsWith(i18n.t("models.meta.filtered", { source: "", when, shown: 1, count: 3 }).split(" · ").pop()));
  assert.ok(search.meta.includes(w.of), `${code} filtered meta: ${search.meta}`);
  const byName = view("ready", { query: "whisp" });
  assert.deepEqual(byName.shown.map((m) => m.id), ["whisper-large-v3"]);
  const none = view("ready", { query: "zzz" });
  assert.equal(none.noMatch, true);
  assert.equal(view("empty", { query: "zzz" }).noMatch, false, "an empty catalog is not a failed search");

  // An IPC error (a storage failure) is shown with the bad role, over the catalog still on screen.
  const storage = view("ready", { error: "Could not save" });
  assert.equal(storage.status, "Could not save");
  assert.deepEqual(storage.tone, ["bad", "alert"]);
  assert.equal(storage.shown.length, 3);

  // Running query and a vanished account.
  const running = view("ready", { progress: i18n.t("models.syncing") });
  assert.equal(running.status, i18n.t("models.syncing"));
  assert.equal(running.tone, null);
  const gone = view("ready", { gone: true });
  assert.equal(gone.status, i18n.t("models.gone"));
  assert.equal(gone.barHidden, true);
  assert.equal(gone.shown.length, 0);

  console.log(`${code}: ${failed.status} | ${ready.meta}`);
}

// Every language has the whole-line keys, with the placeholders the code fills.
const { readdir, readFile } = await import("node:fs/promises");
const { fileURLToPath } = await import("node:url");
const dir = fileURLToPath(new URL("../apps/desktop/ui/locales/", import.meta.url));
for (const file of (await readdir(dir)).filter((f) => f.endsWith(".json"))) {
  const catalogFile = JSON.parse(await readFile(`${dir}${file}`, "utf8"));
  const need = {
    "models.state.failed": ["{status}", "{error}", "{when}"],
    "models.state.interrupted": ["{note}", "{when}"],
    "models.meta": ["{source}", "{when}"],
    "models.meta.filtered": ["{source}", "{when}", "{shown}", "{count}"],
  };
  for (const [key, vars] of Object.entries(need)) {
    assert.equal(typeof catalogFile[key], "string", `${file}: ${key} missing`);
    for (const v of vars) assert.ok(catalogFile[key].includes(v), `${file}: ${key} lacks ${v}`);
  }
  for (const gone of ["models.attemptAt", "models.fetchedAt", "models.shown"]) {
    assert.equal(catalogFile[gone], undefined, `${file}: ${gone} is no longer used`);
  }
}
console.log("PASS: desktop Models section states, roles, dates and search in en-US, pt-BR and es (PRD 0002 RF6)");
