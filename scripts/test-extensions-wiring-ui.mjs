#!/usr/bin/env node
// Browser-free smoke for the Extensions dialog's shell wiring (PRD 0022 RF10, PRD 0028 RF7.4): every command the
// UI calls is registered in the Tauri invoke handler, and the guide link the UI opens is the exact address the
// shell's allow-list lets through (the UI never chooses an arbitrary destination).
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const read = (path) => readFile(fileURLToPath(new URL(`../${path}`, import.meta.url)), "utf8");
const [lib, shell, about] = await Promise.all([
  read("apps/desktop/ui/js/lib.js"),
  read("apps/desktop/src-tauri/src/lib.rs"),
  read("apps/desktop/src-tauri/src/about.rs"),
]);

for (const command of ["open_extensions_folder", "forget_extension_secrets", "extension_secrets", "set_extension_secret"]) {
  assert.ok(lib.includes(`call("${command}"`), `lib.js calls ${command}`);
  assert.match(shell, new RegExp(`async fn ${command}\\b|\\bfn ${command}\\b`), `${command} is a Tauri command`);
  assert.match(shell, new RegExp(`^\\s+${command},$`, "m"), `${command} is in the invoke handler`);
}

const guide = lib.match(/EXTENSIONS_GUIDE_URL = "([^"]+)"/)?.[1];
assert.ok(guide, "lib.js declares the guide address");
assert.ok(about.includes(`"${guide}"`), "the shell's allow-list constant is the same address");
assert.match(about, /url == EXTENSIONS_GUIDE/, "the guide address is in is_listed");
assert.match(about, /fn open_extensions_folder\(\)[\s\S]*cuotai_core::extensions::default_dir\(\)/, "the folder comes from the core, never from the UI");
// PRD 0020 RF2.5: Settings > Advanced > Extensions reuses the dialog's manager; the main window keeps its entry point.
const [dialog, page, registry, main] = await Promise.all([
  read("apps/desktop/ui/js/extensions-dialog.js"),
  read("apps/desktop/ui/js/settings/extensions.js"),
  read("apps/desktop/ui/js/settings/registry.js"),
  read("apps/desktop/ui/js/main.js"),
]);
assert.match(dialog, /export async function mountExtensions\(/, "the manager body is shared");
assert.match(dialog, /export async function openExtensionsDialog\(/, "the dialog stays");
assert.match(dialog, /await mountExtensions\(dlg,/, "the dialog is built on the shared body");
assert.match(page, /mountExtensions\(host,/, "the page mounts the shared body");
assert.match(registry, /id: "extensions", group: ADVANCED/, "Extensions is an Advanced page");
assert.match(main, /openExtensionsDialog\(/, "the main window still opens the dialog");
assert.match(main, /data-extensions/, "the main window keeps the Extensions button");
for (const code of ["en-US", "pt-BR", "es", "it"]) {
  const catalog = JSON.parse(await read(`apps/desktop/ui/locales/${code}.json`));
  for (const key of ["settings.page.extensions", "settings.kw.extensions"]) assert.ok(catalog[key], `${code}: ${key}`);
}
// PRD 0028 RF7.4: removing an extension's account from the Settings page keeps the "also delete its secrets" offer:
// the page forwards the extension id, and the main window passes it to the confirmation.
assert.match(page, /onRemove: \(index, _name, secretsOf = null\) => requestAccountAction\("remove", index, secretsOf \? \{ secretsOf \} : \{\}\)/,
  "the page forwards the extension whose secrets the removal may delete");
assert.match(main, /onAccountAction\(\(\{ action, index, secretsOf = null \}\)/, "the main window reads it");
assert.match(main, /action === "remove" && secretsOf\) return removeExtensionAccount\(index, accountLabel\(r\), String\(secretsOf\)\)/);
assert.match(main, /const removeExtensionAccount = \(index, label, secretsOf = null\) =>\s*confirmDelete\(index, label, \{\s*usesOauth: false,\s*secretsOf,/);
console.log("extensions wiring ok");
