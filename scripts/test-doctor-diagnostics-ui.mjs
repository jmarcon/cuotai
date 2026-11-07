#!/usr/bin/env node
// Browser-free UI smoke for PRD 0052 RF3.4 (under its §13 option A, inside PRD 0046's Check setup): after the check runs,
// the exact JSON "Copy diagnostics" writes is previewed first (the same report as `cuotai --doctor --json`), the button
// copies that very text, a clipboard failure is said in place, and nothing opens a browser or sends data.
import assert from "node:assert/strict";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv();
const i18n = await import("../apps/desktop/ui/js/i18n.js");
const lib = await import("../apps/desktop/ui/js/lib.js");
const { doctorHtml, diagnosticsText, bindDiagnostics } = await import("../apps/desktop/ui/js/settings/files.js");
await i18n.initI18n?.();

const report = await lib.doctorReport();
const text = diagnosticsText(report);
assert.deepEqual(JSON.parse(text), report, "the copied text is the report itself");
assert.ok(JSON.parse(text).artifacts.length > 0, "the artifact registry rides along");

const html = doctorHtml(report);
const escaped = text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;");
const pre = html.match(/<pre class="snippet" data-doctor-json[^>]*>([\s\S]*?)<\/pre>/);
assert.ok(pre, "the preview is drawn with the list");
assert.equal(pre[1], escaped, "the preview shows exactly what is copied");
assert.ok(html.indexOf("data-doctor-json") < html.indexOf("data-doctor-copy>"), "the preview comes before the button");
assert.match(html, new RegExp(i18n.t("settings.doctor.copy")));
assert.doesNotMatch(html, /https?:\/\/(?!127\.0\.0\.1)/, "no link to open");

// A fake box: the button's click writes to the injected clipboard, and the status line says how it went.
const box = (() => {
  const button = { onclick: null };
  const msg = { textContent: "" };
  return { button, msg, querySelector: (sel) => (sel === "[data-doctor-copy]" ? button : sel === "#doctor-copy-msg" ? msg : null) };
})();
let written = null;
bindDiagnostics(box, report, { writeText: async (value) => { written = value; } });
await box.button.onclick();
assert.equal(written, text, "the clipboard receives the previewed text");
assert.equal(box.msg.textContent, i18n.t("settings.doctor.copied"));

bindDiagnostics(box, report, { writeText: async () => { throw new Error("denied"); } });
await box.button.onclick();
assert.equal(box.msg.textContent, i18n.t("settings.doctor.copyFailed"), "a failure is shown locally");
bindDiagnostics(box, report, undefined);
await box.button.onclick();
assert.equal(box.msg.textContent, i18n.t("settings.doctor.copyFailed"), "no clipboard is a local failure too");

for (const { code } of i18n.LOCALES) {
  await i18n.setLocale(code);
  for (const key of ["copy", "copyHelp", "preview", "copied", "copyFailed"]) {
    assert.notEqual(i18n.t(`settings.doctor.${key}`), `settings.doctor.${key}`, `${code}: ${key}`);
  }
}
console.log("doctor diagnostics: exact preview, copy, local failure and catalogs OK");
