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

/** A stand-in for a container element whose markup a binding replaces with `innerHTML` and then wires through
 *  `querySelector`: enough to call the handlers a binding sets (`onclick`, `onchange`) without a browser. Selectors
 *  are one `#id`, `[attr]` or `.class`; each element keeps its handlers and state (`hidden`, `checked`, `value`,
 *  `textContent`, attributes) until the next `innerHTML`. `focused` is the last element `focus()` was called on. */
export function markupRoot() {
  let tree = parseHtml("");
  let html = "";
  const wrapped = new Map();
  const root = {
    focused: null,
    get innerHTML() { return html; },
    set innerHTML(next) { html = next; tree = parseHtml(next); wrapped.clear(); },
    querySelector(sel) { return wrap(tree.find(matcher(sel))); },
    querySelectorAll(sel) { return tree.all(matcher(sel)).map(wrap); },
    contains: () => false,
  };
  const matcher = (sel) => sel.startsWith("#") ? (n) => n.attrs.id === sel.slice(1)
    : sel.startsWith("[") ? hasAttr(sel.slice(1, -1)) : hasClass(sel.slice(1));
  const wrap = (node) => {
    if (!node) return null;
    if (wrapped.has(node)) return wrapped.get(node);
    const attrs = { ...node.attrs };
    const el = {
      node, onclick: null, onchange: null,
      hidden: "hidden" in attrs, checked: "checked" in attrs, disabled: "disabled" in attrs,
      value: node.tag === "textarea" ? node.text() : attrs.value ?? "", textContent: node.text(),
      getAttribute: (name) => attrs[name] ?? null,
      setAttribute: (name, value) => { attrs[name] = String(value); },
      hasAttribute: (name) => name in attrs,
      removeAttribute: (name) => { delete attrs[name]; },
      addEventListener: () => {},
      focus: () => { root.focused = el; },
      closest: () => null,
      querySelector: () => null,
      getClientRects: () => [],
    };
    wrapped.set(node, el);
    return el;
  };
  return root;
}
