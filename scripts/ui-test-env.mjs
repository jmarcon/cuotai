// Shared browser-free environment for the PRD 0022 UI tests: just enough of `window`, `document`, storage and
// `fetch` (reading the real catalogs from apps/desktop/ui) to import the production modules in Node.
// No browser, package manager or network is involved.
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const repository = new URL("../", import.meta.url);

/** Installs the globals; `locale` is stored before the modules load. Returns the storage map. */
export function installUiEnv({ locale = "en-US", storage = new Map() } = {}) {
  storage.set("cuotai.locale", locale);
  globalThis.window = {};
  globalThis.document = {
    hidden: true,
    activeElement: null,
    body: {},
    documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
    addEventListener: () => {},
    querySelector: () => null,
    querySelectorAll: () => [],
  };
  globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
  globalThis.addEventListener = () => {};
  Object.defineProperty(globalThis, "navigator", { value: { languages: [locale], language: locale }, configurable: true });
  globalThis.location = { search: "" };
  globalThis.localStorage = {
    getItem: (key) => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, String(value)),
    removeItem: (key) => storage.delete(key),
  };
  globalThis.fetch = async (relative) => {
    try {
      const path = new URL(`apps/desktop/ui/${relative}`, repository);
      const data = JSON.parse(await readFile(fileURLToPath(path), "utf8"));
      return { ok: true, json: async () => data };
    } catch {
      return { ok: false, json: async () => ({}) };
    }
  };
  return storage;
}

/** A ready report with one capped metric, in the shape of the core's view. */
export function report({ provider = "claude", account = "work@example.com", index = 0, left = 50, pace = null, status = "ready",
  freshness = "fresh", lastGood = null, exhausted = false } = {}) {
  const used = exhausted ? 100 : 100 - left;
  const metric = {
    label: "weekly", used, limit: 100, remaining: 100 - used, unit: "%", kind: "quota", window: "weekly", period: null,
    resets_at: new Date(Date.now() + 5 * 3600e3).toISOString(), state: exhausted ? "exhausted" : left == null ? "unmeasured" : "available",
    ratio: left == null ? null : exhausted ? 0 : left / 100, uncapped: false, no_cap: false, pace,
  };
  return {
    provider, account, index, status, checked_at: new Date().toISOString(), freshness, clock_adjusted: false, last_good: lastGood,
    metrics: status === "ready" && left != null ? [metric] : [], notes: [], class: exhausted ? "exhausted" : "available",
    binding: status === "ready" && left != null ? { index: 0, used_ratio: used / 100, window: "weekly", label: "weekly", reason: exhausted ? "exhausted" : "tightest", blocked_until: null } : null,
    identity: { email: null, badges: [], copy: null },
  };
}
