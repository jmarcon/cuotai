export const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

/** Deixa `el` igual a `html` mexendo só no que mudou (PRD 0004 RF1.3): nós mantidos preservam foco,
 *  rolagem e transições CSS (ex.: `.bar > span` anima a largura nova). Filhos com `data-card` casam pela
 *  chave (criados, movidos ou removidos); os demais, por posição e tipo. */
export function patch(el, html) {
  const t = document.createElement("template");
  t.innerHTML = html;
  const focused = document.activeElement;
  patchChildren(el, t.content);
  // Mover um cartão (insertBefore) tira o foco no Chromium/WebKit; devolve sem rolar.
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
    else c.remove(); // remove antes de alinhar: os vizinhos não precisam ser movidos (mover tira o foco)
  }
  let cur = old.firstChild;
  for (const n of [...next.childNodes]) {
    const k = keyOf(n);
    let match = k != null ? keyed.get(k) : cur && keyOf(cur) == null && cur.nodeName === n.nodeName ? cur : null;
    if (!match && k == null && n.nodeType === 1) {
      // Um elemento sumiu antes deste (ex.: um aviso fechado): descarta os nós sem chave até o par, em vez de
      // recriar o resto (recriar o `<main>` perderia foco, barras e os cartões).
      let c = cur;
      while (c && keyOf(c) == null && c.nodeName !== n.nodeName) c = c.nextSibling;
      if (c && keyOf(c) == null) { while (cur !== c) { const nx = cur.nextSibling; cur.remove(); cur = nx; } match = c; }
    }
    if (k != null) keyed.delete(k); // chave repetida: o segundo cartão ganha um nó novo
    if (!match) { old.insertBefore(n, cur); continue; }
    patchNode(match, n);
    if (match === cur) cur = cur.nextSibling;
    else old.insertBefore(match, cur);
  }
  while (cur) { const nx = cur.nextSibling; cur.remove(); cur = nx; }
}
function patchNode(a, b) {
  if (a.nodeType !== 1) { if (a.nodeValue !== b.nodeValue) a.nodeValue = b.nodeValue; return; }
  for (const { name } of [...a.attributes]) if (!b.hasAttribute(name)) a.removeAttribute(name);
  for (const { name, value } of b.attributes) if (a.getAttribute(name) !== value) a.setAttribute(name, value);
  if (a instanceof HTMLInputElement && a.value !== b.value) a.value = b.value; // o atributo não muda o valor digitado
  patchChildren(a, b);
}
