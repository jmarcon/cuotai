#!/usr/bin/env node
// PRD 0022 RF12 (DS section 7): only the refresh spinner loops; no other keyframes, no transform transitions on data
// elements, no count-up, flash, shake, entrance, exit ghost, one-turn spin or pulse; tokens at 120/180 ms; the
// dialog fades with opacity only. Also checks RF1.9's lint on the tree and on a fixture (Python 3 when available).
import assert from "node:assert/strict";
import { readFile, readdir, mkdtemp, writeFile, rm } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ui = new URL("../apps/desktop/ui/", import.meta.url);
const read = (rel) => readFile(new URL(rel, ui), "utf8");
const strip = (css) => css.replace(/\/\*[\s\S]*?\*\//g, "");

const cssFiles = (await readdir(new URL("css/", ui))).filter((f) => f.endsWith(".css"));
for (const f of cssFiles) {
  const css = strip(await read(`css/${f}`));
  for (const [, name] of css.matchAll(/@keyframes\s+([\w-]+)/g)) assert.equal(name, "spin", `${f}: @keyframes ${name}`);
  for (const [, value] of css.matchAll(/transition\s*:([^;]+);/g)) {
    assert.doesNotMatch(value, /\bwidth\b|stroke-dasharray|\btransform\b[^;]*--motion-(base|slow)/, `${f}: data or slow transform transition: ${value}`);
  }
  assert.doesNotMatch(css, /scale\(1\.01\)/, `${f}: hover scale`);
  assert.doesNotMatch(css, /backdrop-filter/, `${f}: component blur`);
}
const themes = strip(await read("css/themes.css"));
assert.match(themes, /--motion-fast: 120ms;/);
assert.match(themes, /--motion-base: 180ms;/);
assert.match(themes, /--motion-slow: var\(--motion-base\);/);

const motion = await read("js/motion.js");
for (const gone of ["countUp", "spinOnce", "pulseIfDue", "export function after", "export function before", "scale("]) {
  assert.ok(!motion.includes(gone), `motion.js still has ${gone}`);
}
assert.match(motion, /animate\(\[\{ opacity: 0 \}, \{ opacity: 1 \}\]/, "the dialog fades with opacity only");
const tray = await read("js/tray.js");
assert.doesNotMatch(tray, /animateRings|\.animate\(/, "the tray animates nothing");
const main = await read("js/main.js");
assert.doesNotMatch(main, /motion\.(before|after|spinOnce|pulseIfDue)/);

// RF1.9: the literal-color lint passes on the tree and fails on a fixture with a literal color.
const python = ["python3", "python"].find((cmd) => spawnSync(cmd, ["--version"]).status === 0);
if (python) {
  const tool = fileURLToPath(new URL("../apps/desktop/tools/lint_ui.py", import.meta.url));
  const tree = spawnSync(python, [tool], { encoding: "utf8" });
  assert.equal(tree.status, 0, tree.stdout + tree.stderr);
  const dir = await mkdtemp(join(tmpdir(), "cuotai-lint-"));
  const fixture = join(dir, "fixture.css");
  await writeFile(fixture, ".x { color: #ff0000; transition: opacity 300ms; }\n@keyframes wobble { to { opacity: 0; } }\n");
  const bad = spawnSync(python, [tool, "--files", fixture], { encoding: "utf8" });
  await rm(dir, { recursive: true, force: true });
  assert.equal(bad.status, 1, "the fixture must fail");
  assert.match(bad.stdout, /literal hex color/);
  assert.match(bad.stdout, /literal duration/);
  assert.match(bad.stdout, /@keyframes other than spin: wobble/);
} else {
  console.log("note: no Python 3 found; lint_ui.py checks skipped");
}

console.log("PASS: only the spinner loops; motion tokens, dialog fade and the literal-color lint hold");
