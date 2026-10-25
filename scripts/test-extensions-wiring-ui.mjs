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
console.log("extensions wiring ok");
