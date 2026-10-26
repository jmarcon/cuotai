// PRD 0035 RF10.5: letters that do not decompose under NFD.
const FOLD_EXTRA = { "đ": "d", "Đ": "d", "ı": "i", "ß": "ss", "ẞ": "ss", "ł": "l", "Ł": "l" };
/** Search folding (the twin of `cuotai_core::text::fold`): NFD, strip combining marks, map đ ı ß ł, lower-case. CJK,
 *  Hangul and Devanagari stay as typed (recomposed to NFC). `dong` finds `Đồng`, `istanbul` finds `İstanbul`. */
export const fold = (s) => String(s ?? "").normalize("NFD").replace(/[\u0300-\u036f]/g, "")
  .replace(/[đĐıßẞłŁ]/g, (c) => FOLD_EXTRA[c]).toLowerCase().normalize("NFC");

export const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

/** Makes `el` equal to `html` touching only what changed (PRD 0004 RF1.3): kept nodes preserve focus,
 *  scroll and CSS transitions (e.g. `.bar > span` animates the new width). Children with `data-card` match by
 *  key (created, moved or removed); the others, by position and type. */
export function patch(el, html) {
  const t = document.createElement("template");
  t.innerHTML = html;
  const focused = document.activeElement;
  patchChildren(el, t.content);
  // Moving a card (insertBefore) drops focus in Chromium/WebKit; it gives focus back without scrolling.
  if (focused !== document.activeElement && focused?.isConnected) focused.focus({ preventScroll: true });
}
const keyOf = (n) => (n.nodeType === 1 ? n.getAttribute("data-card") : null);
function patchChildren(old, next) {
  const nextKeys = new Set([...next.childNodes].map(keyOf).filter((k) => k != null));
  const keyed = new Map();
  for (const c of [...old.childNodes]) {
    const k = keyOf(c);
    if (k == null) continue;
    if (nextKeys.has(k)) keyed.set(k, c);
    else c.remove(); // remove before aligning: the neighbors do not need to be moved (moving drops focus)
  }
  let cur = old.firstChild;
  for (const n of [...next.childNodes]) {
    const k = keyOf(n);
    let match = k != null ? keyed.get(k) : cur && keyOf(cur) == null && cur.nodeName === n.nodeName ? cur : null;
    if (!match && k == null && n.nodeType === 1) {
      // An element vanished before this one (e.g. a dismissed notice): discard the unkeyed nodes up to the pair, instead of
      // recreating the rest (recreating `<main>` would lose focus, bars and the cards).
      let c = cur;
      while (c && keyOf(c) == null && c.nodeName !== n.nodeName) c = c.nextSibling;
      if (c && keyOf(c) == null) { while (cur !== c) { const nx = cur.nextSibling; cur.remove(); cur = nx; } match = c; }
    }
    if (k != null) keyed.delete(k); // duplicate key: the second card gets a new node
    if (!match) { old.insertBefore(n, cur); continue; }
    // A card (keyed node) identical to the new one has nothing to fix: the native comparison saves walking the
    // subtree in JS on every redraw (cards have no text fields, whose typed value the attribute does not reflect).
    if (k == null || !match.isEqualNode(n)) patchNode(match, n);
    if (match === cur) cur = cur.nextSibling;
    else old.insertBefore(match, cur);
  }
  while (cur) { const nx = cur.nextSibling; cur.remove(); cur = nx; }
}
function patchNode(a, b) {
  if (a.nodeType !== 1) { if (a.nodeValue !== b.nodeValue) a.nodeValue = b.nodeValue; return; }
  for (const { name } of [...a.attributes]) if (!b.hasAttribute(name)) a.removeAttribute(name);
  for (const { name, value } of b.attributes) if (a.getAttribute(name) !== value) a.setAttribute(name, value);
  if (a instanceof HTMLInputElement && a.value !== b.value) a.value = b.value; // the attribute does not change the typed value
  patchChildren(a, b);
}

/** Prefix of the interface's own files (catalogs, images) relative to the page: empty in the app windows and the
 *  demo, set by a dev-only page served from elsewhere with `<meta name="cuotai-ui-base">` (a `<base>` element is
 *  refused by the app CSP's `base-uri 'none'`). */
const UI_BASE = globalThis.document?.querySelector?.('meta[name="cuotai-ui-base"]')?.getAttribute("content") ?? "";
export const uiPath = (path) => `${UI_BASE}${path}`;
