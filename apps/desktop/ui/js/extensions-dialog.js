// PRD 0017 RF5: presentation-only extension manager. All discovery,
// integrity, consent, account and execution decisions come from core.
import {
  disableExtension, enableExtension, errText, extensionDisclosure,
  listExtensions, testExtension,
} from "./lib.js";
import { t, te, tl } from "./i18n.js";
import { showDialog } from "./motion.js";

const el = (tag, cls, text) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text != null) node.textContent = text;
  return node;
};

export async function openExtensionsDialog({ onChanged = () => {}, onRemove = () => {} } = {}) {
  const dlg = el("dialog", "dlg extensions-dlg");
  const header = el("header", "dlg-head");
  header.append(el("h2", "", t("extensions.title")));
  const close = el("button", "icon", "×");
  close.type = "button";
  close.setAttribute("aria-label", te("common.close"));
  close.addEventListener("click", () => dlg.close());
  header.append(close);
  const intro = el("p", "help", t("extensions.help"));
  const status = el("p", "ext-status");
  status.setAttribute("role", "status");
  const list = el("div", "extension-list");
  dlg.append(header, intro, status, list);
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
    const title = el("div", "extension-title");
    title.append(el("strong", "", row.name));
    title.append(el("code", "", row.id));
    title.append(el("span", `extension-state ${row.state}`, tl(row.state_label)));
    const version = el("small", "", row.version ? t("extensions.version", { version: row.version }) : "");
    const description = el("p", "", row.description || t("extensions.noDescription"));
    const runtime = el("p", "extension-runtime");
    const when = row.last_run ? new Date(row.last_run).toLocaleString() : t("extensions.neverRun");
    runtime.append(document.createTextNode(`${t("extensions.lastRun")}: ${when}`));
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
    if (row.actions?.remove && row.account) {
      actions.append(actionButton(t("extensions.removeAccount"), () => {
        dlg.close();
        onRemove(row.account.index, row.account.name);
      }));
    }
    article.append(title, version, description, runtime, actions);
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
}
