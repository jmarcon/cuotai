#!/usr/bin/env node
// Browser-free UI smoke for PRD 0028 (re-validation C-02, C-05): the details of an account whose provider changed its
// answer show the "What changed" block (DS-CMP-DIAG-1: the core's sentence, the field paths in mono without
// backticks, "Values are never recorded") and do not repeat the drift note; the problem-report dialog opens GitHub
// only through the shell's `open_problem_report`, never by handing a URL to `open_external`.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { installUiEnv, report } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const { detailHtml, driftHtml } = await import("../apps/desktop/ui/js/detail.js");
await i18n.initI18n?.();

const sentence = { id: "diagnostic.schema_drift", params: { path: "five_hour.utilization", expected: "number", found: "string" }, fallback: "The provider changed five_hour.utilization (number → string)." };
const failing = {
  ...report({ provider: "claude", status: "error", left: null }),
  metrics: [],
  notes: ["Response is incompatible with the query interface; no value was estimated.", sentence.fallback],
  notes_l10n: [{ id: "query.schema", params: {}, fallback: "Response is incompatible with the query interface; no value was estimated." }, sentence],
  drift: {
    heading: { id: "diagnostic.what_changed", params: {}, fallback: "What changed" },
    sentence,
    paths: [{ path: "five_hour.utilization", expected: "number", found: "string" }, { path: "seven_day", expected: "object", found: "missing" }],
    privacy: { id: "diagnostic.values_never_recorded", params: {}, fallback: "Values are never recorded." },
  },
};

const block = driftHtml(failing);
assert.match(block, /data-drift/, "the block is drawn");
assert.match(block, /What changed/);
assert.match(block, /Values are never recorded\./);
assert.match(block, /<code class="det-drift-path">five_hour\.utilization<\/code>/, "paths are set in the mono element");
assert.match(block, /<code class="det-drift-path">seven_day<\/code>/);
assert.doesNotMatch(block, /`/, "no literal backticks");
assert.equal(driftHtml({ ...failing, drift: undefined }), "", "no drift, no block");

const html = detailHtml(failing);
assert.ok(html.includes("data-drift"), "the details carry the block");
assert.equal(html.split("The provider changed five_hour.utilization").length - 1, 1, "the sentence is shown once, in the block");
assert.doesNotMatch(html, /`/);
assert.match(readFileSync(new URL("../apps/desktop/ui/css/detail.css", import.meta.url), "utf8"), /\.det-drift-path \{\s*font: var\(--text-sm\) var\(--font-mono\)/, "the paths use --font-mono");

// C-05 (S-01) with V-09: Open goes through the shell's own command; no URL is handed to the browser by the page.
const dialog = readFileSync(new URL("../apps/desktop/ui/js/report-dialog.js", import.meta.url), "utf8");
assert.match(dialog, /openProblemReport\(index, generation\)/, "Open asks the shell to rebuild and open the draft");
assert.doesNotMatch(dialog, /openExternal|draft\.url/, "the page never opens a URL of its own");
assert.match(dialog, /report\.openForm/, "a copy-only draft still offers the form");
const lib = await import("../apps/desktop/ui/js/lib.js");
assert.equal(typeof lib.openProblemReport, "function");
const draft = await lib.reportProblem(0, 1);
assert.equal(draft.url, undefined, "the draft that reaches the page carries no link");

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  for (const key of ["report.intro", "report.open", "report.openForm"]) {
    assert.notEqual(i18n.t(key), key, `${code}: ${key}`);
  }
  assert.match(i18n.t("report.intro"), /[.。।]$/, `${code}: report.intro ends with a period`);
}
console.log("report dialog: drift block, shell-opened report and catalogs OK");
