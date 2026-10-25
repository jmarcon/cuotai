// Settings › Advanced › Integrations (PRD 0020 RF2.5): the local server, hooks (`hooks.js`) and widgets, as the old
// "Integration" section held them, one group each, built from the shared rows (RF3).
import { t, te, tle } from "../i18n.js";
import { copyServeToken, deleteWidgetData, esc, generateServeToken, icon, revokeServeToken, serveStatus, setServe, setWidgets, widgetsStatus } from "../lib.js";
import { copySecret, group, l10nError, serveErrorText } from "./common.js";
import { bindHooks } from "./hooks.js";
import { buttons, commitField, disc, num, pathHtml, row, sw, toast } from "./rows.js";

export function html() {
  return group("serve", te("settings.serve.title"), `<div data-serve><p class="hint">${te("common.loading")}</p></div>`, "data-integration")
    + group("hooks", te("settings.hooks.title"), `<div data-hooks><p class="hint">${te("common.loading")}</p></div>`)
    + group("widgets", te("settings.widgets.title"), `<div data-widgets><p class="hint">${te("common.loading")}</p></div>`);
}

/** Resolves once every group has its state drawn (the shell then lands on an anchor). */
export function bind(root, ctx) {
  return Promise.all([
    bindIntegration(root.querySelector("[data-serve]"), ctx),
    bindHooks(root.querySelector("[data-hooks]"), ctx),
    bindWidgets(root.querySelector("[data-widgets]"), ctx),
  ]);
}

/** Puts the failure of the last change back into the line `id` of freshly drawn markup (RF4.3). */
const withError = (html, [id, text] = []) => (id ? html.replace(`id="${id}" role="alert"></p>`, `id="${id}" role="alert">${esc(text)}</p>`) : html);

/** PRD 0012 RF5: the local server, off by default. The switch, the state (the effective address, or why it is not
 *  serving, naming another Cuotai server on the port), then the connection details collapsed (RF5.2): the port
 *  (bound exactly), the token (generated into the system vault, copied, renewed or revoked; never shown) and "allow
 *  browsers" behind the core's warning. Every change restarts the server and the group repaints from the state the
 *  backend answers. */
export function integrationHtml(sv, message = []) {
  const state = sv.state === "on"
    ? `<p class="hint" data-serve-state><strong>${te("settings.serve.state.on")}</strong> <code>${esc(sv.url)}</code>
        <button class="btn sm" data-serve-copy-url>${icon("copy")} ${te("settings.serve.copyUrl")}</button></p>`
    : sv.state === "error"
      ? `<p class="hint warn-txt" data-serve-state role="alert">${icon("warn")} <strong>${te("settings.serve.state.error")}</strong> ${tle(sv.error)}${
        sv.cuotai_on_port ? ` ${te("settings.serve.cuotaiOnPort", { port: sv.port })}` : ""}</p>`
      : `<p class="hint" data-serve-state><strong>${te("settings.serve.state.off")}</strong></p>`;
  const token = sv.token
    ? buttons(`<button class="btn sm" data-serve-token-copy>${icon("copy")} ${te("settings.serve.token.copy")}</button>`,
      `<button class="btn sm" data-serve-token-new>${te("settings.serve.token.renew")}</button>`,
      `<button class="btn sm danger-btn" data-serve-token-revoke>${te("settings.serve.token.revoke")}</button>`)
    : buttons(`<button class="btn sm" data-serve-token-new>${te("settings.serve.token.generate")}</button>`);
  return withError(row({ key: "serve", label: te("settings.serve.on"), info: `${te("settings.serve.help")}<br />${te("settings.serve.privacy")}`, msg: "serve-msg",
    control: (a) => sw("serve-on", sv.enabled, a), body: state })
    + disc("serve-connection", te("settings.serve.connection"),
      row({ key: "port", label: te("settings.serve.port"), info: te("settings.serve.portHelp", { port: sv.default_port, min: sv.port_min, max: sv.port_max }), msg: "serve-port-msg",
        control: (a) => num("serve-port", { value: sv.port, min: sv.port_min, max: sv.port_max, apply: "data-serve-port" }, a) })
      + row({ key: "token", label: te("settings.serve.token.title"), msg: "serve-token-msg", control: () => token,
        info: `${te(sv.token ? "settings.serve.token.on" : "settings.serve.token.off")}${sv.token ? `<br />${te("settings.serve.token.revokeHelp")}` : ""}` })
      + row({ key: "browsers", label: te("settings.serve.browsers"), info: te("settings.serve.browsersOff"), msg: "serve-browsers-msg",
        control: (a) => sw("serve-browsers", sv.allow_browsers, a),
        // RF5.3: the warning shows only in the state it applies to.
        body: `${sv.allow_browsers ? `<p class="hint warn-txt">${tle(sv.browsers_warning)}</p>` : ""}
        <div class="confirm" id="serve-browsers-confirm" role="alertdialog" aria-labelledby="serve-browsers-text" hidden>
          <p id="serve-browsers-text">${tle(sv.browsers_warning)}</p>
          <div class="dlg-actions left"><button class="btn danger-btn" data-serve-browsers-confirm>${te("settings.serve.browsersConfirm")}</button>
            <button class="btn" data-serve-browsers-cancel>${te("common.cancel")}</button></div>
        </div>` })), message);
}

/** Paints the server and binds it; a failed change keeps the previous state and shows why in its row. */
async function bindIntegration(sec, ctx) {
  let sv;
  try { sv = await serveStatus(); } catch (e) { sec.querySelector(".hint").textContent = l10nError(e); return; }
  const paint = (next, message = []) => {
    sv = next;
    sec.innerHTML = integrationHtml(sv, message);
    const run = (work, line = "serve-msg") => work().then((answer) => { paint(answer); toast(); ctx.changed("serve"); }, (e) => paint(sv, [line, serveErrorText(e)]));
    sec.querySelector("#serve-on").onchange = (e) => run(() => setServe(e.target.checked, sv.port, sv.allow_browsers));
    // RF4.2: the port saves on Enter, on leaving the field or with the row's Apply; a refusal stays in its row.
    commitField(sec.querySelector("#serve-port"), (value) => setServe(sv.enabled, Number(value), sv.allow_browsers).then(
      (answer) => { paint(answer); ctx.changed("serve"); }, (e) => { throw serveErrorText(e); }));
    sec.querySelector("[data-serve-token-new]").onclick = () => run(generateServeToken, "serve-token-msg");
    const revoke = sec.querySelector("[data-serve-token-revoke]");
    if (revoke) revoke.onclick = () => run(revokeServeToken, "serve-token-msg");
    const copied = (btn) => { const prev = btn.innerHTML; btn.textContent = t("settings.serve.copied"); setTimeout(() => { btn.innerHTML = prev; }, 1500); };
    const copyToken = sec.querySelector("[data-serve-token-copy]");
    if (copyToken) copyToken.onclick = () => copySecret(copyServeToken).then(() => copied(copyToken), (e) => { sec.querySelector("#serve-token-msg").textContent = serveErrorText(e); });
    const copyUrl = sec.querySelector("[data-serve-copy-url]");
    if (copyUrl) copyUrl.onclick = () => navigator.clipboard?.writeText(sv.url).then(() => copied(copyUrl), () => { sec.querySelector("#serve-msg").textContent = t("settings.serve.copyFailed"); });
    // PRD 0012 RF3.3: pages are only let in after the warning was read and confirmed.
    const confirm = sec.querySelector("#serve-browsers-confirm");
    sec.querySelector("#serve-browsers").onchange = (e) => {
      if (!e.target.checked) { run(() => setServe(sv.enabled, sv.port, false), "serve-browsers-msg"); return; }
      e.target.checked = false;
      confirm.hidden = false;
      sec.querySelector("[data-serve-browsers-confirm]").focus();
    };
    sec.querySelector("[data-serve-browsers-confirm]").onclick = () => run(() => setServe(sv.enabled, sv.port, true), "serve-browsers-msg");
    sec.querySelector("[data-serve-browsers-cancel]").onclick = () => { confirm.hidden = true; };
  };
  paint(sv);
}

/** PRD 0014 RF4.1/RF4.2/RF2.3: the widget snapshot of this device, off by default. The switch, whether identity
 *  (e-mail) travels with it, how many accounts are published, the file location (and whether it is the app group a
 *  WidgetKit extension can read), and "delete widget data". The redraw of a widget depends on the operating system,
 *  so the data on it may lag; on macOS the extension also needs a signed app with the app group entitlement. */
export function widgetsHtml(wv, message = []) {
  const p = wv.prefs ?? { enabled: false, include_identity: false, max_accounts: 6 };
  const where = wv.app_group
    ? te("settings.widgets.path.appGroup")
    : wv.macos ? te("settings.widgets.path.userDataMac") : te("settings.widgets.path.userData");
  const off = p.enabled ? "" : "disabled";
  return withError(row({ key: "widgets", label: te("settings.widgets.on"), desc: te(p.enabled ? "settings.widgets.onState" : "settings.widgets.off"),
    info: `${te("settings.widgets.help")}<br />${te("settings.widgets.redraw")}`, msg: "widgets-msg", control: (a) => sw("widgets-on", p.enabled, a) })
    + row({ key: "identity", label: te("settings.widgets.identity"), dep: true, info: te("settings.widgets.identityHelp"), msg: "widgets-identity-msg",
      control: (a) => sw("widgets-identity", p.include_identity, a, off) })
    + row({ key: "max", label: te("settings.widgets.maxAccounts"), dep: true, info: te("settings.widgets.maxAccountsHelp", { min: wv.accounts_min, max: wv.accounts_max }),
      msg: "widgets-max-msg", control: (a) => num("widgets-max", { value: p.max_accounts, min: wv.accounts_min, max: wv.accounts_max }, `${a} ${off}`) })
    + disc("widgets-path", te("settings.widgets.path.label"), `${pathHtml(wv.path)}<p class="hint">${where}</p>
      ${wv.macos && !wv.app_group ? `<p class="hint warn-txt">${icon("warn")} ${te("settings.widgets.macos")}</p>` : ""}`)
    + row({ key: "widgets-delete", label: te("settings.widgets.delete"), danger: true, msg: "widgets-delete-msg",
      control: () => buttons(`<button class="btn sm danger-btn" data-widgets-delete aria-labelledby="widgets-delete-label">${icon("trash")}</button>`) }), message);
}

/** Paints the widgets and binds them. Each change is saved at once and answers the new state; a refused change
 *  keeps the previous values and says why in its row. */
async function bindWidgets(sec, ctx) {
  let wv;
  try { wv = await widgetsStatus(); } catch (e) { sec.querySelector(".hint").textContent = l10nError(e); return; }
  const paint = (next, message = []) => {
    wv = next;
    sec.innerHTML = widgetsHtml(wv, message);
    const on = sec.querySelector("#widgets-on");
    const identity = sec.querySelector("#widgets-identity");
    const max = sec.querySelector("#widgets-max");
    const save = (n = max.value) => setWidgets(on.checked, identity.checked, Number(n));
    const run = (work, line, done = t("settings.saved")) => work().then((answer) => { paint(answer); toast(done); ctx.changed("widgets"); },
      (e) => paint(wv, [line, serveErrorText(e)]));
    on.onchange = () => run(save, "widgets-msg");
    identity.onchange = () => run(save, "widgets-identity-msg");
    commitField(max, (value) => save(value).then((answer) => { paint(answer); ctx.changed("widgets"); }, (e) => { throw serveErrorText(e); }));
    sec.querySelector("[data-widgets-delete]").onclick = () => run(async () => {
      await deleteWidgetData();
      return widgetsStatus();
    }, "widgets-delete-msg", t("settings.widgets.deleted"));
  };
  paint(wv);
}
