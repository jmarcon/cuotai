// Settings › Advanced › Files (PRD 0020 RF2.5): the accounts file and the themes-and-languages folder, as path rows
// (RF3.2) whose full path sits in a remembered disclosure (RF5.2).
import { t, te, tl } from "../i18n.js";
import { THEMES } from "../themes.js";
import { LOCALES, skippedMessage, userContent } from "../usercontent.js";
import { accountFullLabel, BACKUP_GUIDE_URL, configLabel, doctorReport, esc, icon, openExternal, openUserDir, otherConfigNotice, pickFolder, pickTomlFile, setConfigFile, setDataDir, setUserDir, vaultAudit, vaultPrune, vaultRemove } from "../lib.js";
import { openBackupDialog, openRestoreDialog } from "../backup-dialog.js";
import { configErrorText, errorText, group } from "./common.js";
import { buttons, disc, pathHtml, row } from "./rows.js";

// The last failure of each group, shown once by the next paint.
const flash = { config: "", folder: "", datadir: "", credentials: "", doctor: "", backup: "" };
const take = (key) => { const text = flash[key]; flash[key] = ""; return text; };

export function html({ settings: s, view }) {
  const v = view();
  const nThemes = THEMES.filter((x) => x.user).length;
  const nLocales = LOCALES.filter((x) => x.user).length;
  const warn = (text) => `<p class="hint warn-txt">${text}</p>`;
  const configRow = row({ key: "config-file", label: te("settings.config.title"), info: te("settings.config.help"), msg: "config-msg",
    control: () => buttons(`<button class="btn sm" data-pick-config ${s.config_env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.config.choose")}</button>`,
      `<button class="btn sm" data-reset-config ${s.config_env_locked || !s.config_file ? "disabled" : ""}>${te("settings.config.reset")}</button>`),
    body: `${otherConfigNotice(v) ? warn(esc(otherConfigNotice(v))) : ""}${s.config_env_locked ? warn(te("settings.config.env")) : ""}
      ${disc("config-path", te("settings.config.path"), pathHtml(configLabel(v)))}` });
  const folderRow = row({ key: "user-dir", attrs: 'id="set-folder" data-section="folder"', label: te("settings.folder.title"), info: te("settings.folder.help"), msg: "folder-msg",
    desc: `${s.custom_dir ? te("settings.folder.custom") : te("settings.folder.default")} · ${te("settings.folder.loaded", { themes: t("settings.folder.themes", { count: nThemes }), locales: t("settings.folder.locales", { count: nLocales }) })}`,
    control: () => buttons(`<button class="btn sm" data-pick ${s.env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.folder.choose")}</button>`,
      `<button class="btn sm" data-reset ${s.env_locked || !s.custom_dir ? "disabled" : ""}>${te("settings.folder.reset")}</button>`,
      `<button class="btn sm" data-open-dir>${te("settings.folder.open")}</button>`),
    body: `${s.env_locked ? warn(te("settings.folder.env")) : ""}${userContent.errors.map((e) => warn(esc(skippedMessage(e)))).join("")}
      ${disc("folder-path", te("settings.config.path"), pathHtml(s.user_dir))}` });
  // PRD 0051 RF5.2: the folder of every local store, below CUOTAI_DATA_DIR; a change applies at the next start.
  const pending = !s.data_dir_env_locked && !s.data_dir_error && (s.data_dir_custom ?? null) !== (s.data_dir_source === "desktop" ? (s.data_dir ?? null) : null);
  const dataDirRow = row({ key: "data-dir", attrs: 'data-section="datadir"', label: te("settings.dataDir.title"), info: te("settings.dataDir.help"), msg: "datadir-msg",
    desc: s.data_dir_source === "environment" ? "CUOTAI_DATA_DIR" : s.data_dir_source === "desktop" ? te("settings.folder.custom") : te("settings.folder.default"),
    control: () => buttons(`<button class="btn sm" data-pick-data-dir ${s.data_dir_env_locked ? "disabled" : ""}>${icon("folder")} ${te("settings.folder.choose")}</button>`,
      `<button class="btn sm" data-reset-data-dir ${s.data_dir_env_locked || !s.data_dir_custom ? "disabled" : ""}>${te("settings.folder.reset")}</button>`),
    body: `${s.data_dir_env_locked ? warn(te("settings.dataDir.env")) : ""}${s.data_dir_error ? warn(esc(tl(s.data_dir_error))) : ""}${pending ? warn(te("settings.dataDir.pending")) : ""}
      ${disc("data-dir-path", te("settings.config.path"), `${pathHtml(s.data_dir)}<p class="hint">${te("settings.dataDir.cli")}</p>`)}` });
  // PRD 0029 RF6.2: what Cuotai stored in the system vault, filled in by `bind` (never a value).
  const vaultRowHtml = row({ key: "vault", attrs: 'data-section="credentials"', label: te("settings.vault.title"), info: te("settings.vault.help"), msg: "vault-msg",
    body: `<div data-vault data-protected-texts="vault" aria-live="polite"><p class="hint">${te("settings.vault.loading")}</p></div>` });
  // PRD 0046 RF2.1/RF3.3/RF5.2: a secret-free backup of these settings, its restore, and the recipes.
  const backupRowHtml = row({ key: "backup", attrs: 'data-section="backup"', label: te("settings.backup.title"), info: te("settings.backup.help"), msg: "backup-msg",
    control: () => buttons(`<button class="btn sm" data-backup>${icon("download")} ${te("settings.backup.backup")}</button>`,
      `<button class="btn sm" data-restore>${te("settings.backup.restore")}</button>`),
    body: `<p class="hint"><button class="link" data-backup-guide>${te("settings.backup.guide")} ${icon("ext")}</button></p>` });
  // PRD 0046 RF4.4: the core's self-check, run on demand and listed by status with each fix.
  const doctorRowHtml = row({ key: "doctor", attrs: 'data-section="doctor"', label: te("settings.doctor.title"), info: te("settings.doctor.help"), msg: "doctor-msg",
    control: () => buttons(`<button class="btn sm" data-doctor-run>${te("settings.doctor.run")}</button>`),
    body: `<div data-doctor aria-live="polite"></div>` });
  // RF2.2 (PRD 0020 re-validation V-14): two labelled groups, the files Cuotai reads and its upkeep.
  return group("config", te("settings.files.group.files"), configRow + folderRow + dataDirRow + vaultRowHtml)
    + group("maintenance", te("settings.files.group.maintenance"), backupRowHtml + doctorRowHtml);
}

/** 2026-11 re-validation F1/F2/R-01: the core's status of each entry; only "unused" is removed in bulk. */
const VAULT_OWNERS = { channel: "settings.vault.owner.channel", extension_secret: "settings.vault.owner.extension" };

/** What uses an entry (an account, a channel, an extension) or, when not in use, why Cuotai keeps or lists it. */
function vaultOwner(e) {
  if (e.status !== "in_use") return te(`settings.vault.status.${e.status}`);
  if (VAULT_OWNERS[e.kind]) return te(VAULT_OWNERS[e.kind]);
  // PRD 0029 RF3.4 (V-07): an unnamed account by its provider once.
  return esc(accountFullLabel({ provider: e.provider, named: e.named, display_name: e.account }));
}

const vaultKind = (e) => te(e.kind ? `settings.vault.kind.${e.kind}` : "settings.vault.kind.unknown");

/** One indexed entry: its kind, what uses it (or its status) and since when; never a value. */
function vaultRow(e, i) {
  const date = new Date(e.created_at).toLocaleDateString(document.documentElement.lang || undefined);
  const remove = e.status === "in_use" ? "" : ` <button class="link" data-vault-remove="${i}">${te("settings.vault.remove")}</button>`;
  return `<li><strong>${vaultKind(e)}</strong> — ${vaultOwner(e)}${e.present ? "" : ` · ${te("settings.vault.gone")}`} <small>${te("settings.vault.created", { date })}</small>${remove}</li>`;
}

export function vaultHtml(plan) {
  const entries = plan?.entries ?? [];
  const path = plan?.config_path ?? "";
  if (!entries.length) return `<p class="hint">${te("settings.vault.empty")}</p>`;
  const unused = entries.filter((e) => e.status === "unused").length;
  return `<p class="hint">${te("settings.vault.checked", { path })}</p>
    <ul class="hook-runs">${entries.map(vaultRow).join("")}</ul>
    ${unused ? `<div class="dlg-actions left"><button class="btn danger-btn" data-vault-prune>${icon("trash")} ${te("settings.vault.prune", { count: unused })}</button></div>
    <div class="confirm" data-vault-confirm data-protected-texts="vault-prune" role="alertdialog" aria-labelledby="vault-confirm-text" hidden>
      <p id="vault-confirm-text">${te("settings.vault.confirm", { count: unused, path })}</p>
      <div class="dlg-actions left"><button class="btn danger-btn" data-vault-prune-confirm>${te("settings.vault.pruneConfirm")}</button>
        <button class="btn" data-vault-prune-cancel>${te("confirm.cancel")}</button></div>
    </div>` : ""}
    <div class="confirm" data-vault-one data-protected-texts="vault-one" role="alertdialog" aria-labelledby="vault-one-text" hidden>
      <p id="vault-one-text"></p>
      <div class="dlg-actions left"><button class="btn danger-btn" data-vault-one-confirm>${te("settings.vault.pruneConfirm")}</button>
        <button class="btn" data-vault-one-cancel>${te("confirm.cancel")}</button></div>
    </div>`;
}

/** The line after a removal: how many went and how many were kept because they changed since the confirmation. */
export function vaultOutcomeText(outcome) {
  const deleted = outcome?.deleted?.length ?? 0;
  const skipped = outcome?.skipped?.length ?? 0;
  return [t("settings.vault.done", { count: deleted }), skipped ? t("settings.vault.skipped", { count: skipped }) : ""].filter(Boolean).join(" ");
}

const DOCTOR_ORDER = ["fail", "warn", "ok", "skip"];

/** The doctor's report, problems first; the status is a word, never only a color. */
export function doctorHtml(report) {
  const checks = [...(report?.checks ?? [])].sort((a, b) => DOCTOR_ORDER.indexOf(a.status) - DOCTOR_ORDER.indexOf(b.status));
  return `<ul class="hook-runs">${checks.map((c) => `<li><strong>${te(`settings.doctor.status.${c.status}`)}</strong> — ${esc(tl(c.title))}${c.detail ? `: ${esc(tl(c.detail))}` : ""}${c.fix ? `<br><small>${esc(tl(c.fix))}</small>` : ""}</li>`).join("")}</ul>`
    + diagnosticsHtml(report);
}

/** PRD 0052 RF3.4: the exact text "Copy diagnostics" writes (the same JSON as `cuotai --doctor --json`). */
export const diagnosticsText = (report) => JSON.stringify(report, null, 2);

/** The preview, visible before anything reaches the clipboard; nothing is sent or opened. */
function diagnosticsHtml(report) {
  return `<div class="doctor-copy" data-doctor-copy-box>
    <p class="hint">${te("settings.doctor.copyHelp")}</p>
    <p class="hint" data-doctor-report-hint>${te("settings.files.doctor_report_hint")}</p>
    <pre class="snippet" data-doctor-json tabindex="0" aria-label="${te("settings.doctor.preview")}">${esc(diagnosticsText(report))}</pre>
    <span class="set-btns"><button class="btn sm" data-doctor-copy>${icon("copy")} ${te("settings.doctor.copy")}</button></span>
    <p class="hint" id="doctor-copy-msg" role="status"></p>
  </div>`;
}

/** Copies the previewed text; a failure is said in place. */
export function bindDiagnostics(box, report, clipboard = navigator.clipboard) {
  const button = box.querySelector("[data-doctor-copy]");
  const msg = box.querySelector("#doctor-copy-msg");
  if (!button) return;
  button.onclick = async () => {
    try {
      if (!clipboard?.writeText) throw new Error("no clipboard");
      await clipboard.writeText(diagnosticsText(report));
      msg.textContent = t("settings.doctor.copied");
    } catch { msg.textContent = t("settings.doctor.copyFailed"); }
  };
}

export function bind(root, ctx) {
  const s = ctx.settings;
  // A failure is shown in its row after the repaint (RF4.3).
  for (const [key, id] of [["config", "#config-msg"], ["folder", "#folder-msg"], ["datadir", "#datadir-msg"], ["credentials", "#vault-msg"], ["backup", "#backup-msg"], ["doctor", "#doctor-msg"]]) root.querySelector(id).textContent = take(key);
  const fail = (key, text) => { flash[key] = text; ctx.rerender(); };
  root.querySelector("[data-open-dir]").onclick = () => openUserDir().catch((e) => fail("folder", errorText(e)));
  const apply = async (path) => {
    // Every window reloads on `user-content-changed` to read the new folder, this one included.
    try { await setUserDir(path); location.reload(); }
    catch (e) { fail("folder", errorText(e)); }
  };
  root.querySelector("[data-pick]").onclick = async () => {
    const picked = await pickFolder(s.user_dir);
    if (typeof picked === "string" && picked) apply(picked);
  };
  root.querySelector("[data-reset]").onclick = () => apply(null);
  const applyDataDir = async (path) => {
    try { await setDataDir(path); ctx.changed("data_dir"); ctx.rerender(); }
    catch (e) { fail("datadir", errorText(e)); }
  };
  root.querySelector("[data-pick-data-dir]").onclick = async () => {
    const picked = await pickFolder(s.data_dir_custom ?? s.data_dir);
    if (typeof picked === "string" && picked) applyDataDir(picked);
  };
  root.querySelector("[data-reset-data-dir]").onclick = () => applyDataDir(null);
  const applyConfig = async (path) => {
    try { ctx.setView(await setConfigFile(path)); ctx.changed("config_file"); ctx.rerender(); }
    catch (e) { fail("config", configErrorText(e)); }
  };
  root.querySelector("[data-pick-config]").onclick = async () => {
    const picked = await pickTomlFile(s.config_file ?? ctx.view()?.config_path);
    if (typeof picked === "string" && picked) applyConfig(picked);
  };
  root.querySelector("[data-reset-config]").onclick = () => applyConfig(null);
  const vault = root.querySelector("[data-vault]");
  const vaultMsg = root.querySelector("#vault-msg");
  const showVault = () => vaultAudit().then((plan) => {
    vault.innerHTML = vaultHtml(plan);
    const entries = plan?.entries ?? [];
    // One by one: the confirmation names the entry's kind and status.
    const one = vault.querySelector("[data-vault-one]");
    let pending = null;
    for (const button of vault.querySelectorAll("[data-vault-remove]")) {
      button.onclick = () => {
        const e = entries[Number(button.dataset.vaultRemove)];
        pending = { key: e.key, button };
        one.querySelector("#vault-one-text").textContent = t("settings.vault.removeConfirm", { label: `${t(e.kind ? `settings.vault.kind.${e.kind}` : "settings.vault.kind.unknown")} (${t(`settings.vault.status.${e.status}`)})` });
        one.hidden = false;
        one.querySelector("[data-vault-one-cancel]").focus();
      };
    }
    one.querySelector("[data-vault-one-cancel]").onclick = () => { one.hidden = true; pending?.button.focus(); pending = null; };
    one.querySelector("[data-vault-one-confirm]").onclick = async () => {
      const key = pending?.key;
      if (!key) return;
      try { await vaultRemove(key); vaultMsg.textContent = t("settings.vault.done", { count: 1 }); await showVault(); }
      catch (e) { fail("credentials", errorText(e)); }
    };
    const prune = vault.querySelector("[data-vault-prune]");
    if (!prune) return;
    const box = vault.querySelector("[data-vault-confirm]");
    // Exactly the keys shown; the core skips any that changed since.
    const shown = entries.filter((e) => e.status === "unused").map((e) => e.key);
    prune.onclick = () => { box.hidden = false; box.querySelector("[data-vault-prune-cancel]").focus(); };
    box.querySelector("[data-vault-prune-cancel]").onclick = () => { box.hidden = true; prune.focus(); };
    box.querySelector("[data-vault-prune-confirm]").onclick = async () => {
      try { const outcome = await vaultPrune(shown); vaultMsg.textContent = vaultOutcomeText(outcome); await showVault(); }
      catch (e) { fail("credentials", errorText(e)); }
    };
  }).catch((e) => { vault.innerHTML = `<p class="hint warn-txt">${esc(errorText(e))}</p>`; });
  showVault();
  const backupButton = root.querySelector("[data-backup]");
  backupButton.onclick = () => openBackupDialog(backupButton);
  const restoreButton = root.querySelector("[data-restore]");
  restoreButton.onclick = () => openRestoreDialog(restoreButton);
  root.querySelector("[data-backup-guide]").onclick = () => openExternal(BACKUP_GUIDE_URL).catch((e) => fail("backup", errorText(e)));
  const doctor = root.querySelector("[data-doctor]");
  const runDoctor = async () => {
    doctor.innerHTML = `<p class="hint">${te("settings.doctor.running")}</p>`;
    try { const report = await doctorReport(); doctor.innerHTML = doctorHtml(report); bindDiagnostics(doctor, report); }
    catch (e) { doctor.innerHTML = `<p class="hint warn-txt">${esc(errorText(e))}</p>`; }
  };
  root.querySelector("[data-doctor-run]").onclick = runDoctor;
  // PRD 0046 RF4.4: arriving from a card's "Check setup" runs it at once.
  if (ctx.anchor === "doctor") { ctx.anchor = null; return runDoctor(); }
}
