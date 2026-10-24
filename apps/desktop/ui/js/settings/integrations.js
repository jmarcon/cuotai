// Settings › Advanced › Integrations (PRD 0020 RF2.5): the local server, hooks (`hooks.js`) and widgets, as the old
// "Integration" section held them.
import { t, te, tle } from "../i18n.js";
import { copyServeToken, deleteWidgetData, esc, generateServeToken, icon, revokeServeToken, serveStatus, setServe, setWidgets, widgetsStatus } from "../lib.js";
import { copySecret, group, l10nError, serveErrorText } from "./common.js";
import { bindHooks } from "./hooks.js";

export function html() {
  return group("serve", te("settings.serve.title"), `
      <div data-serve><p class="hint">${te("common.loading")}</p></div>
      <div data-hooks id="set-hooks" data-section="hooks"><p class="hint">${te("common.loading")}</p></div>
      <div data-widgets id="set-widgets" data-section="widgets"><p class="hint">${te("common.loading")}</p></div>`, "data-integration");
}

/** Resolves once every group has its state drawn (the shell then lands on an anchor). */
export function bind(root, ctx) {
  return Promise.all([
    bindIntegration(root.querySelector("[data-serve]"), ctx),
    bindHooks(root.querySelector("[data-hooks]"), ctx),
    bindWidgets(root.querySelector("[data-widgets]"), ctx),
  ]);
}

/** PRD 0012 RF5: the local server, off by default. The switch, the state (the effective address, or why it is not
 *  serving, naming another Cuotai server on the port), the port (bound exactly), the token (generated into the system
 *  vault, copied, renewed or revoked; never shown) and "allow browsers" behind the core's warning. Every change
 *  restarts the server and the group repaints from the state the backend answers. */
export function integrationHtml(sv) {
  const state = sv.state === "on"
    ? `<p class="hint" data-serve-state><strong>${te("settings.serve.state.on")}</strong> <code>${esc(sv.url)}</code></p>
        <div class="dlg-actions left"><button class="btn" data-serve-copy-url>${icon("copy")} ${te("settings.serve.copyUrl")}</button></div>`
    : sv.state === "error"
      ? `<p class="hint warn-txt" data-serve-state role="alert">${icon("warn")} <strong>${te("settings.serve.state.error")}</strong> ${tle(sv.error)}${
        sv.cuotai_on_port ? ` ${te("settings.serve.cuotaiOnPort", { port: sv.port })}` : ""}</p>`
      : `<p class="hint" data-serve-state><strong>${te("settings.serve.state.off")}</strong></p>`;
  const token = sv.token
    ? `<p class="hint">${te("settings.serve.token.on")}</p>
        <div class="dlg-actions left"><button class="btn" data-serve-token-copy>${icon("copy")} ${te("settings.serve.token.copy")}</button>
          <button class="btn" data-serve-token-new>${te("settings.serve.token.renew")}</button>
          <button class="btn danger-btn" data-serve-token-revoke>${te("settings.serve.token.revoke")}</button></div>
        <p class="hint">${te("settings.serve.token.revokeHelp")}</p>`
    : `<p class="hint">${te("settings.serve.token.off")}</p>
        <div class="dlg-actions left"><button class="btn" data-serve-token-new>${te("settings.serve.token.generate")}</button></div>`;
  return `<p class="hint">${te("settings.serve.help")}</p>
        <label class="check"><input id="serve-on" type="checkbox" ${sv.enabled ? "checked" : ""} /> ${te("settings.serve.on")}</label>
        ${state}
        <label class="f"><span>${te("settings.serve.port")}</span>
          <input id="serve-port" type="number" inputmode="numeric" min="${sv.port_min}" max="${sv.port_max}" step="1" value="${esc(sv.port)}" />
          <small>${te("settings.serve.portHelp", { port: sv.default_port, min: sv.port_min, max: sv.port_max })}</small></label>
        <div class="dlg-actions left"><button class="btn primary" data-serve-port>${te("settings.serve.portSave")}</button></div>
        <h4>${te("settings.serve.token.title")}</h4>
        ${token}
        <label class="check"><input id="serve-browsers" type="checkbox" ${sv.allow_browsers ? "checked" : ""} /> ${te("settings.serve.browsers")}</label>
        ${sv.allow_browsers ? `<p class="hint warn-txt">${tle(sv.browsers_warning)}</p>` : `<p class="hint">${te("settings.serve.browsersOff")}</p>`}
        <div class="confirm" id="serve-browsers-confirm" role="alertdialog" aria-labelledby="serve-browsers-text" hidden>
          <p id="serve-browsers-text">${tle(sv.browsers_warning)}</p>
          <div class="dlg-actions left"><button class="btn danger-btn" data-serve-browsers-confirm>${te("settings.serve.browsersConfirm")}</button>
            <button class="btn" data-serve-browsers-cancel>${te("common.cancel")}</button></div>
        </div>
        <p class="hint">${te("settings.serve.privacy")}</p>
        <p class="err" id="serve-msg" role="alert"></p>`;
}

/** Paints the server and binds it; a failed change keeps the previous state and shows why. */
async function bindIntegration(sec, ctx) {
  let sv;
  try { sv = await serveStatus(); } catch (e) { sec.querySelector(".hint").textContent = l10nError(e); return; }
  const paint = (next, message = "") => {
    sv = next;
    sec.innerHTML = integrationHtml(sv);
    const msg = sec.querySelector("#serve-msg");
    msg.textContent = message;
    const run = (work) => work().then((answer) => { paint(answer); ctx.changed("serve"); }, (e) => paint(sv, serveErrorText(e)));
    const port = () => Number(sec.querySelector("#serve-port").value);
    sec.querySelector("#serve-on").onchange = (e) => run(() => setServe(e.target.checked, sv.port, sv.allow_browsers));
    sec.querySelector("[data-serve-port]").onclick = () => run(() => setServe(sv.enabled, port(), sv.allow_browsers));
    sec.querySelector("[data-serve-token-new]").onclick = () => run(generateServeToken);
    const revoke = sec.querySelector("[data-serve-token-revoke]");
    if (revoke) revoke.onclick = () => run(revokeServeToken);
    const copied = (btn) => { const prev = btn.innerHTML; btn.textContent = t("settings.serve.copied"); setTimeout(() => { btn.innerHTML = prev; }, 1500); };
    const copyToken = sec.querySelector("[data-serve-token-copy]");
    if (copyToken) copyToken.onclick = () => copySecret(copyServeToken).then(() => copied(copyToken), (e) => { msg.textContent = serveErrorText(e); });
    const copyUrl = sec.querySelector("[data-serve-copy-url]");
    if (copyUrl) copyUrl.onclick = () => navigator.clipboard?.writeText(sv.url).then(() => copied(copyUrl), () => { msg.textContent = t("settings.serve.copyFailed"); });
    // PRD 0012 RF3.3: pages are only let in after the warning was read and confirmed.
    const confirm = sec.querySelector("#serve-browsers-confirm");
    sec.querySelector("#serve-browsers").onchange = (e) => {
      if (!e.target.checked) { run(() => setServe(sv.enabled, sv.port, false)); return; }
      e.target.checked = false;
      confirm.hidden = false;
      sec.querySelector("[data-serve-browsers-confirm]").focus();
    };
    sec.querySelector("[data-serve-browsers-confirm]").onclick = () => run(() => setServe(sv.enabled, sv.port, true));
    sec.querySelector("[data-serve-browsers-cancel]").onclick = () => { confirm.hidden = true; };
  };
  paint(sv);
}

/** PRD 0014 RF4.1/RF4.2/RF2.3: the widget snapshot of this device, off by default. The switch, whether identity
 *  (e-mail) travels with it, how many accounts are published, the file location (and whether it is the app group a
 *  WidgetKit extension can read), and "delete widget data". The redraw of a widget depends on the operating system,
 *  so the data on it may lag; on macOS the extension also needs a signed app with the app group entitlement. */
export function widgetsHtml(wv, message = "") {
  const p = wv.prefs ?? { enabled: false, include_identity: false, max_accounts: 6 };
  const where = wv.app_group
    ? te("settings.widgets.path.appGroup")
    : wv.macos ? te("settings.widgets.path.userDataMac") : te("settings.widgets.path.userData");
  return `<h4>${te("settings.widgets.title")}</h4>
    <p class="hint">${te("settings.widgets.help")}</p>
    <label class="check"><input id="widgets-on" type="checkbox" ${p.enabled ? "checked" : ""} /> ${te("settings.widgets.on")}</label>
    <p class="hint">${te(p.enabled ? "settings.widgets.onState" : "settings.widgets.off")}</p>
    <label class="check"><input id="widgets-identity" type="checkbox" ${p.include_identity ? "checked" : ""} /> ${te("settings.widgets.identity")}</label>
    <p class="hint">${te("settings.widgets.identityHelp")}</p>
    <label class="f"><span>${te("settings.widgets.maxAccounts")}</span>
      <input id="widgets-max" type="number" inputmode="numeric" min="${esc(wv.accounts_min)}" max="${esc(wv.accounts_max)}" step="1" value="${esc(p.max_accounts)}" />
      <small>${te("settings.widgets.maxAccountsHelp", { min: wv.accounts_min, max: wv.accounts_max })}</small></label>
    <p class="hint">${te("settings.widgets.path.label")}</p>
    <div class="path-row"><code title="${esc(wv.path ?? "")}">${esc(wv.path || "—")}</code></div>
    <p class="hint">${where}</p>
    <p class="hint">${te("settings.widgets.redraw")}</p>
    ${wv.macos && !wv.app_group ? `<p class="hint warn-txt">${icon("warn")} ${te("settings.widgets.macos")}</p>` : ""}
    <div class="dlg-actions left"><button class="btn danger-btn" data-widgets-delete>${icon("trash")} ${te("settings.widgets.delete")}</button></div>
    <p class="err" id="widgets-msg" role="alert">${esc(message)}</p>`;
}

/** Paints the widgets and binds them. Each change is saved at once and answers the new state; a refused change
 *  keeps the previous values and says why. */
async function bindWidgets(sec, ctx) {
  let wv;
  try { wv = await widgetsStatus(); } catch (e) { sec.querySelector(".hint").textContent = l10nError(e); return; }
  const paint = (next, message = "") => {
    wv = next;
    sec.innerHTML = widgetsHtml(wv, message);
    const msg = sec.querySelector("#widgets-msg");
    const save = () => setWidgets(
      sec.querySelector("#widgets-on").checked,
      sec.querySelector("#widgets-identity").checked,
      Number(sec.querySelector("#widgets-max").value));
    const run = (work) => work().then((answer) => { paint(answer); ctx.changed("widgets"); }, (e) => { msg.textContent = serveErrorText(e); });
    sec.querySelector("#widgets-on").onchange = () => run(save);
    sec.querySelector("#widgets-identity").onchange = () => run(save);
    sec.querySelector("#widgets-max").onchange = () => run(save);
    sec.querySelector("[data-widgets-delete]").onclick = () => run(async () => {
      await deleteWidgetData();
      return widgetsStatus();
    }).then(() => {
      sec.querySelector("#widgets-msg").textContent = t("settings.widgets.deleted");
    });
  };
  paint(wv);
}
