// PRD 0004 RF2: efeitos do painel em volta da reconciliação de `patch` (RF1.3). `before` mede o painel antes
// do patch; `after` compara e anima só o que mudou (entrada, FLIP, saída, contagem, nível, esgotamento).
// Toda duração vem dos tokens de movimento: com "Reduzidas"/"Desligadas", `prefers-reduced-motion`, o CRT
// (sem interpolação) ou a janela escondida ela é 0 e o efeito não roda. Só Web Animations e rAF de vida
// curta: com o painel parado, `document.getAnimations()` fica vazio.

const rootStyle = () => getComputedStyle(document.documentElement);
/** Duração efetiva de um token de movimento, em ms; 0 = sem efeito. */
export function motionMs(token) {
  if (document.hidden) return 0;
  const v = rootStyle().getPropertyValue(token).trim();
  return parseFloat(v) * (v.endsWith("ms") ? 1 : v.endsWith("s") ? 1000 : 0) || 0;
}
// Temas do usuário podem trazer uma curva inválida; `animate()` lançaria, então cai para o padrão.
export const easeOut = (v = rootStyle().getPropertyValue("--ease-out").trim()) => (CSS.supports("transition-timing-function", v) ? v : "ease-out");

// O shimmer (CSS) para com a janela escondida: `:root[data-hidden]` desliga a animação.
const markHidden = () => document.documentElement.toggleAttribute("data-hidden", document.hidden);
markHidden();
document.addEventListener("visibilitychange", markHidden);

const RANK = { good: 1, warn: 2, bad: 3 };
/** Pior nível entre as porcentagens do cartão ("" sem nenhuma). */
function worst(card) {
  let lv = "";
  for (const e of card.querySelectorAll(LIVE_PCT)) for (const c of e.classList) if (RANK[c] > (RANK[lv] ?? 0)) lv = c;
  return lv;
}

// PRD 0010 RF3.2: a preserved (stale) reading is still: no count-up and no level flash.
const LIVE_PCT = "[data-pct]:not(.stale-reading *)";
const counting = new Map(); // contagem em andamento: elemento → id do rAF
function countUp(el, from, to, ms) {
  const text = el.firstChild; // "42%" (o `<small>` do resumo vem depois)
  const t0 = performance.now();
  const step = (now) => {
    const k = Math.min(1, Math.max(0, now - t0) / ms);
    text.nodeValue = `${Math.round(from + (to - from) * (1 - (1 - k) ** 3))}%`;
    if (k < 1) counting.set(el, requestAnimationFrame(step));
    else counting.delete(el);
  };
  counting.set(el, requestAnimationFrame(step));
}

/** Antes do `patch`: interrompe contagens (a próxima parte do número exibido) e mede cada cartão. */
export function before(root) {
  for (const id of counting.values()) cancelAnimationFrame(id);
  counting.clear();
  if (!motionMs("--motion-base") && !motionMs("--motion-slow")) return null;
  const cards = new Map();
  for (const el of root.querySelectorAll("[data-card]")) {
    cards.set(el.dataset.card, { el, rect: el.getBoundingClientRect(), lv: worst(el), out: el.classList.contains("exhausted") });
  }
  return { cards, pcts: new Map([...root.querySelectorAll(LIVE_PCT)].map((e) => [e, parseFloat(e.textContent)])) };
}

/** Depois do `patch`: anima a diferença em relação a `before`. */
export function after(root, prev) {
  if (!prev) return;
  const base = motionMs("--motion-base"), slow = motionMs("--motion-slow"), easing = easeOut();
  const now = [...root.querySelectorAll("[data-card]")];
  for (const el of now) for (const a of el.getAnimations()) if (a.id === "flip") a.cancel(); // mede sem o FLIP anterior
  const rects = now.map((el) => el.getBoundingClientRect());

  // Entrada: só cartões novos, em cascata de 30 ms com teto de 300 ms no total.
  const fresh = now.filter((el) => prev.cards.get(el.dataset.card)?.el !== el);
  const gap = Math.min(30, 300 / fresh.length);
  if (base) fresh.forEach((el, i) => el.animate([{ opacity: 0, transform: "translateY(8px)" }, { opacity: 1, transform: "none" }],
    { duration: base, delay: i * gap, easing, fill: "backwards" }));

  now.forEach((el, i) => {
    const old = prev.cards.get(el.dataset.card);
    if (old?.el !== el) return;
    // FLIP: o cartão volta à posição antiga com `transform` e desliza até a nova (arrastar, setas, ordenação).
    const dx = old.rect.left - rects[i].left, dy = old.rect.top - rects[i].top;
    if (base && Math.abs(dx) + Math.abs(dy) > 1) {
      el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { id: "flip", duration: base, easing });
    }
    // Mudança de nível: um destaque único (600 ms com os tokens padrão) na cor do nível novo.
    const lv = worst(el);
    if (slow && old.lv && lv && lv !== old.lv) {
      const color = rootStyle().getPropertyValue(`--${lv}`).trim();
      el.animate([{ outline: `2px solid ${color}`, outlineOffset: "2px" }, { outline: "2px solid transparent", outlineOffset: "2px" }],
        { duration: slow * 1.5, easing });
    }
    // Esgotou agora (não a cada refresh): um ciclo de "shake".
    if (slow && !old.out && el.classList.contains("exhausted")) {
      el.animate([{ transform: "none" }, { transform: "translateX(-4px)" }, { transform: "translateX(4px)" }, { transform: "none" }],
        { duration: slow, easing: "ease-in-out" });
    }
  });

  // Saída: o nó que o patch tirou do DOM volta como fantasma inerte, fixo onde estava, e encolhe até sumir.
  if (base) {
    for (const { el, rect } of prev.cards.values()) {
      if (el.isConnected || el.classList.contains("skeleton")) continue;
      Object.assign(el.style, { position: "fixed", left: `${rect.left}px`, top: `${rect.top}px`, width: `${rect.width}px`,
        height: `${rect.height}px`, margin: "0", zIndex: "1", pointerEvents: "none" });
      el.inert = true;
      el.removeAttribute("data-card"); // o fantasma não é mais um cartão (seletores e chaves do painel o ignoram)
      el.setAttribute("aria-hidden", "true");
      document.body.append(el);
      const a = el.animate([{ opacity: 1, transform: "scale(1)" }, { opacity: 0, transform: "scale(0.92)" }], { duration: base, easing, fill: "forwards" });
      a.onfinish = a.oncancel = () => el.remove();
    }
  }

  // Porcentagens: contam do valor exibido até o novo enquanto a barra (CSS) transita.
  if (slow) {
    for (const e of root.querySelectorAll(LIVE_PCT)) {
      const from = prev.pcts.get(e), to = Number(e.dataset.pct);
      if (Number.isFinite(from) && from !== to) countUp(e, from, to, slow);
    }
  }
}

/** Uma volta no ícone de atualizar ao clicar. */
export function spinOnce(el) {
  const ms = motionMs("--motion-spin");
  if (el && ms) el.animate([{ transform: "rotate(0deg)" }, { transform: "rotate(360deg)" }], { duration: ms, easing: easeOut() });
}

const pulsed = new WeakMap(); // chip → instante que já pulsou
/** O chip da próxima consulta pulsa uma vez quando chega a zero. */
export function pulseIfDue(chip, at) {
  if (at > Date.now() || pulsed.get(chip) === at) return;
  pulsed.set(chip, at);
  const ms = motionMs("--motion-base");
  if (ms) chip.animate([{ transform: "scale(1)" }, { transform: "scale(1.15)" }, { transform: "scale(1)" }], { duration: ms * 2, easing: easeOut() });
}

/** RF3.1: abre `dlg` (já no DOM) como modal com fade + escala 0,96 → 1 e o backdrop esmaecendo; `from` (o cartão
 *  de origem) faz o diálogo crescer a partir dele. Qualquer fechamento (`close()`, `Esc`, `form method=dialog`)
 *  devolve o foco ao elemento que o abriu na hora (o WebKit do sistema não o restaura sozinho); a saída inversa toca
 *  no mesmo elemento, inerte, de volta à camada superior como popover manual. No fim (ou na hora, sem movimento)
 *  ele sai do DOM. */
export function showDialog(dlg, from = null) {
  const opener = document.activeElement;
  dlg.showModal();
  const frames = () => {
    if (!from?.isConnected) return [{ opacity: 0, transform: "scale(0.96)" }, { opacity: 1, transform: "none" }];
    const r = from.getBoundingClientRect(), d = dlg.getBoundingClientRect();
    const dx = r.left + r.width / 2 - (d.left + d.width / 2), dy = r.top + r.height / 2 - (d.top + d.height / 2);
    return [{ opacity: 0, transform: `translate(${dx}px, ${dy}px) scale(${Math.min(0.96, r.width / d.width)})` }, { opacity: 1, transform: "none" }];
  };
  const play = (duration, direction, fill) => {
    const o = { duration, direction, fill, easing: easeOut() };
    try { dlg.animate([{ opacity: 0 }, { opacity: 1 }], { ...o, pseudoElement: "::backdrop" }); } catch {} // WebKit antigo: só o diálogo anima
    return dlg.animate(frames(), o);
  };
  const ms = motionMs("--motion-base");
  if (ms) play(ms, "normal", "none");
  // Microtarefa: roda depois de todo caminho que tira `open`, antes do próximo quadro (sem piscar).
  new MutationObserver((_, mo) => {
    if (dlg.open) return;
    mo.disconnect();
    // Foco certo: o que já estiver fora do diálogo (WebKit às vezes já devolve, ou um handler abriu outro diálogo);
    // senão, quem abriu.
    const a0 = document.activeElement;
    const target = !a0 || a0 === document.body || dlg.contains(a0) ? opener : a0;
    const out = motionMs("--motion-fast");
    let exiting = false;
    if (out && dlg.isConnected) {
      dlg.inert = true;
      try { dlg.popover = "manual"; dlg.showPopover(); exiting = true; } catch {}
    }
    // Depois do `showPopover`: no WebKit ele tira o foco da página.
    if (target?.isConnected && target !== document.body && document.activeElement !== target) target.focus({ preventScroll: true });
    if (!exiting) return dlg.remove();
    const a = play(out, "reverse", "forwards");
    a.onfinish = a.oncancel = () => dlg.remove();
  }).observe(dlg, { attributeFilter: ["open"] });
}

/** RF3.3: troca de tema/idioma com cross-fade (View Transitions, `--motion-base`) quando há suporte e movimento;
 *  senão, na hora. Devolve o fim da atualização. */
export function crossFade(update) {
  if (!document.startViewTransition || !motionMs("--motion-base")) return Promise.resolve(update());
  return document.startViewTransition(update).updateCallbackDone;
}
