// Formulário de conta: todo o esquema, visibilidade e validação vêm do core.
// RF2: o login é orquestrado pelo core; este diálogo só renderiza eventos
// tipados (LoginEvent, sem segredos) e envia entradas do usuário.
import {
  configChangedOnDisk, deleteAccount, errText, esc, fieldChoices, formSchema, getAccount, getView, listProviders,
  loginCancel, loginStart, loginSubmitCode, modelSupport, onLogin, reloadConfig,
  reportKey, saveAccount, visibleFields,
} from "./lib.js";
import { mountModels } from "./detail.js";
import { currentLocale, t, te, tl } from "./i18n.js";
import { showDialog } from "./motion.js";

function open(html) {
  const dlg = document.createElement("dialog");
  dlg.className = "dlg";
  dlg.innerHTML = html;
  document.body.append(dlg);
  showDialog(dlg);
  return dlg;
}

export async function openAccountDialog(index = null, { onSaved = () => {} } = {}) {
  // RF4: cheap external-change detection. If the file on disk
  // changed since the last view render, give the user a chance to
  // reload BEFORE they spend time editing the now-stale draft.
  // Falls back silently on the browser demo (`configChangedOnDisk`
  // always returns `false` there).
  try {
    if (await configChangedOnDisk()) {
      const choice = await offerExternalReload();
      if (choice === "reload") await reloadConfig();
      // "keep" leaves the wizard open with the old snapshot — the
      // user gets the typed-conflict banner on save either way.
    }
  } catch {
    // Detection failure (headless test, missing backend): open
    // the wizard anyway. The save path's own conflict handling
    // remains the last line of defence.
  }
  const editing = index != null;
  const providers = (await listProviders()).sort((a, b) => a.label.localeCompare(b.label, currentLocale(), { sensitivity: "base" }));
  const existing = editing ? await getAccount(index) : null;
  let provider = existing?.provider ?? providers[0].id;
  let schema = await formSchema(provider);
  // Valores correntes por chave: { text } ou { choice }.
  const values = new Map((existing?.fields ?? []).map((f) => [f.key, f]));
  // authMode inicial: prioriza a config existente (oauth ou credential) ou
  // o primeiro modo do core quando o provedor não tem credencial manual.
  let authMode = existing?.auth_mode
    || (schema.credential_kind === "none"
      ? ((schema.login?.modes ?? []).find((m) => !m.unavailable_reason)?.id ?? "credential")
      : "credential");
  let visible = new Set();
  // Estado do login RF2: null ou { phase, lines, url, userCode, params, codeError, attempt }.
  // phase: "running" | "done" | "failed". O token NUNCA passa por aqui.
  let loginState = null;
  let committing = false;

  const dlg = open(`<form method="dialog" class="dlg-body" autocomplete="off">
    <h2>${te(editing ? "dialog.editTitle" : "dialog.addTitle")}</h2>
    <label class="f" data-field="provider"><span>${te("dialog.provider")}</span>
      <select name="provider" ${editing ? "disabled" : ""}>${providers.map((p) => `<option value="${esc(p.id)}" ${p.id === provider ? "selected" : ""}>${esc(p.label)}</option>`).join("")}</select>
      <small class="field-err" hidden></small></label>
    <label class="f" data-field="name"><span>${te("dialog.name")}</span><input name="name" value="${esc(existing?.name ?? "")}" required /><small class="field-err" id="name-err" role="alert" hidden></small></label>
    <div id="dyn"><div id="dyn-auth"></div>
    <div class="login-panel" id="login-panel" hidden>
      <div class="login-log" id="login-log" role="status" aria-live="polite"></div>
      <div id="login-prompt" hidden>
        <label class="f" id="login-url-row" hidden><span>${te("dialog.login.urlLabel")}</span>
          <div class="login-url-row">
            <input id="login-url" readonly aria-label="${te("dialog.login.urlLabel")}" />
            <button type="button" class="btn" id="copy-url">${te("dialog.copy")}</button>
          </div></label>
        <p class="login-usercode" id="login-usercode-row" hidden>${te("dialog.login.userCode")} <code id="login-code-text"></code>
          <button type="button" class="btn" id="copy-code" aria-label="${te("dialog.login.copyCodeLabel")}">${te("dialog.copy")}</button></p>
      </div>
      <div id="login-code-row" hidden>
        <label class="f" data-field="code"><span>${te("dialog.login.codeLabel")}</span>
          <input id="login-code-input" autocomplete="off" placeholder="${te("dialog.login.codePlaceholder")}" />
          <small id="login-code-status" role="alert"></small></label>
        <div class="dlg-actions"><button type="button" class="btn primary" id="login-code-send">${te("dialog.login.sendCode")}</button></div>
      </div>
      <div class="dlg-actions"><button type="button" class="btn" id="login-cancel">${te("dialog.login.cancel")}</button></div>
    </div>
    <div id="dyn-fields"></div></div>
    <label class="f" data-field="interval"><span>${te("dialog.interval")}</span><input name="interval" inputmode="numeric" placeholder="${te("dialog.intervalPlaceholder")}" value="${esc(existing?.interval ?? "")}" /><small class="field-err" hidden></small></label>
    <p class="cost-advisory" id="cost-warn" hidden></p>
    <p class="cost-advisory" id="choice-notice" role="status" aria-live="polite" hidden></p>
    <div id="models-host"></div>
    <p class="err" id="err" role="alert"></p>
    <div class="dlg-actions"><button type="button" class="btn" data-cancel>${te("common.cancel")}</button><button type="submit" class="btn primary" id="ok">${te("common.save")}</button></div>
  </form>`);
  const form = dlg.querySelector("form");
  const dyn = dlg.querySelector("#dyn");
  const dynAuth = dlg.querySelector("#dyn-auth");
  const dynFields = dlg.querySelector("#dyn-fields");
  const nameErr = dlg.querySelector("#name-err");
  const err = dlg.querySelector("#err");
  const costWarn = dlg.querySelector("#cost-warn");
  const choiceNotice = dlg.querySelector("#choice-notice");
  const nameInput = dlg.querySelector('[name="name"]');
  // O login só começa com a conta nomeada (ao terminar ela é salva sozinha). Em vez de `disabled`, que engole o
  // clique sem explicar, o botão fica apagado, mostra o motivo logo abaixo e, ao clicar, leva ao campo do nome.
  const showNameError = () => {
    nameInput.classList.add("invalid");
    nameErr.textContent = t("dialog.login.nameFirst");
    nameErr.hidden = false;
    nameInput.scrollIntoView({ block: "center", behavior: "smooth" });
    nameInput.focus();
  };
  const syncLoginBtn = () => {
    const missing = nameInput.value.trim() === "";
    if (!missing) { nameInput.classList.remove("invalid"); nameErr.hidden = true; }
    const btn = dlg.querySelector("#login-btn");
    if (!btn) return;
    btn.classList.toggle("is-disabled", missing);
    btn.setAttribute("aria-disabled", String(missing));
    btn.disabled = loginState?.phase === "running";
    const hint = dlg.querySelector("#login-hint");
    if (hint) { hint.hidden = !missing; hint.textContent = t("dialog.login.nameFirst"); }
  };
  nameInput.addEventListener("input", syncLoginBtn);

  // ----- RF4 per-field error + conflict-recovery helpers -----
  //
  // The core returns an `EditError::Validation { field, note }` whose
  // `field` names the offending schema field (e.g. "name",
  // "credential", "credential_env", "params.<key>", "interval",
  // or any provider-specific key). The dialog renders a `<label
  // data-field=…>` row for every input the user can edit, plus
  // `data-field="provider"` / `"name"` / `"credential"` / `"interval"`
  // for the static rows. Mapping the typed `field` to the row is
  // straightforward: for dynamic params we split on the first dot
  // and look up `data-field="f:<key>"`; everything else is a
  // literal token.
  const clearFieldErrors = () => {
    for (const row of dlg.querySelectorAll("[data-field]")) {
      row.classList.remove("invalid");
      // `codeStatus` is owned by the login code flow (it's set after
      // `login_submit_code`), so we deliberately leave it alone:
      // clearFieldErrors runs on every save attempt, but the user
      // can keep typing a code in flight.
      const small = row.querySelector(".field-err");
      if (small && small.id !== "login-code-status") {
        small.textContent = "";
        small.hidden = true;
      }
    }
    err.textContent = "";
    err.classList.remove("err-conflict");
    // Drop any banner from a previous failed save so the form opens
    // clean; new banner is rebuilt by `showConflict` when needed.
    const oldBanner = dlg.querySelector("#conflict-banner");
    if (oldBanner) oldBanner.remove();
  };

  // Resolve the `field` token produced by the core (`name`, `credential`,
  // `credential_env`, `interval`, `params.<key>`, `params`, `oauth`,
  // `field.<key>`) to the dialog row.
  const fieldRow = (field) => {
    if (!field) return null;
    // The `params` summary from `build_account` covers both a missing
    // `<key>` and any "params" structural failure; the per-key form
    // is the most specific row we can show, falling back to the first
    // visible dynamic row when no key was named.
    if (field.startsWith("params.") || field === "params" || field.startsWith("field.")) {
      const key = field.startsWith("field.") ? field.slice("field.".length) : field.startsWith("params.") ? field.slice("params.".length) : null;
      const row = key
        ? dlg.querySelector(`[data-field="f:${CSS.escape(key)}"]`)
        : dlg.querySelector('[data-field^="f:"]');
      return row || dlg.querySelector('[data-field="credential"]');
    }
    return dlg.querySelector(`[data-field="${CSS.escape(field)}"]`);
  };

  const setFieldError = (field, message) => {
    if (!field) {
      err.textContent = message;
      return;
    }
    const row = fieldRow(field);
    if (!row) {
      // Field was named but the dialog has no corresponding input
      // (e.g. `credential_env` derived from credential+oauth, or a
      // doc-wide rule that somehow carries a token): surface the
      // message globally instead of guessing a row.
      err.textContent = message;
      return;
    }
    row.classList.add("invalid");
    const small = row.querySelector(".field-err");
    if (small) {
      small.textContent = message;
      small.hidden = false;
      if (row.querySelector("input, select")) row.querySelector("input, select").focus({ preventScroll: true });
    } else {
      err.textContent = message;
    }
  };

  // RF4: a `Conflict` from the core means the file on disk changed
  // since we loaded it; show the typed message and offer a reload
  // button. The wizard draft stays in place so the user can retry
  // the same save afterwards without retyping.
  const showConflict = (message, reload) => {
    err.textContent = "";
    let banner = dlg.querySelector("#conflict-banner");
    if (!banner) {
      banner = document.createElement("div");
      banner.id = "conflict-banner";
      banner.className = "banner bad";
      err.parentNode.insertBefore(banner, err);
    }
    banner.innerHTML = `${esc(message)} <button type="button" class="btn" id="conflict-reload">${te("dialog.conflict.reload")}</button>`;
    banner.querySelector("#conflict-reload").addEventListener("click", () => reload());
  };

  clearFieldErrors();

  // Pull the structured envelope out of a value that may be a JSON
  // string from Tauri, a plain string, or a thrown object. Returns
  // the parsed `{ id, params, fallback, field? }` shape when it
  // matches the contract; otherwise `null` so the caller can fall
  // back to the legacy generic text path.
  const parseErrorEnvelope = (value) => {
    let obj = value;
    if (typeof value === "string") {
      try {
        obj = JSON.parse(value);
      } catch {
        return null;
      }
    } else if (value && typeof value === "object" && "message" in value) {
      obj = value.message;
      if (typeof obj === "string") {
        try { obj = JSON.parse(obj); }
        catch { return null; }
      } else {
        return null;
      }
    }
    if (obj && typeof obj === "object" && typeof obj.id === "string" && "fallback" in obj) {
      return obj;
    }
    return null;
  };

  // Reload the config from disk without closing the dialog so the
  // user can retry the same save. Re-runs the active schema fetch
  // only when the provider changed (rare for the wizard, but
  // cheap), then drops the conflict banner so a fresh save attempt
  // can succeed against the new snapshot.
  const reloadDialog = async () => {
    const reloadBtn = dlg.querySelector("#conflict-reload");
    if (reloadBtn) reloadBtn.disabled = true;
    try {
      const view = await reloadConfig();
      // Refresh the "name collision" form baseline in case another
      // account appeared while the dialog was open; renaming the
      // current edit into an existing name still fails at save
      // with the same field error.
      const exist = await getAccountExists(index, view);
      existing = exist;
      const cfg = view?.config_exists ? view : null;
      // Re-validate the name when the live config differs.
      clearFieldErrors();
      const banner = dlg.querySelector("#conflict-banner");
      if (banner) banner.remove();
      err.textContent = t("dialog.conflict.reloaded");
    } catch (x) {
      if (reloadBtn) reloadBtn.disabled = false;
      err.textContent = errText(x);
    }
  };

  // `get_account` requires an index; `null` accounts (a brand-new
  // dialog) re-fetch nothing and we just trust the current
  // `existing` shape.
  const getAccountExists = async (idx, _view) => {
    if (idx == null) return existing;
    try { return await getAccount(idx); }
    catch { return existing; }
  };

  const panel = dlg.querySelector("#login-panel");
  const log = dlg.querySelector("#login-log");
  const promptBox = dlg.querySelector("#login-prompt");
  const urlRow = dlg.querySelector("#login-url-row");
  const urlInput = dlg.querySelector("#login-url");
  const codeRow = dlg.querySelector("#login-code-row");
  const codeInput = dlg.querySelector("#login-code-input");
  const codeStatus = dlg.querySelector("#login-code-status");
  const usercodeRow = dlg.querySelector("#login-usercode-row");
  const codeText = dlg.querySelector("#login-code-text");

  const modes = () => schema.login?.modes ?? [];
  const selectedMode = () => modes().find((m) => m.id === authMode && !m.unavailable_reason) ?? null;
  const loginDone = () => loginState?.phase === "done";
  const loginParams = () => (loginDone() ? loginState.params : {});


  const collect = () => {
    for (const f of schema.fields) {
      const el = dyn.querySelector(`[name="f:${f.key}"]`);
      if (!el) continue;
      values.set(f.key, f.options
        ? { key: f.key, choice: Number(el.value), text: null }
        : { key: f.key, text: el.value, choice: null });
    }
  };
  const choices = () => Object.fromEntries(
    schema.fields
      .filter((f) => f.options && Number.isInteger(values.get(f.key)?.choice))
      .map((f) => [f.key, values.get(f.key).choice]),
  );
  function copyText(text, btn) {
    navigator.clipboard?.writeText(text).then(
      () => { const prev = btn.textContent; btn.textContent = t("dialog.copied"); setTimeout(() => (btn.textContent = prev), 1500); },
      () => { err.textContent = t("dialog.copyFailed"); },
    );
  }
  dlg.querySelector("#copy-url").addEventListener("click", (e) => copyText(urlInput.value, e.currentTarget));
  dlg.querySelector("#copy-code").addEventListener("click", (e) => copyText(codeText.textContent, e.currentTarget));

  // Cancelamento aguardado: o backend reage serialmente, então trocar de
  // modo/provider e iniciar um novo login não colide com o anterior.
  async function resetLogin() {
    const attempt = loginState?.attempt;
    if (loginState) await loginCancel(attempt);
    loginState = null;
    // Limpa envelopes pendentes: o novo login começa com attempt novo e
    // não deve herdar envelopes de uma tentativa anterior que ainda não
    // tinham attempt conhecido no momento da chegada.
    pendingEnvelopes = [];
    panel.hidden = true;
    codeRow.hidden = true;
    promptBox.hidden = true;
    codeInput.value = "";
    codeStatus.textContent = "";
  }

  function renderLoginPanel() {
    if (!loginState) { panel.hidden = true; return; }
    panel.hidden = false;
    log.innerHTML = loginState.lines.map((l) => `<p>${esc(l)}</p>`).join("");
    promptBox.hidden = !(loginState.url || loginState.userCode);
    urlRow.hidden = !loginState.url;
    if (loginState.url) urlInput.value = loginState.url;
    usercodeRow.hidden = !loginState.userCode;
    if (loginState.userCode) codeText.textContent = loginState.userCode;
    codeRow.hidden = loginState.phase !== "running" || !loginState.waitingCode;
    if (loginState.codeError) codeStatus.textContent = loginState.codeError;
    dlg.querySelector("#login-cancel").disabled = loginState.phase !== "running";
  }

  // Eventos tipados do core (RF2.1/RF2.5): nunca contêm tokens nem URLs de
  // callback com credencial. Cada evento chega envelopado em
  // `{ attempt, ...LoginEvent }`. Janela de aceitação: um evento é
  // enfileirado SOMENTE quando existe um `loginState` ativo e ainda sem
  // `attempt` real atribuído (a tentativa acaba de começar e o
  // `await loginStart` ainda não resolveu). Envelopes com `attempt`
  // não-zero e diferente do ativo são forwarders velhos e descartados.
  // Sem login ativo, descarta: o Tauri side já roteou o evento, e manter
  // em buffer seria unbounded.
  let pendingEnvelopes = [];
  let drainScheduled = false;
  const drainPending = () => {
    if (!loginState) return;
    if (loginState.attempt === 0) return; // ainda sem id; chamada feita cedo demais
    const ready = pendingEnvelopes;
    pendingEnvelopes = [];
    for (const ev of ready) dispatch(ev);
  };
  const unlisten = await onLogin((ev) => {
    if (!loginState) return; // sem login ativo: descarta
    if (typeof ev.attempt === "number" && ev.attempt !== loginState.attempt) {
      // attempt conhecido e diferente: forwarder velho. Descarta.
      if (loginState.attempt === 0) {
        // Janela start: ainda sem id, mas esse envelope já traz um
        // attempt concreto; pode ser a nossa tentativa chegando cedo.
        // Guarda até o id bater.
        pendingEnvelopes.push(ev);
      }
      return;
    }
    if (loginState.attempt === 0 && typeof ev.attempt === "number" && ev.attempt !== 0) {
      // Janela start: enfileira para drenar quando o id real chegar.
      pendingEnvelopes.push(ev);
      return;
    }
    dispatch(ev);
  });
  function dispatch(ev) {
    if (!loginState) return;
    if (typeof ev.attempt === "number" && ev.attempt !== 0 && ev.attempt !== loginState.attempt) return;
    switch (ev.type) {
      case "prompt":
        loginState.lines = ev.instructions ? [tl(ev.instructions)] : [];
        loginState.url = ev.url || null;
        loginState.userCode = ev.user_code ?? null;
        break;
      case "browser_unavailable":
        loginState.lines.push(t("dialog.login.browserUnavailable"));
        loginState.url = ev.url;
        break;
      case "progress":
        if (loginState.placeholder) { loginState.lines = []; loginState.placeholder = false; } // o core já diz o mesmo
        loginState.lines.push(tl(ev.text));
        break;
      case "waiting_code":
        loginState.waitingCode = true;
        break;
      case "code_rejected":
        loginState.codeError = tl(ev.message);
        break;
      case "finished": {
        loginState.phase = "done";
        loginState.params = ev.params ?? {};
        // Aplica os metadados públicos devolvidos pelo login aos campos.
        for (const [key, value] of Object.entries(loginState.params)) {
          const f = schema.fields.find((x) => x.key === key);
          if (!f) continue;
          const option = f.options?.find((candidate) => candidate.value === value);
          values.set(key, f.options
            ? { key, choice: option?.index ?? f.options[0]?.index ?? 0, text: null }
            : { key, text: value, choice: null });
        }
        loginState.lines.push(selectedMode()?.completion_notice
          ? tl(selectedMode().completion_notice)
          : t("dialog.login.authorized"));
        // Atualiza o formulário com os parâmetros do login e só então salva (o `collect()` do salvar lê esses campos).
        renderDynamic().then(() => {
          if (!dlg.open || loginState?.phase !== "done" || committing) return;
          loginState.lines.push(t("dialog.login.saving"));
          renderLoginPanel();
          form.requestSubmit();
        });
        break;
      }
      case "failed":
        loginState.phase = "failed";
        loginState.lines.push(tl(ev.message));
        break;
    }
    renderLoginPanel();
    if (ev.type === "waiting_code") codeInput.focus();
  }
  dlg.addEventListener("close", () => {
    unlisten();
    // Fechar o diálogo com login ativo e sem commit cancela o fluxo: o token
    // autorizado morre em memória e nada é gravado no cofre (RF2.4).
    if (loginState && !committing) {
      const attempt = loginState.attempt;
      loginCancel(attempt);
    }
  });
  dlg.addEventListener("cancel", (e) => {
    if (committing) e.preventDefault();
  });

  dlg.querySelector("#login-cancel").addEventListener("click", async () => {
    const attempt = loginState?.attempt;
    await loginCancel(attempt);
    loginState = null;
    pendingEnvelopes = [];
    panel.hidden = true;
    codeRow.hidden = true;
    promptBox.hidden = true;
    codeInput.value = "";
    codeStatus.textContent = "";
    renderDynamic();
  });
  dlg.querySelector("#login-code-send").addEventListener("click", async () => {
    const code = codeInput.value;
    if (!code.trim()) { codeStatus.textContent = t("dialog.login.pasteFirst"); return; }
    codeStatus.textContent = "";
    try {
      await loginSubmitCode(code);
      codeInput.value = "";
      codeStatus.textContent = t("dialog.login.validating");
    } catch (x) {
      codeStatus.textContent = errText(x);
    }
  });

  const collectModeInputs = (mode) => {
    const options = {};
    const registration = {};
    if (!mode) return { options, registration };
    const read = (option, isRegistration) => {
      const prefix = isRegistration ? "r" : "o";
      const el = dyn.querySelector(`[name="${prefix}:${option.key}"]`);
      if (!el) return "";
      return option.choices
        ? option.choices[Number(el.value)]?.value ?? ""
        : el.value.trim();
    };
    for (const option of mode.options ?? []) {
      const value = read(option, false);
      if (value !== "") options[option.key] = value;
    }
    for (const option of mode.registration ?? []) {
      const value = read(option, true);
      if (value !== "") registration[option.key] = value;
    }
    return { options, registration };
  };

  const syncModeChoicesToFields = (mode) => {
    const { options } = collectModeInputs(mode);
    for (const [key, value] of Object.entries(options)) {
      const field = schema.fields.find((candidate) => candidate.key === key && candidate.options);
      const option = field?.options.find((candidate) => candidate.value === value);
      if (field && option) {
        values.set(key, { key, choice: option.index, text: null });
      }
    }
  };

  async function startLogin(btn) {
    const mode = selectedMode();
    if (!mode) return;
    err.textContent = "";
    if (nameInput.value.trim() === "") { showNameError(); return; }
    await resetLogin();
    const { options, registration } = collectModeInputs(mode);
    btn.disabled = true;
    pendingEnvelopes = [];
    loginState = { phase: "running", lines: [t("dialog.login.preparing")], placeholder: true, url: null, userCode: null, waitingCode: false, params: {}, codeError: "", attempt: 0 };
    renderLoginPanel();
    try {
      const attempt = await loginStart({ provider, index, mode: mode.id, options, registration });
      loginState.attempt = attempt;
      drainPending();
    } catch (x) {
      err.textContent = errText(x);
      loginState = null;
      renderLoginPanel();
    }
    syncLoginBtn();
  }

  async function renderDynamic() {
    const optSnapshot = new Map();
    for (const el of dyn.querySelectorAll("[name^='o:'],[name^='r:']")) {
      optSnapshot.set(el.name, el.value);
    }
    const resolvedChoices = await fieldChoices(provider, choices(), authMode);
    const notices = [];
    for (const resolved of resolvedChoices) {
      const field = schema.fields.find((candidate) => candidate.key === resolved.key);
      if (!field) continue;
      field.options = resolved.options;
      values.set(resolved.key, { key: resolved.key, choice: resolved.choice, text: null });
      if (resolved.notice) notices.push(tl(resolved.notice));
    }
    choiceNotice.textContent = notices.join(" ");
    choiceNotice.hidden = notices.length === 0;
    visible = new Set(await visibleFields(provider, choices(), authMode, loginParams()));
    if (schema.cost_advisory) {
      costWarn.textContent = tl(schema.cost_advisory);
      costWarn.hidden = false;
    } else {
      costWarn.textContent = "";
      costWarn.hidden = true;
    }
    const cred = schema.credential_kind;
    const mode = selectedMode();
    const available = modes().filter((m) => !m.unavailable_reason);
    const unavailable = modes().filter((m) => m.unavailable_reason);

    let authHtml = "";
    if (modes().length || cred !== "none") {
      const radios = [];
      if (cred !== "none") {
        const label = t(cred === "profile" ? "dialog.credProfile" : "dialog.credEnvShort");
        radios.push(`<label class="mode"><input type="radio" name="auth" value="credential" ${authMode === "credential" ? "checked" : ""} /> ${esc(label)}</label>`);
      }
      for (const m of available) {
        radios.push(`<label class="mode"><input type="radio" name="auth" value="${esc(m.id)}" ${authMode === m.id ? "checked" : ""} /> ${esc(tl(m.label, { id: m.id }))}${m.help ? ` <small>${esc(tl(m.help, { id: m.id }))}</small>` : ""}</label>`);
      }
      for (const m of unavailable) {
        radios.push(`<label class="mode mode-off"><input type="radio" name="auth" value="${esc(m.id)}" disabled /> ${esc(tl(m.label, { id: m.id }))} <small>${esc(tl(m.unavailable_reason))}</small></label>`);
      }
      authHtml = `<fieldset class="auth"><legend>${te("dialog.authLegend")}</legend>${radios.join("")}</fieldset>`;
    }

    let bodyHtml = "";
    if (!mode) {
      bodyHtml = cred === "none"
        ? `<p class="hint">${te("dialog.credNone")}</p>`
        : `<label class="f" data-field="credential"><span>${te(cred === "profile" ? "dialog.credProfile" : "dialog.credEnv")}</span>
            <input name="credential" value="${esc(existing?.credential ?? "")}" placeholder="${esc(cred === "env" ? schema.default_env : "")}" required />
            <small class="field-err" hidden></small>
            <small>${esc(tl(schema.credential_help))}</small></label>`;
    } else {
      const renderOption = (option, isRegistration) => {
        const prefix = isRegistration ? "r" : "o";
        const sharedField = !isRegistration
          ? schema.fields.find((field) => field.key === option.key && field.options)
          : null;
        const sharedChoice = sharedField?.options.find(
          (choice) => choice.index === values.get(option.key)?.choice,
        )?.value;
        const prefill = sharedChoice ?? (isRegistration
          ? existing?.auth_registration?.[option.key] ?? ""
          : existing?.auth_options?.[option.key] ?? "");
        const input = option.choices
          ? `<select name="${prefix}:${esc(option.key)}">${option.choices.map((choice, i) => `<option value="${i}" ${(prefill && choice.value === prefill) || (!prefill && i === 0) ? "selected" : ""}>${esc(tl(choice.label))}</option>`).join("")}</select>`
          : `<input name="${prefix}:${esc(option.key)}" value="${esc(prefill)}" ${option.required ? "required" : ""} />`;
        return `<label class="f"><span>${esc(tl(option.label, { key: option.key }))}${option.required ? " *" : ""}</span>${input}${option.help ? `<small>${esc(tl(option.help, { key: option.key }))}</small>` : ""}</label>`;
      };
      const optInputs = [
        ...(mode.registration ?? []).map((option) => renderOption(option, true)),
        ...(mode.options ?? []).map((option) => renderOption(option, false)),
      ];
      const button = loginDone()
        ? `<p class="hint">${te("dialog.login.doneHint")}</p>
           <button type="button" class="btn" id="login-btn">${te("dialog.login.redo")}</button>`
        : `<button type="button" class="btn" id="login-btn">${te(existing?.uses_oauth ? "dialog.login.reauth" : "dialog.login.start")}</button>`;
      const loginHint = `<p class="hint login-hint" id="login-hint" hidden></p>`;
      bodyHtml = optInputs.join("") + button + loginHint;
    }

    dynAuth.innerHTML = authHtml + bodyHtml;
    dynFields.innerHTML =
      schema.fields.filter((f) => visible.has(f.key)).map((f) => {
        const v = values.get(f.key);
        const input = f.options
          ? `<select name="f:${esc(f.key)}">${f.options.map((option) => `<option value="${option.index}" ${(v?.choice ?? f.options[0]?.index ?? 0) === option.index ? "selected" : ""}>${esc(tl(option.label))}</option>`).join("")}</select>`
          : `<input name="f:${esc(f.key)}" value="${esc(v?.text ?? "")}" ${f.required ? "required" : ""} />`;
        return `<label class="f" data-field="f:${esc(f.key)}"><span>${esc(tl(f.label))}${f.required ? " *" : ""}</span>${input}<small class="field-err" hidden></small>${f.help ? `<small>${esc(tl(f.help))}</small>` : ""}</label>`;
      }).join("");
    for (const [name, value] of optSnapshot) {
      const el = dyn.querySelector(`[name="${name}"]`);
      if (el && value !== "") el.value = value;
    }
    const loginBtn = dyn.querySelector("#login-btn");
    if (loginBtn) loginBtn.addEventListener("click", () => startLogin(loginBtn));
    syncLoginBtn();
  }

  dlg.addEventListener("change", async (e) => {
    if (e.target.name === "provider") {
      provider = e.target.value;
      schema = await formSchema(provider);
      values.clear();
      await resetLogin();
      authMode = schema.credential_kind === "none" ? (modes()[0]?.id ?? "credential") : "credential";
      await renderDynamic();
      renderModels();
    } else if (e.target.name === "auth") {
      syncModeChoicesToFields(selectedMode());
      await resetLogin();
      authMode = e.target.value;
      await renderDynamic();
    } else if (e.target.name?.startsWith("o:") || e.target.name?.startsWith("r:")) {
      syncModeChoicesToFields(selectedMode());
      await resetLogin();
      await renderDynamic();
    } else if (e.target.name?.startsWith("f:")) {
      collect();
      if (e.target.tagName === "SELECT") await renderDynamic();
    }
  });
  dlg.querySelector("[data-cancel]").addEventListener("click", () => {
    if (!committing) dlg.close();
  });
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (committing) return;
    collect();
    const ok = dlg.querySelector("#ok");
    err.textContent = "";
    const mode = selectedMode();
    if (loginState?.phase === "running") {
      err.textContent = t("dialog.save.loginRunning");
      return;
    }
    if (mode?.credential_store === "cuotai_vault" && !loginDone()) {
      const sameAsExisting = !!existing?.uses_oauth && existing?.auth_mode === authMode;
      if (!sameAsExisting) {
        err.textContent = t("dialog.save.needLogin");
        return;
      }
    }
    ok.disabled = true;
    committing = true;
    dlg.querySelector("[data-cancel]").disabled = true;
    const data = new FormData(form);
    try {
      const { options } = collectModeInputs(mode);
      const credential = mode ? "" : String(data.get("credential") ?? "");
      const result = await saveAccount({
        index, provider, name: String(data.get("name") ?? ""),
        credential,
        fields: schema.fields.filter((f) => visible.has(f.key)).map((f) => ({ key: f.key, text: values.get(f.key)?.text ?? (f.options ? null : ""), choice: values.get(f.key)?.choice ?? (f.options ? 0 : null) })),
        interval: String(data.get("interval") ?? ""),
        commitLogin: loginDone(),
        loginAttempt: loginState?.attempt ?? 0,
        authMode,
        authOptions: options,
        authParams: loginDone() ? loginState.params : {},
      });
      committing = false;
      dlg.close();
      onSaved(result);
    } catch (x) {
      // RF4: typed per-field errors + conflict reload offer.
      // Errors come from the backend as JSON strings with
      // `{ id, params, fallback, field? }`. Parse the envelope here
      // (without going through `errText`, which only renders the
      // human text) so a `field` token points at the corresponding
      // form row, and so a `Conflict` shows the reload banner
      // instead of just a red message.
      const parsed = parseErrorEnvelope(x);
      clearFieldErrors();
      if (parsed && parsed.id === "edit_error.conflict") {
        // Offer the reload — the user keeps the dialog draft and
        // can retry the same save afterwards. The button replaces
        // the OK button's role until reload completes, so we
        // re-enable submit/cancel so the user isn't trapped.
        showConflict(errText(parsed), () => reloadDialog());
        ok.disabled = false;
        committing = false;
        dlg.querySelector("[data-cancel]").disabled = false;
        return;
      }
      if (parsed && parsed.field) {
        setFieldError(parsed.field, errText(parsed));
      } else {
        err.textContent = errText(x);
      }
      ok.disabled = false;
      committing = false;
      dlg.querySelector("[data-cancel]").disabled = false;
    }
  });
  // PRD 0002 RF6.1: catálogo da conta em edição; numa conta nova, escolha explícita de uma conta configurada do
  // mesmo provedor (a nova ainda não tem credencial salva) ou a orientação do core quando não há nenhuma.
  const modelsHost = dlg.querySelector("#models-host");
  let models = null;
  let modelsSeq = 0;
  const mountIn = (key, i, generation) => {
    models?.dispose();
    models = key == null ? null : mountModels({ key, index: i, generation });
    modelsHost.querySelector("[data-models-slot]").replaceChildren(...(models ? [models.el] : []));
  };
  async function renderModels() {
    const seq = ++modelsSeq;
    const view = await getView().catch(() => null);
    const generation = view?.generation ?? 0;
    const same = editing ? [] : (view?.reports ?? []).filter((r) => r.provider === provider);
    const support = !editing && !same.length ? await modelSupport(provider).catch(() => null) : null;
    if (seq !== modelsSeq) return; // o provedor mudou durante a leitura
    models?.dispose();
    models = null;
    modelsHost.innerHTML = editing || !same.length
      ? `<div data-models-slot></div>${support ? `<p class="hint">${te("models.title")}: ${tl(support.text, {}, true)}${support.supported ? ` ${te("models.saveFirst")}` : ""}</p>` : ""}`
      : `<label class="f"><span>${te("models.pick")}</span><select id="models-pick"><option value="">${te("models.pickNone")}</option>${same.map((r) => `<option value="${esc(reportKey(r))}">${esc(r.account)}</option>`).join("")}</select></label><div data-models-slot></div>`;
    if (editing) return mountIn(`${existing.provider}::${existing.name}`, index, generation);
    modelsHost.querySelector("#models-pick")?.addEventListener("change", (e) => {
      const r = same.find((x) => reportKey(x) === e.target.value);
      mountIn(r ? reportKey(r) : null, r?.index, generation);
    });
  }
  dlg.addEventListener("close", () => { modelsSeq++; models?.dispose(); });
  await renderDynamic();
  renderModels();
  dlg.querySelector("[name=name]").focus();
}

/** Confirma a remoção. Conta com credencial no cofre pergunta se a credencial também deve ser apagada.
 *  `onDone(resultado)` recebe o que o core fez (removida, apagada do cofre, mantida, compartilhada…). */
export function confirmDelete(index, label, { usesOauth = false, onDone = () => {} } = {}) {
  const buttons = usesOauth
    ? `<button type="button" class="btn" data-purge="0">${te("dialog.deleteVaultKeep")}</button>
       <button type="button" class="btn danger-btn" data-purge="1">${te("dialog.deleteVaultPurge")}</button>`
    : `<button type="button" class="btn danger-btn" data-purge="0">${te("common.remove")}</button>`;
  const dlg = open(`<form method="dialog" class="dlg-body">
    <h2>${te("dialog.deleteTitle")}</h2>
    <p>${te(usesOauth ? "dialog.deleteBodyVault" : "dialog.deleteBody", { label })}</p>
    <p class="err" id="err" role="alert"></p>
    <div class="dlg-actions"><button type="button" class="btn" data-cancel>${te("common.cancel")}</button>${buttons}</div>
  </form>`);
  dlg.querySelector("[data-cancel]").addEventListener("click", () => dlg.close());
  for (const btn of dlg.querySelectorAll("[data-purge]")) {
    btn.addEventListener("click", async () => {
      dlg.querySelectorAll("button").forEach((b) => (b.disabled = true));
      try {
        const result = await deleteAccount(index, btn.dataset.purge === "1");
        dlg.close();
        onDone(result);
      } catch (x) {
        dlg.querySelector("#err").textContent = errText(x);
        dlg.querySelectorAll("button").forEach((b) => (b.disabled = false));
      }
    });
  }
}

/** Modal prompt offered when the file on disk drifted from the snapshot we
 *  hold (detected via the backend's `config_changed_on_disk`): on opening an
 *  account editor and when the main window regains focus. Resolves
 *  `"reload"` when the user chose to refresh the snapshot, or `"keep"` when
 *  they accept editing against a stale snapshot. */
export function offerExternalReload() {
  return new Promise((resolve) => {
    const dlg = document.createElement("dialog");
    dlg.className = "dlg";
    dlg.innerHTML = `<form method="dialog" class="dlg-body">
      <h2>${te("dialog.external.title")}</h2>
      <p>${te("dialog.external.body")}</p>
      <p class="err" id="err" role="alert"></p>
      <div class="dlg-actions">
        <button type="button" class="btn" data-choice="keep">${te("dialog.external.keep")}</button>
        <button type="button" class="btn primary" data-choice="reload">${te("dialog.external.reload")}</button>
      </div>
    </form>`;
    document.body.append(dlg);
    showDialog(dlg);
    for (const btn of dlg.querySelectorAll("[data-choice]")) {
      btn.addEventListener("click", () => {
        const choice = btn.dataset.choice;
        dlg.close();
        resolve(choice);
      });
    }
  });
}
