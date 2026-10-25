// PRD 0017 RF5: presentation-only extension manager. All discovery,
// integrity, consent, account and execution decisions come from core.
import {
  disableExtension, enableExtension, errText, EXTENSIONS_GUIDE_URL, extensionDisclosure,
  extensionSecrets, listExtensions, openExtensionsFolder, openExternal, setExtensionOverride,
  setExtensionSecret, testExtension,
} from "./lib.js";
import { t, te, tl } from "./i18n.js";
import { icon } from "./lib.js";
import { showDialog } from "./motion.js";

const el = (tag, cls, text) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text != null) node.textContent = text;
  return node;
};

export async function openExtensionsDialog({ onChanged = () => {}, onRemove = () => {}, consentFor = null } = {}) {
  const dlg = el("dialog", "dlg extensions-dlg");
  const header = el("header", "dlg-head");
  header.append(el("h2", "", t("extensions.title")));
  const close = el("button", "icon");
  close.innerHTML = icon("x");
  close.type = "button";
  close.title = te("common.close");
  close.setAttribute("aria-label", te("common.close"));
  close.addEventListener("click", () => dlg.close());
  header.append(close);
  const intro = el("p", "help", t("extensions.help"));
  const status = el("p", "ext-status");
  status.setAttribute("role", "status");
  const list = el("div", "extension-list");
  // Mockup 40: what an extension is allowed to do, a guide link and the folder it lives in.
  const note = el("p", "help ext-note", `${t("extensions.note")} `);
  const guide = el("a", "text-link", t("extensions.guide"));
  guide.href = EXTENSIONS_GUIDE_URL;
  guide.addEventListener("click", (e) => {
    e.preventDefault();
    openExternal(EXTENSIONS_GUIDE_URL).catch((error) => { status.textContent = errText(error); });
  });
  note.append(guide);
  const foot = el("footer", "dlg-actions ext-foot");
  const folder = el("button", "btn ghost ext-folder");
  folder.type = "button";
  folder.innerHTML = `${icon("folder")}${te("extensions.openFolder")}`;
  folder.addEventListener("click", () => {
    openExtensionsFolder().catch((error) => { status.textContent = errText(error); });
  });
  const done = el("button", "btn", t("common.close"));
  done.type = "button";
  done.addEventListener("click", () => dlg.close());
  foot.append(folder, done);
  dlg.append(header, intro, status, list, note, foot);
  document.body.append(dlg);
  dlg.addEventListener("close", () => dlg.remove());
  showDialog(dlg);

  async function load() {
    list.replaceChildren(el("p", "help", t("common.loading")));
    try {
      const rows = await listExtensions();
      list.replaceChildren();
      if (!rows.length) list.append(el("p", "help", t("extensions.empty")));
      rows.forEach((row) => list.append(renderRow(row)));
    } catch (error) {
      list.replaceChildren(el("p", "bad", errText(error)));
    }
  }

  function renderRow(row) {
    const article = el("article", "extension-row");
    const mark = el("span", "extension-ico");
    mark.innerHTML = icon("puzzle");
    mark.setAttribute("aria-hidden", "true");
    const body = el("div", "extension-body");
    const title = el("div", "extension-title");
    title.append(el("strong", "", row.name));
    // DS-CMP-BADGE-1/-2: the version as a neutral badge, the state as a chip in its role.
    if (row.version) title.append(el("span", "ver", t("extensions.version", { version: row.version })));
    const chip = el("span", `extension-state ${row.state}`);
    chip.innerHTML = icon(row.state === "enabled" ? "check" : "info");
    chip.append(tl(row.state_label));
    title.append(chip);
    const description = el("p", "extension-desc", row.description || t("extensions.noDescription"));
    const runtime = el("p", "extension-runtime");
    const when = row.last_run ? new Date(row.last_run).toLocaleString() : t("extensions.neverRun");
    runtime.append(el("code", "extension-id", row.id));
    runtime.append(document.createTextNode(` · ${t("extensions.lastRun")}: ${when}`));
    if (row.error) {
      runtime.append(document.createTextNode(" · "));
      runtime.append(el("span", "extension-runtime-error", tl(row.error)));
    }
    const actions = el("div", "extension-actions");
    // Core decides what a row allows (`actions`); the dialog renders it and
    // decides nothing itself.
    for (const [key, label, cls] of [
      ["test", t("extensions.test"), ""],
      ["disable", t("extensions.disable"), "danger"],
      ["enable", t("extensions.enable"), "primary"],
    ]) {
      if (!row.actions?.[key]) continue;
      const handler = { test: runTest, disable: runDisable, enable: consent }[key];
      actions.append(actionButton(label, () => handler(row.id), cls));
    }
    // PRD 0028 RF7.4: an approved extension's secrets are entered here and
    // kept in the system vault by core; the dialog never shows a value.
    if (row.actions?.test && row.declares_secrets) {
      actions.append(actionButton(t("extensions.secrets"), () => secrets(row.id)));
    }
    if (row.actions?.remove && row.account) {
      actions.append(actionButton(t("extensions.removeAccount"), async () => {
        // PRD 0028 RF7.4: when the vault holds secrets of this extension, the
        // removal asks whether to delete them too (core lists, never the value).
        let hasSecrets = false;
        try { hasSecrets = (await extensionSecrets(row.id)).some((item) => item.stored); } catch { /* removal still works */ }
        dlg.close();
        onRemove(row.account.index, row.account.name, hasSecrets ? row.id : null);
      }));
    }
    if (actions.children.length > 2) actions.classList.add("many");
    body.append(title, description, runtime);
    article.append(mark, body, actions);
    // PRD 0028 RF7.1: an enabled override extension lists the accounts of
    // its provider; each checkbox is "Use extension override" for that one.
    if (row.override_targets?.length) {
      const box = el("fieldset", "extension-overrides");
      box.append(el("legend", "", t("extensions.overrideTitle")));
      box.append(el("p", "help", t("extensions.overrideHelp")));
      for (const target of row.override_targets) {
        const label = el("label", "extension-confirm");
        const check = document.createElement("input");
        check.type = "checkbox";
        check.checked = !!target.active;
        check.addEventListener("change", async () => {
          check.disabled = true;
          try {
            await setExtensionOverride(target.index, row.id, check.checked);
            status.textContent = check.checked
              ? t("extensions.overrideOn", { account: target.name })
              : t("extensions.overrideOff", { account: target.name });
            onChanged();
          } catch (error) {
            check.checked = !check.checked;
            status.textContent = errText(error);
          } finally {
            check.disabled = false;
          }
        });
        label.append(check, document.createTextNode(t("extensions.overrideUse", { account: target.name })));
        box.append(label);
      }
      article.append(box);
    }
    return article;
  }

  const actionButton = (label, action, cls = "") => {
    const button = el("button", `btn ${cls}`, label);
    button.type = "button";
    button.addEventListener("click", async () => {
      button.disabled = true;
      try {
        await action();
      } catch (error) {
        // Every action reports here: an async rejection must never escape the
        // click handler and leave the dialog with a stale row.
        status.textContent = errText(error);
        await load();
      } finally {
        if (button.isConnected) button.disabled = false;
      }
    });
    return button;
  };

  async function consent(id) {
    try {
      const { lines, candidate } = await extensionDisclosure(id);
      const box = el("section", "extension-consent");
      box.setAttribute("role", "alertdialog");
      box.setAttribute("aria-label", te("extensions.consentTitle"));
      box.append(el("h3", "", t("extensions.consentTitle")));
      for (const line of lines) box.append(el("p", "", tl(line)));
      const label = el("label", "extension-confirm");
      const check = document.createElement("input");
      check.type = "checkbox";
      label.append(check, document.createTextNode(t("extensions.confirm")));
      const buttons = el("div", "extension-actions");
      const cancel = actionButton(t("common.cancel"), async () => box.remove());
      const approve = actionButton(t("extensions.approve"), async () => {
        if (!check.checked) return;
        try {
          const result = await enableExtension(candidate);
          status.textContent = tl(result.message);
          box.remove();
          await load();
          onChanged();
        } catch (error) {
          // The candidate is single use: whether the files changed or the
          // confirmation expired, show the reason and ask again, so the
          // user never approves bytes they did not read.
          status.textContent = errText(error);
          box.remove();
          await load();
        }
      }, "primary");
      approve.disabled = true;
      check.addEventListener("change", () => { approve.disabled = !check.checked; });
      buttons.append(cancel, approve);
      box.append(label, buttons);
      list.prepend(box);
      check.focus();
    } catch (error) {
      status.textContent = errText(error);
    }
  }

  async function secrets(id) {
    try {
      const items = await extensionSecrets(id);
      if (!items.length) {
        status.textContent = t("extensions.secretsNone");
        return;
      }
      const box = el("section", "extension-consent extension-secrets");
      box.setAttribute("aria-label", te("extensions.secretsTitle"));
      box.append(el("h3", "", t("extensions.secretsTitle")));
      box.append(el("p", "help", t("extensions.secretsHelp")));
      for (const item of items) {
        const label = el("label", "field");
        const caption = el("span", "", `${item.name} (${item.env})`);
        const input = document.createElement("input");
        input.type = "password";
        input.autocomplete = "off";
        input.spellcheck = false;
        input.placeholder = item.stored ? t("extensions.secretStored") : "";
        label.append(caption, input, el("small", "help", item.description));
        const save = actionButton(t("extensions.secretSave"), async () => {
          if (!input.value.trim()) return;
          const value = input.value;
          input.value = "";
          const result = await setExtensionSecret(id, item.name, value);
          status.textContent = tl(result.message);
          input.placeholder = t("extensions.secretStored");
        }, "primary");
        box.append(label, save);
      }
      const buttons = el("div", "extension-actions");
      buttons.append(actionButton(t("common.close"), async () => box.remove()));
      box.append(buttons);
      list.prepend(box);
      box.querySelector("input")?.focus();
    } catch (error) {
      status.textContent = errText(error);
    }
  }

  async function runDisable(id) {
    try {
      const result = await disableExtension(id);
      status.textContent = tl(result.message);
      await load();
      onChanged();
    } catch (error) { status.textContent = errText(error); }
  }

  async function runTest(id) {
    try {
      const result = await testExtension(id);
      status.textContent = result.status === "ready"
        ? t("extensions.testReady")
        : (result.notes?.map(tl).join(" · ") || t("extensions.testFailed"));
      await load();
    } catch (error) { status.textContent = errText(error); }
  }

  await load();
  // PRD 0027 RF1.3: chosen from the Add-account picker: review it at once.
  if (consentFor) await consent(consentFor);
}
