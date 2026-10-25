// PRD 0018 RF2/RF3 — o aviso de atualização e a seção "Updates" das configurações.
// A interface nunca vê uma URL de manifesto, uma assinatura ou uma chave: o backend
// entrega só o que mostrar (fase, versão, notas, progresso, motivo). Todos os textos
// moram nos quatro catálogos.
import { te, t, currentLocale } from "./i18n.js";
import { showDialog } from "./motion.js";
import { row, sw } from "./settings/rows.js";
import {
  esc, errText, fmtDate, icon, onUpdate, openExternal, updaterAnswerAutomatic, updaterCancelDownload, updaterCheckNow,
  updaterDismiss, updaterInstall, updaterRestart, updaterStatus, setUpdaterAutomatic,
} from "./lib.js";

let status = null;
const listeners = new Set();
let started = false;
let initialStatus = null;
let consentPrompted = false;

const publish = (view) => {
  status = view;
  listeners.forEach((cb) => cb(view));
  return view;
};

/** Lê o estado uma vez e passa a reagir ao evento `updater` das duas janelas. */
export async function initUpdates() {
  if (!started) {
    started = true;
    onUpdate(publish);
    initialStatus = updaterStatus().then(publish, () => null);
  }
  await initialStatus;
  return status;
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
  if (!s || s.consent_pending) return "";
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

/**
 * PRD 0018 RF2.6 / DS-CMP-DLG-1, DS-CMP-DLG-2, DS-CMP-DLG-4,
 * DS-CMP-BTN-2, DS-CMP-BTN-4, DS-A11Y-1, DS-A11Y-2 and DS-A11Y-5:
 * one focused consent dialog in the main window. Closing it records nothing,
 * so this session continues without a request and the question returns on the
 * next launch.
 */
export const updateConsentDialogHtml = () => `<div class="dlg-body">
  <header class="dlg-head">
    <h2 id="update-consent-title">${te("update.consent.title")}</h2>
    <button class="icon" data-update-consent-close title="${te("common.close")}" aria-label="${te("common.close")}">${icon("x")}</button>
  </header>
  <p>${te("update.consent.body")}</p>
  <p class="err" data-update-consent-error role="alert"></p>
  <div class="dlg-actions">
    <button class="btn" data-update-consent-no>${te("update.consent.no")}</button>
    <button class="btn primary" data-update-consent-yes>${te("update.consent.yes")}</button>
  </div>
</div>`;

export async function promptForUpdateConsent() {
  const current = await initUpdates();
  if (!current?.consent_pending || consentPrompted) return false;
  consentPrompted = true;
  const dlg = document.createElement("dialog");
  dlg.className = "dlg update-consent";
  dlg.setAttribute("aria-labelledby", "update-consent-title");
  dlg.innerHTML = updateConsentDialogHtml();
  document.body.append(dlg);
  const no = dlg.querySelector("[data-update-consent-no]");
  const yes = dlg.querySelector("[data-update-consent-yes]");
  const error = dlg.querySelector("[data-update-consent-error]");
  const answer = async (on) => {
    no.disabled = true;
    yes.disabled = true;
    try {
      await updaterAnswerAutomatic(on);
      dlg.close();
    } catch (reason) {
      no.disabled = false;
      yes.disabled = false;
      error.textContent = errText(reason);
    }
  };
  dlg.addEventListener("close", () => dlg.remove(), { once: true });
  dlg.querySelector("[data-update-consent-close]").onclick = () => dlg.close();
  no.onclick = () => answer(false);
  yes.onclick = () => answer(true);
  showDialog(dlg);
  no.focus();
  return true;
}

const availableHtml = (s) => {
  const releaseDate = fmtDate(s.date);
  return `<div class="banner info" role="status" data-update-banner data-update="available">
  ${icon("download")}<div class="banner-main"><b>${te("update.available", { version: s.version })}</b>${releaseDate ? `<span class="hint">${te("update.date", { date: releaseDate })}</span>` : ""}
  <details class="update-notes"><summary>${icon("chev", "upd-chev")}${te("update.notes")}</summary><p>${esc(s.notes ?? "")}</p></details></div>
  <div class="banner-acts">
    <button class="btn ghost" data-update-later>${te("update.later")}</button>
    <button class="btn primary" data-update-now>${te("update.now")}</button>
  </div>
</div>`;
};

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

/** PRD 0018 RF3.1/RF2.5: the complete Settings "Updates" section, including its
 * single heading, switch, last check, current version, channel and disabled reason. */
export function updatesSectionHtml(s, message = "") {
  const pending = !!s?.consent_pending;
  const off = s && !s.enabled;
  // A user's answered preference must never lock the control needed to turn
  // checks back on. The unanswered gate and external policies stay read-only.
  const switchLocked = pending || (off && s.disabled_reason !== "preference");
  const last = s?.last_check ? t("update.lastCheckAt", { date: new Date(s.last_check).toLocaleString(currentLocale()) }) : t("update.lastCheckNever");
  const offText = pending ? te("update.consent.pending") : off ? te(`update.off.${s.disabled_reason}`) : "";
  return `<section class="set-sec" data-updates>
    <h3>${te("update.title")}</h3>
    <div data-update-body>
      ${privacyNoteHtml()}
      ${row({ key: "update-auto", label: te("update.automatic"), msg: "update-auto-msg", control: (a) => sw("update-automatic", s?.automatic, a, switchLocked ? "disabled" : "") })}
      ${off ? `<p class="hint warn-txt" data-update-off>${offText}</p>` : ""}
      <p class="hint">${te("update.version", { version: `${s?.current_version ?? "—"}${s?.build ? ` (${s.build})` : ""}` })} · ${te("update.channel", { channel: s?.channel ?? "stable" })}</p>
      <p class="hint" id="update-last-check">${last}</p>
      <div class="dlg-actions left">
        <button class="btn" data-update-check ${off ? "disabled" : ""}>${te("update.checkNow")}</button>
        ${s?.upgrade_command ? `<code class="update-cmd">${esc(s.upgrade_command)}</code>` : ""}
        ${!off && !s?.can_install ? `<button class="btn" data-update-manual>${te("update.manual")}</button>` : ""}
      </div>
      <p class="hint" data-update-phase>${phaseText(s)}</p>
      <p class="err" id="update-msg" role="alert">${esc(message)}</p>
    </div>
  </section>`;
}

/** Uma linha honesta sobre o que está acontecendo agora. */
const phaseText = (s) => {
  if (!s) return t("common.loading");
  if (s.consent_pending) return t("update.consent.pending");
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
