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

const VOID = new Set(["area", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "wbr"]);
const ENTITIES = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'", apos: "'", nbsp: " " };
const decode = (text) => text.replace(/&(#?\w+);/g, (m, e) => ENTITIES[e] ?? m);

/** Parses the HTML a render function returns into a small tree (`tag`, `attrs`, `children`, `parent`), so a test
 *  asserts elements, attributes and their nesting instead of matching the markup as a string. Enough for the UI's
 *  own markup (quoted attributes, void elements, no scripts). `all(pred)` walks the tree in document order,
 *  `find(pred)` returns the first match, `text()` the decoded text and `cls` the class list. */
export function parseHtml(html) {
  const node = (tag, attrs, parent) => ({ tag, attrs, parent, children: [],
    get cls() { return (this.attrs.class ?? "").split(/\s+/).filter(Boolean); },
    text() { return this.children.map((c) => (typeof c === "string" ? c : c.text())).join(""); },
    all(pred) { return this.children.flatMap((c) => (typeof c === "string" ? [] : [...(pred(c) ? [c] : []), ...c.all(pred)])); },
    find(pred) { return this.all(pred)[0] ?? null; },
    closest(pred) { let n = this; while (n && !pred(n)) n = n.parent; return n ?? null; } });
  const root = node("#root", {}, null);
  let current = root;
  for (const [, close, tag, attrs, text] of html.matchAll(/<(\/?)([a-zA-Z][\w-]*)((?:\s+[\w:-]+(?:="[^"]*")?)*)\s*\/?>|([^<]+)/g)) {
    if (text != null) { current.children.push(decode(text)); continue; }
    if (close) {
      let n = current;
      while (n !== root && n.tag !== tag.toLowerCase()) n = n.parent;
      if (n !== root) current = n.parent;
      continue;
    }
    const parsed = Object.fromEntries([...attrs.matchAll(/([\w:-]+)(?:="([^"]*)")?/g)].map(([, k, v]) => [k, decode(v ?? "")]));
    const child = node(tag.toLowerCase(), parsed, current);
    current.children.push(child);
    if (!VOID.has(child.tag)) current = child;
  }
  return root;
}
/** Predicates for `parseHtml` trees: an element carrying attribute `name`, or class `c`. */
export const hasAttr = (name) => (n) => name in n.attrs;
export const hasClass = (c) => (n) => n.cls.includes(c);
