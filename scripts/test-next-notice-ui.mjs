#!/usr/bin/env node
// PRD 0003 RF3.4 (re-validation V-02/V-07): the tray's "Next notice" keeps its time whole in a non-shrinking element
// with the full sentence as the tooltip, and the time follows DS-MSG-8 (today the time, within 6 days the weekday,
// otherwise the date) with translated joins. Browser-free.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { installUiEnv } from "./ui-test-env.mjs";

installUiEnv({ locale: "en-US" });
const i18n = await import("../apps/desktop/ui/js/i18n.js");
await i18n.initI18n();
const lib = await import("../apps/desktop/ui/js/lib.js");

const now = new Date(2026, 2, 3, 10, 0); // a Tuesday, local time
const at = (days, h, m) => new Date(2026, 2, 3 + days, h, m).toISOString();
const view = (iso) => ({ notify: { next: { provider: "claude", window: "window_5h", label: "5h", at: iso } } });

const today = lib.fmtNoticeTime(at(0, 14, 30), now);
assert.match(today, /^Today /, "today shows the time only");
const soon = lib.fmtNoticeTime(at(4, 9, 5), now);
assert.match(soon, /^Sat /, "within 6 days: the short weekday and time");
const later = lib.fmtNoticeTime(at(9, 9, 5), now);
assert.match(later, /^Mar 12, /, "beyond 6 days: the short date and time");

const parts = lib.nextNoticeParts(view(at(9, 9, 5)), now);
assert.equal(parts.head + parts.when, parts.text, "the parts rebuild the sentence");
assert.equal(parts.when, later, "the time is the tail");
assert.ok(parts.head.startsWith("Next notice: "));
assert.equal(lib.nextNoticeParts({ notify: {} }), null);

// An `other` window shows the core's translated label, never the adapter's text (re-validation V-01).
const other = lib.nextNoticeParts({ notify: { next: { provider: "claude", window: "other", label: "Gemini (Semanal)",
  label_text: { id: "x.unknown", params: {}, fallback: "Gemini (Weekly)" }, at: at(0, 14, 30) } } }, now);
assert.match(other.text, /Gemini \(Weekly\)/);
assert.doesNotMatch(other.text, /Semanal/);
// An unnamed account's bell names its provider (re-validation V-05).
const bell = lib.bellHtml({ notify: { enabled: true } }, { account: "", provider: "claude", account_key: "k", notify: true });
assert.doesNotMatch(bell, /aria-label="[^"]*for "/);
assert.match(bell, /aria-label="[^"]*claude/i);

// Every catalog has the two joins, with both placeholders.
for (const code of ["pt-BR", "en-US", "es", "it", "zh-CN", "fr", "de", "ja", "ko", "hi-IN", "id-ID", "ru", "tr", "vi"]) {
  const catalog = JSON.parse(await readFile(new URL(`../apps/desktop/ui/locales/${code}.json`, import.meta.url), "utf8"));
  assert.match(catalog["time.weekdayTime"] ?? "", /\{day\}[\s\S]*\{time\}|\{time\}[\s\S]*\{day\}/, `${code} time.weekdayTime`);
  assert.match(catalog["time.dateTime"] ?? "", /\{date\}[\s\S]*\{time\}|\{time\}[\s\S]*\{date\}/, `${code} time.dateTime`);
  assert.match(catalog["notify.next"], /\{when\}$/, `${code}: the time ends the sentence, so only the name shrinks`);
}

// The tray renders the time in its own element, with the full text as the tooltip; the CSS keeps it whole.
const tray = await readFile(new URL("../apps/desktop/ui/js/tray.js", import.meta.url), "utf8");
assert.match(tray, /data-next-notice title="\$\{esc\(next\.text\)\}"/);
assert.match(tray, /class="pop-notice-when"/);
const css = await readFile(new URL("../apps/desktop/ui/css/tray.css", import.meta.url), "utf8");
assert.match(css, /\.pop-notice \.pop-notice-when \{\s*flex: none;/);

console.log("PASS: next notice keeps its time whole, has a tooltip and follows DS-MSG-8");
process.exit(0);
