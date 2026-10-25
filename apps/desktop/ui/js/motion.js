// Motion (PRD 0004, reduced by PRD 0022 RF12 to docs/design-system.md section 7). Motion only shows the continuity
// of a direct user action: a dialog fading in, a card the user moved settling in its new place, the theme or
// language cross-fade. Data changes never animate (no count-up, flash, shake, entrance, exit ghost or pulse); a
// changed value gets the static marker of components.js instead. Every duration comes from the motion tokens: with
// "Reduced"/"Off", `prefers-reduced-motion`, a theme that sets 0 or a hidden window it is 0 and nothing runs. Only
// short-lived Web Animations: an idle window has `document.getAnimations().length === 0` (DS-MOT-9).

const rootStyle = () => getComputedStyle(document.documentElement);
/** Effective duration of a motion token, in ms; 0 = no effect. */
export function motionMs(token) {
  if (document.hidden) return 0;
  const v = rootStyle().getPropertyValue(token).trim();
  return parseFloat(v) * (v.endsWith("ms") ? 1 : v.endsWith("s") ? 1000 : 0) || 0;
}
// User themes may bring an invalid curve; `animate()` would throw, so it falls back to the default.
export const easeOut = (v = rootStyle().getPropertyValue("--ease-out").trim()) => (CSS.supports("transition-timing-function", v) ? v : "ease-out");

// The hidden window runs no CSS animation either (`:root[data-hidden]`), the refresh spinner included.
const markHidden = () => document.documentElement.toggleAttribute("data-hidden", document.hidden);
markHidden();
document.addEventListener("visibilitychange", markHidden);

/** Before a user-initiated move (drag or Alt+arrows, DS §7.2 item 16): the cards' positions. */
export function measure(root) {
  if (!motionMs("--motion-base")) return null;
  return new Map([...root.querySelectorAll("[data-card]")].map((el) => [el.dataset.card, el.getBoundingClientRect()]));
}
/** After the move: each card slides from its old place to the new one (opacity is untouched; never after a
 *  refresh or a sort). */
export function flip(root, before) {
  if (!before) return;
  const ms = motionMs("--motion-base"), easing = easeOut();
  for (const el of root.querySelectorAll("[data-card]")) {
    const old = before.get(el.dataset.card);
    if (!old) continue;
    const now = el.getBoundingClientRect();
    const dx = old.left - now.left, dy = old.top - now.top;
    if (Math.abs(dx) + Math.abs(dy) > 1) el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: ms, easing });
  }
}

/** Opens `dlg` (already in the DOM) as a modal with an opacity fade of `--motion-base` (no scale, no grow, DS §7.2
 *  item 20). Closing is instant. Whatever closes it (`close()`, `Esc`, `form method=dialog`), focus returns at once to
 *  the element that opened it (the system WebKit does not restore it), and the dialog leaves the DOM. */
export function showDialog(dlg) {
  const opener = document.activeElement;
  dlg.showModal();
  const ms = motionMs("--motion-base");
  if (ms) dlg.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ms, easing: easeOut() });
  // Microtask: runs after every path that removes `open`, before the next frame (no flash).
  new MutationObserver((_, mo) => {
    if (dlg.open) return;
    mo.disconnect();
    // The right focus: what is already outside the dialog (WebKit sometimes restores it, or a handler opened another
    // dialog); otherwise whoever opened it.
    const a0 = document.activeElement;
    const target = !a0 || a0 === document.body || dlg.contains(a0) ? opener : a0;
    if (target?.isConnected && target !== document.body && document.activeElement !== target) target.focus({ preventScroll: true });
    dlg.remove();
  }).observe(dlg, { attributeFilter: ["open"] });
}

/** RF3.3: theme/language switch with a cross-fade (View Transitions, `--motion-fast`) when supported and motion is
 *  on; otherwise at once. Resolves when the update is done. */
export function crossFade(update) {
  if (!document.startViewTransition || !motionMs("--motion-fast")) return Promise.resolve(update());
  return document.startViewTransition(update).updateCallbackDone;
}
