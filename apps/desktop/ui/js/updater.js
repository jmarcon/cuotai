// PRD 0018 RF2/RF3 — o aviso de atualização e a seção "Updates" das configurações.
// A interface nunca vê uma URL de manifesto, uma assinatura ou uma chave: o backend
// entrega só o que mostrar (fase, versão, notas, progresso, motivo). Todos os textos
// moram nos três catálogos.
import { te, t, currentLocale } from "./i18n.js";
import {
  esc, errText, icon, onUpdate, openExternal, updaterCancelDownload, updaterCheckNow, updaterDismiss,
  updaterExplanationSeen, updaterInstall, updaterRestart, updaterStatus, setUpdaterAutomatic,
} from "./lib.js";

let status = null;
const listeners = new Set();
let started = false;

/** Lê o estado uma vez e passa a reagir ao evento `updater` das duas janelas. */
export async function initUpdates() {
  if (!started) {
    started = true;
    onUpdate((view) => { status = view; listeners.forEach((cb) => cb(view)); });
    updaterStatus().then((view) => { status = view; listeners.forEach((cb) => cb(view)); }, () => {});
  }
}

/** O último estado recebido (null antes da primeira resposta). */
export const updateState = () => status;

/** Assina as mudanças do estado (repintar, avisar a bandeja). */
export function onUpdateState(cb) {
  listeners.add(cb);
  if (status) cb(status);
  return () => listeners.delete(cb);
}

/** PRD 0018 RF2.2: o aviso da janela principal. Nada baixa sem "Atualizar agora". */
export function updateBannerHtml() {
  const s = status;
  if (!s) return "";
  if (s.explanation_pending) return explanationHtml();
  if (!s.enabled) return "";
  switch (s.phase) {
    case "available":
      return s.can_install ? availableHtml(s) : managedHtml(s);
    case "downloading":
      return downloadingHtml(s);
    case "installing":
      return `<div class="banner" role="status" data-update-banner>${icon("refresh")} ${te("update.installing", { version: s.version })}</div>`;
    case "ready":
      return readyHtml(s);
    case "failed":
      return failedHtml(s);
    default:
      return "";
  }
}

/** PRD 0018 RF2.1: primeira execução, o que o pedido envia. */
const explanationHtml = () => `<div class="banner" role="status" data-update-banner data-update-explain>
  <b>${te("update.explain.title")}</b> ${te("update.explain.body")}
  <div class="dlg-actions left"><button class="btn primary" data-update-explain-ok>${te("common.close")}</button></div>
</div>`;

const availableHtml = (s) => `<div class="banner" role="status" data-update-banner data-update="available">
  <b>${te("update.available", { version: s.version })}</b>
  <details class="update-notes"><summary>${te("update.notes")}</summary>${esc(s.notes ?? "")}</details>
  <div class="dlg-actions left">
    <button class="btn primary" data-update-now>${te("update.now")}</button>
    <button class="btn" data-update-later>${te("update.later")}</button>
    ${s.date ? `<span class="hint">${te("update.date", { date: s.date })}</span>` : ""}
  </div>
</div>`;

/** PRD 0018 RF2.5 / PRD 0021 RF8.1: instalação gerenciada ou sem escrita. */
const managedHtml = (s) => `<div class="banner" role="status" data-update-banner data-update="managed">
  <b>${te("update.available", { version: s.version })}</b>
  <p class="hint">${te(`update.cannot.${s.upgrade_command ? "managed" : "place"}`)}</p>
  ${s.upgrade_command ? `<code class="update-cmd">${esc(s.upgrade_command)}</code>` : ""}
  <div class="dlg-actions left"><button class="btn" data-update-manual>${te("update.manual")}</button></div>
</div>`;

const downloadingHtml = (s) => `<div class="banner" role="status" data-update-banner data-update="downloading">
  <b>${te("update.downloading", { version: s.version })}</b>
  ${s.progress == null ? "" : `<progress class="update-progress" max="100" value="${s.progress}"></progress><span class="hint">${s.progress}%</span>`}
  <div class="dlg-actions left"><button class="btn" data-update-cancel>${te("update.cancel")}</button></div>
</div>`;

const readyHtml = (s) => `<div class="banner" role="status" data-update-banner data-update="ready">
  <b>${te("update.ready", { version: s.version })}</b>
  <div class="dlg-actions left"><button class="btn primary" data-update-restart>${te("update.restart")}</button></div>
</div>`;

const failedHtml = (s) => `<div class="banner bad" role="alert" data-update-banner data-update="failed">
  ${icon("warn")} ${te(`update.error.${s.error ?? "other"}`)}
  <div class="dlg-actions left"><button class="btn" data-update-manual>${te("update.manual")}</button></div>
</div>`;

/**
 * PRD 0018 RF2.2/RF2.3/RF2.4: os cliques do aviso. `rerender` repinta a janela depois da
 * resposta; `onError` mostra a recusa (por exemplo o login em andamento que bloqueia o
 * reinício).
 */
export function bindUpdateBanner(root, { rerender, onError }) {
  root.addEventListener("click", async (e) => {
    const t = e.target;
    if (t.closest("[data-update-explain-ok]")) return updaterExplanationSeen().catch(() => {});
    if (t.closest("[data-update-now]")) return updaterInstall().then(rerender, onError);
    if (t.closest("[data-update-later]")) return updaterDismiss().then(rerender, onError);
    if (t.closest("[data-update-cancel]")) return updaterCancelDownload().then(rerender, onError);
    if (t.closest("[data-update-manual]")) return openExternal(status?.manual_url).catch(onError);
    if (t.closest("[data-update-restart]")) {
      try {
        await updaterRestart();
      } catch (err) {
        onError(err);
        rerender();
      }
    }
  });
}

/** PRD 0018 RF2.1/RF3.1: o que a verificação envia (primeira execução e Settings). */
export const privacyNoteHtml = () => `<p class="hint" data-update-privacy>${te("update.privacy")}</p>`;

/** PRD 0018 RF3.1/RF2.5: a seção "Updates" — o interruptor, "Verificar agora", a data da
 * última verificação, a versão atual, o canal e o motivo de estar desligado. */
export function updatesSectionHtml(s, message = "") {
  const off = s && !s.enabled;
  const last = s?.last_check ? t("update.lastCheckAt", { date: new Date(s.last_check).toLocaleString(currentLocale()) }) : t("update.lastCheckNever");
  return `<h3>${te("update.title")}</h3>
    ${privacyNoteHtml()}
    <label class="check"><input id="update-automatic" type="checkbox" ${s?.automatic ? "checked" : ""} ${off ? "disabled" : ""} /> ${te("update.automatic")}</label>
    ${off ? `<p class="hint warn-txt" data-update-off>${te(`update.off.${s.disabled_reason}`)}</p>` : ""}
    <p class="hint">${te("update.version", { version: s?.current_version ?? "—" })} · ${te("update.channel", { channel: s?.channel ?? "stable" })}</p>
    <p class="hint" id="update-last-check">${last}</p>
    <div class="dlg-actions left">
      <button class="btn" data-update-check ${off ? "disabled" : ""}>${te("update.checkNow")}</button>
      ${s?.upgrade_command ? `<code class="update-cmd">${esc(s.upgrade_command)}</code>` : ""}
      ${!off && !s?.can_install ? `<button class="btn" data-update-manual>${te("update.manual")}</button>` : ""}
    </div>
    <p class="hint" data-update-phase>${phaseText(s)}</p>
    <p class="err" id="update-msg" role="alert">${esc(message)}</p>`;
}

/** Uma linha honesta sobre o que está acontecendo agora. */
const phaseText = (s) => {
  if (!s) return t("common.loading");
  if (s.explanation_pending) return t("update.pendingExplanation");
  if (!s.enabled) return t(`update.off.${s.disabled_reason}`);
  switch (s.phase) {
    case "checking": return t("update.state.checking");
    case "uptodate": return t("update.state.uptodate");
    case "available": return t("update.state.available", { version: s.version });
    case "downloading": return t("update.state.downloading", { version: s.version, progress: s.progress ?? "" });
    case "installing": return t("update.state.installing");
    case "ready": return t("update.state.ready");
    case "failed": return t(`update.error.${s.error ?? "other"}`);
    default: return t("update.state.idle");
  }
};

/** PRD 0018 RF3.1: os controles da seção, cada mudança salva na hora. */
export async function bindUpdates(sec) {
  const automatic = sec.querySelector("#update-automatic");
  const msg = sec.querySelector("#update-msg");
  const last = sec.querySelector("#update-last-check");
  const phase = sec.querySelector("[data-update-phase]");
  const repaint = async (view, message = "") => {
    if (last) last.textContent = view.last_check
      ? t("update.lastCheckAt", { date: new Date(view.last_check).toLocaleString(currentLocale()) })
      : t("update.lastCheckNever");
    if (phase) phase.textContent = phaseText(view);
    if (msg) msg.textContent = message;
  };
  automatic?.addEventListener("change", async (e) => {
    try {
      const view = await setUpdaterAutomatic(e.target.checked);
      repaint(view);
    } catch (err) {
      e.target.checked = !e.target.checked;
      repaint(status, errText(err));
    }
  });
  sec.querySelector("[data-update-check]")?.addEventListener("click", async (e) => {
    const button = e.currentTarget;
    button.disabled = true;
    repaint({ ...status, phase: "checking" });
    try {
      const view = await updaterCheckNow();
      repaint(view);
    } catch (err) {
      repaint(status, errText(err));
    } finally {
      button.disabled = false;
    }
  });
  sec.querySelector("[data-update-manual]")?.addEventListener("click", (e) => {
    openExternal(status?.manual_url).catch((err) => repaint(status, errText(err)));
  });
}
