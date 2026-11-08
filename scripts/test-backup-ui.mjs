#!/usr/bin/env node
// Browser-free UI smoke for PRD 0046 RF2.1/RF3.3/RF4.4 (DS-CMP-BAK-1..3, DS-CMP-DOC-1): the Back up… preview, the
// restore plan with a choice per replacement, the next steps (in the desktop's words, never a variable name), the
// shell commands they call, and the "Check setup" link of a "Needs setup" card, in every built-in catalog.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const storage = new Map();
globalThis.window = {};
globalThis.document = {
  hidden: false,
  documentElement: { lang: "", dataset: {}, toggleAttribute: () => {} },
  addEventListener: () => {},
};
globalThis.matchMedia = () => ({ matches: false, addEventListener: () => {} });
globalThis.addEventListener = () => {};
Object.defineProperty(globalThis, "navigator", { value: { languages: ["en-US"], language: "en-US" }, configurable: true });
globalThis.location = { search: "" };
globalThis.localStorage = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) };
globalThis.fetch = async (relative) => {
  try {
    const path = new URL(`apps/desktop/ui/${relative}`, root);
    const data = JSON.parse(await readFile(fileURLToPath(path), "utf8"));
    return { ok: true, json: async () => data };
  } catch {
    return { ok: false, json: async () => ({}) };
  }
};

const i18n = await import("../apps/desktop/ui/js/i18n.js");
const { backupBodyHtml, planBodyHtml, nextStepsHtml } = await import("../apps/desktop/ui/js/backup-dialog.js");
await i18n.initI18n();

const view = {
  files: [{ path: "config/cuotai.toml", kind: "config" }, { path: "history/acc_demo00000001.csv", kind: "history" }],
  excluded: ["vault", "serve_token"].map((item) => ({ id: `bundle.excluded.${item}`, params: {}, fallback: item })),
  history_bytes: 40 << 20,
  history_large: true,
  default_name: "cuotai-backup-2026-11-07.cuotai-backup.zip",
};
const plan = {
  created_at: "2026-11-01T09:30:00Z",
  cuotai_version: "0.83.1",
  items: [
    { path: "config/cuotai.toml", target: "~/.config/cuotai/config.toml", action: "replace", accounts: { added: 1, changed: 0, removed: 2 } },
    { path: "prefs/desktop.json", target: "~/desktop.json", action: "skip" },
    { path: "config/hooks.toml", target: "~/hooks.toml", action: "add" },
    { path: "history/acc_demo00000001.csv", target: "history", action: "merge", merge: { rows: 120, new: 80 } },
    { path: "user/themes/night.css", target: "~/night.css", action: "keep" },
  ],
  needs: [
    { id: "bundle.need.env", params: { number: 1, provider: "groq", variable: "SEEDED_GROQ_VARIABLE" }, fallback: "Set {variable} for account {number} ({provider})." },
    { id: "bundle.need.sign_in", params: { number: 2, provider: "claude" }, fallback: "Sign in again to account {number} ({provider})." },
  ],
  next_steps: [{ kind: "env", account: 0, provider: "groq" }, { kind: "sign_in", account: 1, provider: "claude" }],
};

const VARIABLE = /SEEDED_GROQ_VARIABLE|_env\b|\{[a-z]+\}/;
for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  // DS-CMP-BAK-1: what goes in, what stays out with why, the history choice and its size warning.
  const backup = backupBodyHtml(view, true);
  assert.match(backup, /<code>config\/cuotai\.toml<\/code>/, code);
  assert.match(backup, /data-backup-history checked/, code);
  assert.match(backup, /warn-txt/, code);
  assert.ok(!/\{[a-z]+\}/.test(backup), `${code}: an unfilled placeholder: ${backup}`);
  // DS-CMP-BAK-2: a replacement is chosen by default, a newer local edit is not; additions and merges are not asked.
  const body = planBodyHtml(plan);
  assert.match(body, /data-restore-item="0" checked/, code);
  assert.match(body, /data-restore-item="1" (?!checked)/, code);
  assert.ok(!/data-restore-item="[234]"/.test(body), code);
  assert.match(body, /120/, code);
  assert.match(body, /80/, code);
  assert.ok(!/\{[a-z]+\}/.test(body), `${code}: an unfilled placeholder: ${body}`);
  // RF3.3 next steps: one action per need once the backup's accounts are the ones in use; never a variable name.
  const steps = nextStepsHtml(plan, { written: 3, restart: true }, true);
  assert.equal((steps.match(/data-restore-step=/g) ?? []).length, 2, code);
  assert.match(steps, /data-restore-step="1"/, code);
  assert.ok(!VARIABLE.test(steps), `${code}: next steps name a variable: ${steps}`);
  assert.ok(!nextStepsHtml(plan, { written: 3, restart: false }, false).includes("data-restore-step"), code);
  for (const key of ["firstrun.restore", "card.checkSetup", "settings.backup.title", "backup.title", "restore.title", "restore.restart"]) {
    assert.notEqual(i18n.t(key), key, `${code}: ${key} missing`);
  }
}

// The commands these dialogs call are registered by the shell, and the guide link is allow-listed.
const libRs = await readFile(fileURLToPath(new URL("apps/desktop/src-tauri/src/lib.rs", root)), "utf8");
const aboutRs = await readFile(fileURLToPath(new URL("apps/desktop/src-tauri/src/about.rs", root)), "utf8");
const libJs = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/lib.js", root)), "utf8");
for (const command of ["backup_preview", "backup_write", "restore_plan", "restore_apply", "restart_app", "doctor_report"]) {
  assert.match(libRs, new RegExp(`\\n\\s+${command},\\n`), `${command} is not registered`);
  assert.match(libJs, new RegExp(`call\\("${command}"`), `${command} is not called`);
}
const guide = libJs.match(/BACKUP_GUIDE_URL = "([^"]+)"/)[1];
assert.ok(aboutRs.includes(`"${guide}"`), "the backup guide is not allow-listed");

// DS-CMP-DOC-1: a "Needs setup" card links the setup check on the Files page, which runs it on arrival.
const mainJs = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/main.js", root)), "utf8");
assert.match(mainJs, /needs_configuration[\s\S]{0,120}data-settings="files" data-settings-anchor="doctor"/);
assert.match(mainJs, /data-restore-backup[\s\S]{0,200}data-add/, "Restore from a backup comes before Add account");
const filesJs = await readFile(fileURLToPath(new URL("apps/desktop/ui/js/settings/files.js", root)), "utf8");
assert.match(filesJs, /ctx\.anchor === "doctor"/);
console.log("backup ui: ok");
