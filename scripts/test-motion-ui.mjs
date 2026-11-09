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
import { installUiEnv, report } from "./ui-test-env.mjs";

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

// PRD 0022 RF12.3/RF13.4 (PRD 0004 re-validation R-05): the static change marker, its tooltip and the polite
// announcement, rendered by the production component module in Node. An absent figure is never announced as 0%.
installUiEnv();
const live = { textContent: "" };
globalThis.document.getElementById = (id) => (id === "cuotai-live" ? live : null);
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const lib = await import("../apps/desktop/ui/js/lib.js");
await lib.loadProviderLabels();
const c = await import("../apps/desktop/ui/js/components.js");
const work = report({ account: "work@example.com", left: 31 });
const home = report({ account: "home@example.com", index: 1, left: 80 });
const [workKey, homeKey] = [lib.reportKey(work), lib.reportKey(home)];
c.trackChanges({ finished_at: "2026-01-01T00:00:00Z", reports: [work, home] });
assert.equal(c.changeMarkHtml(workKey), "", "the first refresh marks nothing");
c.announceChanges();
assert.equal(live.textContent, "", "the first refresh announces nothing");
c.trackChanges({ finished_at: "2026-01-01T00:05:00Z", reports: [report({ account: "work@example.com", left: 12 }), home] });
const mark = c.changeMarkHtml(workKey);
assert.match(mark, /^<i class="chg" aria-hidden="true" title="[^"]*31%[^"]*"><\/i>$/, `decorative marker with the previous value: ${mark}`);
assert.equal(c.changeMarkHtml(homeKey), "", "an unchanged account has no marker");
c.announceChanges();
assert.match(live.textContent, /work@example\.com/);
assert.match(live.textContent, /12%/);
assert.match(live.textContent, /31%/);
assert.doesNotMatch(live.textContent, /home@example\.com/);
live.textContent = "";
c.announceChanges();
assert.equal(live.textContent, "", "the announcement is written once per refresh");
// The same refresh seen again changes nothing; opening the account clears its marker.
c.trackChanges({ finished_at: "2026-01-01T00:05:00Z", reports: [report({ account: "work@example.com", left: 99 }), home] });
assert.notEqual(c.changeMarkHtml(workKey), "");
c.clearChange(workKey);
assert.equal(c.changeMarkHtml(workKey), "", "opening the account clears the marker");
// A figure that goes absent announces the state's name, never 0%.
c.trackChanges({ finished_at: "2026-01-01T00:10:00Z", reports: [report({ account: "work@example.com", left: null }), home] });
c.announceChanges();
assert.match(live.textContent, /work@example\.com/);
assert.doesNotMatch(live.textContent, /\b0%/, `absent is never zero: ${live.textContent}`);
assert.notEqual(c.changeMarkHtml(workKey), "");
// The next refresh with no change removes the marker.
c.trackChanges({ finished_at: "2026-01-01T00:15:00Z", reports: [report({ account: "work@example.com", left: null }), home] });
assert.equal(c.changeMarkHtml(workKey), "", "the marker lasts one refresh");

// RF12.4 / DS-MOT-7 (PRD 0004 re-validation V-03): every refresh button that spins while busy also carries
// aria-busy and the busy name, so reduced motion (which stops the spin) still says "Refreshing…".
for (const file of ["js/main.js", "js/tray.js"]) {
  const source = await read(file);
  const buttons = [...source.matchAll(/<button\b(?:[^<]|<(?!\/button>))*?icon\("refresh",[^)]*"spin"[^)]*\)/g)].map((m) => m[0]);
  assert.ok(buttons.length > 0, `${file}: no spinning refresh button found`);
  for (const button of buttons) {
    assert.match(button, /aria-busy="true"/, `${file}: spinning refresh button without aria-busy: ${button.slice(0, 120)}`);
    // The name is either written inline or a variable that switches to "tray.refreshing" while busy.
    const named = /aria-label="\$\{(?:esc\()?(\w+)\)?\}"/.exec(button)?.[1];
    const busyName = /tray\.refreshing/.test(button)
      || (named && new RegExp(`const ${named} = \\w+ \\? t[e]?\\("tray\\.refreshing"\\)`).test(source));
    assert.ok(busyName, `${file}: spinning refresh button without the busy name: ${button.slice(0, 120)}`);
  }
}

console.log("PASS: only the spinner loops; motion tokens, dialog fade, the literal-color lint, the change marker, the live region and the busy names hold");
