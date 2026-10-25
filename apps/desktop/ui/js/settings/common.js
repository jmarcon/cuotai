// Shared by the Settings pages (PRD 0020 RF10.4): group wrapper, error texts, the confirmation dialog and small
// formatting helpers. Business rules stay in the core and the shell commands.
import { t, te, tl } from "../i18n.js";
import { fmtNumber } from "../lib.js";
import { showDialog } from "../motion.js";

/** RF2.2: one group of a page (a small-caps label over a card). `id` is the section id, also the anchor
 *  (`#set-<id>`) a link can land on (RF1.3); `attrs` keeps the data attributes the bindings look for. */
export const group = (id, title, body, attrs = "") =>
  `<section class="set-sec" id="set-${id}" data-section="${id}" ${attrs}>${title ? `<h3>${title}</h3>` : ""}${body}</section>`;

/** Shell error codes (`invalid_motion`, `unwritable`, …): their translation, else the generic failure. */
export const errorText = (e) => {
  const code = typeof e === "string" ? e : "";
  const key = `settings.error.${code}`;
  const tr = t(key);
  return tr === key ? t("common.unexpected") : tr;
};

/** Shell codes (`not_file`, `unwritable`, `env_locked`) or the core's typed error when loading the TOML. */
export const configErrorText = (e) => {
  const key = `settings.config.error.${e}`;
  const tr = typeof e === "string" ? t(key) : key;
  return tr === key ? l10nError(e) : tr;
};

/** Shell error: JSON `{ id, params, fallback }` (RF8 of PRD 0002) or plain text. */
export const l10nError = (e) => {
  if (typeof e === "string") {
    try { const p = JSON.parse(e); if (p && typeof p === "object" && "id" in p) return tl(p); } catch {}
    return e;
  }
  return e?.message ?? t("common.unexpected");
};

/** Shell codes (`invalid_port`, `unwritable`, `hooks_off`, …) or the core's typed error. */
export const serveErrorText = (e) => {
  const key = `settings.error.${e}`;
  const tr = typeof e === "string" ? t(key) : key;
  return tr === key ? l10nError(e) : tr;
};

/** "1.8 MiB" / "12.5 KiB": a file size (binary units, as the 64 MiB ceiling). */
export const fmtBytes = (bytes) => (bytes < 1 << 20 ? `${fmtNumber(bytes / 1024, 1)} KiB` : `${fmtNumber(bytes / (1 << 20), 1)} MiB`);

/** A modal question with Cancel focused first; resolves `true` only for the confirming button. */
export function askDialog(id, title, body, yes) {
  return new Promise((resolve) => {
    const d = document.createElement("dialog");
    d.className = `dlg ${id}`;
    d.setAttribute("role", "alertdialog");
    d.setAttribute("aria-labelledby", `${id}-title`);
    d.innerHTML = `<div class="dlg-body"><h2 id="${id}-title">${title}</h2>${body}
      <div class="dlg-actions"><button class="btn" data-ask-no>${te("common.cancel")}</button>
        <button class="btn danger-btn" data-ask-yes>${yes}</button></div></div>`;
    document.body.append(d);
    let answer = false;
    d.addEventListener("close", () => { d.remove(); resolve(answer); }, { once: true });
    d.querySelector("[data-ask-yes]").onclick = () => { answer = true; d.close(); };
    d.querySelector("[data-ask-no]").onclick = () => d.close();
    showDialog(d);
    d.querySelector("[data-ask-no]").focus();
  });
}

/** Copies a secret fetched on click. WebKit only lets the clipboard follow the click synchronously, so the text goes
 *  in as a pending `ClipboardItem` when supported; otherwise it is written once it arrives. */
export function copySecret(fetchText) {
  if (window.ClipboardItem && navigator.clipboard?.write) {
    const blob = fetchText().then((text) => new Blob([text], { type: "text/plain" }));
    return navigator.clipboard.write([new ClipboardItem({ "text/plain": blob })]);
  }
  return fetchText().then((text) => navigator.clipboard.writeText(text));
}
