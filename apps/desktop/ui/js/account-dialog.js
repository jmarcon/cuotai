// Account form: the whole schema, visibility and validation come from the core.
// RF2: the login is orchestrated by the core; this dialog only renders typed
// events (LoginEvent, no secrets) and sends user input.
import {
  accountLabel, configChangedOnDisk, deleteAccount, detectSources, forgetExtensionSecrets, dismissDetected, draftAccess, errText, esc, fieldChoices, formSchema, getAccount,
  getSettings, getView, icon, listProviders, logo, NEW_PROVIDER_URL, openExternal, providerModes, setDetectEnabled,
  loginCancel, loginStart, loginSubmitCode, forgetSession, migrateSession, modelSupport, onLogin, previewSession, reloadConfig,
  reportKey, saveAccount, sessionNoticeHtml, testAggregator, testSession, validMonogram, visibleFields,
} from "./lib.js";
import { mountModels } from "./detail.js";
import { entryChips, mountPicker, pickerHtml } from "./provider-picker.js";
import { t, te, tl } from "./i18n.js";
import { showDialog } from "./motion.js";

function open(html) {
  const dlg = document.createElement("dialog");
  dlg.className = "dlg";
  dlg.innerHTML = html;
  document.body.append(dlg);
  showDialog(dlg);
  return dlg;
}

export async function openAccountDialog(index = null, { onSaved = () => {}, pasteSession = false, onExtension = null } = {}) {
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
  // Core decides which providers can be added (`pickable`); the account being edited keeps its own.
  let existing = editing ? await getAccount(index) : null;
  const providers = (await listProviders()).filter((p) => p.pickable || p.id === existing?.provider);
  // The order is the core's (`picker_providers`), the same one the TUI shows; never re-sorted here.
  let provider = existing?.provider ?? providers[0].id;
  let schema = await formSchema(provider);
  // Current values by key: { text } or { choice }.
  const values = new Map((existing?.fields ?? []).map((f) => [f.key, f]));
  // Initial authMode: prefers the existing config (oauth or credential) or
  // the core's first mode when the provider has no manual credential.
  let authMode = existing?.auth_mode
    || (schema.credential_kind === "none"
      ? ((schema.login?.modes ?? []).find((m) => !m.unavailable_reason)?.id ?? "credential")
      : "credential");
  let visible = new Set();
  // RF2 login state: null or { phase, lines, url, userCode, params, codeError, attempt }.
  // phase: "running" | "done" | "failed". The token NEVER passes through here.
  let loginState = null;
  let committing = false;

  const dlg = open(`<form method="dialog" class="dlg-body" autocomplete="off">
    <header class="dlg-head"><h2>${te(editing ? "dialog.editTitle" : "dialog.addTitle")}</h2>
      <button type="button" class="icon" data-x aria-label="${te("common.close")}" title="${te("common.close")}">${icon("x")}</button></header>
    <div class="f" data-field="provider"><span id="prov-label">${te("dialog.provider")}</span>
      ${editing ? "" : pickerHtml("prov-label")}
      <div class="prov-head" id="prov-head" ${editing ? "" : "hidden"}></div>
      <small class="field-err" hidden></small></div>
    <div class="form-rest" id="form-rest" ${editing ? "" : "hidden"}>
    <p class="cost-advisory offer-consent" id="offer-consent" role="note" hidden></p>
    <label class="f" data-field="name"><span>${te("dialog.nameOptional")}</span><input name="name" value="${esc(existing?.name ?? "")}" aria-describedby="name-hint" /><small class="hint" id="name-hint"></small><small class="field-err" id="name-err" role="alert" hidden></small></label>
    <label class="f" data-field="peer_name" id="peer-row" hidden><span id="peer-label"></span><input name="peer_name" /><small class="field-err" role="alert" hidden></small></label>
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
    <div class="session-box" id="session-box" hidden></div>
    <div class="session-box" id="move-box" hidden></div>
    <div class="session-box" id="aggregator-box" hidden>
      <h3>${te("dialog.aggregator.title")}</h3>
      <p class="hint">${te("dialog.aggregator.hint")}</p>
      <div class="dlg-actions"><button type="button" class="btn" id="aggregator-test">${te("dialog.aggregator.test")}</button></div>
      <p class="hint" id="aggregator-status" role="status" aria-live="polite"></p>
    </div>
    <label class="f" data-field="interval"><span>${te("dialog.interval")}</span><input name="interval" inputmode="numeric" placeholder="${te("dialog.intervalPlaceholder")}" value="${esc(existing?.interval ?? "")}" /><small class="field-err" hidden></small></label>
    <label class="f" data-field="group"><span>${te("dialog.group")}</span><input name="group" autocomplete="off" maxlength="32" spellcheck="false" placeholder="${te("dialog.groupPlaceholder")}" value="${esc(existing?.group ?? "")}" /><small class="field-hint">${te("dialog.groupHint")}</small><small class="field-err" hidden></small></label>
    <label class="f" data-field="monogram"><span>${te("dialog.monogram")}</span><input name="monogram" autocomplete="off" spellcheck="false" size="4" placeholder="${te("dialog.monogramPlaceholder")}" value="${esc(existing?.monogram ?? "")}" aria-describedby="monogram-hint" /><small class="field-hint" id="monogram-hint">${te("dialog.monogramHint")}</small><small class="field-err" hidden></small></label>
    <p class="cost-advisory" id="cost-warn" hidden></p>
    <p class="cost-advisory" id="choice-notice" role="status" aria-live="polite" hidden></p>
    <div id="models-host"></div>
    </div>
    <p class="err" id="err" role="alert"></p>
    <div class="dlg-actions"><button type="button" class="btn" data-cancel>${te("common.cancel")}</button><button type="submit" class="btn primary" id="ok" ${editing ? "" : "hidden"}>${te("common.save")}</button></div>
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
  const peerRow = dlg.querySelector("#peer-row");
  const peerInput = dlg.querySelector('[name="peer_name"]');
  // PRD 0029 RF3: the name is optional unless another account of the provider exists (the core decides on save;
  // this only shapes the form). Adding beside an unnamed account also asks for that account's name.
  let nameNeeded = false;
  let peerShown = false;
  const providerLabel = () => providers.find((p) => p.id === provider)?.label ?? provider;
  async function syncNameRule() {
    const view = await getView().catch(() => null);
    const same = (view?.reports ?? []).filter((r) => r.provider === provider && r.index !== index);
    nameNeeded = same.length > 0;
    const peer = editing ? null : same.find((r) => !(r.account ?? "").trim());
    const label = providerLabel();
    nameInput.placeholder = label;
    dlg.querySelector("#name-hint").textContent = nameNeeded ? t("dialog.nameNeeded", { provider: label }) : t("dialog.nameHint", { provider: label });
    if (Boolean(peer) !== peerShown) peerInput.value = peer ? label : "";
    peerShown = Boolean(peer);
    peerRow.hidden = !peerShown;
    dlg.querySelector("#peer-label").textContent = t("dialog.peerName", { provider: label });
    syncLoginBtn();
  }
  // The login only starts with the account named (when it finishes it is saved automatically). Instead of `disabled`, which swallows the
  // click without explaining, the button is dimmed, shows the reason right below and, when clicked, leads to the name field.
  const showNameError = () => {
    nameInput.classList.add("invalid");
    nameErr.textContent = t("dialog.login.nameFirst");
    nameErr.hidden = false;
    nameInput.scrollIntoView({ block: "center", behavior: "smooth" });
    nameInput.focus();
  };
  const syncLoginBtn = () => {
    const missing = nameNeeded && nameInput.value.trim() === "";
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
    // PRD 0047: the main credential row stands for the account's `credential_env`, typed or kept in the vault.
    if (field === "credential_env") return dlg.querySelector('[data-field="credential"]');
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
  // Visible field values in the core draft shape (save and access share it).
  const draftFields = () => schema.fields.filter((f) => visible.has(f.key)).map((f) => ({ key: f.key, text: values.get(f.key)?.text ?? (f.options ? null : ""), choice: values.get(f.key)?.choice ?? (f.options ? 0 : null) }));
  // PRD 0005 RF3: the core describes how the draft's usage is read; the dialog only renders it.
  let confirmUnavailable = false;
  // SES-02: set when the core's test before save could not reach the provider; the next save is "Save anyway".
  let confirmUntested = false;
  let accessSeq = 0;
  async function renderAccess() {
    const seq = ++accessSeq;
    const access = await draftAccess(provider, authMode, draftFields(), sessionSlots());
    if (seq !== accessSeq) return; // a newer change is already being described
    confirmUnavailable = false; // any change withdraws a pending "Save anyway"
    confirmUntested = false;
    dlg.querySelector("#ok").textContent = t("common.save");
    costWarn.textContent = tl(access.cost_advisory);
    costWarn.hidden = !access.cost_advisory;
    const notes = dyn.querySelector("#access-notes");
    if (notes) notes.innerHTML = access.notes.map((n) => `<small>${esc(tl(n))}</small>`).join("");
  }
  // PRD 0047 G2: the keys, tokens and secrets the form types (masked, never prefilled, never echoed back). The slots of
  // the session box (`schema.sessions`) stay there; every other `secret_inputs` entry is a row of the form.
  const sessionSlotSet = () => new Set((schema.sessions ?? []).map((s) => s.slot));
  const formSecrets = () => (schema.secret_inputs ?? []).filter((s) => !sessionSlotSet().has(s.slot));
  const mainSecret = () => formSecrets().find((s) => s.slot === "credential_env") ?? null;
  // G4/G5: where a saved secret comes from (`vault`, `environment` set up in the terminal, `unset`); never a value or name.
  const secretSource = (slot) => (existing && existing.provider === provider
    ? existing.secret_states?.find((s) => s.slot === slot)?.source : null) ?? "unset";
  const secretEls = () => [...dyn.querySelectorAll(".secret-input[data-slot]")];
  const typedSecrets = () => secretEls().filter((el) => el.value.trim()).map((el) => ({ slot: el.dataset.slot, value: el.value }));
  const clearSecrets = () => secretEls().forEach((el) => { el.value = ""; el.type = "password"; });
  // Saved secrets the user chose to replace: their input stays open across re-renders of the form.
  const replacing = new Set();
  const secretRowHtml = (s, extra = "") => {
    const source = secretSource(s.slot);
    const open = source === "unset" || replacing.has(s.slot);
    const id = `secret-${s.slot}`;
    const required = s.required && source === "unset";
    const state = source === "unset" ? ""
      : `<p class="secret-state">${icon(source === "vault" ? "check" : "terminal")}<span>${te(source === "vault" ? "dialog.secret.saved" : "dialog.secret.terminal")}</span>
          ${open ? "" : `<button type="button" class="btn sm" data-replace="${esc(s.slot)}" aria-describedby="${id}-l">${te("dialog.secret.replace")}</button>`}</p>`;
    return `<div class="f secret-row" data-field="${s.slot === "credential_env" ? "credential" : `f:${esc(s.slot)}`}">
        <span id="${id}-l">${esc(tl(s.label))}${required ? " *" : ""}</span>${state}
        <div class="secret-field" ${open ? "" : "hidden"}>
          <input type="password" id="${id}" class="secret-input" data-slot="${esc(s.slot)}" aria-labelledby="${id}-l" aria-describedby="${id}-h"
            autocomplete="new-password" autocapitalize="off" autocorrect="off" spellcheck="false"
            placeholder="${te(source === "unset" ? "dialog.secret.placeholder" : "dialog.secret.replacePlaceholder")}" ${required ? "required" : ""} />
          <button type="button" class="icon secret-reveal" aria-controls="${id}" aria-pressed="false" aria-label="${te("dialog.secret.show")}" title="${te("dialog.secret.show")}">${icon("eye")}</button>
        </div>
        <small class="field-err" hidden></small>
        <small id="${id}-h" ${open ? "" : "hidden"}>${esc(tl(s.help))}${source !== "unset" && open ? ` ${te("dialog.secret.keepHint")}` : ""}</small>${extra}</div>`;
  };
  // PRD 0016 RF1: slots the saved account keeps in the vault (the provider cannot change while editing), plus
  // RF2: the slots with a paste and PRD 0047 the typed secrets, which the save writes to the vault.
  const sessionSlots = () => [...new Set([...(existing?.session_slots ?? []), ...pastes().map((p) => p.slot), ...typedSecrets().map((s) => s.slot)])];
  const sessionBox = dlg.querySelector("#session-box");
  // PRD 0016 RF2.1: the pasted sessions live only in the masked fields; they go to the core on preview, test and save.
  const pastes = () => [...sessionBox.querySelectorAll(".session-paste[data-slot]")]
    .filter((el) => el.value.trim())
    .map((el) => ({ slot: el.dataset.slot, paste: el.value }));
  const clearPastes = () => sessionBox.querySelectorAll(".session-paste[data-slot]").forEach((el) => { el.value = ""; });
  let confirmMove = false;
  let confirmForget = false;
  let previewSeq = 0;
  function renderSession(status = "") {
    const slots = schema.sessions ?? [];
    // PRD 0047: the vault reference may also hold typed keys; this box speaks only of its own session slots, and
    // "Forget" (which empties the whole reference) is offered only when nothing else lives there.
    const kept = existing?.provider === provider ? existing?.session_slots ?? [] : [];
    const stored = kept.filter((slot) => slots.some((s) => s.slot === slot));
    const forgettable = stored.length > 0 && stored.length === kept.length;
    clearPastes();
    sessionBox.hidden = !slots.length && !stored.length && !status;
    if (sessionBox.hidden) { sessionBox.innerHTML = ""; return; }
    confirmForget = false;
    // PRD 0016 RF4.1: the core's `UserSession` level and risk warning lead the paste box.
    sessionBox.innerHTML = `<h3>${te("dialog.session.title")}</h3>${slots.length ? sessionNoticeHtml(schema.session_notice) : ""}`
      + slots.map((s) => `<div class="session-slot">
          <label class="f"><span>${esc(tl(s.label))}</span>
            ${s.secret
              // PRD 0036 RF2.1: a vault password or key is a masked one-line field, never a cookie box.
              ? `<input type="password" class="session-paste" data-slot="${esc(s.slot)}" autocomplete="new-password" autocapitalize="off" spellcheck="false">`
              : `<textarea class="session-paste" data-slot="${esc(s.slot)}" rows="3" autocomplete="off" autocapitalize="off" spellcheck="false"
              placeholder="${te("dialog.session.placeholder")}"></textarea>`}
            <small class="hint">${esc(tl(s.instructions))}</small>
            ${s.hosts.length ? `<small class="hint">${te("dialog.session.hosts", { hosts: s.hosts.join(", ") })}</small>` : ""}
            ${stored.includes(s.slot) ? `<small class="hint">${te("dialog.session.replace")}</small>` : ""}
            <small class="hint" data-preview="${esc(s.slot)}" role="status" aria-live="polite"></small></label>
        </div>`).join("")
      + (slots.length ? `<div class="dlg-actions"><button type="button" class="btn" id="session-test">${te("dialog.session.test")}</button></div>
          <p class="hint" id="session-test-status" role="status" aria-live="polite"></p>` : "")
      + (stored.length ? `<p class="hint">${te("dialog.session.stored")}</p>` : "")
      + (forgettable ? `<div class="dlg-actions"><button type="button" class="btn danger-btn" id="session-forget">${te("dialog.session.forget")}</button></div>` : "")
      + `<p class="hint" id="session-status" role="status" aria-live="polite">${esc(status)}</p>`;
    // RF2.2: the core says what each paste keeps (names, counts, expiry) — never the value.
    sessionBox.querySelectorAll(".session-paste[data-slot]").forEach((el) => el.addEventListener("input", async () => {
      const seq = ++previewSeq;
      const out = sessionBox.querySelector(`[data-preview="${el.dataset.slot}"]`);
      sessionBox.querySelector("#session-test-status").textContent = "";
      let text = "";
      if (el.value.trim()) {
        try { text = tl((await previewSession(provider, el.dataset.slot, el.value)).summary); } catch (x) { text = errText(x); }
      }
      if (seq === previewSeq) out.textContent = text;
      await renderAccess();
    }));
    sessionBox.querySelector("#session-test")?.addEventListener("click", async (e) => {
      const btn = e.currentTarget;
      const statusEl = sessionBox.querySelector("#session-test-status");
      if (!pastes().length) { statusEl.textContent = t("dialog.session.pasteFirst"); return; }
      collect();
      btn.disabled = true;
      statusEl.textContent = t("dialog.session.testing");
      try {
        const result = await testSession({ ...draftInput(), sessions: pastes() });
        statusEl.textContent = result.ok
          ? [t("dialog.session.testOk", { count: result.metrics }), result.expiry ? tl(result.expiry.label) : ""].filter(Boolean).join(" ")
          : [tl(result.status), result.note ? tl(result.note) : ""].filter(Boolean).join(": ");
      } catch (x) {
        statusEl.textContent = errText(x);
      } finally {
        btn.disabled = false;
      }
    });
    // PRD 0016 RF4.2: "Forget this session" asks once more, then deletes the vault entry and the reference.
    const forgetBtn = sessionBox.querySelector("#session-forget");
    forgetBtn?.addEventListener("click", async () => {
      const statusEl = sessionBox.querySelector("#session-status");
      if (!confirmForget) {
        confirmForget = true;
        forgetBtn.textContent = t("dialog.session.forgetConfirm");
        statusEl.textContent = t("dialog.session.forgetHint");
        return;
      }
      forgetBtn.disabled = true;
      try {
        existing = await forgetSession(index);
        renderSession(t("dialog.session.forgotten"));
        renderMove();
        await renderDynamic();
      } catch (x) {
        forgetBtn.disabled = false;
        confirmForget = false;
        forgetBtn.textContent = t("dialog.session.forget");
        statusEl.textContent = errText(x);
      }
    });
  }
  renderSession();
  // PRD 0047 G5: secrets set up in the terminal (keys and sessions alike) show no variable name; one click, confirmed
  // once more, copies them into the system vault and the account stops depending on that setup.
  const moveBox = dlg.querySelector("#move-box");
  function renderMove(status = "") {
    const offers = existing?.provider === provider ? existing?.vault_migration ?? [] : [];
    moveBox.hidden = !offers.length && !status;
    confirmMove = false;
    if (moveBox.hidden) { moveBox.innerHTML = ""; return; }
    moveBox.innerHTML = `<h3>${te("dialog.move.title")}</h3>`
      + (offers.length ? `<p class="hint">${te("dialog.move.intro")}</p>`
        + offers.map((c) => `<p class="hint">${esc(tl(c.consent))}</p>`).join("")
        + `<div class="dlg-actions"><button type="button" class="btn" id="move-btn">${te("dialog.move.button")}</button></div>` : "")
      + `<p class="hint" id="move-status" role="status" aria-live="polite">${esc(status)}</p>`;
    const btn = moveBox.querySelector("#move-btn");
    btn?.addEventListener("click", async () => {
      const statusEl = moveBox.querySelector("#move-status");
      if (!confirmMove) {
        confirmMove = true;
        btn.textContent = t("dialog.move.confirm");
        statusEl.textContent = t("dialog.move.confirmHint");
        return;
      }
      btn.disabled = true;
      try {
        const moved = offers.map((c) => c.slot);
        existing = await migrateSession(index);
        // The saved account no longer points at the terminal setup; the draft must not bring those references back.
        for (const slot of moved) {
          if (values.has(slot)) values.set(slot, { key: slot, text: "", choice: null });
          replacing.delete(slot);
        }
        renderSession();
        renderMove(t("dialog.move.done"));
        await renderDynamic();
      } catch (x) {
        btn.disabled = false;
        confirmMove = false;
        btn.textContent = t("dialog.move.button");
        // The terminal setup is often invisible to a desktop app; never show the core's text, which names the variable.
        const parsed = parseErrorEnvelope(x);
        statusEl.textContent = parsed?.id?.startsWith("user_session.migrate.") ? t("dialog.move.unreadable") : errText(x);
      }
    });
  }
  renderMove();
  // PRD 0036 RF2.2: an aggregator (9router) is tested before its first save: health, version, sign-in and the
  // connection list, with one core sentence as the outcome (never a body or URL).
  const aggregatorBox = dlg.querySelector("#aggregator-box");
  const aggregatorStatus = dlg.querySelector("#aggregator-status");
  function renderAggregator() {
    aggregatorBox.hidden = !schema.aggregator;
    aggregatorStatus.textContent = "";
  }
  async function runAggregatorTest(pending) {
    aggregatorStatus.textContent = t(pending);
    const result = await testAggregator({ ...draftInput(), sessions: pastes() });
    aggregatorStatus.textContent = tl(result.message);
    // PRD 0036 RF2.3: trust on first use — the fingerprint is pinned only after the user confirms it.
    if (!result.ok && result.fingerprint && (await confirmFingerprint(result.fingerprint)) === "trust") {
      setFieldText("tls_pin_sha256", result.fingerprint);
      setFieldText("tls_ca_file", "");
      return runAggregatorTest(pending);
    }
    return result.ok;
  }
  function setFieldText(key, text) {
    values.set(key, { key, text, choice: null });
    const el = dyn.querySelector(`[name="f:${key}"]`);
    if (el) el.value = text;
  }
  dlg.querySelector("#aggregator-test").addEventListener("click", async (e) => {
    const btn = e.currentTarget;
    collect();
    btn.disabled = true;
    try { await runAggregatorTest("dialog.aggregator.testing"); } catch (x) { aggregatorStatus.textContent = errText(x); }
    finally { btn.disabled = false; }
  });
  renderAggregator();
  // PRD 0016 RF4.2: as with the TUI's F5, any other key or click withdraws a pending "Forget this session".
  const disarmForget = (e) => {
    const forgetBtn = sessionBox.querySelector("#session-forget");
    if (!confirmForget || !forgetBtn) return;
    const confirming = e.type === "keydown" ? e.target === forgetBtn && (e.key === "Enter" || e.key === " ") : forgetBtn.contains(e.target);
    if (confirming) return;
    confirmForget = false;
    forgetBtn.textContent = t("dialog.session.forget");
    sessionBox.querySelector("#session-status").textContent = "";
  };
  dlg.addEventListener("pointerdown", disarmForget, true);
  dlg.addEventListener("keydown", disarmForget, true);
  function copyText(text, btn) {
    navigator.clipboard?.writeText(text).then(
      () => { const prev = btn.textContent; btn.textContent = t("dialog.copied"); setTimeout(() => (btn.textContent = prev), 1500); },
      () => { err.textContent = t("dialog.copyFailed"); },
    );
  }
  dlg.querySelector("#copy-url").addEventListener("click", (e) => copyText(urlInput.value, e.currentTarget));
  dlg.querySelector("#copy-code").addEventListener("click", (e) => copyText(codeText.textContent, e.currentTarget));

  // Awaited cancellation: the backend reacts serially, so switching
  // mode/provider and starting a new login does not collide with the previous one.
  async function resetLogin() {
    const attempt = loginState?.attempt;
    if (loginState) await loginCancel(attempt);
    loginState = null;
    // Clears pending envelopes: the new login starts with a new attempt and
    // must not inherit envelopes from a previous attempt that did not
    // yet have a known attempt at arrival time.
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

  // Typed core events (RF2.1/RF2.5): they never contain tokens or callback
  // URLs with credentials. Each event arrives wrapped in
  // `{ attempt, ...LoginEvent }`. Acceptance window: an event is
  // queued ONLY when there is an active `loginState` with no
  // real `attempt` assigned yet (the attempt just started and
  // `await loginStart` has not resolved). Envelopes with a nonzero
  // `attempt` different from the active one are stale forwarders and are dropped.
  // With no active login, drop: the Tauri side already routed the event, and keeping
  // it buffered would be unbounded.
  let pendingEnvelopes = [];
  let drainScheduled = false;
  const drainPending = () => {
    if (!loginState) return;
    if (loginState.attempt === 0) return; // still no id; call made too early
    const ready = pendingEnvelopes;
    pendingEnvelopes = [];
    for (const ev of ready) dispatch(ev);
  };
  const unlisten = await onLogin((ev) => {
    if (!loginState) return; // no active login: drop
    if (typeof ev.attempt === "number" && ev.attempt !== loginState.attempt) {
      // attempt conhecido e diferente: forwarder velho. Descarta.
      if (loginState.attempt === 0) {
        // Start window: still no id, but this envelope already carries a
        // concrete attempt; it may be our attempt arriving early.
        // Keep it until the id matches.
        pendingEnvelopes.push(ev);
      }
      return;
    }
    if (loginState.attempt === 0 && typeof ev.attempt === "number" && ev.attempt !== 0) {
      // Start window: queue it to drain when the real id arrives.
      pendingEnvelopes.push(ev);
      return;
    }
    dispatch(ev);
  });
  function dispatch(ev) {
    if (!loginState) return;
    if (typeof ev.attempt === "number" && ev.attempt !== 0 && ev.attempt !== loginState.attempt) return;
    // PRD 0034 RF2: Cuotai's own login lines carry a catalog id (`l10n`, `instructions_l10n`); provider CLI lines do not.
    switch (ev.type) {
      case "prompt":
        loginState.lines = ev.instructions ? [tl(ev.instructions_l10n ?? ev.instructions)] : [];
        loginState.url = ev.url || null;
        loginState.userCode = ev.user_code ?? null;
        break;
      case "browser_unavailable":
        loginState.lines.push(t("dialog.login.browserUnavailable"));
        loginState.url = ev.url;
        break;
      case "progress":
        if (loginState.placeholder) { loginState.lines = []; loginState.placeholder = false; } // the core already says the same
        loginState.lines.push(tl(ev.l10n ?? ev.text));
        break;
      case "waiting_code":
        loginState.waitingCode = true;
        break;
      case "code_rejected":
        loginState.codeError = tl(ev.l10n ?? ev.message);
        break;
      case "finished": {
        loginState.phase = "done";
        loginState.params = ev.params ?? {};
        // Applies the public metadata returned by the login to the fields.
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
        // Updates the form with the login parameters and only then saves (save's `collect()` reads these fields).
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
        loginState.lines.push(tl(ev.l10n ?? ev.message));
        break;
    }
    renderLoginPanel();
    if (ev.type === "waiting_code") codeInput.focus();
  }
  dlg.addEventListener("close", () => {
    unlisten();
    // PRD 0016 RF2.3 / PRD 0047: a pasted session or typed secret never outlives the dialog.
    clearPastes();
    clearSecrets();
    // Closing the dialog with an active login and no commit cancels the flow: the authorized
    // token dies in memory and nothing is written to the vault (RF2.4).
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
    if (nameNeeded && nameInput.value.trim() === "") { showNameError(); return; }
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
    // PRD 0047: typed secrets survive the re-render in the DOM only (never in `values`, never logged).
    const secretSnapshot = new Map(typedSecrets().map((x) => [x.slot, x.value]));
    for (const slot of secretSnapshot.keys()) replacing.add(slot);
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
    const cred = schema.credential_kind;
    const mode = selectedMode();
    const available = modes().filter((m) => !m.unavailable_reason);
    const unavailable = modes().filter((m) => m.unavailable_reason);

    let authHtml = "";
    if (modes().length || cred !== "none") {
      const radios = [];
      if (cred !== "none") {
        const label = cred === "profile" ? t("dialog.credProfile") : mainSecret() ? tl(mainSecret().label) : t("dialog.credKey");
        radios.push(`<label class="mode"><input type="radio" name="auth" value="credential" ${authMode === "credential" ? "checked" : ""} /> ${esc(label)}</label>`);
      }
      for (const m of available) {
        radios.push(`<label class="mode"><input type="radio" name="auth" value="${esc(m.id)}" ${authMode === m.id ? "checked" : ""} /> ${esc(tl(m.label, { id: m.id }))}${m.help ? ` <small>${esc(tl(m.help, { id: m.id }))}</small>` : ""}</label>`);
      }
      for (const m of unavailable) {
        radios.push(`<label class="mode mode-off"><input type="radio" name="auth" value="${esc(m.id)}" disabled /> ${esc(tl(m.label, { id: m.id }))} <small>${esc(tl(m.unavailable_reason))}</small></label>`);
      }
      authHtml = `<fieldset class="auth"><legend>${te("dialog.authLegend")}</legend>${radios.join("")}</fieldset>`;
      // PRD 0027 RF5.1 / DS-CMP-CAT-3: a new account opens the recommended mode; the others sit, collapsed, under
      // "Other ways to connect", each with the core's hint.
      if (!editing && radios.length > 1) {
        const hints = new Map(catalogModes.map((m) => [m.mode ?? "credential", tl(m.hint)]));
        const current = radios.find((r) => r.includes("checked")) ?? "";
        const others = radios.filter((r) => r !== current).map((r) => {
          const id = /value="([^"]*)"/.exec(r)?.[1];
          return hints.has(id) ? r.replace("</label>", ` <small class="pick-hint">${esc(hints.get(id))}</small></label>`) : r;
        });
        authHtml = `<fieldset class="auth"><legend>${te("dialog.authLegend")}</legend>${current}
          <details class="other-ways"><summary>${te("dialog.picker.otherWays")}${icon("chev")}</summary>${others.join("")}</details></fieldset>`;
      }
    }

    // RF3.2: the access notes share the area of the credential help (`#access-notes`, filled by renderAccess).
    const notesSlot = `<span class="access-notes" id="access-notes"></span>`;
    let bodyHtml = "";
    if (!mode) {
      // PRD 0047 G1/G2: the key is typed (masked) and kept in the vault; the variable name, the terminal default and the
      // terminal help (`credential`, `default_env`, `credential_help`) are never shown. An AWS profile stays a name.
      bodyHtml = cred === "none"
        ? `<p class="hint">${te("dialog.credNone")}${notesSlot}</p>`
        : cred === "profile"
          ? `<label class="f" data-field="credential"><span>${te("dialog.credProfile")}</span>
            <input name="credential" value="${esc(existing?.credential ?? "")}" required />
            <small class="field-err" hidden></small>
            <small>${esc(tl(schema.credential_help))}</small>${notesSlot}</label>`
          : mainSecret()
            ? secretRowHtml(mainSecret(), notesSlot)
            : `<p class="hint">${notesSlot}</p>`;
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
      bodyHtml = optInputs.join("") + button + loginHint + `<p class="hint">${notesSlot}</p>`;
    }

    dynAuth.innerHTML = authHtml + bodyHtml;
    // PRD 0047 G1: a field that names a variable is terminal-only; its secret input (if any) takes its place.
    const terminalOnly = new Set(schema.terminal_only_fields ?? []);
    dynFields.innerHTML =
      schema.fields.filter((f) => visible.has(f.key)).map((f) => {
        if (terminalOnly.has(f.key)) {
          const secret = formSecrets().find((x) => x.field === f.key);
          return secret ? secretRowHtml(secret) : "";
        }
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
    for (const el of secretEls()) if (secretSnapshot.has(el.dataset.slot)) el.value = secretSnapshot.get(el.dataset.slot);
    secretSnapshot.clear();
    const loginBtn = dyn.querySelector("#login-btn");
    if (loginBtn) loginBtn.addEventListener("click", () => startLogin(loginBtn));
    syncLoginBtn();
    await renderAccess();
  }

  dlg.addEventListener("change", async (e) => {
    if (e.target.name === "auth") {
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
      await (e.target.tagName === "SELECT" ? renderDynamic() : renderAccess());
    } else if (e.target.classList?.contains("secret-input")) {
      await renderAccess();
    }
  });
  // PRD 0047 G2/G4: Show/Hide reveals only what the user typed; Replace opens the input of a saved secret (blank keeps it).
  dyn.addEventListener("click", (e) => {
    const reveal = e.target.closest(".secret-reveal");
    if (reveal) {
      const input = dyn.querySelector(`#${CSS.escape(reveal.getAttribute("aria-controls"))}`);
      const shown = input.type === "password";
      input.type = shown ? "text" : "password";
      reveal.setAttribute("aria-pressed", String(shown));
      const label = t(shown ? "dialog.secret.hide" : "dialog.secret.show");
      reveal.setAttribute("aria-label", label);
      reveal.title = label;
      reveal.innerHTML = icon(shown ? "eyeOff" : "eye");
      input.focus();
      return;
    }
    const replace = e.target.closest("[data-replace]");
    if (replace) {
      const row = replace.closest(".secret-row");
      replacing.add(replace.dataset.replace);
      row.querySelector(".secret-field").hidden = false;
      const help = row.querySelector(`#secret-${CSS.escape(replace.dataset.replace)}-h`);
      if (help) { help.hidden = false; help.insertAdjacentText("beforeend", ` ${t("dialog.secret.keepHint")}`); }
      row.querySelector(".secret-input").placeholder = t("dialog.secret.replacePlaceholder");
      replace.remove();
      row.querySelector(".secret-input").focus();
    }
  });
  // PRD 0047: a typed key travels in `secrets` (the credential text is then empty); an untouched key the terminal set up
  // keeps its reference, which the form never displays; an AWS profile is the name typed in its row.
  function credentialText(data) {
    if (!mainSecret()) return String(data.get("credential") ?? "");
    const typed = typedSecrets().some((x) => x.slot === "credential_env");
    return !typed && secretSource("credential_env") === "environment" ? existing?.credential ?? "" : "";
  }
  // The dialog's draft as the backend reads it (save and the PRD 0016 session test).
  function draftInput() {
    const data = new FormData(form);
    const mode = selectedMode();
    const { options } = collectModeInputs(mode);
    return {
      index, provider, name: String(data.get("name") ?? ""),
      peerName: peerShown ? String(data.get("peer_name") ?? "") : null,
      credential: mode ? "" : credentialText(data),
      fields: draftFields(),
      interval: String(data.get("interval") ?? ""),
      group: String(data.get("group") ?? ""),
      // PRD 0022 MONO-4: the display override (blank = derived from the name); the shell keeps it in desktop.json.
      monogram: String(data.get("monogram") ?? "").trim(),
      commitLogin: loginDone(),
      loginAttempt: loginState?.attempt ?? 0,
      authMode,
      authOptions: options,
      authParams: loginDone() ? loginState.params : {},
      // PRD 0047 G2: blank or absent keeps the stored secret.
      secrets: mode ? typedSecrets().filter((x) => x.slot !== "credential_env") : typedSecrets(),
    };
  }
  dlg.querySelector("[data-cancel]").addEventListener("click", () => {
    if (!committing) dlg.close();
  });
  // SES-02: any edit withdraws a pending "Save anyway" of an untested session.
  form.addEventListener("input", () => {
    if (!confirmUntested) return;
    confirmUntested = false;
    dlg.querySelector("#ok").textContent = t("common.save");
  });
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (committing) return;
    collect();
    const ok = dlg.querySelector("#ok");
    err.textContent = "";
    clearFieldErrors();
    if (!validMonogram(new FormData(form).get("monogram"))) {
      setFieldError("monogram", t("dialog.monogram.invalid"));
      form.querySelector('[name="monogram"]')?.focus();
      return;
    }
    const mode = selectedMode();
    // PRD 0047 G1: a missing required secret is said in the form's own words (the core's text is terminal wording).
    const missing = secretEls().find((el) => el.required && !el.value.trim());
    if (missing) {
      setFieldError(missing.closest("[data-field]").dataset.field, t("dialog.secret.required"));
      return;
    }
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
    // RF3.4: an Unavailable account shows the core's reason first and needs a second, explicit "Save anyway".
    const latest = await draftAccess(provider, authMode, draftFields(), sessionSlots());
    if (latest.save_confirmation && !confirmUnavailable) {
      confirmUnavailable = true;
      err.textContent = tl(latest.save_confirmation);
      ok.textContent = t("dialog.saveAnyway");
      return;
    }
    ok.disabled = true;
    committing = true;
    dlg.querySelector("[data-cancel]").disabled = true;
    // SES-02: a pasted session is saved only after the core's test query, run by the save itself.
    const sessions = pastes();
    const testStatus = sessionBox.querySelector("#session-test-status");
    if (sessions.length && testStatus) testStatus.textContent = t("dialog.session.testingSave");
    // G3: a typed key is checked with one read-only query before it reaches the vault.
    else if (typedSecrets().length) err.textContent = t("dialog.secret.testingSave");
    try {
      // PRD 0036 RF2.2: the save of an aggregator runs its Test connection first; a failed test blocks it.
      if (schema.aggregator && !(await runAggregatorTest("dialog.aggregator.testingSave"))) {
        err.textContent = t("dialog.aggregator.fixFirst");
        ok.disabled = false;
        committing = false;
        dlg.querySelector("[data-cancel]").disabled = false;
        return;
      }
      const result = await saveAccount({ ...draftInput(), sessions, saveAnyway: confirmUntested });
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
      if (testStatus) testStatus.textContent = "";
      // SES-02: only a transport failure of the test before save offers "Save anyway"; a rejection never does.
      if (parsed && parsed.id === "user_session.save.unreachable") {
        confirmUntested = true;
        err.textContent = errText(parsed);
        ok.textContent = t("dialog.saveAnyway");
        ok.disabled = false;
        committing = false;
        dlg.querySelector("[data-cancel]").disabled = false;
        return;
      }
      if (confirmUntested) {
        // "Save anyway" was tried and the save still failed: the next save tests again.
        confirmUntested = false;
        ok.textContent = t("common.save");
      }
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
  // PRD 0002 RF6.1: catalog of the account being edited; for a new account, an explicit choice of a configured account of the
  // same provider (the new one has no saved credential yet) or the core's guidance when there is none.
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
      : `<label class="f"><span>${te("models.pick")}</span><select id="models-pick"><option value="">${te("models.pickNone")}</option>${same.map((r) => `<option value="${esc(reportKey(r))}">${esc(accountLabel(r))}</option>`).join("")}</select></label><div data-models-slot></div>`;
    if (editing) return mountIn(`${existing.provider}::${existing.name}`, index, generation);
    modelsHost.querySelector("#models-pick")?.addEventListener("change", (e) => {
      const r = same.find((x) => reportKey(x) === e.target.value);
      mountIn(r ? reportKey(r) : null, r?.index, generation);
    });
  }
  dlg.addEventListener("close", () => { modelsSeq++; models?.dispose(); });
  await syncNameRule();

  // ----- PRD 0027 RF3/RF5/RF6/RF7: the catalog picker, the recommended mode and consented offers -----
  const head = dlg.querySelector("#prov-head");
  const rest = dlg.querySelector("#form-rest");
  const consentLine = dlg.querySelector("#offer-consent");
  let detected = [];
  const paintHead = (entry, found) => {
    head.innerHTML = `${logo(entry.provider, 32)}<span class="pick-text"><b>${esc(entry.label)}</b>
        <small class="pick-method">${esc(tl(entry.method_label))}</small>
        <span class="pick-chips">${entryChips(entry, found)}</span></span>
      ${editing ? "" : `<button type="button" class="btn sm" id="prov-change">${te("dialog.picker.change")}</button>`}`;
    head.querySelector("#prov-change")?.addEventListener("click", backToPicker);
  };
  // A mode the tile names opens only when the core offers it for this provider now.
  const offered = (mode) => mode && (mode === "credential"
    ? schema.credential_kind !== "none"
    : modes().some((m) => m.id === mode && !m.unavailable_reason));
  async function choose(entry, found) {
    // PRD 0027 RF1.3: an "Extension: {name}" tile has no form; its consent
    // flow lives in the Extensions dialog.
    if (entry.extension) {
      dlg.close();
      onExtension?.(entry.extension);
      return;
    }
    provider = entry.provider;
    schema = await formSchema(provider);
    catalogModes = await providerModes(provider).catch(() => []);
    values.clear();
    await resetLogin();
    await syncNameRule();
    const wanted = found?.mode ?? entry.mode;
    authMode = offered(wanted) ? wanted : (schema.credential_kind === "none" ? (modes()[0]?.id ?? "credential") : "credential");
    paintHead(entry, found);
    // RF7.2: what the offered flow will read, before anything is read; Change provider (or Cancel) reads nothing.
    consentLine.textContent = found ? tl(found.consent) : "";
    consentLine.hidden = !found;
    picker?.closest(".picker").setAttribute("hidden", "");
    dlg.classList.remove("dlg-catalog");
    head.hidden = false;
    rest.hidden = false;
    dlg.querySelector("#ok").hidden = false;
    await renderDynamic();
    renderSession();
    renderAggregator();
    renderModels();
    nameInput.focus();
  }
  function backToPicker() {
    rest.hidden = true;
    head.hidden = true;
    dlg.querySelector("#ok").hidden = true;
    dlg.classList.add("dlg-catalog");
    dlg.querySelector("#picker").hidden = false;
    pickerApi.focus();
  }
  let catalogModes = await providerModes(provider).catch(() => []);
  // The header's close mirrors Cancel, so both clean up the same way (mockup 31).
  dlg.querySelector("[data-x]").addEventListener("click", () => dlg.querySelector("[data-cancel]").click());
  let picker = null;
  let pickerApi = null;
  if (!editing) {
    picker = dlg.querySelector("#picker-list");
    dlg.classList.add("dlg-catalog");
    pickerApi = mountPicker(dlg.querySelector("#picker"), {
      onChoose: (entry, found) => choose(entry, found),
      detected: () => detected,
      onDismiss: async (kind) => {
        try { await dismissDetected(kind); } catch { /* the badge goes away for this dialog anyway */ }
        detected = detected.filter((d) => d.kind !== kind);
        pickerApi.refresh();
      },
      onRequest: () => openExternal(NEW_PROVIDER_URL).catch(() => {}),
      onExtensions: () => { dlg.close(); onExtension?.(null); },
    });
    await pickerApi.reload();
    // RF6.5: detection is opt-in; with no answer the question shows and nothing is checked.
    const ask = dlg.querySelector("#detect-ask");
    const runDetection = () => detectSources().then((found) => { detected = found; pickerApi.refresh(); }, () => {});
    const prefs = (await getSettings().catch(() => null))?.detect ?? { enabled: false };
    if (prefs.enabled == null) {
      ask.hidden = false;
      for (const btn of ask.querySelectorAll("[data-detect]")) btn.addEventListener("click", async () => {
        const on = btn.dataset.detect === "1";
        ask.hidden = true;
        try { await setDetectEnabled(on); } catch { return; }
        if (on) runDetection();
        pickerApi.focus();
      });
    } else if (prefs.enabled) {
      runDetection(); // off the dialog's path (RF6 performance): the tiles gain their chip when it answers
    }
  } else {
    const entry = catalogModes.find((m) => m.mode === (existing?.auth_mode ?? null)) ?? catalogModes[0];
    if (entry) paintHead(entry, null);
  }
  await renderDynamic();
  renderModels();
  // SES-01: "Paste a new session" opens this editor on the account's paste box.
  const pasteField = pasteSession ? sessionBox.querySelector(".session-paste[data-slot]") : null;
  if (pasteField) {
    sessionBox.querySelector("#session-status").textContent = t("dialog.session.pasteNew");
    sessionBox.scrollIntoView({ block: "center" });
    pasteField.focus();
  } else if (pickerApi) {
    pickerApi.focus();
  } else {
    dlg.querySelector("[name=name]").focus();
  }
}

/** Confirms the removal. An account with a credential in the vault asks whether the credential should be deleted too.
 *  `onDone(result)` receives what the core did (removed, deleted from the vault, kept, shared…). */
export function confirmDelete(index, label, { usesOauth = false, secretsOf = null, onDone = () => {} } = {}) {
  const buttons = usesOauth
    ? `<button type="button" class="btn" data-purge="0">${te("dialog.deleteVaultKeep")}</button>
       <button type="button" class="btn danger-btn" data-purge="1">${te("dialog.deleteVaultPurge")}</button>`
    : `<button type="button" class="btn danger-btn" data-purge="0">${te("common.remove")}</button>`;
  const dlg = open(`<form method="dialog" class="dlg-body">
    <h2>${te("dialog.deleteTitle")}</h2>
    <p>${te(usesOauth ? "dialog.deleteBodyVault" : "dialog.deleteBody", { label })}</p>
    ${secretsOf ? `<label class="extension-confirm"><input type="checkbox" id="forget-secrets" />${te("dialog.deleteExtensionSecrets")}</label>` : ""}
    <p class="err" id="err" role="alert"></p>
    <div class="dlg-actions"><button type="button" class="btn" data-cancel>${te("common.cancel")}</button>${buttons}</div>
  </form>`);
  dlg.querySelector("[data-cancel]").addEventListener("click", () => dlg.close());
  for (const btn of dlg.querySelectorAll("[data-purge]")) {
    btn.addEventListener("click", async () => {
      dlg.querySelectorAll("button").forEach((b) => (b.disabled = true));
      try {
        const result = await deleteAccount(index, btn.dataset.purge === "1");
        // The account is gone; deleting the extension's vault secrets is the user's separate, opt-in choice.
        if (secretsOf && dlg.querySelector("#forget-secrets")?.checked) await forgetExtensionSecrets(secretsOf).catch(() => {});
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
// PRD 0036 RF2.3: the certificate a 9router instance presented, for the user to check on the server before
// trusting it (DS-CMP-DLG). Resolves "trust" or "cancel"; nothing is pinned without "trust".
function confirmFingerprint(fingerprint) {
  return new Promise((resolve) => {
    const dlg = document.createElement("dialog");
    dlg.className = "dlg";
    dlg.innerHTML = `<form method="dialog" class="dlg-body">
      <h2>${te("dialog.aggregator.trustTitle")}</h2>
      <p>${te("dialog.aggregator.trustBody")}</p>
      <p><code class="fingerprint">${esc(fingerprint)}</code></p>
      <div class="dlg-actions">
        <button type="button" class="btn" data-choice="cancel">${te("common.cancel")}</button>
        <button type="button" class="btn primary" data-choice="trust">${te("dialog.aggregator.trust")}</button>
      </div>
    </form>`;
    document.body.append(dlg);
    showDialog(dlg);
    let choice = "cancel";
    for (const btn of dlg.querySelectorAll("[data-choice]")) {
      btn.addEventListener("click", () => {
        choice = btn.dataset.choice;
        dlg.close();
      });
    }
    dlg.addEventListener("close", () => { dlg.remove(); resolve(choice); });
  });
}

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
