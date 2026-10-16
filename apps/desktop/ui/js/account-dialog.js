// Formulário de conta: todo o esquema, visibilidade e validação vêm do core.
// RF2: o login é orquestrado pelo core; este diálogo só renderiza eventos
// tipados (LoginEvent, sem segredos) e envia entradas do usuário.
import {
  deleteAccount, esc, formSchema, getAccount, listProviders, loginCancel,
  loginStart, loginSubmitCode, onLogin, saveAccount, visibleFields,
} from "./lib.js";
import { currentLocale, t, te, tl } from "./i18n.js";

function open(html) {
  const dlg = document.createElement("dialog");
  dlg.className = "dlg";
  dlg.innerHTML = html;
  document.body.append(dlg);
  dlg.addEventListener("close", () => dlg.remove());
  dlg.showModal();
  return dlg;
}
// RF8: backend errors are delivered as JSON strings that decode to
// `{ id, params, fallback }`; legacy callers still emit plain strings.
// `tl` handles both shapes; we keep the diagnostic "Falha inesperada."
// fallback for the truly unexpected case.
const errText = (e) => {
  if (e && typeof e === "object" && "id" in e) return tl(e);
  if (typeof e === "string") {
    try {
      const parsed = JSON.parse(e);
      if (parsed && typeof parsed === "object" && "id" in parsed) return tl(parsed);
    } catch {
      return e;
    }
    return e;
  }
  if (e?.message) return errText(e.message);
  return t("common.unexpected");
};

// Chaves de registro de cliente OAuth: viram campos dedicados do pedido de
// login (nunca opções genéricas), conforme o contrato do core.
const REGISTRATION_KEYS = ["client_id", "client_secret_env", "tenant"];

export async function openAccountDialog(index = null) {
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
    <label class="f"><span>${te("dialog.provider")}</span>
      <select name="provider" ${editing ? "disabled" : ""}>${providers.map((p) => `<option value="${esc(p.id)}" ${p.id === provider ? "selected" : ""}>${esc(p.label)}</option>`).join("")}</select></label>
    <label class="f"><span>${te("dialog.name")}</span><input name="name" value="${esc(existing?.name ?? "")}" required /><small class="field-err" id="name-err" role="alert" hidden></small></label>
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
        <label class="f"><span>${te("dialog.login.codeLabel")}</span>
          <input id="login-code-input" autocomplete="off" placeholder="${te("dialog.login.codePlaceholder")}" />
          <small id="login-code-status" role="alert"></small></label>
        <div class="dlg-actions"><button type="button" class="btn primary" id="login-code-send">${te("dialog.login.sendCode")}</button></div>
      </div>
      <div class="dlg-actions"><button type="button" class="btn" id="login-cancel">${te("dialog.login.cancel")}</button></div>
    </div>
    <div id="dyn-fields"></div></div>
    <label class="f"><span>${te("dialog.interval")}</span><input name="interval" inputmode="numeric" placeholder="${te("dialog.intervalPlaceholder")}" value="${esc(existing?.interval ?? "")}" /></label>
    <p class="cost-advisory" id="cost-warn" hidden></p>
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

  // Stable dedicated profile for Bailian's `bl` login; the same value must
  // reach its CLI invocation and the saved account. AWS SSO instead requires
  // an already configured AWS CLI profile entered by the user.
  const sessionSlug = (() => {
    const profile = existing?.credential;
    if (schema.credential_kind === "none" && profile && /^[A-Za-z0-9_-]{1,32}$/.test(profile)
        && !profile.startsWith("-")) return profile;
    const bytes = new Uint8Array(8);
    crypto.getRandomValues(bytes);
    return "cuotai-" + Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  })();

  const collect = () => {
    for (const f of schema.fields) {
      const el = dyn.querySelector(`[name="f:${f.key}"]`);
      if (!el) continue;
      values.set(f.key, f.options ? { key: f.key, choice: Number(el.value), text: null } : { key: f.key, text: el.value, choice: null });
    }
  };
  const choices = () => Object.fromEntries(schema.fields.filter((f) => f.options).map((f) => [f.key, values.get(f.key)?.choice ?? 0]));

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
          values.set(key, f.options
            ? { key, choice: Math.max(0, f.options.findIndex((opt) => opt.value === value)), text: null }
            : { key, text: value, choice: null });
        }
        loginState.lines.push(t("dialog.login.authorized"));
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
    unlisten.then((f) => f());
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

  async function startLogin(btn) {
    const mode = selectedMode();
    if (!mode) return;
    err.textContent = "";
    if (nameInput.value.trim() === "") { showNameError(); return; }
    // Zera qualquer login pendente para que o backend não rejeite a nova
    // tentativa como "já em andamento".
    await resetLogin();
    const options = {};
    for (const o of mode.options) {
      const el = dyn.querySelector(`[name="o:${o.key}"]`);
      if (!el) continue;
      // A dedicated Bailian profile must match the later account save;
      // AWS uses the configured profile entered by the user.
      let prefill = existing?.auth_options?.[o.key] ?? "";
      if (o.key === "profile" && el.value === "" && mode.id === "bailian_console") {
        if (prefill === "") prefill = sessionSlug;
        el.value = prefill;
      } else if (el.value === "" && prefill !== "") {
        el.value = prefill;
      }
      // Como na TUI: opção de texto vazia não vai no pedido (o core copia as opções como vieram).
      if (o.choices) options[o.key] = o.choices[Number(el.value)]?.value ?? "";
      else if (el.value.trim() !== "") options[o.key] = el.value.trim();
    }
    // Bailian exige um perfil dedicado não vazio antes do login; o
    // `start_login` do core rejeita sem isso.
    if (mode.id === "bailian_console" && !options.profile) {
      err.textContent = t("dialog.login.profileRequired");
      return;
    }
    const reg = {};
    if (mode.requires_client_registration) {
      for (const key of REGISTRATION_KEYS) {
        const el = dyn.querySelector(`[name="o:${key}"]`);
        if (!el) continue;
        if (el.value === "") {
          if (key === "client_id" && existing?.client_id) el.value = existing.client_id;
          else if (key === "client_secret_env" && existing?.client_secret_env) el.value = existing.client_secret_env;
          else if (key === "tenant" && existing?.tenant) el.value = existing.tenant;
        }
        reg[key] = el.value.trim();
      }
      if (!reg.client_id) {
        err.textContent = t("dialog.login.clientIdRequired");
        return;
      }
    }
    btn.disabled = true;
    // Zera pendências: nada de uma tentativa anterior pode drenar para a nova.
    pendingEnvelopes = [];
    // Estabelece o estado ANTES do await: para fluxos instantâneos (Cursor
    // import, AWS sso terminado de cara) o `loginStart` pode resolver antes
    // do próximo paint, e o evento `Finished` chega quase junto. Ter o
    // loginState já presente garante que o handler de evento o encontre.
    loginState = { phase: "running", lines: [t("dialog.login.preparing")], placeholder: true, url: null, userCode: null, waitingCode: false, params: {}, codeError: "", attempt: 0, options };
    renderLoginPanel();
    try {
      const attempt = await loginStart({
        provider, index, mode: mode.id, options,
        clientId: reg.client_id || null,
        clientSecretEnv: reg.client_secret_env || null,
        tenant: reg.tenant || null,
      });
      loginState.attempt = attempt;
      // Drena envelopes que chegaram antes do `await loginStart` resolver
      // (fluxos instantâneos: Cursor import, AWS sso instantâneo).
      drainPending();
    } catch (x) {
      err.textContent = errText(x);
      loginState = null;
      renderLoginPanel();
    }
    syncLoginBtn();
  }

  async function renderDynamic() {
    // Snapshot do que o usuário já digitou na seção de autenticação para
    // sobreviver ao `innerHTML` abaixo. Os valores de `f:*` continuam no
    // mapa `values` (atualizado por `collect`); aqui guardamos apenas os
    // `o:*` (opções do modo + registro) e o radio `auth` corrente.
    const optSnapshot = new Map();
    for (const el of dyn.querySelectorAll("[name^='o:']")) {
      optSnapshot.set(el.name, el.value);
    }
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
        : `<label class="f"><span>${te(cred === "profile" ? "dialog.credProfile" : "dialog.credEnv")}</span>
            <input name="credential" value="${esc(existing?.credential ?? "")}" placeholder="${esc(cred === "env" ? schema.default_env : "")}" required />
            <small>${esc(tl(schema.credential_help))}</small></label>`;
    } else {
      const optInputs = mode.options.map((o) => {
        const prefill = existing?.auth_options?.[o.key] ?? "";
        const input = o.choices
          ? `<select name="o:${esc(o.key)}">${o.choices.map((choice, i) => `<option value="${i}" ${(prefill && choice.value === prefill) || (!prefill && i === 0) ? "selected" : ""}>${esc(tl(choice.label))}</option>`).join("")}</select>`
          : `<input name="o:${esc(o.key)}" value="${esc(prefill)}" />`;
        return `<label class="f"><span>${esc(tl(o.label, { key: o.key }))}</span>${input}${o.help ? `<small>${esc(tl(o.help, { key: o.key }))}</small>` : ""}</label>`;
      });
      if (mode.requires_client_registration) {
        const has = (k) => mode.options.some((o) => o.key === k);
        if (!has("client_id")) {
          const prefill = existing?.client_id ?? "";
          optInputs.push(`<label class="f"><span>${te("dialog.reg.clientId")}</span><input name="o:client_id" value="${esc(prefill)}" required /><small>${te("dialog.reg.clientIdHelp")}</small></label>`);
        }
        if (!has("client_secret_env")) {
          const prefill = existing?.client_secret_env ?? "";
          optInputs.push(`<label class="f"><span>${te("dialog.reg.secretEnv")}</span><input name="o:client_secret_env" value="${esc(prefill)}" /><small>${te("dialog.reg.secretEnvHelp")}</small></label>`);
        }
        if (!has("tenant")) {
          const prefill = existing?.tenant ?? "";
          optInputs.push(`<label class="f"><span>${te("dialog.reg.tenant")}</span><input name="o:tenant" value="${esc(prefill)}" /><small>${te("dialog.reg.tenantHelp")}</small></label>`);
        }
      }
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
          ? `<select name="f:${esc(f.key)}">${f.options.map((option, i) => `<option value="${i}" ${(v?.choice ?? 0) === i ? "selected" : ""}>${esc(tl(option.label))}</option>`).join("")}</select>`
          : `<input name="f:${esc(f.key)}" value="${esc(v?.text ?? "")}" ${f.required ? "required" : ""} />`;
        return `<label class="f"><span>${esc(tl(f.label))}${f.required ? " *" : ""}</span>${input}${f.help ? `<small>${esc(tl(f.help))}</small>` : ""}</label>`;
      }).join("");
    // Restaura o que o usuário já tinha digitado nos inputs `o:*` (opções
    // do modo + registro) para que mudanças em `f:` não apaguem a área
    // de autenticação. Não restaura o radio `auth` porque o handler de
    // change já cuida dele.
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
    } else if (e.target.name === "auth") {
      // Trocar de modo invalida qualquer autorização anterior (identidade muda).
      await resetLogin();
      authMode = e.target.value;
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
    // Bloqueia salvar se há login em andamento: enquanto o forwarder
    // pode emitir Finished contra a config que estava em disco no início
    // do save, misturar commit (TOML) com evento ainda em voo corromperia
    // a ordem. Aplica-se tanto a OAuth quanto a qualquer modo.
    if (loginState?.phase === "running") {
      err.textContent = t("dialog.save.loginRunning");
      return;
    }
    // Modo OAuth (sem opção "profile" para servir de credencial) sem
    // login concluído: bloquear. Para contas já OAuth, a exceção de
    // "edição sem novo login" só vale quando o `authMode` escolhido
    // coincide com o da config existente — qualquer outro modo OAuth
    // exigiria nova autorização.
    if (mode && !mode.options.some((o) => o.key === "profile") && !loginDone()) {
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
      const profileOpt = mode?.options.find((o) => o.key === "profile");
      const profileEl = profileOpt ? dyn.querySelector('[name="o:profile"]') : null;
      // Modo `aws_sso`: o profile AWS vai em `credential` (o core grava
      // via `apply_aws_profile` em `params["profile"]`).
      // Modo `bailian_console`: o `profile` (perfil dedicado do `bl`) vai
      // em `cliProfileOverride` para que o backend passe em
      // `credential_params = Some({ cli_profile })`.
      const profileValue = profileEl ? (profileOpt.choices ? profileOpt.choices[Number(profileEl.value)]?.value ?? "" : profileEl.value.trim()) : "";
      if (loginDone() && mode?.options.some((o) => o.key === "profile") && profileValue !== loginState.options.profile) {
        throw t("dialog.save.profileChanged");
      }
      const credential = mode?.id === "aws_sso"
        ? profileValue
        : (mode ? "" : String(data.get("credential") ?? ""));
      await saveAccount({
        index, provider, name: String(data.get("name") ?? ""),
        credential,
        fields: schema.fields.filter((f) => visible.has(f.key)).map((f) => ({ key: f.key, text: values.get(f.key)?.text ?? (f.options ? null : ""), choice: values.get(f.key)?.choice ?? (f.options ? 0 : null) })),
        interval: String(data.get("interval") ?? ""),
        commitLogin: loginDone(),
        loginAttempt: loginState?.attempt ?? 0,
        authMode,
        cliProfileOverride: mode?.id === "bailian_console" ? profileValue : "",
      });
      committing = false;
      dlg.close();
    } catch (x) {
      err.textContent = errText(x);
      ok.disabled = false;
      committing = false;
      dlg.querySelector("[data-cancel]").disabled = false;
    }
  });
  await renderDynamic();
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
